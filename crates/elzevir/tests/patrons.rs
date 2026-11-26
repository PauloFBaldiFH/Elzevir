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

#[path = "../../elzevir-web/tests/contract/mod.rs"]
mod contract;

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
    // The list opens compact; the full view adds counts, never contact or CPF (§24.6).
    assert!(found.contains(r#"data-zoom="compact" data-zoom-screen="patrons""#));
    assert!(found.contains("No loans") && found.contains("No holds"));
    assert!(found.contains("In good standing"));
    for personal in ["ana@example.org", "982.247", "52998224725"] {
        assert!(!found.contains(personal), "{personal}");
    }
    let full = format!("{}; elzevir_zoom_patrons=full\r\n", cir.trim_end());
    let zoomed = request(address, "GET", "/equipe/leitores?q=silva", &full, "");
    assert!(zoomed.contains(r#"data-zoom="full" data-zoom-screen="patrons""#));
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
    // The whole CPF only by the explicit button, logged (interface §18.4).
    let show = format!("formaction=\"/equipe/leitores/{ana}/cpf\" formnovalidate>Show ID number<");
    assert!(page.contains(&show), "{page}");
    let reveal = format!("/equipe/leitores/{ana}/cpf");
    let form = "Content-Type: application/x-www-form-urlencoded\r\n";
    let post_reveal =
        |who: &str, body: &str| request(address, "POST", &reveal, &format!("{who}{form}"), body);
    assert_status(&post_reveal(&cir, "csrf=forged"), 403);
    assert_status(&post_reveal(&lei, "csrf=forged"), 403);
    let shown = post_reveal(&cir, &format!("csrf={}&barcode=&desk=1", csrf(&page)));
    assert_status(&shown, 200);
    assert!(shown.contains("529.982.247-25"), "{shown}");
    assert!(!shown.contains("Show ID number"));
    assert!(
        shown
            .to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
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
    // The first opening and the one that showed the CPF.
    assert_eq!(viewed, 2);
    let revealed: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT count(*) FROM audit_log a JOIN staff_accounts s ON s.id = a.actor
                 WHERE a.kind = 'patrons.reveal_cpf' AND s.login = 'cir'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(revealed, 1);
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
    assert!(bad_cpf.contains("This is not a valid ID number"));
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

#[test]
fn a_patron_is_anonymized_from_the_edit_form_after_confirming() {
    // docs/specs/circulacao/lgpd.md, item 6.
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    let (server, address) = start(data.path());
    let lei = sign_in(address, "lei");
    let cir = sign_in(address, "cir");
    let edit = format!("/equipe/leitores/{ana}/editar");
    let action = format!("/equipe/leitores/{ana}/anonimizar");
    let page = request(address, "GET", &edit, &lei, "");
    assert!(page.contains(&format!("action=\"{action}\"")));
    let token = csrf(&page).to_owned();
    let post = |who: &str, body: &str| {
        request(
            address,
            "POST",
            &action,
            &format!("{who}Content-Type: application/x-www-form-urlencoded\r\n"),
            &format!("csrf={token}{body}"),
        )
    };
    assert_status(&post(&cir, "&confirm=1"), 403);
    let unconfirmed = post(&lei, "");
    assert_status(&unconfirmed, 422);
    assert!(unconfirmed.contains("Tick the box to confirm."));
    let done = post(&lei, "&confirm=1");
    assert_status(&done, 303);
    assert!(done.contains("location: /equipe/leitores?anonimizado=1"));
    assert!(
        request(address, "GET", "/equipe/leitores?anonimizado=1", &lei, "")
            .contains("The patron was anonymized.")
    );
    assert_status(
        &request(address, "GET", &format!("/equipe/leitores/{ana}"), &cir, ""),
        404,
    );
    assert!(!request(address, "GET", "/equipe/leitores?q=silva", &cir, "").contains("Silva, Ana"));
    assert_status(&post(&lei, "&confirm=1"), 404);
    drop(server);
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
                issue_statement: None,
                enumeration: None,
                chronology: None,
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
    // The loan card crosses to the catalogue with the barcode alone (interface §24.4).
    assert!(
        lent.contains(r#"class="loan-title" href="/equipe/acervo?q=0001" data-cross="#),
        "{lent}"
    );
    assert!(lent.contains("urgency-ok") && lent.contains("In good standing"));
    // The record opens full; the zoom this browser chose last wins.
    assert!(lent.contains(r#"data-zoom="full""#));
    let compact = format!("{}; elzevir_zoom_patron=compact\r\n", cir.trim_end());
    let zoomed = request(address, "GET", &record, &compact, "");
    assert!(zoomed.contains(r#"data-zoom="compact""#), "{zoomed}");
    let odd = format!("{}; elzevir_zoom_patron=huge\r\n", cir.trim_end());
    assert!(request(address, "GET", &record, &odd, "").contains(r#"data-zoom="full""#));
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

/// Lends 0001 to Ana and runs the overdue notices with a level that restricts her and
/// prints a notice.
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
                    transports: Transports {
                        email: false,
                        print: true,
                    },
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
fn printed_notices_are_listed_and_marked_as_sent() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    restricted_for_overdues(data.path(), ana);
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    assert!(
        request(address, "GET", "/equipe/barra", &cir, "").contains("/equipe/circulacao/avisos")
    );
    let page = request(address, "GET", "/equipe/circulacao/avisos", &cir, "");
    assert_status(&page, 200);
    assert!(
        page.contains("<h2>Overdue items at ") && page.contains("(0001), due "),
        "{page}"
    );
    let id = page
        .split_once("name=\"n\" value=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
        .to_owned();
    let form = format!("{cir}Content-Type: application/x-www-form-urlencoded\r\n");
    let action = "/equipe/circulacao/avisos";
    let token = csrf(&page).to_owned();
    assert_status(
        &request(address, "POST", action, &form, &format!("csrf={token}&n=x")),
        400,
    );
    let marked = request(
        address,
        "POST",
        action,
        &form,
        &format!("csrf={token}&n={id}"),
    );
    assert_status(&marked, 303);
    let page = request(
        address,
        "GET",
        "/equipe/circulacao/avisos?marked=1",
        &cir,
        "",
    );
    assert!(
        page.contains("1 notice marked as sent.") && page.contains("No printed notice is waiting."),
        "{page}"
    );
    drop(server);
}

#[test]
fn the_checkout_slip_lists_loans_out_and_overdue_and_the_quick_slip_todays() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    restricted_for_overdues(data.path(), ana);
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let record = format!("/equipe/leitores/{ana}");
    let page = request(address, "GET", &record, &cir, "");
    assert!(
        page.contains(&format!("href=\"{record}/recibo?rapido=1\"")),
        "{page}"
    );
    let lei = sign_in(address, "lei");
    assert_status(
        &request(address, "GET", &format!("{record}/recibo"), &lei, ""),
        403,
    );
    let slip = request(address, "GET", &format!("{record}/recibo"), &cir, "");
    assert_status(&slip, 200);
    let overdue = slip.split_once("<h2>Overdue</h2>").unwrap().1;
    assert!(overdue.contains("<code>0001</code>"), "{slip}");
    assert!(
        slip.contains("<h2>Checked out</h2>\n<p>None.</p>"),
        "{slip}"
    );
    // Lent weeks ago: nothing on the quick slip.
    let quick = request(
        address,
        "GET",
        &format!("{record}/recibo?rapido=1"),
        &cir,
        "",
    );
    assert!(
        quick.contains("<h1>Quick slip</h1>") && !quick.contains("<code>0001</code>"),
        "{quick}"
    );
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

#[test]
fn the_staff_header_offers_only_permitted_scopes_and_dispatches_the_search() {
    let data = tempfile::tempdir().unwrap();
    library(data.path());
    let (server, address) = start(data.path());
    let cir = sign_in(address, "cir");
    let bar = request(address, "GET", "/equipe/barra?escopo=devolucao", &cir, "");
    assert_status(&bar, 200);
    assert!(
        bar.contains(r#"value="leitores""#)
            && bar.contains(r#"value="acervo""#)
            && checked(&bar, "devolucao")
            && bar.contains(r#"placeholder="Scan or type the item barcode…""#)
            && !bar.contains("/equipe/acervo/nova"),
        "{bar}"
    );
    let found = request(
        address,
        "GET",
        "/equipe/busca?escopo=leitores&q=Ana%20Silva",
        &cir,
        "",
    );
    assert_status(&found, 303);
    assert!(
        found.contains("location: /equipe/leitores?q=Ana%20Silva"),
        "{found}"
    );
    let scanned = request(
        address,
        "GET",
        "/equipe/busca?escopo=devolucao&q=0001",
        &cir,
        "",
    );
    assert!(
        scanned.contains("location: /equipe/circulacao/devolucao?codigo=0001"),
        "{scanned}"
    );
    let desk = request(
        address,
        "GET",
        "/equipe/circulacao/devolucao?codigo=0001",
        &cir,
        "",
    );
    assert!(desk.contains(r#"value="0001">"#), "{desk}");
    // Cataloguers see the catalog and "New record", never the patron side of the wall.
    let cat = sign_in(address, "cat");
    let bar = request(address, "GET", "/equipe/barra?escopo=leitores", &cat, "");
    assert!(
        bar.contains("/equipe/acervo/nova")
            && !bar.contains(r#"value="leitores""#)
            && !bar.contains("/equipe/leitores/novo")
            && checked(&bar, "acervo"),
        "{bar}"
    );
    assert_status(
        &request(
            address,
            "GET",
            "/equipe/busca?escopo=leitores&q=Ana",
            &cat,
            "",
        ),
        403,
    );
    assert_status(
        &request(address, "GET", "/equipe/busca?escopo=nada&q=x", &cat, ""),
        400,
    );
    // Scopes still to come show as placeholders and lead nowhere yet.
    assert_status(
        &request(
            address,
            "GET",
            "/equipe/busca?escopo=autoridades&q=x",
            &cat,
            "",
        ),
        404,
    );
    let lei = sign_in(address, "lei");
    let bar = request(address, "GET", "/equipe/barra", &lei, "");
    assert!(
        bar.contains("/equipe/leitores/novo") && !bar.contains(r#"value="devolucao""#),
        "{bar}"
    );
    drop(server);
}

/// Whether the header's scope radio `key` is the one checked.
fn checked(bar: &str, key: &str) -> bool {
    bar.split_once(&format!(r#"value="{key}""#))
        .and_then(|(_, rest)| rest.split_once('>'))
        .is_some_and(|(attributes, _)| attributes.ends_with(" checked"))
}

#[test]
fn the_patron_sees_their_loans_restriction_and_fines_on_the_opac() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    late_return(data.path(), ana);
    restricted_for_overdues(data.path(), ana);
    let (_server, address) = start_with_password(data.path(), ana);
    let patron = patron_sign_in(address);
    let page = request(address, "GET", "/conta", &patron, "");
    assert_status(&page, 200);
    for shown in [
        "One loan",
        "Dom Casmurro",
        "<code>0001</code>",
        "overdue",
        "blocked because of overdue items",
        // The open loan keeps accruing on the real clock, so only the returned one is exact.
        "You owe ",
        "Overdue fine · 0001</td><td class=\"account-money\">5.00</td>",
        "Fines are paid at the library desk.",
    ] {
        assert!(page.contains(shown), "{shown}: {page}");
    }
    // The patron reads; paying and lifting stay at the desk.
    for absent in [
        "name=\"line\"",
        "name=\"amount\"",
        "/equipe/leitores",
        "restricoes",
    ] {
        assert!(!page.contains(absent), "{absent}");
    }
}

/// Gives Ana an OPAC password and starts the server.
fn start_with_password(data: &Path, ana: i64) -> (Server, SocketAddr) {
    let store = open(data);
    submit(
        &store,
        WriteCommand::Patrons(PatronCommand::SetPassword {
            patron: PatronId(ana),
            password_hash: Some(hash_password("leitura-segura").unwrap()),
        }),
    );
    store.shutdown().unwrap();
    start(data)
}

/// Signs Ana in on the OPAC and returns the `Cookie:` header line of her session.
fn patron_sign_in(address: SocketAddr) -> String {
    let form = request(address, "GET", "/conta/entrar", "", "");
    let cookie = session_cookie(&form, "elzevir_patron_csrf");
    let signed = request(
        address,
        "POST",
        "/conta/entrar",
        &format!("Cookie: {cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\n"),
        &format!("csrf={}&card=000123&password=leitura-segura", csrf(&form)),
    );
    assert_status(&signed, 303);
    format!("Cookie: {}\r\n", session_cookie(&signed, "elzevir_patron"))
}

#[test]
fn the_patron_renews_a_loan_on_the_opac_while_renewals_are_left() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    let store = open(data.path());
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![
                ("renewalsallowed".to_owned(), Some("1".to_owned())),
                ("renewalperiod".to_owned(), Some("7".to_owned())),
            ],
        })),
    );
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
            patron: PatronId(ana),
            barcode: "0001".to_owned(),
            desk: BranchId(1),
            on_site: false,
            confirmed: true,
            at: "2026-11-19T10:00:00-03:00[America/Sao_Paulo]"
                .parse()
                .unwrap(),
        })),
    );
    store.shutdown().unwrap();
    let (_server, address) = start_with_password(data.path(), ana);
    let patron = patron_sign_in(address);
    let page = request(address, "GET", "/conta", &patron, "");
    assert!(page.contains("One renewal left"), "{page}");
    assert!(page.contains("action=\"/conta/renovar\""));
    let token = csrf(&page).to_owned();
    let renew = |token: &str, barcode: &str| {
        request(
            address,
            "POST",
            "/conta/renovar",
            &format!("{patron}Content-Type: application/x-www-form-urlencoded\r\n"),
            &format!("csrf={token}&barcode={barcode}"),
        )
    };
    assert_status(&renew("forged", "0001"), 403);
    let renewed = renew(&token, "0001");
    assert_status(&renewed, 303);
    assert!(
        renewed.contains("location: /conta?renovado=0001#loans-title"),
        "{renewed}"
    );
    let page = request(address, "GET", "/conta?renovado=0001", &patron, "");
    assert!(
        page.contains("Renewed “Dom Casmurro”; it is now due 2026-12-03."),
        "{page}"
    );
    assert!(page.contains("No renewals left"));
    assert!(!page.contains("action=\"/conta/renovar\""));

    let again = renew(&token, "0001");
    assert!(
        again.contains("location: /conta?recusa=too_many"),
        "{again}"
    );
    let page = request(address, "GET", "/conta?recusa=too_many", &patron, "");
    assert!(page.contains("This loan has no renewals left."));
    let unknown = renew(&token, "9999");
    assert!(unknown.contains("recusa=no_loan"), "{unknown}");
    // Without a patron session the renewal leads to the sign-in form.
    let visitor = request(
        address,
        "POST",
        "/conta/renovar",
        "Content-Type: application/x-www-form-urlencoded\r\n",
        &format!("csrf={token}&barcode=0001"),
    );
    assert_status(&visitor, 303);
    assert!(visitor.contains("location: /conta/entrar"));
}

/// The value of the first `selected` option of the page.
fn selected_option(page: &str) -> &str {
    page.split_once("\" selected>")
        .unwrap()
        .0
        .rsplit_once("value=\"")
        .unwrap()
        .1
}

#[test]
fn the_patron_places_and_cancels_a_hold_from_the_opac_record() {
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
    let (_server, address) = start_with_password(data.path(), ana);
    // The cached record page offers the same link to everyone and nothing of the patron.
    let record = request(address, "GET", "/record/1", "", "");
    assert_status(&record, 200);
    assert!(
        record.contains("href=\"/conta/reservar/1\"") && record.contains("Dom Casmurro"),
        "{record}"
    );
    let visitor = request(address, "GET", "/conta/reservar/1", "", "");
    assert_status(&visitor, 303);
    assert!(visitor.contains("location: /conta/entrar"));

    let patron = patron_sign_in(address);
    let signed_record = request(address, "GET", "/record/1", &patron, "");
    assert!(!signed_record.contains("Ana") && !signed_record.contains("000123"));
    let form = request(address, "GET", "/conta/reservar/1", &patron, "");
    assert_status(&form, 200);
    assert!(
        form.contains("no-store") && form.contains("Confirm hold"),
        "{form}"
    );
    let (token, pickup) = (csrf(&form).to_owned(), selected_option(&form).to_owned());
    let headers = format!("{patron}Content-Type: application/x-www-form-urlencoded\r\n");
    let post = |path: &str, body: &str| request(address, "POST", path, &headers, body);
    assert_status(
        &post("/conta/reservar/1", &format!("csrf=forged&pickup={pickup}")),
        403,
    );
    let placed = post(
        "/conta/reservar/1",
        &format!("csrf={token}&pickup={pickup}"),
    );
    assert_status(&placed, 303);
    assert!(
        placed.contains("location: /conta?aviso=reserva#holds-title"),
        "{placed}"
    );
    let again = post(
        "/conta/reservar/1",
        &format!("csrf={token}&pickup={pickup}"),
    );
    assert_status(&again, 422);
    assert!(
        again.contains("You already have a hold on this record."),
        "{again}"
    );
    let page = request(address, "GET", "/conta?aviso=reserva", &patron, "");
    assert!(
        page.contains("Hold placed.") && page.contains("1 hold") && page.contains("Place 1"),
        "{page}"
    );
    let cancel = page
        .split_once("action=\"/conta/reservas/")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
        .to_owned();
    let cancelled = post(
        &format!("/conta/reservas/{cancel}"),
        &format!("csrf={token}"),
    );
    assert_status(&cancelled, 303);
    let page = request(
        address,
        "GET",
        "/conta?aviso=reserva_cancelada",
        &patron,
        "",
    );
    assert!(
        page.contains("Hold cancelled.") && page.contains("You have no holds."),
        "{page}"
    );
}

#[test]
fn the_patron_chooses_whether_returns_are_kept_in_their_history() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    let (_server, address) = start_with_password(data.path(), ana);
    let patron = patron_sign_in(address);
    let page = request(address, "GET", "/conta", &patron, "");
    assert!(
        page.contains("does not keep a record") && page.contains("Keep my reading history"),
        "{page}"
    );
    let headers = format!("{patron}Content-Type: application/x-www-form-urlencoded\r\n");
    let choose = |body: String| request(address, "POST", "/conta/historico", &headers, &body);
    assert_status(&choose("csrf=forged&keep=1".to_owned()), 403);
    let kept = choose(format!("csrf={}&keep=1", csrf(&page)));
    assert_status(&kept, 303);
    assert!(kept.contains("aviso=historico_sim"), "{kept}");
    let page = request(address, "GET", "/conta?aviso=historico_sim", &patron, "");
    assert!(
        page.contains("From now on, the items you return are kept")
            && page.contains("Stop keeping my reading history"),
        "{page}"
    );
    let stopped = choose(format!("csrf={}", csrf(&page)));
    assert!(stopped.contains("aviso=historico_nao"), "{stopped}");
}

// The patron's account through the REST API (docs/specs/opac/api.md, items 8 to 17).

/// Calls the API with an optional bearer token and JSON body; returns the status, the
/// head and the JSON body (`null` when empty). `operation` is the documented path the
/// answer is checked against.
fn api(
    address: SocketAddr,
    method: &str,
    uri: &str,
    operation: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String, serde_json::Value) {
    let bearer = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let headers = format!("Content-Type: application/json\r\n{bearer}");
    let response = request(address, method, uri, &headers, body);
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    let status: u16 = head.split(' ').nth(1).unwrap().parse().unwrap();
    let body = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(body).unwrap()
    };
    if status != 204 {
        contract::check(operation, &method.to_ascii_lowercase(), status, &body);
    }
    (status, head.to_ascii_lowercase(), body)
}

const TOKENS: &str = "/api/v1/patron/tokens";

fn api_token(address: SocketAddr) -> String {
    let (status, head, body) = api(
        address,
        "POST",
        TOKENS,
        TOKENS,
        None,
        r#"{"card": " 000123 ", "password": "leitura-segura"}"#,
    );
    assert_eq!(status, 201, "{body}");
    assert!(head.contains("cache-control: no-store"), "{head}");
    assert_eq!(body.get("expires_in"), Some(&1800.into()));
    body.get("token").unwrap().as_str().unwrap().to_owned()
}

#[test]
fn the_patron_reads_the_account_and_renews_a_loan_through_the_api() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    stock(data.path());
    late_return(data.path(), ana);
    let store = open(data.path());
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![
                ("renewalsallowed".to_owned(), Some("1".to_owned())),
                ("renewalperiod".to_owned(), Some("7".to_owned())),
            ],
        })),
    );
    // Lent now, as the server reads the real clock: the loan is not overdue.
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
            patron: PatronId(ana),
            barcode: "0001".to_owned(),
            desk: BranchId(1),
            on_site: false,
            confirmed: true,
            at: jiff::Zoned::now()
                .with_time_zone(jiff::tz::TimeZone::get("America/Sao_Paulo").unwrap()),
        })),
    );
    store.shutdown().unwrap();
    let (_server, address) = start_with_password(data.path(), ana);
    let account = "/api/v1/patron/account";
    let (status, head, _) = api(address, "GET", account, account, None, "");
    assert_eq!(status, 401);
    assert!(head.contains("www-authenticate: bearer"), "{head}");
    let (status, _, _) = api(address, "GET", account, account, Some("forged"), "");
    assert_eq!(status, 401);
    let (status, _, _) = api(address, "POST", TOKENS, TOKENS, None, "{not json");
    assert_eq!(status, 400);

    let token = api_token(address);
    let me = Some(token.as_str());
    let (status, head, body) = api(address, "GET", account, account, me, "");
    assert_eq!(status, 200);
    assert!(head.contains("cache-control: no-store"));
    assert_eq!(
        (&body["name"], &body["card"], &body["expires"]),
        (&"Ana Silva".into(), &"000123".into(), &"2027-01-31".into())
    );
    assert!(!body.to_string().contains("529"), "no CPF: {body}");
    let debts = "/api/v1/patron/debts";
    let (_, _, lines) = api(address, "GET", debts, debts, me, "");
    assert!(
        lines
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line["kind"] == "overdue" && line["amount"] == 500),
        "{lines}"
    );

    let loans = "/api/v1/patron/loans";
    let (_, _, list) = api(address, "GET", loans, loans, me, "");
    let loan = list
        .as_array()
        .unwrap()
        .iter()
        .find(|loan| loan["renewals_left"] == 1);
    let item = loan.unwrap()["item"].as_i64().unwrap();
    let renewal = "/api/v1/patron/loans/{item}/renewal";
    let renew = |item: i64| {
        api(
            address,
            "POST",
            &format!("/api/v1/patron/loans/{item}/renewal"),
            renewal,
            me,
            "",
        )
    };
    let (status, _, renewed) = renew(item);
    assert_eq!(status, 200, "{renewed}");
    assert_eq!(renewed["renewals"], 1);
    let (status, _, refused) = renew(item);
    assert_eq!(status, 409);
    assert_eq!(
        refused,
        serde_json::json!({"error": "refused", "reason": "too_many"})
    );
    assert_eq!(renew(9999).0, 404);

    // The OPAC session and the token are apart.
    let opac = request(
        address,
        "GET",
        "/conta",
        &format!("Cookie: elzevir_patron={token}\r\n"),
        "",
    );
    assert!(opac.contains("location: /conta/entrar"), "{opac}");
    let current = "/api/v1/patron/tokens/current";
    assert_eq!(api(address, "DELETE", current, current, me, "").0, 204);
    assert_eq!(api(address, "GET", account, account, me, "").0, 401);
}

#[test]
fn the_patron_holds_and_cancels_through_the_api_and_refusals_lock_the_card() {
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
    let (_server, address) = start_with_password(data.path(), ana);
    let token = api_token(address);
    let me = Some(token.as_str());
    let holds = "/api/v1/patron/holds";
    let place = |body: &str| api(address, "POST", holds, holds, me, body);
    assert_eq!(place(r#"{"instance": 99, "pickup": 1}"#).0, 404);
    let (status, _, placed) = place(r#"{"instance": 1, "pickup": 1}"#);
    assert_eq!(status, 201, "{placed}");
    let (status, _, again) = place(r#"{"instance": 1, "pickup": 1}"#);
    assert_eq!((status, &again["reason"]), (409, &"already_held".into()));
    let (_, _, list) = api(address, "GET", holds, holds, me, "");
    assert_eq!(list[0]["status"], "pending");
    assert_eq!(list[0]["title"], "Dom Casmurro");
    let hold = "/api/v1/patron/holds/{id}";
    let cancel = |id: &str| {
        api(
            address,
            "DELETE",
            &format!("/api/v1/patron/holds/{id}"),
            hold,
            me,
            "",
        )
    };
    assert_eq!(cancel(&placed["hold"].to_string()).0, 204);
    assert_eq!(cancel(&placed["hold"].to_string()).0, 404);
    let (_, _, list) = api(address, "GET", holds, holds, me, "");
    assert_eq!(list, serde_json::json!([]));

    // Refusals through the API and the OPAC count together (item 13).
    let wrong = r#"{"card": "000123", "password": "errada-123"}"#;
    for _ in 0..5 {
        assert_eq!(api(address, "POST", TOKENS, TOKENS, None, wrong).0, 401);
    }
    let right = r#"{"card": "000123", "password": "leitura-segura"}"#;
    assert_eq!(api(address, "POST", TOKENS, TOKENS, None, right).0, 429);
    let form = request(address, "GET", "/conta/entrar", "", "");
    let cookie = session_cookie(&form, "elzevir_patron_csrf");
    let locked = request(
        address,
        "POST",
        "/conta/entrar",
        &format!("Cookie: {cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\n"),
        &format!("csrf={}&card=000123&password=leitura-segura", csrf(&form)),
    );
    assert_status(&locked, 429);
}
