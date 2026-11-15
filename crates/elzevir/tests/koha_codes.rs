// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir koha-codes` against a local stand-in for Koha
//! (docs/specs/catalogo/tabelas-de-codigos.md, items 6 to 13).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};

const LIBRARIES: &str = include_str!("../../elzevir-app/tests/data/koha-libraries.json");
const ITEM_TYPES: &str = include_str!("../../elzevir-app/tests/data/koha-item-types.json");
const LOCATIONS: &str = include_str!("../../elzevir-app/tests/data/koha-loc.json");
const COLLECTIONS: &str = include_str!("../../elzevir-app/tests/data/koha-ccode.json");

const LOGIN_PAGE: &str = r#"<form><input type="hidden" name="csrf_token" value="tok-123"></form>"#;
const HOME_PAGE: &str = r#"<a href="/cgi-bin/koha/mainpage.pl?logout.x=1">Log out</a>"#;

/// Answers one request as Koha would: the sign-in form with a CSRF token, the sign-in
/// itself (user `cat`, password `right pass&word`), and the four lists for the session.
fn answer(stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut length = 0;
    let mut session = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end().to_ascii_lowercase();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
        if line.starts_with("cookie:") && line.contains("cgisessid=signed") {
            session = true;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body = String::from_utf8(body).unwrap();
    let mut parts = request_line.split(' ');
    let (method, path) = (parts.next().unwrap(), parts.next().unwrap());
    let (status, cookie, text) = match (method, path) {
        ("GET", "/cgi-bin/koha/mainpage.pl") => ("200 OK", Some("anon"), LOGIN_PAGE),
        ("POST", "/cgi-bin/koha/mainpage.pl") => {
            let expected = "login_op=cud-login&csrf_token=tok-123&login_userid=cat\
                            &login_password=right+pass%26word";
            if body == expected {
                ("200 OK", Some("signed"), HOME_PAGE)
            } else {
                ("200 OK", None, LOGIN_PAGE)
            }
        }
        ("GET", path) if !session && path.starts_with("/api/") => (
            "401 Unauthorized",
            None,
            r#"{"error":"Authentication failure."}"#,
        ),
        ("GET", "/api/v1/libraries?_per_page=-1") => ("200 OK", None, LIBRARIES),
        ("GET", "/api/v1/item_types?_per_page=-1") => ("200 OK", None, ITEM_TYPES),
        ("GET", "/api/v1/authorised_value_categories/LOC/authorised_values?_per_page=-1") => {
            ("200 OK", None, LOCATIONS)
        }
        ("GET", "/api/v1/authorised_value_categories/CCODE/authorised_values?_per_page=-1") => {
            ("200 OK", None, COLLECTIONS)
        }
        _ => ("404 Not Found", None, "not found"),
    };
    let cookie = cookie.map_or_else(String::new, |value| {
        format!("Set-Cookie: CGISESSID={value}; path=/; HttpOnly\r\n")
    });
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
    .unwrap();
}

/// Starts the stand-in on a free local port and returns its URL.
fn koha() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            answer(stream);
        }
    });
    url
}

fn koha_codes(data: &str, url: &str, password: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_elzevir"));
    command
        .args([
            "koha-codes",
            "--data-dir",
            data,
            "--url",
            url,
            "--user",
            "cat",
        ])
        .env_remove("ELZEVIR_KOHA_PASSWORD")
        .env_remove("ELZEVIR_KOHA_BASIC_USER")
        .env_remove("ELZEVIR_KOHA_BASIC_PASSWORD")
        // The stand-in is local: no proxy between the two.
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");
    if let Some(password) = password {
        command.env("ELZEVIR_KOHA_PASSWORD", password);
    }
    command.output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_names_of_a_koha_are_stored_once() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let data = data.to_str().unwrap();
    let url = koha();

    let first = koha_codes(data, &url, Some("right pass&word"));
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert_eq!(
        text(&first.stdout),
        "branches: 1 read, 1 created, 0 renamed, 0 unchanged, 0 not used yet\n\
         item types: 8 read, 8 created, 0 renamed, 0 unchanged, 0 not used yet\n\
         locations: 5 read, 0 created, 0 renamed, 0 unchanged, 5 not used yet\n\
         collections: 3 read, 0 created, 0 renamed, 0 unchanged, 3 not used yet\n"
    );

    let again = koha_codes(data, &url, Some("right pass&word"));
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert!(
        text(&again.stdout).contains("item types: 8 read, 0 created, 0 renamed, 8 unchanged"),
        "{}",
        text(&again.stdout)
    );
}

#[test]
fn a_refused_sign_in_or_a_missing_password_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let data = data.to_str().unwrap();
    let url = koha();

    let wrong = koha_codes(data, &url, Some("wrong"));
    assert_eq!(wrong.status.code(), Some(1));
    assert!(text(&wrong.stderr).contains("Koha refused the sign-in of cat"));
    assert!(text(&wrong.stdout).is_empty());

    let missing = koha_codes(data, &url, None);
    assert_eq!(missing.status.code(), Some(1));
    assert!(text(&missing.stderr).contains("ELZEVIR_KOHA_PASSWORD"));

    // Nothing was stored: the right password still creates every code.
    let right = koha_codes(data, &url, Some("right pass&word"));
    assert!(text(&right.stdout).contains("branches: 1 read, 1 created"));
}
