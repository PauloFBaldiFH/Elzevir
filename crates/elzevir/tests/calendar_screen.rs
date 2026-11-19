// SPDX-License-Identifier: GPL-3.0-or-later
//! The calendar screen, through the running server (docs/specs/circulacao/calendario.md,
//! items 9 to 14).
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

use elzevir_app::calendar::CalendarBook;
use elzevir_app::catalog::{CatalogCommand, CatalogOutput};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_domain::calendar::{Calendar, DatedEntry};
use elzevir_domain::catalog::BranchId;
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::{Weekday, date};

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

/// An administrator, a cataloguer and two branches. Returns the branches' ids.
fn library(data: &Path) -> (i64, i64) {
    let store = open(data);
    for (login, permission) in [
        ("adm", Permission::Administration),
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
    let mut ids = Vec::new();
    for name in ["Biblioteca Central", "Biblioteca do Bairro"] {
        let WriteOutput::Catalog(CatalogOutput::Branch(branch)) = submit(
            &store,
            WriteCommand::Catalog(CatalogCommand::CreateBranch {
                name: name.to_owned(),
            }),
        ) else {
            panic!("no branch");
        };
        ids.push(branch.0);
    }
    store.shutdown().unwrap();
    let [central, bairro] = ids.as_slice() else {
        panic!("two branches");
    };
    (*central, *bairro)
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

fn location(response: &str) -> &str {
    response
        .lines()
        .find_map(|line| line.strip_prefix("location: "))
        .unwrap()
}

#[test]
fn closings_are_added_shown_removed_and_copied_on_the_screen() {
    let data = tempfile::tempdir().unwrap();
    let (central, bairro) = library(data.path());
    let (server, address) = start(data.path());
    let adm = sign_in(address, "adm");
    let cat = sign_in(address, "cat");
    assert_status(
        &request(address, "GET", "/equipe/admin/calendario", &cat, ""),
        403,
    );
    assert!(request(address, "GET", "/equipe", &adm, "").contains("/equipe/admin/calendario"));

    let month = format!("/equipe/admin/calendario?branch={central}&month=2026-11");
    let page = request(address, "GET", &month, &adm, "");
    assert_status(&page, 200);
    assert!(page.contains("November 2026"), "{page}");
    let headers = format!("{adm}Content-Type: application/x-www-form-urlencoded\r\n");
    let post = |fields: &str| {
        request(
            address,
            "POST",
            "/equipe/admin/calendario",
            &headers,
            &format!(
                "csrf={}&branch={central}&month=2026-11&{fields}",
                csrf(&page)
            ),
        )
    };
    assert_status(&post("action=weekly&weekday=0&title=Domingo"), 303);
    assert_status(
        &post("action=annual&day=25&annual_month=12&title=Natal"),
        303,
    );
    let saved = post("action=dates&first=2026-11-20&last=&title=Consci%C3%AAncia+Negra");
    assert_status(&saved, 303);
    assert!(location(&saved).contains("saved=1"), "{saved}");
    assert_status(
        &post("action=dates&first=2026-11-22&open=1&title=Feira"),
        303,
    );
    // A range that runs backwards is refused, and the page says why.
    let refused = post("action=dates&first=2026-11-30&last=2026-11-01&title=Errado");
    assert_status(&refused, 422);
    assert!(refused.contains("These dates do not make a range"));

    let page = request(address, "GET", &month, &adm, "");
    assert!(page.contains("title=\"Consciência Negra\""), "{page}");
    assert!(page.contains("calendar-exception"));
    assert!(page.contains("25 December"));
    assert_status(
        &post("action=remove_dates&first=2026-11-20&last=2026-11-20"),
        303,
    );
    assert_status(&post(&format!("action=copy&to={bairro}")), 303);
    drop(server);

    let store = open(data.path());
    assert_eq!(
        store.calendar(BranchId(central)).unwrap(),
        Calendar::always_open()
            .closed_every(Weekday::Sunday)
            .closed_annually(elzevir_domain::calendar::AnnualHoliday { month: 12, day: 25 })
            .on(date(2026, 11, 22), DatedEntry::Open)
    );
    let copied = store.calendar(BranchId(bairro)).unwrap();
    assert!(copied.is_closed(date(2026, 11, 29)) && copied.is_closed(date(2027, 12, 25)));
}
