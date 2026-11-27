// SPDX-License-Identifier: GPL-3.0-or-later
//! Each message of the Koha SIP2 recording is answered the same by the Elzevir server
//! (docs/specs/circulacao/sip2.md; `scenarios/circulacao/sip2.koha.json`, recorded with
//! `tools/sip2-recorder`). The greeting and the `BX` are the known deviations DEV-0003
//! and DEV-0004.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a missing code or field fails the test"
)]

#[expect(
    dead_code,
    reason = "the replay adds its own patron and Item, with the recording's card and call number"
)]
mod oracle;

use std::collections::BTreeSet;
use std::io::{self, Cursor, Read, Write};
use std::path::Path;
use std::sync::Arc;

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, ItemData, ItemStatus, NewInstance, NewItem, NewRecord,
    RecordItems,
};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::patrons::{PatronCommand, PatronFields, PatronOutput, ReadingHistory};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_domain::account::DebtSettings;
use elzevir_sip2::{Desk, Encoding, Terminal, Terminator, checksum, serve, verify_checksum};
use jiff::Zoned;
use jiff::civil::date;
use oracle::{Library, RecordedRules, submit};
use serde::Deserialize;

#[derive(Deserialize)]
struct Recording {
    institution: String,
    exchanges: Vec<Exchange>,
}

#[derive(Deserialize)]
struct Exchange {
    name: String,
    request: String,
    response: String,
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

/// What the oracle held: the branch, a patron with the recording's card and an Item with
/// its barcode, title and call number. The oracle has no `renewalperiod`, so a renewal
/// keeps the due date (docs/specs/circulacao/renovacao.md, item 5).
fn library(dir: &tempfile::TempDir, branch: &str) -> Library {
    let codes: BTreeSet<(bool, String)> =
        [(true, branch.to_owned()), (false, "BK".to_owned())].into();
    // The recording shows the category's code, `B`; Elzevir shows its name.
    let library = Library::new(dir, &codes, branch, "B");
    let rules: Vec<RecordedRules> = serde_json::from_str(
        r#"[{"context": {"library_id": "*", "patron_category_id": "*", "item_type_id": "*"},
             "rules": {"issuelength": "14", "maxissueqty": "5", "renewalsallowed": "2",
                       "lengthunit": "days"}}]"#,
    )
    .unwrap();
    library.set_rules(&rules).unwrap();
    let Ok(WriteOutput::Catalog(CatalogOutput::Record { instance, .. })) = submit(
        &library.store,
        WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(NewRecord {
            work_title: "ITEMTITLE".to_owned(),
            work_description: elzevir_domain::statement::Description::default(),
            hub: None,
            relations: Vec::new(),
            classification: None,
            classification_scheme: None,
            summary: None,
            audience: None,
            languages: Vec::new(),
            contributions: Vec::new(),
            subjects: Vec::new(),
            items: RecordItems::default(),
            source: None,
            instance: NewInstance {
                title: "ITEMTITLE".to_owned(),
                ..NewInstance::default()
            },
        }))),
    ) else {
        panic!("no record");
    };
    submit(
        &library.store,
        WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(NewItem {
            instance,
            item: ItemData {
                issue_statement: None,
                enumeration: None,
                chronology: None,
                barcode: Some("ITEM".to_owned()),
                inventory_number: None,
                home_branch: branch.to_owned(),
                holding_branch: branch.to_owned(),
                item_type: "BK".to_owned(),
                call_number: Some("B869.3 R28".to_owned()),
                location: None,
                collection: None,
                price: None,
                replacement_price: None,
                copy_number: None,
                status: ItemStatus::default(),
                acquired_on: None,
            },
        }))),
    )
    .unwrap();
    let Ok(WriteOutput::Patrons(PatronOutput::Added(patron))) = submit(
        &library.store,
        WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
            cardnumber: "PATRON".to_owned(),
            surname: "Leitora".to_owned(),
            first_name: Some("Ana".to_owned()),
            cpf: None,
            date_of_birth: Some(date(1990, 1, 1)),
            email: Some("ana@example.org".to_owned()),
            phone: Some("4499990000".to_owned()),
            address: Some("Rua A, 1".to_owned()),
            branch: library.branches[branch],
            category: library.category,
            enrolled: date(2026, 1, 1),
            expires: None,
            staff_note: None,
            reading_history: ReadingHistory::NotKept,
        }))),
    ) else {
        panic!("no patron");
    };
    submit(
        &library.store,
        WriteCommand::Patrons(PatronCommand::SetPassword {
            patron,
            password_hash: Some(hash_password("leitura-segura").unwrap()),
        }),
    )
    .unwrap();
    submit(
        &library.store,
        WriteCommand::Staff(StaffCommand::Create {
            login: "LOGIN".to_owned(),
            password_hash: hash_password("PASSWORD").unwrap(),
            permissions: [Permission::StaffInterface, Permission::Circulation].into(),
        }),
    )
    .unwrap();
    library
}

/// `message` as the recorder sent it: with the sequence and the checksum.
fn checksummed(message: &str, sequence: usize) -> String {
    let body = format!("{message}AY{}AZ", sequence % 10);
    format!("{body}{:04X}", checksum(body.as_bytes()))
}

/// The response without its time of day, which differs by the second, and without the
/// checksum, which covers it; the sequence stays.
fn comparable(response: &str) -> String {
    let mut text = response.to_owned();
    if let Some(at) = text.rfind("AZ")
        && text.len() - at == 6
    {
        text.truncate(at + 2);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("20261126    ") {
        let (before, after) = rest.split_at(at);
        out.push_str(before);
        let time = after.get(12..18).unwrap_or_default();
        if time == "235900" {
            out.push_str(after.get(..18).unwrap_or(after));
        } else {
            out.push_str("20261126    NOW___");
        }
        rest = after.get(18..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// The recording's placeholders for the patron's personal fields, and the deviations.
fn as_recorded(ours: &str) -> String {
    ours.replace("Ana Leitora", "NAME")
        .replace("|BDRua A, 1|", "|BDADDRESS|")
        .replace("|BEana@example.org|", "|BEEMAIL|")
        .replace("|BF4499990000|", "|BFPHONE|")
}

fn with_deviations(koha: &str) -> String {
    // DEV-0004: the greeting; DEV-0003: the messages Elzevir v0 does not support.
    koha.replace("Greetings from Koha.", "Greetings from Elzevir.")
        .replace("|BXYYYYYYYYYYYNYYYY|", "|BXYYYNYYYYYNYNNNYN|")
}

#[test]
fn every_recorded_message_is_answered_as_koha_answered_it() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/circulacao/sip2.koha.json");
    let recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir, &recording.institution);
    let store = Arc::new(library.store);
    let desk = Desk {
        staff: Arc::clone(&store) as _,
        patrons: Arc::clone(&store) as _,
        accounts: Arc::clone(&store) as _,
        loans: Arc::clone(&store) as _,
        self_check: Arc::clone(&store) as _,
        debt: DebtSettings::default(),
        renewal_base: elzevir_domain::circulation::RenewalBase::DueDate,
        currency: Some("REA".to_owned()),
        terminals: vec![Terminal {
            login: "LOGIN".to_owned(),
            unit: library.branches[&recording.institution],
            institution: recording.institution.clone(),
            terminator: Terminator::Cr,
            encoding: Encoding::Latin1,
        }],
        clock: Arc::new(|| {
            "2026-11-26T07:34:40-03:00[America/Sao_Paulo]"
                .parse::<Zoned>()
                .unwrap()
        }),
        log: Arc::new(|_| {}),
    };
    let mut sent = Vec::new();
    for (sequence, exchange) in recording.exchanges.iter().enumerate() {
        sent.extend_from_slice(checksummed(&exchange.request, sequence).as_bytes());
        sent.push(b'\r');
    }
    let mut wire = Wire {
        sent: Cursor::new(sent),
        answered: Vec::new(),
    };
    serve(&mut wire, &desk).unwrap();
    let answered = String::from_utf8(wire.answered).unwrap();
    let ours: Vec<&str> = answered.split_terminator('\r').collect();
    assert_eq!(ours.len(), recording.exchanges.len(), "{answered}");
    let mut differences = Vec::new();
    for (exchange, ours) in recording.exchanges.iter().zip(ours) {
        if ours.contains("AY") {
            assert!(
                verify_checksum(ours.as_bytes()),
                "{}: {ours}",
                exchange.name
            );
        }
        let koha = comparable(&with_deviations(&exchange.response));
        let ours = comparable(&as_recorded(ours));
        if koha != ours {
            differences.push(format!("{}\n  koha: {koha}\n  ours: {ours}", exchange.name));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
