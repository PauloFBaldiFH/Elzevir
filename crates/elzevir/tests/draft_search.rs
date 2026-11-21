// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir draft-search` against a local stand-in SRU server
//! (docs/specs/catalogo/fontes-de-rascunho.md, items 11 to 23).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

const TWO_RECORDS: &str = include_str!("../../elzevir-sources/tests/data/sru-1.2-two-records.xml");

/// Answers every request with the recorded response, and keeps the request lines.
fn answer(stream: TcpStream, seen: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line.trim_end().is_empty() {
            break;
        }
    }
    seen.lock()
        .unwrap()
        .push(request_line.trim_end().to_owned());
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{TWO_RECORDS}",
        TWO_RECORDS.len()
    )
    .unwrap();
}

/// Starts the stand-in on a free local port: its URL and the requests it saw.
fn sru() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/sru", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            answer(stream, &kept);
        }
    });
    (url, seen)
}

fn draft_search(data: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["draft-search", "--data-dir", data])
        .args(args)
        // The stand-in is local: no proxy between the two.
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_configured_servers_answer_from_the_command_line() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_str().unwrap();
    let none = draft_search(data, &["--isbn", "9780000000002"]);
    assert!(!none.status.success());
    assert!(text(&none.stderr).contains("no draft source is configured"));

    let (url, seen) = sru();
    // A closed local port answers nothing.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_url = format!("http://{}/sru", closed.local_addr().unwrap());
    drop(closed);
    std::fs::write(
        dir.path().join("elzevir.toml"),
        format!(
            "[[draft_sources]]\nname = \"Local\"\nkind = \"sru\"\nurl = \"{url}\"\n\
             [draft_sources.fields]\nisbn = \"bath.isbn\"\n\
             [[draft_sources]]\nname = \"Fechada\"\nkind = \"sru\"\nurl = \"{closed_url}\"\n\
             timeout_seconds = 2\n"
        ),
    )
    .unwrap();
    let empty = draft_search(data, &["--title", "  "]);
    assert!(text(&empty.stderr).contains("fill in at least one field"));

    let found = draft_search(data, &["--isbn", "978-0-00-000000-2"]);
    assert!(found.status.success(), "{}", text(&found.stderr));
    assert_eq!(
        text(&found.stdout),
        "Local: 37 found\n  \
         O rio das águas claras : romance | Lemos, Teodora, 1950- | 2. ed | Curitiba · Editora Exemplo · 2020\n    \
         ISBN 9780000000002\n  \
         Contos do interior \"reunidos\"\n  \
         not readable: 1\n\
         Fechada: no response\n"
    );
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [
            "GET /sru?operation=searchRetrieve&version=1.2&query=bath.isbn%3D%229780000000002%22\
          &startRecord=1&maximumRecords=20&recordSchema=marcxml&recordPacking=xml HTTP/1.1"
        ]
    );
}
