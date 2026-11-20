// SPDX-License-Identifier: GPL-3.0-or-later
//! Patrons behind the wall, through the running server (docs/specs/circulacao/leitores.md,
//! items 10 to 14).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration helpers fail the test on error"
)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, ItemData, ItemStatus, NewInstance, NewItem, NewRecord,
    RecordItems,
};
use elzevir_app::circulation_rules::{CirculationCommand, OverdueNoticeRule, RuleChange};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::loans::{CheckinRequest, CheckoutRequest};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_domain::catalog::BranchId;
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::notice::{NoticeLevel, OverdueLevel, Transports};
use elzevir_domain::patron::{Cpf, Enrolment, PatronCategoryId, PatronId};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn start(data: &Path) -> (Server, SocketAddr) {
    let path = data.join("output.log");
    let output = std::fs::File::create(&path).unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_elzevir"))
            .args(["serve", "--data-dir"])
            .arg(data)
            .args(["--listen", "127.0.0.1:0"])
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = std::fs::read_to_string(&path).unwrap();
        if let Some(address) = output
            .lines()
            .find_map(|line| line.strip_prefix("elzevir: listening on http://"))
        {
            return (server, address.parse().unwrap());
        }
        assert!(server.0.try_wait().unwrap().is_none(), "{output}");
        assert!(Instant::now() < deadline, "{output}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn request(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.0\r\nHost: localhost\r\n{headers}Content-Length: {}\r\n\r\n{body}",
        body.len(),
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn open(data: &Path) -> ElzevirStore {
    ElzevirStore::open(
        &data.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn submit(store: &ElzevirStore, command: WriteCommand) -> WriteOutput {
    store.submit(command).unwrap().wait().unwrap()
}

/// Three accounts (circulation, patrons, cataloguing only), a branch, a category and Ana.
/// Returns Ana's id.
fn library(data: &Path) -> i64 {
    let store = open(data);
    for (login, permission) in [
        ("cir", Permission::Circulation),
        ("lei", Permission::Patrons),
        ("cat", Permission::Cataloguing),
    ] {
        submit(
            &store,
            WriteCommand::Staff(StaffCommand::Create {
                login: login.to_owned(),
                password_hash: hash_password("synthetic-password").unwrap(),
                permissions: [Permission::StaffInterface, permission].into(),
            }),
        );
    }
    let WriteOutput::Catalog(CatalogOutput::Branch(branch)) = submit(
        &store,
        WriteCommand::Catalog(CatalogCommand::CreateBranch {
            name: "Biblioteca Central".to_owned(),
        }),
    ) else {
        panic!("no branch");
    };
    let WriteOutput::Patrons(PatronOutput::CategoryChanged(Some(category))) = submit(
        &store,
        WriteCommand::Patrons(PatronCommand::ChangeCategory(CategoryChange::Create {
            name: "Estudante".to_owned(),
            enrolment: Enrolment::Months(12),
        })),
    ) else {
        panic!("no category");
    };
    let WriteOutput::Patrons(PatronOutput::Added(ana)) = submit(
        &store,
        WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
            cardnumber: "000123".to_owned(),
            surname: "Silva".to_owned(),
            first_name: Some("Ana".to_owned()),
            cpf: Some(Cpf::parse("529.982.247-25").unwrap()),
            date_of_birth: None,
            email: Some("ana@example.org".to_owned()),
            phone: None,
            address: None,
            branch,
            category,
            enrolled: date(2026, 1, 31),
            expires: None,
            staff_note: None,
            reading_history: ReadingHistory::NotKept,
        }))),
    ) else {
        panic!("no patron");
    };
    store.shutdown().unwrap();
    ana.0
}

fn csrf(page: &str) -> &str {
    page.split_once("name=\"csrf\" value=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}

fn session_cookie(response: &str, name: &str) -> String {
    let line = response
        .lines()
        .find(|line| {
            line.to_ascii_lowercase()
                .starts_with(&format!("set-cookie: {name}="))
        })
        .unwrap();
    line.split_once(": ")
        .unwrap()
        .1
        .split_once(';')
        .unwrap()
        .0
        .to_owned()
}

/// Signs in and returns the `Cookie:` header line of the session.
fn sign_in(address: SocketAddr, login: &str) -> String {
    let page = request(address, "GET", "/equipe/login", "", "");
    let cookie = session_cookie(&page, "elzevir_login_csrf");
    let headers =
        format!("Cookie: {cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\n");
    let signed = request(
        address,
        "POST",
        "/equipe/login",
        &headers,
        &format!(
            "login={login}&password=synthetic-password&csrf={}",
            csrf(&page)
        ),
    );
    assert_status(&signed, 303);
    format!("Cookie: {}\r\n", session_cookie(&signed, "elzevir_session"))
}

fn assert_status(response: &str, expected: u16) {
    assert!(
        response.starts_with(&format!("HTTP/1.0 {expected} ")),
        "{response}"
    );
}

#[test]
fn the_record_is_behind_the_wall_and_every_opening_is_logged() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let cat = sign_in(address, "cat");
    let lei = sign_in(address, "lei");
    assert_status(&request(address, "GET", "/equipe/leitores", &cat, ""), 403);
    // Only the circulation permission opens the record.
    let record = format!("/equipe/leitores/{ana}");
    assert_status(&request(address, "GET", &record, &lei, ""), 403);

    let found = request(address, "GET", "/equipe/leitores?q=silva", &cir, "");
    assert_status(&found, 200);
    assert!(found.contains(&format!("href=\"/equipe/leitores/{ana}\">Silva, Ana")));
    // A card number found alone goes straight to the record.
    let scanned = request(address, "GET", "/equipe/leitores?q=000123", &cir, "");
    assert_status(&scanned, 303);
    assert!(
        scanned.contains(&format!("location: {record}")),
        "{scanned}"
    );
    let nothing = request(address, "GET", "/equipe/leitores?q=zz", &cir, "");
    assert!(nothing.contains("No patron found."));

    let page = request(address, "GET", &record, &cir, "");
    assert_status(&page, 200);
    assert!(page.contains("***.982.247-**"));
    assert!(!page.contains("52998224725") && !page.contains("529.982.247-25"));
    assert!(page.contains("ana@example.org"));
    // The home page links to patrons only for who may see them.
    assert!(request(address, "GET", "/equipe", &cir, "").contains("href=\"/equipe/leitores\""));
    assert!(!request(address, "GET", "/equipe", &cat, "").contains("href=\"/equipe/leitores\""));
    drop(server);

    let store = open(data.path());
    let viewed: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT count(*) FROM audit_log a JOIN staff_accounts s ON s.id = a.actor
                 WHERE a.kind = 'patrons.view' AND s.login = 'cir'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(viewed, 1);
}

#[test]
fn categories_are_created_with_months_or_a_date_by_who_manages_patrons() {
    let data = tempfile::tempdir().unwrap();
    library(data.path());
    let (_server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let lei = sign_in(address, "lei");
    assert_status(
        &request(address, "GET", "/equipe/leitores/categorias", &cir, ""),
        403,
    );
    let page = request(address, "GET", "/equipe/leitores/categorias", &lei, "");
    assert_status(&page, 200);
    assert!(page.contains("1 patron"));
    let token = csrf(&page).to_owned();
    let form = format!("{lei}Content-Type: application/x-www-form-urlencoded\r\n");
    let post = |body: String| {
        request(
            address,
            "POST",
            "/equipe/leitores/categorias",
            &form,
            &format!("csrf={token}&{body}"),
        )
    };
    let created = post("action=create&name=Projeto&months=&until=2026-12-31".to_owned());
    assert_status(&created, 303);
    let both = post("action=create&name=Outra&months=6&until=2026-12-31".to_owned());
    assert_status(&both, 422);
    assert!(both.contains("either 1 to 999 months or a fixed date"));
    let same_name = post("action=create&name=Estudante&months=6&until=".to_owned());
    assert_status(&same_name, 409);
    let page = request(address, "GET", "/equipe/leitores/categorias", &lei, "");
    assert!(page.contains("value=\"Projeto\""));
    assert!(page.contains("value=\"2026-12-31\""));
}

#[test]
fn a_patron_is_enrolled_and_edited_on_the_form() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    let (server, address) = start(data.path());
    let lei = sign_in(address, "lei");
    let cir = sign_in(address, "cir");
    assert_status(
        &request(address, "GET", "/equipe/leitores/novo", &cir, ""),
        403,
    );
    let page = request(address, "GET", "/equipe/leitores/novo", &lei, "");
    assert_status(&page, 200);
    assert!(page.contains("Biblioteca Central") && page.contains("Estudante"));
    let token = csrf(&page).to_owned();
    let form = format!("{lei}Content-Type: application/x-www-form-urlencoded\r\n");
    let post = |path: &str, body: &str| {
        request(
            address,
            "POST",
            path,
            &form,
            &format!("csrf={token}&{body}"),
        )
    };
    let fields = "surname=Souza&first_name=Bruno&branch=1&category=1&expires=&history_kept=1";
    let bad_cpf = post(
        "/equipe/leitores/novo",
        &format!("cardnumber=000200&cpf=111.111.111-11&{fields}"),
    );
    assert_status(&bad_cpf, 422);
    assert!(bad_cpf.contains("This is not a valid CPF"));
    // What was typed comes back in the form.
    assert!(bad_cpf.contains("value=\"Souza\""));
    let same_card = post(
        "/equipe/leitores/novo",
        &format!("cardnumber=000123&{fields}"),
    );
    assert_status(&same_card, 409);
    let added = post(
        "/equipe/leitores/novo",
        &format!("cardnumber=000200&cpf=000.000.006-04&{fields}"),
    );
    assert_status(&added, 303);
    let location = added
        .lines()
        .find_map(|line| line.strip_prefix("location: "))
        .unwrap()
        .to_owned();
    let edit = location.trim_end_matches("?saved=1").to_owned();
    assert_ne!(edit, format!("/equipe/leitores/{ana}/editar"));
    let page = request(address, "GET", &location, &lei, "");
    assert_status(&page, 200);
    // Only the edit form shows the whole CPF.
    assert!(page.contains("value=\"00000000604\""));
    assert!(page.contains("name=\"history_kept\" value=\"1\" checked"));
    let row_version = "row_version=1";
    let changed = post(
        &edit,
        &format!(
            "{row_version}&cardnumber=000200&cpf=&surname=Souza&first_name=Bruno&branch=1&category=1&expires=2030-01-31&phone=4499"
        ),
    );
    assert_status(&changed, 303);
    let stale = post(
        &edit,
        &format!(
            "{row_version}&cardnumber=000200&surname=Souza&branch=1&category=1&expires=2030-01-31"
        ),
    );
    assert_status(&stale, 409);
    assert!(stale.contains("Someone changed this patron"));
    drop(server);

    let store = open(data.path());
    let (phone, expires, cpf): (String, String, Option<String>) = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT phone, expires, cpf FROM patrons WHERE cardnumber = '000200'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .unwrap();
    assert_eq!(
        (phone.as_str(), expires.as_str(), cpf),
        ("4499", "2030-01-31", None)
    );
    let views: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT count(*) FROM audit_log WHERE kind = 'patrons.view'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(views, 1);
}

/// An Item "0001" of "Dom Casmurro" at the Anexo branch, and rules that lend for a week.
fn stock(data: &Path) {
    let store = open(data);
    for (table, code, name) in [
        (CodeTable::Branch, "B2", "Anexo"),
        (CodeTable::ItemType, "P1", "Livros"),
    ] {
        submit(
            &store,
            WriteCommand::Catalog(CatalogCommand::NameCodes(vec![NamedCode {
                table,
                code: code.to_owned(),
                name: name.to_owned(),
            }])),
        );
    }
    let WriteOutput::Catalog(CatalogOutput::Record { instance, .. }) = submit(
        &store,
        WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(NewRecord {
            work_title: "Dom Casmurro".to_owned(),
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
                title: "Dom Casmurro".to_owned(),
                ..NewInstance::default()
            },
        }))),
    ) else {
        panic!("no record");
    };
    submit(
        &store,
        WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(NewItem {
            instance,
            item: ItemData {
                barcode: Some("0001".to_owned()),
                inventory_number: None,
                home_branch: "B2".to_owned(),
                holding_branch: "B2".to_owned(),
                item_type: "P1".to_owned(),
                call_number: None,
                location: None,
                collection: None,
                price: None,
                replacement_price: None,
                copy_number: None,
                status: ItemStatus::default(),
                acquired_on: None,
            },
        }))),
    );
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![
                ("maxissueqty".to_owned(), Some("5".to_owned())),
                ("issuelength".to_owned(), Some("7".to_owned())),
            ],
        })),
    );
    store.shutdown().unwrap();
}

/// The branch the desk select starts at.
fn selected_desk(page: &str) -> &str {
    page.split_once("\" selected>")
        .unwrap()
        .0
        .rsplit_once("value=\"")
        .unwrap()
        .1
}

#[test]
fn an_item_is_lent_renewed_and_returned_from_the_patrons_side_of_the_wall() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let lei = sign_in(address, "lei");
    let record = format!("/equipe/leitores/{ana}");
    let action = format!("{record}/circulacao");
    let page = request(address, "GET", &record, &cir, "");
    assert_status(&page, 200);
    assert!(page.contains("No loans."));
    let (token, desk) = (csrf(&page).to_owned(), selected_desk(&page).to_owned());
    let form = format!("{cir}Content-Type: application/x-www-form-urlencoded\r\n");
    let post = |op: &str, barcode: &str| {
        request(
            address,
            "POST",
            &action,
            &form,
            &format!("csrf={token}&op={op}&barcode={barcode}&desk={desk}"),
        )
    };

    let lent = post("lend", "0001");
    assert_status(&lent, 200);
    assert!(lent.contains("Lent until"), "{lent}");
    assert!(lent.contains("loan-new") && lent.contains("Dom Casmurro"));
    // No rule gives renewals.
    let renewed = post("renew%3A0001", "");
    assert_status(&renewed, 422);
    assert!(renewed.contains("This loan has no renewals left."));
    let again = post("lend", "0001");
    assert_status(&again, 422);
    // The Item belongs at the annex, so the desk sends it there.
    let back = post("return%3A0001", "");
    assert_status(&back, 200);
    assert!(back.contains("Returned: 0001.") && back.contains("Send it to Anexo."));
    assert!(back.contains("No loans."));
    // The circulation permission and the session's token are both needed.
    let forged = request(
        address,
        "POST",
        &action,
        &form,
        &format!("csrf=forged&op=lend&barcode=0001&desk={desk}"),
    );
    assert_status(&forged, 403);
    assert_status(
        &request(
            address,
            "POST",
            &action,
            &format!("{lei}Content-Type: application/x-www-form-urlencoded\r\n"),
            "csrf=x&op=lend&desk=1",
        ),
        403,
    );

    // The checkin desk shows what happened, and nothing about the patron.
    let desk_page = request(address, "GET", "/equipe/circulacao/devolucao", &cir, "");
    assert_status(&desk_page, 200);
    post("lend", "0001");
    let scanned = request(
        address,
        "POST",
        "/equipe/circulacao/devolucao",
        &form,
        &format!("csrf={token}&barcode=0001&desk={desk}"),
    );
    assert_status(&scanned, 200);
    assert!(scanned.contains("Returned: 0001."));
    assert!(!scanned.contains("Silva") && !scanned.contains("000123"));
    let unknown = request(
        address,
        "POST",
        "/equipe/circulacao/devolucao",
        &form,
        &format!("csrf={token}&barcode=9999&desk={desk}"),
    );
    assert_status(&unknown, 422);
    drop(server);
}

/// The value of the first hidden field `name` in a page.
fn hidden<'a>(page: &'a str, name: &str) -> &'a str {
    page.split_once(&format!("name=\"{name}\" value=\""))
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}

#[test]
fn an_item_is_lent_from_its_pill_and_the_arrow_crosses_the_wall() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let cat = sign_in(address, "cat");
    let catalog = request(address, "GET", "/equipe/acervo", &cir, "");
    assert_status(&catalog, 200);
    assert!(catalog.contains("class=\"lend-box\""), "{catalog}");
    // Without the circulation permission there is nothing to lend.
    let browsing = request(address, "GET", "/equipe/acervo", &cat, "");
    assert!(!browsing.contains("lend-box"));
    let (token, instance, item) = (
        csrf(&catalog).to_owned(),
        hidden(&catalog, "instance").to_owned(),
        hidden(&catalog, "item").to_owned(),
    );
    let form =
        format!("{cir}Content-Type: application/x-www-form-urlencoded\r\nHX-Request: true\r\n");
    let check_out = |cardnumber: &str| {
        request(
            address,
            "POST",
            "/equipe/acervo/emprestar",
            &form,
            &format!("csrf={token}&instance={instance}&item={item}&cardnumber={cardnumber}"),
        )
    };
    let unknown = check_out("999999");
    assert_status(&unknown, 200);
    assert!(unknown.contains("No patron has this card number.") && unknown.contains(" open>"));

    let lent = check_out("000123");
    assert_status(&lent, 200);
    assert!(lent.contains("class=\"pill-due\""), "{lent}");
    let arrow = format!("/equipe/circulacao/exemplar/{item}");
    assert!(lent.contains(&format!("href=\"{arrow}\"")));
    // The catalog shows the loan and its due date, and nothing of the patron.
    let browsing = request(address, "GET", "/equipe/acervo", &cat, "");
    assert!(browsing.contains("On loan") && browsing.contains("pill-due"));
    assert!(
        !browsing.contains(&arrow) && !browsing.contains("Silva") && !browsing.contains("000123")
    );
    assert!(!request(address, "GET", "/equipe/acervo", &cir, "").contains("Silva"));
    // The arrow asks the server who has the Item, with the circulation permission only.
    let crossed = request(address, "GET", &arrow, &cir, "");
    assert_status(&crossed, 303);
    assert!(
        crossed.contains(&format!("location: /equipe/leitores/{ana}")),
        "{crossed}"
    );
    assert_status(&request(address, "GET", &arrow, &cat, ""), 403);
    // Lent already: the pill stays yellow and offers no box.
    let again = check_out("000123");
    assert!(!again.contains("lend-box"));
    drop(server);
}

/// Lends "0001" to Ana on 2 November and takes it back on 19 November, ten days late at
/// 0.50 a day: a fine of 5.00.
fn late_return(data: &Path, ana: i64) {
    let store = open(data);
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![
                ("fine".to_owned(), Some("0.50".to_owned())),
                ("chargeperiod".to_owned(), Some("1".to_owned())),
            ],
        })),
    );
    let at = |day: &str| -> jiff::Zoned { format!("{day}[America/Sao_Paulo]").parse().unwrap() };
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
            patron: PatronId(ana),
            barcode: "0001".to_owned(),
            desk: BranchId(1),
            on_site: false,
            confirmed: true,
            at: at("2026-11-02T10:00:00-03:00"),
        })),
    );
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::Checkin(CheckinRequest {
            barcode: "0001".to_owned(),
            desk: BranchId(1),
            at: at("2026-11-19T10:00:00-03:00"),
        })),
    );
    store.shutdown().unwrap();
}

#[test]
fn the_account_shows_the_fine_and_takes_payments_and_write_offs() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    late_return(data.path(), ana);
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let record = format!("/equipe/leitores/{ana}");
    let page = request(address, "GET", &record, &cir, "");
    assert_status(&page, 200);
    assert!(
        page.contains("Owes 5.00.") && page.contains("Overdue fine · 0001"),
        "{page}"
    );
    let line = page
        .split_once("name=\"line\" value=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
        .to_owned();
    let (token, desk) = (csrf(&page).to_owned(), selected_desk(&page).to_owned());
    let form = format!("{cir}Content-Type: application/x-www-form-urlencoded\r\n");
    let action = format!("{record}/conta");
    let post = |body: &str| {
        request(
            address,
            "POST",
            &action,
            &form,
            &format!("csrf={token}&desk={desk}&{body}"),
        )
    };
    assert_status(&post("op=pay&amount=abc"), 422);
    let over = post(&format!("op=pay&amount=6&line={line}"));
    assert_status(&over, 422);
    assert!(over.contains("more than what is owed"));
    let paid = post("op=pay&amount=2%2C50");
    assert_status(&paid, 200);
    assert!(paid.contains("Paid 2.50. The patron owes 2.50."), "{paid}");
    let written = post(&format!("op=writeoff&amount=2.50&line={line}"));
    assert_status(&written, 200);
    assert!(written.contains("Wrote off 2.50. The patron owes 0.00."));
    assert!(!written.contains("Owes ") && !written.contains("name=\"amount\""));
    // The token and the circulation permission are needed.
    assert_status(
        &request(
            address,
            "POST",
            &action,
            &form,
            &format!("csrf=forged&desk={desk}&op=pay&amount=1"),
        ),
        403,
    );
    drop(server);
}

/// Lends 0001 to Ana and runs the overdue notices with a level that restricts her.
fn restricted_for_overdues(data: &Path, ana: i64) {
    let store = open(data);
    let at = |day: &str| -> jiff::Zoned { format!("{day}[America/Sao_Paulo]").parse().unwrap() };
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
            patron: PatronId(ana),
            barcode: "0001".to_owned(),
            desk: BranchId(1),
            on_site: false,
            confirmed: true,
            at: at("2026-11-02T10:00:00-03:00"),
        })),
    );
    let category: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT category_id FROM patrons WHERE id = ?1",
                [ana],
                |row| row.get(0),
            )
        })
        .unwrap();
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetOverdueNoticeRule(
            OverdueNoticeRule {
                branch: None,
                category: PatronCategoryId(category),
                levels: vec![OverdueLevel {
                    level: NoticeLevel::First,
                    delay: 1,
                    transports: Transports::default(),
                    restrict: true,
                }],
            },
        )),
    );
    let due: String = store
        .readers()
        .read(|c| c.query_row("SELECT due FROM loans", [], |row| row.get(0)))
        .unwrap();
    let due: jiff::Zoned = due.parse().unwrap();
    let next_day = due.date().tomorrow().unwrap();
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::QueueOverdueNotices {
            at: at(&format!("{next_day}T10:00:00-03:00")),
        }),
    );
    store.shutdown().unwrap();
}

#[test]
fn a_restriction_for_overdues_is_shown_and_lifted_on_the_record() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    restricted_for_overdues(data.path(), ana);
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let record = format!("/equipe/leitores/{ana}");
    let page = request(address, "GET", &record, &cir, "");
    assert_status(&page, 200);
    assert!(
        page.contains("Restricted for overdue items until the staff lift it.")
            && page.contains("name=\"lift\" value=\"OVERDUES\""),
        "{page}"
    );
    let (token, desk) = (csrf(&page).to_owned(), selected_desk(&page).to_owned());
    let form = format!("{cir}Content-Type: application/x-www-form-urlencoded\r\n");
    let action = format!("{record}/restricoes");
    // The whole record's form comes along with the button.
    let body = |lift: &str| format!("csrf={token}&desk={desk}&barcode=&lift={lift}");
    assert_status(
        &request(address, "POST", &action, &form, &body("NONE")),
        400,
    );
    let lifted = request(address, "POST", &action, &form, &body("OVERDUES"));
    assert_status(&lifted, 200);
    assert!(lifted.contains("Restriction lifted.") && !lifted.contains("name=\"lift\""));
    let again = request(address, "POST", &action, &form, &body("OVERDUES"));
    assert_status(&again, 422);
    assert!(again.contains("The patron has no such restriction."));
    drop(server);
}

#[test]
fn holds_are_placed_listed_and_cancelled_on_the_record() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    let store = open(data.path());
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![("reservesallowed".to_owned(), Some("3".to_owned()))],
        })),
    );
    store.shutdown().unwrap();
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let record = format!("/equipe/leitores/{ana}");
    let page = request(address, "GET", &record, &cir, "");
    assert_status(&page, 200);
    assert!(
        page.contains("Place a hold") && page.contains(">Holds<"),
        "{page}"
    );
    let (token, desk) = (csrf(&page).to_owned(), selected_desk(&page).to_owned());
    let form = format!("{cir}Content-Type: application/x-www-form-urlencoded\r\n");
    let action = format!("{record}/reservas");
    let post = |body: &str| {
        request(
            address,
            "POST",
            &action,
            &form,
            &format!("csrf={token}&desk={desk}&{body}"),
        )
    };
    let placed = post(&format!("barcode=0001&level=record&pickup={desk}&op=place"));
    assert_status(&placed, 200);
    assert!(
        placed.contains("Hold placed, place 1 in the queue.")
            && placed.contains("1 hold")
            && placed.contains("Dom Casmurro"),
        "{placed}"
    );
    let again = post(&format!("barcode=0001&level=item&pickup={desk}&op=place"));
    assert_status(&again, 422);
    assert!(
        again.contains("as many holds on this record as allowed"),
        "{again}"
    );
    let hold = placed
        .split_once("value=\"cancel:")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
        .to_owned();
    let cancelled = post(&format!("op=cancel:{hold}"));
    assert_status(&cancelled, 200);
    assert!(cancelled.contains("Hold cancelled.") && !cancelled.contains("cancel:"));
    assert_status(
        &request(
            address,
            "POST",
            &action,
            &form,
            &format!("csrf=forged&desk={desk}&op=cancel:{hold}"),
        ),
        403,
    );
    drop(server);
}

#[test]
fn a_returned_item_is_set_aside_for_its_hold_at_the_checkin_desk() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    let store = open(data.path());
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![
                ("reservesallowed".to_owned(), Some("3".to_owned())),
                ("maxissueqty".to_owned(), Some("5".to_owned())),
                ("issuelength".to_owned(), Some("7".to_owned())),
            ],
        })),
    );
    store.shutdown().unwrap();
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let record = format!("/equipe/leitores/{ana}");
    let page = request(address, "GET", &record, &cir, "");
    let (token, desk) = (csrf(&page).to_owned(), selected_desk(&page).to_owned());
    let form = format!("{cir}Content-Type: application/x-www-form-urlencoded\r\n");
    let placed = request(
        address,
        "POST",
        &format!("{record}/reservas"),
        &form,
        &format!("csrf={token}&desk={desk}&barcode=0001&level=record&pickup={desk}&op=place"),
    );
    assert_status(&placed, 200);
    let scanned = request(
        address,
        "POST",
        "/equipe/circulacao/devolucao",
        &form,
        &format!("csrf={token}&barcode=0001&desk={desk}"),
    );
    assert_status(&scanned, 200);
    assert!(
        scanned.contains("It fills a hold to pick up at")
            && scanned.contains("Set aside for the hold"),
        "{scanned}"
    );
    // The checkin desk still shows nothing about the patron.
    assert!(!scanned.contains("Silva") && !scanned.contains("000123"));
    let hold = hidden(&scanned, "hold").to_owned();
    let aside = request(
        address,
        "POST",
        "/equipe/circulacao/devolucao/reserva",
        &form,
        &format!("csrf={token}&hold={hold}&barcode=0001&desk={desk}"),
    );
    assert_status(&aside, 200);
    assert!(
        aside.contains("Put it on the hold shelf; it waits until"),
        "{aside}"
    );
    let shown = request(address, "GET", &record, &cir, "");
    assert!(shown.contains("Waiting until"), "{shown}");
    // The patron borrows it: the hold is filled and leaves the record.
    let lent = request(
        address,
        "POST",
        &format!("{record}/circulacao"),
        &form,
        &format!("csrf={token}&op=lend&barcode=0001&desk={desk}"),
    );
    assert_status(&lent, 200);
    assert!(
        lent.contains("Lent until") && !lent.contains("Waiting until"),
        "{lent}"
    );
    assert_status(
        &request(
            address,
            "POST",
            "/equipe/circulacao/devolucao/reserva",
            &form,
            &format!("csrf=forged&hold={hold}&barcode=0001&desk={desk}"),
        ),
        403,
    );
    drop(server);
}
