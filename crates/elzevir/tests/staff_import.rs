// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff import route uses the running institution and its existing writer.
#![expect(
    clippy::unwrap_used,
    reason = "integration helpers fail the test on error"
)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const IMPORT: &str = "/equipe/acervo/importar";

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

#[test]
fn staff_import_requires_a_session() {
    let data = tempfile::tempdir().unwrap();
    let (_server, address) = start(data.path());
    let page = request(address, "GET", IMPORT, "", "");
    assert!(page.starts_with("HTTP/1.0 303"), "{page}");
    assert!(page.contains("location: /equipe/login"), "{page}");
}

fn account(data: &Path) -> elzevir_app::staff::StaffId {
    use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
    use elzevir_app::staff::{Permission, hash_password};
    use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

    let store = ElzevirStore::open(
        &data.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let mut users = Vec::new();
    for (login, can_catalogue) in [("ana", true), ("bia", true), ("leo", false)] {
        let mut permissions = std::collections::BTreeSet::from([Permission::StaffInterface]);
        if can_catalogue {
            permissions.insert(Permission::Cataloguing);
        }
        let created = store
            .submit(WriteCommand::Staff(StaffCommand::Create {
                login: login.to_owned(),
                password_hash: hash_password("synthetic-password").unwrap(),
                permissions,
            }))
            .unwrap()
            .wait()
            .unwrap();
        if let WriteOutput::StaffCreated(user) = created {
            users.push(user);
        }
    }
    store.shutdown().unwrap();
    assert_eq!(users.len(), 3);
    *users.first().unwrap()
}

fn header<'a>(response: &'a str, name: &str) -> &'a str {
    response
        .split_once("\r\n\r\n")
        .unwrap()
        .0
        .lines()
        .filter_map(|line| line.split_once(": "))
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .unwrap()
        .1
}

fn csrf(page: &str) -> &str {
    page.split_once("name=\"csrf\" value=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}

fn sign_in(address: SocketAddr, login: &str) -> String {
    let page = request(address, "GET", "/equipe/login", "", "");
    let cookie = header(&page, "set-cookie").split_once(';').unwrap().0;
    let headers =
        format!("Cookie: {cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\n");
    let login = request(
        address,
        "POST",
        "/equipe/login",
        &headers,
        &format!(
            "login={login}&password=synthetic-password&csrf={}",
            csrf(&page)
        ),
    );
    assert_status(&login, 303);
    let header = login
        .lines()
        .find(|line| line.starts_with("set-cookie: elzevir_session="))
        .unwrap();
    header
        .split_once(": ")
        .unwrap()
        .1
        .split_once(';')
        .unwrap()
        .0
        .to_owned()
}

fn assert_status(response: &str, expected: u16) {
    assert!(
        response.starts_with(&format!("HTTP/1.0 {expected} ")),
        "{response}"
    );
}

fn snapshot(response: &str, token: &str, id: &str, name: &str, expected: &str) {
    let mut html = response
        .split_once("\r\n\r\n")
        .unwrap()
        .1
        .replace(token, "SESSION-CSRF");
    if !id.is_empty() {
        html = html.replace(id, "BATCH-ID");
    }
    html = html
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    html.push('\n');
    if std::env::var_os("UPDATE_STAFF_IMPORT_SNAPSHOTS").is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(name);
        std::fs::write(path, html).unwrap();
    } else {
        assert_eq!(html, expected, "{name}");
    }
}

fn upload(address: SocketAddr, cookie: &str, token: &str, marc: &str, duplicate: bool) -> String {
    let headers = format!(
        "Cookie: {cookie}\r\nContent-Type: multipart/form-data; boundary=elzevir-test-boundary\r\n",
    );
    let part = format!(
        "--elzevir-test-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../../catalog.mrk\"\r\nContent-Type: text/plain\r\n\r\n{marc}\r\n",
    );
    request(
        address,
        "POST",
        IMPORT,
        &headers,
        &format!(
            "--elzevir-test-boundary\r\nContent-Disposition: form-data; name=\"csrf\"\r\n\r\n{token}\r\n{part}{}--elzevir-test-boundary--\r\n",
            if duplicate { &part } else { "" },
        ),
    )
}

#[test]
fn selecting_a_marc_reader_does_not_override_container_evidence() {
    use elzevir_app::import_detection::{ImportFormat, detect_format, select_marc};

    for prefix in [b"PK\x03\x04".as_slice(), b"\x1f\x8b\x08".as_slice()] {
        let detected = detect_format(prefix);
        assert!(select_marc(detected, Some(ImportFormat::MarcText)).is_none());
    }
}

fn form(address: SocketAddr, url: &str, cookie: &str, data: &str) -> String {
    request(
        address,
        "POST",
        url,
        &format!("Cookie: {cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\n"),
        data,
    )
}

fn records(data: &Path) -> Vec<Option<i64>> {
    let store = elzevir_store::ElzevirStore::open(
        &data.join("elzevir.db"),
        &elzevir_store::StoreConfig::default(),
        elzevir_store::ElzevirHandler,
    )
    .unwrap();
    let actors = store
        .readers()
        .read(|connection| {
            let mut query = connection
                .prepare("SELECT actor FROM audit_log WHERE kind = 'catalog.add_record'")?;
            query
                .query_map([], |row| row.get(0))?
                .collect::<Result<_, _>>()
        })
        .unwrap();
    store.shutdown().unwrap();
    actors
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one batch crosses the full authenticated workflow"
)]
fn staff_import_reviews_private_revisions_and_commits_with_the_session_actor() {
    const SOURCE: &str = concat!(
        "=LDR  00000nam a2200000 a 4500\n=245  10$aBefore <script>fictitious()</script>\n\n",
        "=LDR  00000nam a2200000 a 4500\n=245  10$aSecond synthetic record\n\n",
        "=LDR  00000nam a2200000 a 4500\n=245  10$aThird synthetic record\n\n",
        "=LDR  00000nam a2200000 a 4500\n=245  10$aFourth synthetic record\n",
    );
    let data = tempfile::tempdir().unwrap();
    let operator = account(data.path());
    let (server, address) = start(data.path());
    let (ana, bia, leo) = (
        sign_in(address, "ana"),
        sign_in(address, "bia"),
        sign_in(address, "leo"),
    );
    let get = |url: &str, cookie: &str| {
        request(address, "GET", url, &format!("Cookie: {cookie}\r\n"), "")
    };
    assert_status(&get(IMPORT, &leo), 403);
    let page = request(
        address,
        "GET",
        IMPORT,
        &format!("Cookie: {ana}\r\nAccept-Language: pt-BR,pt;q=0.9\r\n"),
        "",
    );
    assert_status(&page, 200);
    assert!(page.contains("lang=\"pt-BR\"") && page.contains("Arquivo de origem"));
    assert_eq!(header(&page, "cache-control"), "no-store");
    let token = csrf(&page);
    snapshot(
        &page,
        token,
        "",
        "staff-import-home-pt-br.html",
        include_str!("snapshots/staff-import-home-pt-br.html"),
    );

    assert_status(&upload(address, &leo, token, SOURCE, false), 403);
    assert_status(&upload(address, &ana, "invalid", SOURCE, false), 403);
    assert_status(&upload(address, &ana, token, SOURCE, true), 422);
    assert_eq!(
        std::fs::read_dir(data.path().join("imports"))
            .unwrap()
            .count(),
        1
    );
    let created = upload(address, &ana, token, SOURCE, false);
    assert_status(&created, 303);
    let url = header(&created, "location");
    let id = url.rsplit('/').next().unwrap();
    assert!(get(IMPORT, &ana).contains(id));
    assert!(!get(IMPORT, &bia).contains(id));
    assert_status(&get(url, &bia), 404);
    let cancel_other = format!("csrf={}&revision=1", csrf(&get(IMPORT, &bia)));
    assert_status(
        &form(address, &format!("{url}/cancelar"), &bia, &cancel_other),
        404,
    );
    assert_status(
        &form(
            address,
            &format!("{url}/confirmar"),
            &ana,
            "csrf=invalid&revision=1",
        ),
        403,
    );
    let one = format!("csrf={token}&revision=1");
    assert_status(&form(address, &format!("{url}/confirmar"), &ana, &one), 409);
    let original = data.path().join("imports").join(id).join("original");
    assert_eq!(std::fs::read_to_string(&original).unwrap(), SOURCE);

    let recipe = r#"{"edits":[{"record":1,"tag":"245","change":{"op":"replace_subfield","code":"a","value":"Revised title"}}]}"#;
    let invalid = form(
        address,
        &format!("{url}/editar"),
        &ana,
        &format!("{one}&recipe=bad-syntax"),
    );
    assert_status(&invalid, 422);
    assert!(invalid.contains(">bad-syntax</textarea>"));
    let wrong_format = format!("{one}&format=xml&recipe=%7B%7D");
    assert_status(
        &form(address, &format!("{url}/editar"), &ana, &wrong_format),
        303,
    );
    let two = format!("csrf={token}&revision=2");
    assert_status(&form(address, &format!("{url}/prever"), &ana, &two), 422);
    let edit = format!(
        "{two}&format=mrk&recipe={}",
        elzevir_web_templates::url_encode(recipe)
    );
    assert_status(&form(address, &format!("{url}/editar"), &ana, &edit), 303);
    assert!(!get("/equipe/acervo", &ana).contains("Revised title"));

    let cli = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["import-batch", "--data-dir"])
        .arg(data.path())
        .args(["--operator", &operator.0.to_string(), "preview", id])
        .output()
        .unwrap();
    assert!(cli.status.success(), "{cli:?}");
    let cli = String::from_utf8(cli.stdout).unwrap();
    assert!(cli.contains("Revised title") && cli.contains("4 original records"));
    assert_status(&form(address, &format!("{url}/confirmar"), &ana, &one), 409);

    let current = format!("csrf={token}&revision=3");
    let preview = form(address, &format!("{url}/prever"), &ana, &current);
    assert_status(&preview, 200);
    assert!(preview.contains("1 edited") && preview.contains("Revised title"));
    assert!(!preview.contains("<script>fictitious()") && preview.contains("fictitious()"));
    assert!(preview.contains("Next page") && !preview.contains("Fourth synthetic record"));
    snapshot(
        &preview,
        token,
        id,
        "staff-import-review-en.html",
        include_str!("snapshots/staff-import-review-en.html"),
    );
    let later = get(&format!("{url}?from=3"), &ana);
    assert_status(&later, 200);
    assert!(later.contains("Fourth synthetic record"));
    assert!(!get("/equipe/acervo", &ana).contains("Revised title"));

    let confirmed = form(address, &format!("{url}/confirmar"), &ana, &current);
    assert_status(&confirmed, 303);
    let result = header(&confirmed, "location");
    for _ in 0..2 {
        let page = get(result, &ana);
        assert_status(&page, 200);
        assert!(page.contains("saved edits were removed"));
    }
    assert_status(&get(result, &bia), 404);
    let download = get(&format!("{result}?download=true"), &ana);
    assert_status(&download, 200);
    let report: serde_json::Value =
        serde_json::from_str(download.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(report["receipt"]["stored"], 4);
    assert_eq!(report["removed"], true);
    assert!(!download.contains("Revised title"));
    assert!(!original.exists());
    assert_status(
        &form(address, &format!("{url}/confirmar"), &ana, &current),
        404,
    );
    drop(server);
    assert_eq!(records(data.path()), vec![Some(operator.0); 4]);
}

#[test]
fn staff_import_rules_form_edits_the_saved_recipe() {
    const SOURCE: &str = concat!(
        "=LDR  00000nam a2200000 a 4500\n=245  10$aFirst synthetic record\n\n",
        "=LDR  00000nam a2200000 a 4500\n=245  10$aSecond synthetic record\n",
    );
    let data = tempfile::tempdir().unwrap();
    account(data.path());
    let (server, address) = start(data.path());
    let (ana, bia) = (sign_in(address, "ana"), sign_in(address, "bia"));
    let get = |url: &str, cookie: &str| {
        request(address, "GET", url, &format!("Cookie: {cookie}\r\n"), "")
    };
    let token = csrf(&get(IMPORT, &ana)).to_owned();
    let created = upload(address, &ana, &token, SOURCE, false);
    assert_status(&created, 303);
    let url = header(&created, "location").to_owned();
    let rule = format!("{url}/regra");
    let post = |cookie: &str, body: &str| form(address, &rule, cookie, body);

    let add =
        "action=add&record=&tag=500&op=add_field&indicators=%23%23&subfields=%24aDonated+copy";
    assert_status(&post(&ana, &format!("csrf=invalid&revision=1&{add}")), 403);
    let other = csrf(&get(IMPORT, &bia)).to_owned();
    assert_status(&post(&bia, &format!("csrf={other}&revision=1&{add}")), 404);
    let refused = post(
        &ana,
        &format!("csrf={token}&revision=1&action=add&tag=24&op=remove_field"),
    );
    assert_status(&refused, 422);
    assert!(refused.contains("three digits"));

    assert_status(&post(&ana, &format!("csrf={token}&revision=1&{add}")), 303);
    let page = get(&url, &ana);
    assert!(page.contains("Every record") && page.contains("$aDonated copy"));
    assert_status(&post(&ana, &format!("csrf={token}&revision=1&{add}")), 409);

    let exclude = format!("csrf={token}&revision=2&action=exclude&record=2");
    assert_status(&post(&ana, &exclude), 303);
    let matching = format!(
        "csrf={token}&revision=3&action=matching&rule=isbn&on_match=replace&on_no_match=create&items=new"
    );
    assert_status(&post(&ana, &matching), 303);
    let page = get(&url, &ana);
    assert!(page.contains("Include again"));
    assert!(page.contains(r#"<option value="isbn" selected>"#));

    let remove = format!("csrf={token}&revision=4&action=remove&index=0");
    assert_status(&post(&ana, &remove), 303);
    let page = get(&url, &ana);
    assert!(page.contains("No edit rules yet") && !page.contains("$aDonated copy"));
    let preview = form(
        address,
        &format!("{url}/prever"),
        &ana,
        &format!("csrf={token}&revision=5"),
    );
    assert_status(&preview, 200);
    assert!(preview.contains("First synthetic record"));
    assert!(!preview.contains("Second synthetic record"));
    drop(server);
}
