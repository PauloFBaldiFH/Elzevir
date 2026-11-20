// SPDX-License-Identifier: GPL-3.0-or-later
//! The notice board, in the store and through the running server
//! (docs/specs/interface/mural.md).
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

use elzevir_app::board::{
    Audience, BoardCommand, BoardError, BoardNoteId, BoardOutput, BoardRefusal, MAX_NOTES,
    NoteContent, NoteKind, NoteText, NoticeBoard,
};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::staff::{Permission, StaffId, hash_password};
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

/// An administrator and a cataloguer; returns the administrator's id.
fn library(store: &ElzevirStore) -> StaffId {
    let mut ids = Vec::new();
    for (login, permission) in [
        ("adm", Permission::Administration),
        ("cat", Permission::Cataloguing),
    ] {
        let WriteOutput::StaffCreated(id) = submit(
            store,
            WriteCommand::Staff(StaffCommand::Create {
                login: login.to_owned(),
                password_hash: hash_password("synthetic-password").unwrap(),
                permissions: [Permission::StaffInterface, permission].into(),
            }),
        ) else {
            panic!("no account");
        };
        ids.push(id);
    }
    ids.first().copied().unwrap()
}

fn content(author: StaffId, audience: Audience, text: &str) -> NoteContent {
    NoteContent {
        kind: NoteKind::Institutional,
        audience,
        text: NoteText::new(text).unwrap(),
        author,
        on: jiff::civil::date(2026, 11, 20),
    }
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
fn the_store_checks_audience_version_and_size() {
    let data = tempfile::tempdir().unwrap();
    let store = open(data.path());
    let adm = library(&store);
    let BoardOutput::Added(id) = store
        .change(
            adm,
            BoardCommand::Add(content(adm, Audience::Public, "Feriado")),
        )
        .unwrap()
    else {
        panic!("not added");
    };
    let note = store.note(id).unwrap().unwrap();
    assert_eq!((note.author.as_str(), note.row_version), ("adm", 1));
    // A command that says the note is the staff's does not reach a public note.
    let as_staff = BoardCommand::Remove {
        id,
        was: Audience::Staff,
    };
    assert_eq!(
        store.change(adm, as_staff),
        Err(BoardError::Refused(BoardRefusal::NotFound))
    );
    let update = |was, row_version| BoardCommand::Update {
        id,
        was,
        row_version,
        content: content(adm, Audience::Staff, "Feriado na sexta"),
    };
    assert_eq!(
        store.change(adm, update(Audience::Public, 1)),
        Ok(BoardOutput::Done)
    );
    assert_eq!(
        store.change(adm, update(Audience::Staff, 1)),
        Err(BoardError::Refused(BoardRefusal::Stale))
    );
    let note = store.note(id).unwrap().unwrap();
    assert_eq!((note.audience, note.row_version), (Audience::Staff, 2));
    assert!(store.notes(Some(Audience::Public), 6).unwrap().is_empty());

    for number in 1..MAX_NOTES {
        let text = format!("Recado {number}");
        store
            .change(adm, BoardCommand::Add(content(adm, Audience::Staff, &text)))
            .unwrap();
    }
    assert_eq!(
        store.change(
            adm,
            BoardCommand::Add(content(adm, Audience::Staff, "Mais um"))
        ),
        Err(BoardError::Refused(BoardRefusal::Full))
    );
    let all = store.notes(None, MAX_NOTES).unwrap();
    assert_eq!(all.len(), MAX_NOTES);
    // Same day: the newest id first.
    assert_eq!(all[0].text, format!("Recado {}", MAX_NOTES - 1));
    assert_eq!(all.last().map(|note| note.id), Some(id));
    let missing = BoardCommand::Remove {
        id: BoardNoteId(9999),
        was: Audience::Staff,
    };
    assert_eq!(
        store.change(adm, missing),
        Err(BoardError::Refused(BoardRefusal::NotFound))
    );
}

#[test]
fn notes_are_written_on_the_staff_home_and_public_ones_reach_the_opac() {
    let data = tempfile::tempdir().unwrap();
    let store = open(data.path());
    library(&store);
    store.shutdown().unwrap();
    let (_server, address) = start(data.path());
    let adm = sign_in(address, "adm");
    let cat = sign_in(address, "cat");

    // An empty board shows the welcome notes.
    let home = request(address, "GET", "/equipe", &cat, "");
    assert!(home.contains("href=\"/equipe/mural/nova\""), "{home}");
    assert!(request(address, "GET", "/", "", "").contains("Welcome to the online catalog"));

    let form = request(address, "GET", "/equipe/mural/nova", &cat, "");
    assert_status(&form, 200);
    let token = csrf(&form).to_owned();
    let post = |who: &str, path: &str, fields: &str| {
        let headers = format!("{who}Content-Type: application/x-www-form-urlencoded\r\n");
        request(address, "POST", path, &headers, fields)
    };
    let cat_token = token.clone();
    let add = |fields: &str| post(&cat, "/equipe/mural", &format!("csrf={cat_token}&{fields}"));
    assert_status(
        &add("text=Reuni%C3%A3o+%3Cb%3E%C3%A0s+14h%3C%2Fb%3E&kind=urgent"),
        303,
    );
    assert_status(&add("text=+++&kind=staff"), 422);
    // Publishing on the OPAC needs administration.
    assert_status(&add("text=Aberto+no+s%C3%A1bado&kind=staff&public=1"), 403);
    assert_status(
        &post(&cat, "/equipe/mural", "csrf=wrong&text=x&kind=staff"),
        403,
    );

    let adm_form = request(address, "GET", "/equipe/mural/nova", &adm, "");
    let adm_token = csrf(&adm_form).to_owned();
    assert_status(
        &post(
            &adm,
            "/equipe/mural",
            &format!("csrf={adm_token}&text=Aberto+no+s%C3%A1bado&kind=institutional&public=1"),
        ),
        303,
    );

    let home = request(address, "GET", "/equipe", &cat, "");
    assert!(
        home.contains("Reunião &#60;b&#62;às 14h&#60;/b&#62;"),
        "{home}"
    );
    assert!(home.contains("Aberto no sábado"));
    assert!(!home.contains("Welcome to Elzevir"));
    // The cataloguer edits the team's note but not the public one.
    assert_eq!(home.matches("/equipe/mural/").count() - 1, 1, "{home}");
    let opac = request(address, "GET", "/", "", "");
    assert!(opac.contains("Aberto no sábado"), "{opac}");
    assert!(!opac.contains("Reunião"));
    assert!(!opac.contains(">adm<"), "the OPAC names no staff member");

    let public = opac_note_id(
        &request(address, "GET", "/equipe", &adm, ""),
        "Aberto no sábado",
    );
    assert_status(
        &request(address, "GET", &format!("/equipe/mural/{public}"), &cat, ""),
        403,
    );
    let edit = request(address, "GET", &format!("/equipe/mural/{public}"), &adm, "");
    assert_status(&edit, 200);
    assert!(edit.contains("Aberto no sábado"));
    assert_status(
        &post(
            &adm,
            &format!("/equipe/mural/{public}/remover"),
            &format!("csrf={adm_token}"),
        ),
        303,
    );
    assert!(request(address, "GET", "/", "", "").contains("Welcome to the online catalog"));
    assert_status(
        &request(address, "GET", &format!("/equipe/mural/{public}"), &adm, ""),
        404,
    );
}

/// The id of the note whose text starts with `text` on a staff home.
fn opac_note_id(home: &str, text: &str) -> i64 {
    let (_, after_note) = home.split_once(text).unwrap();
    let (_, link) = after_note.split_once("href=\"/equipe/mural/").unwrap();
    link.split_once('"').unwrap().0.parse().unwrap()
}
