// SPDX-License-Identifier: GPL-3.0-or-later
//! A patron's OPAC password and sign-in, through the running server
//! (docs/specs/opac/conta-do-leitor.md).
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

const FORM: &str = "Content-Type: application/x-www-form-urlencoded\r\n";

fn post(address: SocketAddr, path: &str, cookie: &str, body: &str) -> String {
    request(address, "POST", path, &format!("{cookie}{FORM}"), body)
}

fn location(response: &str) -> &str {
    response
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(": ")?;
            name.eq_ignore_ascii_case("location").then_some(value)
        })
        .unwrap()
}

fn has_header(response: &str, header: &str) -> bool {
    let head = response.split_once("\r\n\r\n").unwrap().0;
    head.lines().any(|line| line.eq_ignore_ascii_case(header))
}

/// The sign-in form's `Cookie:` line and token.
fn login_form(address: SocketAddr) -> (String, String) {
    let page = request(address, "GET", "/conta/entrar", "", "");
    assert_status(&page, 200);
    let cookie = session_cookie(&page, "elzevir_patron_csrf");
    (format!("Cookie: {cookie}\r\n"), csrf(&page).to_owned())
}

fn patron_sign_in(address: SocketAddr, card: &str, password: &str) -> String {
    let (cookie, token) = login_form(address);
    post(
        address,
        "/conta/entrar",
        &cookie,
        &format!("csrf={token}&card={card}&password={password}"),
    )
}

#[test]
fn the_staff_sets_and_takes_away_a_password() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    let (_server, address) = start(data.path());
    let lei = sign_in(address, "lei");
    let cat = sign_in(address, "cat");
    let edit = format!("/equipe/leitores/{ana}/editar");
    let password_path = format!("/equipe/leitores/{ana}/senha");

    let form = request(address, "GET", &edit, &lei, "");
    assert!(form.contains("This patron has no password"), "{form}");
    let token = csrf(&form).to_owned();
    // Without a password there is no account.
    assert_status(&patron_sign_in(address, "000123", "leitura-segura"), 401);

    let set = |who: &str, password: &str| {
        post(
            address,
            &password_path,
            who,
            &format!("csrf={token}&password={password}"),
        )
    };
    // Only who manages patrons sets a password (checked before the form token).
    assert_status(
        &post(
            address,
            &password_path,
            &cat,
            "csrf=x&password=leitura-segura",
        ),
        403,
    );
    let short = set(&lei, "curta");
    assert_status(&short, 422);
    assert!(short.contains("at least 8 characters"), "{short}");
    let cpf = set(&lei, "52998224725");
    assert_status(&cpf, 422);
    assert!(cpf.contains("cannot be the card number or the CPF"));
    assert_status(&set(&lei, "leitura-segura"), 303);
    let form = request(address, "GET", &format!("{edit}?senha=1"), &lei, "");
    assert!(form.contains("The OPAC password was saved."), "{form}");
    assert!(
        !form.contains("leitura-segura"),
        "the password is never shown"
    );

    // Taking the password away closes the account.
    assert_status(
        &post(
            address,
            &password_path,
            &lei,
            &format!("csrf={token}&remove=1"),
        ),
        303,
    );
    assert_status(&patron_sign_in(address, "000123", "leitura-segura"), 401);
}

#[test]
fn the_patron_signs_in_sees_the_account_and_signs_out() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    let (_server, address) = start(data.path());
    let lei = sign_in(address, "lei");
    give_password(address, &lei, ana);

    // A visitor is sent to the sign-in form.
    assert_eq!(
        location(&request(address, "GET", "/conta", "", "")),
        "/conta/entrar"
    );
    let wrong = patron_sign_in(address, "000123", "outra-senha");
    assert_status(&wrong, 401);
    assert!(
        wrong.contains("Card number or password is wrong."),
        "{wrong}"
    );
    assert!(wrong.contains("value=\"000123\""), "the card is kept");
    let unknown = patron_sign_in(address, "999999", "leitura-segura");
    assert_status(&unknown, 401);
    assert!(unknown.contains("Card number or password is wrong."));

    // Spaces around the card number do not matter.
    let signed = patron_sign_in(address, "+000123+", "leitura-segura");
    assert_status(&signed, 303);
    assert_eq!(location(&signed), "/conta");
    let patron = format!("Cookie: {}\r\n", session_cookie(&signed, "elzevir_patron"));
    let account = request(address, "GET", "/conta", &patron, "");
    assert_status(&account, 200);
    assert!(has_header(&account, "cache-control: no-store"), "{account}");
    for fact in [
        "Ana Silva",
        "000123",
        "Biblioteca Central",
        "Estudante",
        "31/01/2027",
    ] {
        assert!(account.contains(fact), "{fact}: {account}");
    }
    assert!(!account.contains("529.982"), "no CPF on the patron's page");
    let home = request(address, "GET", "/", &patron, "");
    assert!(home.contains("Hello, Ana!"), "{home}");
    assert!(has_header(&home, "cache-control: no-store"));
    assert_eq!(
        location(&request(address, "GET", "/conta/entrar", &patron, "")),
        "/conta"
    );

    // The two sessions do not open each other's pages.
    assert_eq!(
        location(&request(address, "GET", "/equipe", &patron, "")),
        "/equipe/login"
    );
    assert_eq!(
        location(&request(address, "GET", "/conta", &lei, "")),
        "/conta/entrar"
    );

    assert_status(&post(address, "/conta/sair", &patron, "csrf=wrong"), 403);
    let out = post(
        address,
        "/conta/sair",
        &patron,
        &format!("csrf={}", csrf(&account)),
    );
    assert_status(&out, 303);
    assert!(
        out.to_ascii_lowercase()
            .contains("set-cookie: elzevir_patron=;")
    );
    assert_eq!(
        location(&request(address, "GET", "/conta", &patron, "")),
        "/conta/entrar"
    );
}

/// Sets Ana's password to `leitura-segura` as `staff`.
fn give_password(address: SocketAddr, staff: &str, ana: i64) {
    let form = request(
        address,
        "GET",
        &format!("/equipe/leitores/{ana}/editar"),
        staff,
        "",
    );
    assert_status(
        &post(
            address,
            &format!("/equipe/leitores/{ana}/senha"),
            staff,
            &format!("csrf={}&password=leitura-segura", csrf(&form)),
        ),
        303,
    );
}

#[test]
fn five_refusals_lock_the_card() {
    let data = tempfile::tempdir().unwrap();
    let ana = library(data.path());
    let (_server, address) = start(data.path());
    let lei = sign_in(address, "lei");
    give_password(address, &lei, ana);
    let (cookie, token) = login_form(address);
    let attempt = |password: &str| {
        post(
            address,
            "/conta/entrar",
            &cookie,
            &format!("csrf={token}&card=000123&password={password}"),
        )
    };
    for _ in 0..5 {
        assert_status(&attempt("errada-123"), 401);
    }
    let locked = attempt("leitura-segura");
    assert_status(&locked, 429);
    assert!(locked.contains("Too many attempts"), "{locked}");
    // A forged form is refused before anything is counted.
    assert_status(
        &post(
            address,
            "/conta/entrar",
            &cookie,
            "csrf=wrong&card=000123&password=x",
        ),
        403,
    );
}
