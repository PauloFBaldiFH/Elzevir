// SPDX-License-Identifier: GPL-3.0-or-later
use super::{ElzevirHandler, ElzevirStore, StoreConfig, WriteCommand, institution, start_http};
use elzevir_app::{
    commands::StaffCommand,
    staff::{Permission, hash_password},
};
use std::{fmt::Write as _, path::Path};

type Response = ureq::http::Response<String>;
const ROOT: &str = "/equipe/repositorio";
const URLENCODED: &str = "application/x-www-form-urlencoded";
struct Browser {
    host: String,
    agent: ureq::Agent,
    session: String,
}
impl Browser {
    fn new(host: String) -> Self {
        Self {
            host,
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .proxy(None)
                    .http_status_as_error(false)
                    .max_redirects(0)
                    .build(),
            ),
            session: String::new(),
        }
    }
    fn request(&self, path: &str, post: Option<(&str, &str)>) -> Response {
        let url = format!("{}{path}", self.host);
        let response = if let Some((kind, body)) = post {
            self.agent
                .post(url)
                .header("Cookie", &self.session)
                .header("Content-Type", kind)
                .send(body)
        } else {
            self.agent.get(url).header("Cookie", &self.session).call()
        }
        .unwrap();
        let (head, mut body) = response.into_parts();
        Response::from_parts(head, body.read_to_string().unwrap())
    }
    fn sign_in(&mut self, login: &str) {
        self.session.clear();
        let page = self.request("/equipe/login", None);
        self.session = cookie(&page, "elzevir_login_csrf=");
        let body = format!(
            "login={login}&password=synthetic-password&csrf={}",
            input(page.body(), "csrf")
        );
        let response = self.request("/equipe/login", Some((URLENCODED, &body)));
        status(&response, 303);
        self.session = cookie(&response, "elzevir_session=");
    }
    fn change(&self, path: &str, csrf: &str, form: &str) -> Response {
        self.request(path, Some((URLENCODED, &format!("csrf={csrf}&{form}"))))
    }
}
fn status(r: &Response, n: u16) {
    assert_eq!(r.status().as_u16(), n, "{}", r.body());
}
fn cookie(response: &Response, prefix: &str) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap())
        .find(|v| v.starts_with(prefix))
        .unwrap()
        .into()
}
fn input<'a>(page: &'a str, key: &str) -> &'a str {
    page.split_once(&format!("name=\"{key}\" value=\""))
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}
fn open(path: &Path) -> ElzevirStore {
    ElzevirStore::open(
        &path.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}
fn accounts(path: &Path) {
    let store = open(path);
    for (login, permission) in [
        ("cataloguer", Permission::Cataloguing),
        ("reader", Permission::Patrons),
    ] {
        store
            .submit(WriteCommand::Staff(StaffCommand::Create {
                login: login.into(),
                password_hash: hash_password("synthetic-password").unwrap(),
                permissions: [Permission::StaffInterface, permission].into(),
            }))
            .unwrap()
            .wait()
            .unwrap();
    }
    store.shutdown().unwrap();
}
fn multipart(csrf: &str, operation: &str, expected: &str, data: &str, closing: &str) -> String {
    let mut body = String::new();
    for (k, v) in [
        ("csrf", csrf),
        ("operation", operation),
        ("expected", expected),
    ] {
        write!(
            body,
            "--test\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"
        )
        .unwrap();
    }
    write!(body, "--test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"thesis.pdf\"\r\nContent-Type: application/pdf\r\n\r\n{data}{closing}").unwrap();
    body
}
fn prepare_files_and_collections(browser: &Browser, path: &str, csrf: &str) {
    status(
        &browser.change(path, csrf, "action=collection&name=Theses"),
        303,
    );
    let page = browser.request(&format!("{path}?filter=Theses"), None);
    let collection = page
        .body()
        .split_once("\">Theses</option>")
        .unwrap()
        .0
        .rsplit_once("value=\"")
        .unwrap()
        .1;
    status(
        &browser.change(path, csrf, &format!("action=join&collection={collection}")),
        303,
    );
    status(
        &browser.change(
            path,
            csrf,
            "action=file&name=Consent&role=administrative&access=public",
        ),
        409,
    );
    status(
        &browser.change(
            path,
            csrf,
            "action=file&name=PDF&role=primary&access=public",
        ),
        303,
    );
}
#[test]
fn staff_manage_versions_while_the_server_holds_the_institution() {
    let fixture = institution();
    accounts(fixture.path());
    let (server, host, _) = start_http(fixture.path());
    let mut browser = Browser::new(host);
    browser.sign_in("reader");
    status(&browser.request(ROOT, None), 403);
    browser.sign_in("cataloguer");
    let lookup = format!("{ROOT}?instance=1");
    let form = browser.request(&lookup, None);
    assert!(form.body().contains("Digital thesis"));
    let csrf = input(form.body(), "csrf");
    status(&browser.change(ROOT, "bad", "instance=1"), 403);
    let draft = browser.change(ROOT, csrf, "instance=1");
    status(&draft, 303);
    let path = draft.headers()["location"].to_str().unwrap();
    assert_eq!(browser.request(&lookup, None).headers()["location"], path);
    let (_, deposit) = path.rsplit_once('/').unwrap();
    let public = format!("/repository/{deposit}");
    status(&browser.request(&public, None), 404);
    prepare_files_and_collections(&browser, path, csrf);
    let page = browser.request(path, None);
    assert!(page.body().contains("<li>Theses</li>"));
    let file = page
        .body()
        .split_once("/upload\"")
        .unwrap()
        .0
        .rsplit_once("/files/")
        .unwrap()
        .1;
    let endpoint = format!("{path}/files/{file}/upload");
    let operation = input(page.body(), "operation");
    let send = |token, op, expected, bytes, ending| {
        browser.request(
            &endpoint,
            Some((
                "multipart/form-data; boundary=test",
                &multipart(token, op, expected, bytes, ending),
            )),
        )
    };
    let end = "\r\n--test--\r\n";
    status(&send("bad", operation, "0", "broken", end), 403);
    status(&send(csrf, operation, "0", "broken", ""), 400);
    status(
        &send(
            csrf,
            operation,
            "0",
            "extra",
            "\r\n--test\r\nContent-Disposition: form-data; name=\"extra\"\r\n\r\nignored\r\n--test--\r\n",
        ),
        400,
    );
    status(&browser.change(path, csrf, "action=publish"), 409);
    status(&send(csrf, operation, "0", "first", end), 303);
    status(&send(csrf, operation, "0", "first", end), 303);
    status(&send(csrf, operation, "0", "different", end), 409);
    let page = browser.request(path, None);
    let next = input(page.body(), "operation");
    status(&send(csrf, next, "0", "revised", end), 409);
    status(&send(csrf, next, "1", "revised", end), 303);
    status(&browser.change(path, csrf, "action=publish"), 303);
    status(&browser.request(&public, None), 200);
    status(
        &browser.request(&format!("{public}/files/{file}/versions/1"), None),
        404,
    );
    let download = format!("{path}/files/{file}/download?seq=1");
    let historic = browser.request(&download, None);
    status(&historic, 200);
    assert_eq!(historic.body(), "first");
    assert_eq!(historic.headers()["cache-control"], "no-store");
    let retire = format!("action=retire&file={file}&seq=2");
    status(&browser.change(path, csrf, &retire), 409);
    status(
        &browser.change(path, csrf, &format!("{retire}&together=yes")),
        303,
    );
    status(&browser.request(&public, None), 410);
    assert_eq!(
        browser
            .request(&format!("{path}/files/{file}/download?seq=2"), None)
            .body(),
        "revised"
    );
    browser.sign_in("reader");
    status(&browser.request(&download, None), 403);
    drop(server);
    verify_audit(fixture.path());
}

fn verify_audit(path: &Path) {
    let store = open(path);
    let (uploads, reads): (i64, i64) = store
        .readers()
        .read(|conn| {
            conn.query_row(
                "SELECT count(*) FILTER(WHERE kind='repository.version'),
                    count(*) FILTER(WHERE kind='repository.download')
             FROM audit_log WHERE actor=(SELECT id FROM staff_accounts WHERE login='cataloguer')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!((uploads, reads), (2, 2));
    store.shutdown().unwrap();
}

#[test]
fn disconnected_upload_releases_the_slot_and_preserves_the_restricted_version() {
    use std::{
        io::Write as _,
        net::TcpStream,
        time::{Duration, Instant},
    };
    let fixture = institution();
    let data = fixture.path();
    accounts(data);
    let deposit = super::run(data, &["draft", "1"]);
    let file = super::run(
        data,
        &["add-file", &deposit, "Consent", "administrative", "staff"],
    );
    super::upload(data, &deposit, &file, 1, None, b"original");
    let (_server, host, _) = start_http(data);
    let mut browser = Browser::new(host);
    browser.sign_in("cataloguer");
    let path = format!("{ROOT}/{deposit}");
    let endpoint = format!("{path}/files/{file}/upload");
    let page = browser.request(&path, None);
    let csrf = input(page.body(), "csrf");
    let operation = input(page.body(), "operation");
    let partial = multipart(csrf, operation, "1", &"x".repeat(16 * 1024), "");
    let addr = browser.host.strip_prefix("http://").unwrap();
    let mut socket = TcpStream::connect(addr).unwrap();
    write!(socket,"POST {endpoint} HTTP/1.1\r\nHost: {addr}\r\nCookie: {}\r\nContent-Type: multipart/form-data; boundary=test\r\nContent-Length: {}\r\n\r\n{partial}",browser.session,partial.len()+1000).unwrap();
    let staging = data.join("repository/staging");
    let deadline = Instant::now() + Duration::from_secs(30);
    while staging.read_dir().unwrap().next().is_none() {
        assert!(
            Instant::now() < deadline,
            "the upload did not enter staging"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let complete = multipart(csrf, operation, "1", "replacement", "\r\n--test--\r\n");
    let retry = || {
        browser.request(
            &endpoint,
            Some(("multipart/form-data; boundary=test", &complete)),
        )
    };
    status(&retry(), 503);
    assert_eq!(
        browser
            .request(&format!("{path}/files/{file}/download?seq=1"), None)
            .body(),
        "original"
    );
    drop(socket);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let result = retry();
        if result.status() != 503 {
            status(&result, 303);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the cancelled upload still owns the slot"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(staging.read_dir().unwrap().next().is_none());
    assert_eq!(
        browser
            .request(&format!("{path}/files/{file}/download?seq=2"), None)
            .body(),
        "replacement"
    );
    status(
        &browser.request(
            &format!("/repository/{deposit}/files/{file}/versions/2"),
            None,
        ),
        404,
    );
}
