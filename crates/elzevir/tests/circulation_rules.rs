// SPDX-License-Identifier: GPL-3.0-or-later
//! The circulation rules and notices screens, through the running server
//! (docs/specs/circulacao/regras.md, items 8 to 13; avisos.md, items 28 to 30).
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
use elzevir_app::circulation_rules::{CirculationCommand, RuleChange};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::patrons::{CategoryChange, PatronCommand, PatronOutput};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::patron::Enrolment;
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

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

/// An administrator, a cataloguer, a branch, a policy, a category and a loan period of 21
/// days for any scope. Returns the ids of the branch, the category and the policy.
fn library(data: &Path) -> (i64, i64, i64) {
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
    let WriteOutput::Catalog(CatalogOutput::Branch(branch)) = submit(
        &store,
        WriteCommand::Catalog(CatalogCommand::CreateBranch {
            name: "Biblioteca Central".to_owned(),
        }),
    ) else {
        panic!("no branch");
    };
    let WriteOutput::Catalog(CatalogOutput::Policy(policy)) = submit(
        &store,
        WriteCommand::Catalog(CatalogCommand::CreatePolicy {
            name: "Empréstimo curto".to_owned(),
        }),
    ) else {
        panic!("no policy");
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
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![("issuelength".to_owned(), Some("21".to_owned()))],
        })),
    );
    store.shutdown().unwrap();
    (branch.0, category.0, policy.0)
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
fn rules_are_changed_by_scope_and_say_where_their_value_comes_from() {
    let data = tempfile::tempdir().unwrap();
    let (branch, _category, policy) = library(data.path());
    let (server, address) = start(data.path());
    let adm = sign_in(address, "adm");
    let cat = sign_in(address, "cat");
    assert_status(
        &request(address, "GET", "/equipe/admin/regras", &cat, ""),
        403,
    );
    assert!(!request(address, "GET", "/equipe", &cat, "").contains("/equipe/admin/regras"));
    assert!(request(address, "GET", "/equipe", &adm, "").contains("/equipe/admin/regras"));

    // The branch inherits the loan period of any scope, and says so.
    let scope = format!("/equipe/admin/regras?branch={branch}&category=0&policy=0");
    let page = request(address, "GET", &scope, &adm, "");
    assert_status(&page, 200);
    assert!(
        page.contains("<strong>21</strong> <span class=\"form-hint\">from any · any · any</span>"),
        "{page}"
    );

    let form = |fields: &str| {
        format!(
            "csrf={}&branch={branch}&category=0&policy=0&{fields}",
            csrf(&page)
        )
    };
    let headers = format!("{adm}Content-Type: application/x-www-form-urlencoded\r\n");
    let saved = request(
        address,
        "POST",
        "/equipe/admin/regras",
        &headers,
        &form("v.issuelength=14&v.fine=0.50&v.finedays=&b.finedays=1&v.renewalsallowed="),
    );
    assert_status(&saved, 303);
    assert!(location(&saved).contains("saved=1"), "{saved}");
    let page = request(address, "GET", location(&saved), &adm, "");
    assert!(page.contains("<strong>14</strong> <span class=\"form-hint\">this scope</span>"));
    // The empty suspension is kept on purpose, and shown as such.
    assert!(page.contains("<strong>empty</strong> <span class=\"form-hint\">this scope</span>"));
    assert!(page.contains("name=\"b.finedays\" value=\"1\" checked"));
    // Overview: any scope with one rule, the branch with three.
    assert!(
        page.contains(">1 rule<") && page.contains(">3 rules<"),
        "{page}"
    );

    // A refused value keeps what was typed and stores nothing.
    let refused = request(
        address,
        "POST",
        "/equipe/admin/regras",
        &headers,
        &form("v.issuelength=7&v.fine=1%2C50"),
    );
    assert_status(&refused, 422);
    assert!(refused.contains("Fine amount: write an amount with digits and a dot, such as 1.50."));
    assert!(refused.contains("value=\"1,50\""));
    let page = request(address, "GET", &scope, &adm, "");
    assert!(page.contains("<strong>14</strong>") && !page.contains("<strong>7</strong>"));

    // With a policy chosen, a rule set only per branch and category has no field.
    let by_policy = format!("/equipe/admin/regras?branch=0&category=0&policy={policy}");
    let page = request(address, "GET", &by_policy, &adm, "");
    assert!(page.contains("Not set per circulation policy."));
    assert!(!page.contains("name=\"v.max_holds\""));
    drop(server);

    // Only what changed went to the writer, as one command.
    let store = open(data.path());
    let changes: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT count(*) FROM audit_log WHERE kind = 'circ.set_rules'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    // One from the setup, one from the screen.
    assert_eq!(changes, 2);
}

fn count(data: &Path, sql: &str) -> i64 {
    open(data)
        .readers()
        .read(|c| c.query_row(sql, [], |row| row.get(0)))
        .unwrap()
}

#[test]
fn notice_rules_and_hold_notices_are_set_on_their_screen() {
    let data = tempfile::tempdir().unwrap();
    let (branch, category, _policy) = library(data.path());
    let (server, address) = start(data.path());
    let adm = sign_in(address, "adm");
    let cat = sign_in(address, "cat");
    assert_status(
        &request(address, "GET", "/equipe/admin/avisos", &cat, ""),
        403,
    );
    assert!(request(address, "GET", "/equipe", &adm, "").contains("/equipe/admin/avisos"));

    let page = request(address, "GET", "/equipe/admin/avisos", &adm, "");
    assert_status(&page, 200);
    assert!(page.contains("No overdue notice rule yet"), "{page}");
    let headers = format!("{adm}Content-Type: application/x-www-form-urlencoded\r\n");
    let token = csrf(&page).to_owned();
    let post = |path: &str, fields: &str| {
        request(
            address,
            "POST",
            path,
            &headers,
            &format!("csrf={token}&{fields}"),
        )
    };

    // Days that are not a number come back as typed, and nothing is stored.
    let refused = post(
        "/equipe/admin/avisos",
        &format!("branch={branch}&category={category}&d1=tr%C3%AAs&e1=1&d2=&d3="),
    );
    assert_status(&refused, 422);
    assert!(refused.contains("Days overdue are a whole number above zero."));
    assert!(refused.contains("value=\"três\""));
    // Levels out of order are refused by the command, also without storing.
    let refused = post(
        "/equipe/admin/avisos",
        &format!("branch={branch}&category={category}&d1=10&d2=5&d3="),
    );
    assert_status(&refused, 422);
    assert!(refused.contains("after the level before it"), "{refused}");

    let saved = post(
        "/equipe/admin/avisos",
        &format!("branch={branch}&category={category}&d1=3&e1=1&d2=&d3=10&p3=1&r3=1"),
    );
    assert_status(&saved, 303);
    let page = request(address, "GET", location(&saved), &adm, "");
    assert!(
        page.contains("Level 1: 3 days, E-mail<br>Level 3: 10 days, Print, Restricts the patron")
    );
    assert!(page.contains("name=\"d3\" value=\"10\""));
    assert!(page.contains("name=\"r3\" value=\"1\" checked"));

    // Only the category that changed goes to the writer.
    let saved = post("/equipe/admin/avisos/reservas", &format!("p.{category}=1"));
    assert_status(&saved, 303);
    let page = request(address, "GET", location(&saved), &adm, "");
    assert!(page.contains(&format!("name=\"p.{category}\" value=\"1\" checked")));
    assert!(!page.contains(&format!("name=\"e.{category}\" value=\"1\" checked")));
    assert_status(
        &post("/equipe/admin/avisos/reservas", &format!("p.{category}=1")),
        303,
    );
    drop(server);

    assert_eq!(
        count(
            data.path(),
            "SELECT count(*) FROM overdue_notice_rules WHERE restricts"
        ),
        1
    );
    assert_eq!(
        count(
            data.path(),
            "SELECT count(*) FROM audit_log WHERE kind = 'circ.set_hold_notice'"
        ),
        1
    );
    assert_eq!(
        count(
            data.path(),
            "SELECT count(*) FROM audit_log WHERE kind = 'circ.set_overdue_notice_rules'"
        ),
        1
    );
}
