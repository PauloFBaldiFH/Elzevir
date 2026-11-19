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

use elzevir_app::catalog::{CatalogCommand, CatalogOutput};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_domain::patron::{Cpf, Enrolment};
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
