// SPDX-License-Identifier: GPL-3.0-or-later
//! A self-check conversation through the SIP2 server on the real store
//! (docs/specs/circulacao/sip2.md, items 16 to 30).

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a missing code or field fails the test"
)]

mod oracle;

use std::collections::BTreeSet;
use std::io::{self, Cursor, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use elzevir_app::catalog::ItemStatus;
use elzevir_app::circulation_rules::CirculationCommand;
use elzevir_app::commands::WriteOutput;
use elzevir_app::commands::{StaffCommand, WriteCommand};
use elzevir_app::holds::{HoldLevel, HoldRequest};
use elzevir_app::patrons::{PatronCommand, PatronFields, PatronOutput, ReadingHistory};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_domain::account::DebtSettings;
use elzevir_domain::catalog::BranchId;
use elzevir_domain::patron::PatronId;
use elzevir_sip2::{Desk, SipEvent, Terminal, Terminator, serve};
use elzevir_store::ElzevirStore;
use jiff::Zoned;
use jiff::civil::date;
use oracle::{Library, RecordedRules, submit};

/// The fixed fields' date of every request and response.
const NOW: &str = "20261126    143005";

/// The running binary, stopped when the test ends.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

/// The messages a terminal sends, then what the server wrote back.
struct Wire {
    sent: Cursor<Vec<u8>>,
    answered: Vec<u8>,
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.sent.read(buffer)
    }
}

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.answered.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The store, and the unit of the terminals.
struct Setup {
    /// A second patron, who places holds.
    other: PatronId,
    store: Arc<ElzevirStore>,
    unit: BranchId,
}

fn library(dir: &tempfile::TempDir) -> Setup {
    let codes: BTreeSet<(bool, String)> =
        [(true, "Central".to_owned()), (false, "Livros".to_owned())].into();
    let library = Library::new(dir, &codes, "Central", "Adulto");
    let rules: Vec<RecordedRules> = serde_json::from_str(
        r#"[{"context": {"library_id": "*", "patron_category_id": "*", "item_type_id": "*"},
             "rules": {"issuelength": "14", "maxissueqty": "5", "renewalsallowed": "2",
                       "renewalperiod": "7", "lengthunit": "days",
                       "reservesallowed": "5", "holds_per_record": "1"}}]"#,
    )
    .unwrap();
    library.set_rules(&rules).unwrap();
    library.add_item(
        "1001",
        ("Central", "Central"),
        "Livros",
        ItemStatus::default(),
    );
    library.add_item(
        "1002",
        ("Central", "Central"),
        "Livros",
        ItemStatus::default(),
    );
    submit(
        &library.store,
        WriteCommand::Staff(StaffCommand::Create {
            login: "term1".to_owned(),
            password_hash: hash_password("terminal-secret").unwrap(),
            permissions: [Permission::StaffInterface, Permission::Circulation].into(),
        }),
    )
    .unwrap();
    submit(
        &library.store,
        WriteCommand::Staff(StaffCommand::Create {
            login: "reader".to_owned(),
            password_hash: hash_password("reader-secret").unwrap(),
            permissions: [Permission::StaffInterface].into(),
        }),
    )
    .unwrap();
    submit(
        &library.store,
        WriteCommand::Patrons(PatronCommand::SetPassword {
            patron: library.patron,
            password_hash: Some(hash_password("leitura-segura").unwrap()),
        }),
    )
    .unwrap();
    let Ok(WriteOutput::Patrons(PatronOutput::Added(other))) = submit(
        &library.store,
        WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
            cardnumber: "000002".to_owned(),
            surname: "Souza".to_owned(),
            first_name: Some("Bia".to_owned()),
            cpf: None,
            date_of_birth: None,
            email: Some("bia@example.org".to_owned()),
            phone: None,
            address: None,
            branch: library.branches["Central"],
            category: library.category,
            enrolled: date(2026, 1, 1),
            expires: None,
            staff_note: None,
            reading_history: ReadingHistory::NotKept,
        }))),
    ) else {
        panic!("no second patron");
    };
    Setup {
        other,
        unit: library.branches["Central"],
        store: Arc::new(library.store),
    }
}

/// Runs one connection that sends `messages`; the answers, one per line, and the log.
fn converse(setup: &Setup, messages: &[&str]) -> (Vec<String>, Vec<SipEvent>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let store = &setup.store;
    let desk = Desk {
        staff: Arc::clone(store) as _,
        patrons: Arc::clone(store) as _,
        accounts: Arc::clone(store) as _,
        loans: Arc::clone(store) as _,
        self_check: Arc::clone(store) as _,
        debt: DebtSettings::default(),
        currency: Some("BRL".to_owned()),
        terminals: ["term1", "reader"]
            .into_iter()
            .map(|login| Terminal {
                login: login.to_owned(),
                unit: setup.unit,
                institution: "Central".to_owned(),
                terminator: Terminator::Cr,
            })
            .collect(),
        clock: Arc::new(|| {
            "2026-11-26T14:30:05-03:00[America/Sao_Paulo]"
                .parse::<Zoned>()
                .unwrap()
        }),
        log: {
            let log = Arc::clone(&log);
            Arc::new(move |event| log.lock().unwrap().push(event))
        },
    };
    let mut sent = Vec::new();
    for message in messages {
        sent.extend_from_slice(message.as_bytes());
        sent.push(b'\r');
    }
    let mut wire = Wire {
        sent: Cursor::new(sent),
        answered: Vec::new(),
    };
    serve(&mut wire, &desk).unwrap();
    let answers = String::from_utf8(wire.answered)
        .unwrap()
        .split_terminator('\r')
        // A refused login ends in CR LF: no terminal chose its terminator yet.
        .map(|answer| answer.trim_start_matches('\n').to_owned())
        .filter(|answer| !answer.is_empty())
        .collect();
    let events = log.lock().unwrap().clone();
    (answers, events)
}

#[test]
fn a_terminal_signs_in_lends_renews_and_takes_back() {
    let dir = tempfile::tempdir().unwrap();
    let setup = library(&dir);
    let (answers, events) = converse(
        &setup,
        &[
            "9300CNterm1|COterminal-secret|CPCentral|",
            "9900302.00",
            "2300120261126    143005AOCentral|AA000001|ADleitura-segura|",
            &format!("11YN{NOW}                  AOCentral|AA000001|AB1001|ADleitura-segura|"),
            &format!("63001{NOW}Y         AOCentral|AA000001|ADleitura-segura|"),
            &format!("17{NOW}AOCentral|AB1001|"),
            &format!("29NN{NOW}                  AOCentral|AA000001|AB1001|"),
            &format!("09N{NOW}{NOW}APCentral|AOCentral|AB1001|"),
            &format!("35{NOW}AOCentral|AA000001|"),
        ],
    );
    let greeting = "BHBRL|BV0|AFGreetings from Elzevir. |";
    let expected = [
        "941".to_owned(),
        format!("98YYYYNN100005{NOW}2.00AOCentral|BXYYYNYYYYYNYNNNYN|"),
        // Koha joins the first name and the surname with a space, even without a first name.
        format!("24              001{NOW}AE Leitora|AA000001|BLY|CQY|{greeting}AOCentral|"),
        format!("121NNY{NOW}AOCentral|AA000001|AB1001|AJObra|AH20261210    235900|"),
        format!(
            "64              001{NOW}000000000001000000000000AOCentral|AA000001|AE Leitora|BLY|CQY|BHBRL|BV0|CC5|PCAdulto|PIY|AFGreetings from Elzevir. |"
        ),
        format!("18040201{NOW}AB1001|AJObra|AQCentral|APCentral|BGCentral|AH20261210    235900|"),
        format!("301YNN{NOW}AA000001|AB1001|AJObra|AH20261217    235900|CK|AOCentral|"),
        format!("101YNN{NOW}AOCentral|AB1001|AQCentral|AJObra|AA000001|"),
        format!("36Y{NOW}AOCentral|AA000001|AFThank you!|"),
    ];
    assert_eq!(answers, expected);
    // The log names the terminal and the message, never the card or the barcode.
    assert_eq!(events.len(), 9);
    assert!(
        events
            .iter()
            .all(|event| event.terminal.as_deref() == Some("term1"))
    );
    assert!(events.iter().all(|event| event.outcome == "answered"));
}

#[test]
fn a_login_without_circulation_or_a_wrong_password_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let setup = library(&dir);
    for login in [
        "9300CNreader|COreader-secret|",
        "9300CNterm1|COwrong|",
        "9300CNunknown|COterminal-secret|",
    ] {
        let (answers, events) = converse(&setup, &[login, "9900302.00"]);
        // Refused: `940`, and the next message closes the connection.
        assert_eq!(answers, ["940"], "{login}");
        assert_eq!(events[0].outcome, "refused");
        assert_eq!(events[1].outcome, "closed");
    }
}

#[test]
fn a_message_before_the_login_closes_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let setup = library(&dir);
    let (answers, _) = converse(&setup, &["9900302.00", "9300CNterm1|COterminal-secret|"]);
    assert!(answers.is_empty(), "{answers:?}");
}

#[test]
fn a_checkin_with_a_hold_waiting_sets_it_aside_and_names_who_asked() {
    let dir = tempfile::tempdir().unwrap();
    let setup = library(&dir);
    let (answers, _) = converse(
        &setup,
        &[
            "9300CNterm1|COterminal-secret|",
            &format!("11YN{NOW}                  AOCentral|AA000001|AB1001|"),
        ],
    );
    assert!(answers[1].starts_with("121"), "{answers:?}");
    submit(
        &setup.store,
        WriteCommand::Circulation(CirculationCommand::PlaceHold(HoldRequest {
            patron: setup.other,
            barcode: "1001".to_owned(),
            level: HoldLevel::Item,
            pickup: setup.unit,
            at: "2026-11-26T14:00:00-03:00[America/Sao_Paulo]"
                .parse()
                .unwrap(),
        })),
    )
    .unwrap();
    let (answers, _) = converse(
        &setup,
        &[
            "9300CNterm1|COterminal-secret|",
            &format!("17{NOW}AOCentral|AB1001|"),
            &format!("09N{NOW}{NOW}APCentral|AOCentral|AB1001|"),
            &format!("17{NOW}AOCentral|AB1001|"),
        ],
    );
    let item = format!("{NOW}AB1001|AJObra|AQCentral|APCentral|BGCentral|CF1|");
    let expected = [
        "941".to_owned(),
        format!("18040201{item}AH20261210    235900|"),
        // Alert 01: the hold is picked up here, and the Item waits for it.
        format!(
            "101YNY{NOW}AOCentral|AB1001|AQCentral|AJObra|AA000001|CY000002|CTCentral|DABia Souza (bia@example.org)|CV01|"
        ),
        format!("18080201{item}CM20261203    235900|"),
    ];
    assert_eq!(answers, expected);
}

#[test]
fn the_server_listens_for_terminals_from_the_configuration() {
    let dir = tempfile::tempdir().unwrap();
    drop(library(&dir));
    std::fs::write(
        dir.path().join("elzevir.toml"),
        "[sip2]\nlisten = \"127.0.0.1:0\"\n\n[[sip2.terminals]]\nlogin = \"term1\"\nunit = \"Central\"\nterminator = \"cr\"\n",
    )
    .unwrap();
    let log = dir.path().join("output.log");
    let output = std::fs::File::create(&log).unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["serve", "--data-dir"])
            .arg(dir.path())
            .args(["--listen", "127.0.0.1:0"])
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let address: SocketAddr = loop {
        let printed = std::fs::read_to_string(&log).unwrap();
        if let Some(address) = printed
            .lines()
            .find_map(|line| line.strip_prefix("SIP2 on "))
        {
            break address.parse().unwrap();
        }
        assert!(server.0.try_wait().unwrap().is_none(), "{printed}");
        assert!(Instant::now() < deadline, "{printed}");
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .write_all(b"9300CNterm1|COterminal-secret|\r9900302.00\r")
        .unwrap();
    let mut answered = Vec::new();
    let mut buffer = [0_u8; 512];
    while !answered.ends_with(b"\r") || answered.split(|byte| *byte == b'\r').count() < 3 {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "closed: {answered:?}");
        answered.extend_from_slice(&buffer[..read]);
    }
    let answered = String::from_utf8(answered).unwrap();
    assert!(answered.starts_with("941\r98YYYYNN100005"), "{answered}");
}
