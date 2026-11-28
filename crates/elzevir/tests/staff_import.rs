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
        if login == "bia" {
            permissions.extend([Permission::Patrons, Permission::Circulation]);
        }
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

#[track_caller]
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
    upload_to(address, cookie, token, marc, duplicate, IMPORT)
}

fn upload_to(
    address: SocketAddr,
    cookie: &str,
    token: &str,
    marc: &str,
    duplicate: bool,
    route: &str,
) -> String {
    let headers = format!(
        "Cookie: {cookie}\r\nContent-Type: multipart/form-data; boundary=elzevir-test-boundary\r\n",
    );
    let part = format!(
        "--elzevir-test-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../../catalog.mrk\"\r\nContent-Type: text/plain\r\n\r\n{marc}\r\n",
    );
    request(
        address,
        "POST",
        route,
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
        .args(["import", "preview", "--data-dir"])
        .arg(data.path())
        .args(["--operator", &operator.0.to_string(), id])
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

#[test]
fn staff_import_maps_custom_spreadsheet_columns_by_form() {
    const SOURCE: &str = "Título;Edição;Observação\nPrimeiro livro fictício;1. ed.;sem uso\nSegundo livro fictício;2. ed. revista;sem uso\n";
    let data = tempfile::tempdir().unwrap();
    account(data.path());
    let (server, address) = start(data.path());
    let ana = sign_in(address, "ana");
    let get = |url: &str| request(address, "GET", url, &format!("Cookie: {ana}\r\n"), "");
    let token = csrf(&get(IMPORT)).to_owned();
    let created = upload(address, &ana, &token, SOURCE, false);
    assert_status(&created, 303);
    let url = header(&created, "location").to_owned();
    let page = get(&url);
    assert!(page.contains("Spreadsheet columns"));
    assert!(page.contains(r#"name="target-1" list="import-targets" value="Work.title""#));
    assert!(
        page.contains(r#"name="target-2" list="import-targets" value="Instance.editionStatement""#)
    );
    assert!(page.contains(r#"name="target-3" list="import-targets" value="""#));
    assert!(!page.contains("New rule"));

    let unmapped = form(
        address,
        &format!("{url}/prever"),
        &ana,
        &format!("csrf={token}&revision=1"),
    );
    assert_status(&unmapped, 422);
    assert!(
        unmapped.contains("Choose a destination for each spreadsheet column"),
        "{unmapped}"
    );
    assert!(!unmapped.contains("select a versioned column mapping"));
    let sheet = format!("{url}/planilha");
    let base = format!(
        "csrf={token}&revision=1&action=columns&target-1=Work.title&target-2=Instance.editionStatement"
    );
    let refused = form(
        address,
        &sheet,
        &ana,
        &format!("{base}&target-3=Item.catalog.item_type"),
    );
    assert_status(&refused, 422);
    assert!(refused.contains("Choose a destination from the list for every column."));
    assert_status(
        &form(
            address,
            &sheet,
            &ana,
            &format!("{base}&target-3=ignore&trim-1=on"),
        ),
        303,
    );
    let page = get(&url);
    assert!(page.contains(r#"name="trim-1" aria-label="Trim spaces Título" checked"#));
    let preview = form(
        address,
        &format!("{url}/prever"),
        &ana,
        &format!("csrf={token}&revision=2"),
    );
    assert_status(&preview, 200);
    assert!(preview.contains("Primeiro livro fictício") && preview.contains("2. ed. revista"));

    assert_status(
        &form(
            address,
            &sheet,
            &ana,
            &format!(
                "csrf={token}&revision=2&action=columns&target-1=Work.title&target-2=Instance.editionStatement&target-3=Item.catalog.home_branch"
            ),
        ),
        303,
    );
    let cell = form(
        address,
        &format!("{url}/prever"),
        &ana,
        &format!("csrf={token}&revision=3"),
    );
    assert_status(&cell, 422);
    assert!(
        cell.contains("Row 2, column 3: there is no home branch named &#34;sem uso&#34;."),
        "{cell}"
    );
    drop(server);
}

#[test]
fn staff_can_select_native_registries_and_confirm_the_biblivre_profile() {
    use elzevir_app::catalog::CatalogCommand;
    use elzevir_app::commands::WriteCommand;
    use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
    let data = tempfile::tempdir().unwrap();
    account(data.path());
    let store = ElzevirStore::open(
        &data.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    for command in [
        CatalogCommand::CreateBranch {
            name: "School library".into(),
        },
        CatalogCommand::CreatePolicy {
            name: "Local lending".into(),
        },
    ] {
        store
            .submit(WriteCommand::Catalog(command))
            .unwrap()
            .wait()
            .unwrap();
    }
    store.shutdown().unwrap();
    let (_server, address) = start(data.path());
    let user = sign_in(address, "ana");
    let home = request(address, "GET", IMPORT, &format!("Cookie: {user}\r\n"), "");
    let token = csrf(&home);
    let source = include_str!(
        "../../../samples-import-tool/biblivre_simulacao_10_registros_com_exemplares_sinteticos.mrk"
    );
    let uploaded = upload(address, &user, token, source, false);
    assert_status(&uploaded, 303);
    let url = header(&uploaded, "location");
    let post = |endpoint: &str, rev, extra: &str| {
        form(
            address,
            &format!("{url}/{endpoint}"),
            &user,
            &format!("csrf={token}&revision={rev}{extra}"),
        )
    };
    let unprofiled = post("prever", 1, "");
    assert_status(&unprofiled, 200);
    assert!(
        unprofiled.contains("Biblivre holdings (949)")
            && unprofiled.contains("School library (#1)")
            && unprofiled.contains("Local lending (#1)")
    );
    assert_status(
        &post(
            "regra",
            1,
            "&action=biblivre&origin=escola&date=acquisition&home_branch=1&holding_branch=1",
        ),
        422,
    );
    assert_status(
        &post(
            "regra",
            1,
            "&action=biblivre&origin=escola&date=acquisition&home_branch=1&holding_branch=1&policy=1",
        ),
        303,
    );
    assert_status(&post("confirmar", 2, ""), 409);
    let review = post("prever", 2, "");
    assert_status(&review, 200);
    assert!(
        review.contains("16764") && review.contains("2025-03-24") && review.contains("18 items"),
        "{}",
        review
            .lines()
            .filter(|s| s.contains("items") || s.contains("2025"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_status(&post("confirmar", 2, ""), 303);
}

#[test]
fn postgres_inventory_agrees_with_cli_and_cannot_confirm_a_stale_report() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples-import-tool/BKP_BIBLIOTECA (2).backup");
    let output = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["import", "inventory"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(output.status.success());
    let tables: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(tables["public.acervo"]["rows"], 11561);
    assert_eq!(tables["public.ocorrencias"]["rows"], 5);

    let data = tempfile::tempdir().unwrap();
    account(data.path());
    let (server, address) = start(data.path());
    let route = "/equipe/acervo/importar/inventario/postgresql";
    let log_in = |login| {
        let cookie = sign_in(address, login);
        let page = request(address, "GET", IMPORT, &format!("Cookie: {cookie}\r\n"), "");
        let token = csrf(&page).to_owned();
        (cookie, token, page)
    };
    let (ana, ana_token, home) = log_in("ana");
    assert!(!home.contains(route));
    assert_status(&upload_to(address, &ana, &ana_token, "", false, route), 403);
    let (bia, token, home) = log_in("bia");
    assert!(home.contains(route));
    assert_status(&upload_to(address, &bia, "stale", "", false, route), 403);
    let page = upload_to(
        address,
        &bia,
        &token,
        &std::fs::read_to_string(file).unwrap(),
        false,
        route,
    );
    assert_status(&page, 200);
    assert_eq!(header(&page, "cache-control"), "no-store");
    for (name, table) in tables.as_object().unwrap() {
        let columns = table["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(", ");
        let (rows, nulls, empty) = (&table["rows"], &table["nulls"], &table["empty"]);
        assert!(page.contains(&format!(
            ">{name}</th><td>{columns}</td><td>{rows}</td><td>{nulls}</td><td>{empty}</td>"
        )));
    }
    assert!(!page.contains("/confirmar"));
    let refused = upload_to(
        address,
        &bia,
        &token,
        "COPY t (a) FROM stdin;\nprivate value\n",
        false,
        route,
    );
    assert_status(&refused, 422);
    assert!(!refused.contains("private value"));
    let stale = form(
        address,
        &format!("{IMPORT}/inventario/confirmar"),
        &bia,
        &format!("csrf={token}&revision=1"),
    );
    assert_status(&stale, 404);
    drop(server);
    assert!(records(data.path()).is_empty());
}

#[test]
fn staff_source_sets_require_current_permissions_and_complete_uploads() {
    let data = tempfile::tempdir().unwrap();
    let ana_id = account(data.path());
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples-import-tool");
    // A pre-existing batch owned by an operator who now only has cataloguing.
    let staged = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["import", "stage-set", "--data-dir"])
        .arg(data.path())
        .args(["--operator", &ana_id.0.to_string(), "--origin", "limited"])
        .arg(fixture.join("02-folio/datasets/cornell/instances.tsv"))
        .output()
        .unwrap();
    assert!(staged.status.success(), "{staged:?}");
    let staged = String::from_utf8(staged.stdout).unwrap();
    let private_id = staged.split_whitespace().nth(1).unwrap();
    let (server, address) = start(data.path());
    let ana = sign_in(address, "ana");
    let bia = sign_in(address, "bia");
    let get = |cookie, url| request(address, "GET", url, &format!("Cookie: {cookie}\r\n"), "");
    let home = get(&ana, IMPORT);
    assert!(!home.contains("/conjuntos") && !home.contains(private_id));
    let hidden = format!("{IMPORT}/{private_id}");
    assert_status(&get(&ana, &hidden), 404);
    assert_status(
        &form(
            address,
            &format!("{hidden}/cancelar"),
            &ana,
            &format!("csrf={}&revision=1", csrf(&home)),
        ),
        404,
    );
    assert_status(&get(&bia, &hidden), 404);

    let page = get(&bia, IMPORT);
    let token = csrf(&page);
    let upload = |cookie: &str, token: &str, tail: &str| {
        let body = format!(
            "--B\r\nContent-Disposition: form-data; name=\"csrf\"\r\n\r\n{token}\r\n--B\r\nContent-Disposition: form-data; name=\"origin\"\r\n\r\nlocal-A\r\n--B\r\nContent-Disposition: form-data; name=\"file\"; filename=\"instance.tsv\"\r\n\r\nprivate-first\r\n--B\r\nContent-Disposition: form-data; name=\"file\"; filename=\"item.tsv\"\r\n\r\nprivate-last\r\n{tail}"
        );
        request(
            address,
            "POST",
            &format!("{IMPORT}/conjuntos"),
            &format!("Cookie: {cookie}\r\nContent-Type: multipart/form-data; boundary=B\r\n"),
            &body,
        )
    };
    assert_status(&upload(&ana, csrf(&home), "--B--\r\n"), 403);
    assert_status(&upload(&bia, "wrong", "--B--\r\n"), 403);
    let count_batches = || {
        std::fs::read_dir(data.path().join("imports"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .count()
    };
    let count = count_batches();
    for tail in [
        "",
        "--B\r\nContent-Disposition: form-data; name=\"extra\"\r\n\r\nx\r\n--B--\r\n",
    ] {
        assert_status(&upload(&bia, token, tail), 422);
        assert_eq!(count_batches(), count);
    }
    let staged = upload(&bia, token, "--B--\r\n");
    assert_status(&staged, 303);
    let url = header(&staged, "location");
    let inventory = get(&bia, url);
    assert_status(&inventory, 200);
    assert_eq!(header(&inventory, "cache-control"), "no-store");
    for name in ["instance.tsv", "item.tsv", "local-A", "SHA-256"] {
        assert!(inventory.contains(name));
    }
    assert!(!inventory.contains("private-first") && !inventory.contains("/confirmar"));
    let action = |verb, rev| {
        form(
            address,
            &format!("{url}/{verb}"),
            &bia,
            &format!("csrf={token}&revision={rev}"),
        )
    };
    assert_status(&action("prever", 2), 409);
    assert_status(&action("prever", 1), 200);
    assert_status(&action("confirmar", 1), 409);
    assert_status(&action("cancelar", 1), 303);
    assert_eq!(count_batches(), count);
    drop(server);
    assert!(records(data.path()).is_empty());
}

#[test]
fn staff_groups_archive_members_in_the_order_selected() {
    let data = tempfile::tempdir().unwrap();
    account(data.path());
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples-import-tool/evergreen-sample.zip");
    let stage = Command::new(env!("CARGO_BIN_EXE_elzevir"))
        .args(["import", "stage", "--operator", "2", "--data-dir"])
        .arg(data.path())
        .arg(fixture)
        .output()
        .unwrap();
    assert!(stage.status.success());
    let response = String::from_utf8(stage.stdout).unwrap();
    let id = response.split_whitespace().nth(1).unwrap();
    let (_server, address) = start(data.path());
    let bia = sign_in(address, "bia");
    let get = |url| request(address, "GET", url, &format!("Cookie: {bia}\r\n"), "");
    let parent = format!("{IMPORT}/{id}");
    let page = get(&parent);
    assert_status(&page, 200);
    let token = csrf(&page);
    let route = format!("{parent}/conjunto");
    assert!(page.contains(&route));
    let select = |token, revision, picks| {
        form(
            address,
            &route,
            &bia,
            &format!("csrf={token}&revision={revision}&origin=evergreen&{picks}"),
        )
    };
    assert_status(&select("bad", 1, "member=1"), 403);
    assert_status(&select(token, 2, "member=1"), 409);
    assert_status(&select(token, 1, "member=1&member=1"), 422);
    let created = select(token, 1, "member=3&member=1");
    assert_status(&created, 303);
    let inventory = get(header(&created, "location"));
    assert_status(&inventory, 200);
    let first = inventory.find("01-evergreen/marc/auth-1066.mrc").unwrap();
    assert!(first < inventory.find("01-evergreen/marc/oss.marc").unwrap());
    assert!(inventory.contains("88f7c89842e444e8680e185cc27ab1236eb531fa9b5a501f9af1b65768a6daea"));
    assert!(!inventory.contains("/confirmar"));
}
