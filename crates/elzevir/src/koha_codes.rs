// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir koha-codes`: the names of branches, item types, locations and collections read
//! from a Koha (docs/specs/catalogo/tabelas-de-codigos.md, items 6 to 13).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use elzevir_app::catalog::{CatalogCommand, CatalogOutput};
use elzevir_app::codes::{CodeTable, KOHA_SOURCES, MAX_ANSWER_BYTES, NamingCount, koha_table};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use ureq::Agent;
use ureq::http::Response;
use ureq::tls::{Certificate, RootCerts, TlsConfig};

use crate::open_store;

/// `elzevir koha-codes`.
#[derive(Args)]
pub(crate) struct KohaCodesArgs {
    /// Institution data directory.
    #[arg(long)]
    data_dir: PathBuf,
    /// Koha's staff interface, such as `https://koha-staff.example.org`.
    #[arg(long)]
    url: String,
    /// Koha staff user that signs in; the password comes from `ELZEVIR_KOHA_PASSWORD`.
    #[arg(long)]
    user: String,
}

/// Reads the four tables from Koha and stores their names (items 6 to 12).
pub(crate) fn koha_codes(args: &KohaCodesArgs) -> Result<(), String> {
    let password = std::env::var("ELZEVIR_KOHA_PASSWORD")
        .map_err(|_| "set ELZEVIR_KOHA_PASSWORD to the Koha user's password".to_owned())?;
    let front = match (
        std::env::var("ELZEVIR_KOHA_BASIC_USER"),
        std::env::var("ELZEVIR_KOHA_BASIC_PASSWORD"),
    ) {
        (Ok(user), Ok(password)) => Some(basic_header(&user, &password)),
        _ => None,
    };
    // Locked first: with a server running on the directory, nothing is fetched.
    let data = open_store(&args.data_dir)?;
    let mut koha = Koha::sign_in(args.url.trim_end_matches('/'), front, &args.user, &password)?;
    let mut codes = Vec::new();
    let mut read = [0_usize; 4];
    for (index, source) in KOHA_SOURCES.iter().enumerate() {
        let answer = koha.get(source.path, "application/json")?;
        let table = koha_table(source, &answer).map_err(|e| format!("{}: {e}", source.path))?;
        if table.skipped > 0 {
            eprintln!(
                "elzevir: {} {} without code or name skipped",
                table.skipped,
                source.table.name()
            );
        }
        if let Some(count) = read.get_mut(index) {
            *count = table.codes.len();
        }
        codes.extend(table.codes);
    }
    let answer = data
        .store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(codes)))
        .map_err(|e| e.to_string())?
        .wait()
        .map_err(|e| e.to_string())?;
    let WriteOutput::Catalog(CatalogOutput::CodesNamed(counts)) = answer else {
        return Err("unexpected answer from the store".to_owned());
    };
    for ((table, count), read) in CodeTable::ALL.iter().zip(counts).zip(read) {
        print_count(*table, read, count);
    }
    Ok(())
}

fn print_count(table: CodeTable, read: usize, count: NamingCount) {
    println!(
        "{}: {read} read, {} created, {} renamed, {} unchanged, {} not used yet",
        table.name(),
        count.created,
        count.renamed,
        count.unchanged,
        count.unused
    );
}

/// A signed-in session on Koha's staff interface.
struct Koha {
    agent: Agent,
    base: String,
    front: Option<String>,
    cookies: Vec<(String, String)>,
}

impl Koha {
    /// Signs in with the staff login form, as a person would (item 6).
    fn sign_in(
        base: &str,
        front: Option<String>,
        user: &str,
        password: &str,
    ) -> Result<Self, String> {
        let mut koha = Self {
            agent: agent()?,
            base: base.to_owned(),
            front,
            cookies: Vec::new(),
        };
        let page = koha.get("/cgi-bin/koha/mainpage.pl", "text/html")?;
        let token = between(&page, "name=\"csrf_token\" value=\"", "\"")
            .ok_or("no CSRF token on Koha's sign-in page")?
            .to_owned();
        let form = [
            ("login_op", "cud-login"),
            ("csrf_token", token.as_str()),
            ("login_userid", user),
            ("login_password", password),
        ]
        .iter()
        .map(|(name, value)| format!("{name}={}", form_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
        let signed_in = koha.post_form("/cgi-bin/koha/mainpage.pl", &form)?;
        // The staff menu has a sign-out link only for a signed-in user.
        if !signed_in.contains("logout.x=1") {
            return Err(format!("Koha refused the sign-in of {user}"));
        }
        Ok(koha)
    }

    fn get(&mut self, path: &str, accept: &str) -> Result<String, String> {
        let mut request = self
            .agent
            .get(format!("{}{path}", self.base))
            .header("Accept", accept);
        if let Some(cookie) = self.cookie_header() {
            request = request.header("Cookie", cookie);
        }
        if let Some(front) = &self.front {
            request = request.header("Authorization", front);
        }
        let response = request.call().map_err(|e| format!("GET {path}: {e}"))?;
        self.read(path, response)
    }

    fn post_form(&mut self, path: &str, form: &str) -> Result<String, String> {
        let mut request = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Content-Type", "application/x-www-form-urlencoded");
        if let Some(cookie) = self.cookie_header() {
            request = request.header("Cookie", cookie);
        }
        if let Some(front) = &self.front {
            request = request.header("Authorization", front);
        }
        let response = request
            .send(form)
            .map_err(|e| format!("POST {path}: {e}"))?;
        self.read(path, response)
    }

    /// Keeps the cookies the answer sets and returns its body, refused unless 200.
    fn read(&mut self, path: &str, mut response: Response<ureq::Body>) -> Result<String, String> {
        for value in response.headers().get_all("set-cookie") {
            let Ok(value) = value.to_str() else { continue };
            let pair = value.split(';').next().unwrap_or_default();
            if let Some((name, value)) = pair.split_once('=') {
                let name = name.trim().to_owned();
                self.cookies.retain(|(kept, _)| *kept != name);
                self.cookies.push((name, value.trim().to_owned()));
            }
        }
        let status = response.status();
        let body = response
            .body_mut()
            .with_config()
            .limit(u64::try_from(MAX_ANSWER_BYTES).unwrap_or(u64::MAX))
            .read_to_string()
            .map_err(|e| format!("{path}: {e}"))?;
        if status != 200 {
            return Err(format!("{path}: Koha answered {status}"));
        }
        Ok(body)
    }

    fn cookie_header(&self) -> Option<String> {
        (!self.cookies.is_empty()).then(|| {
            self.cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }
}

/// An HTTP client that trusts the system's certificates, follows no redirect (a sign-in
/// answers with a page) and reads statuses itself.
fn agent() -> Result<Agent, String> {
    let found = rustls_native_certs::load_native_certs();
    let roots: Vec<Certificate<'static>> = found
        .certs
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()).to_owned())
        .collect();
    if roots.is_empty() {
        return Err("no system certificates found to check Koha's".to_owned());
    }
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::new_with_certs(&roots))
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let config = Agent::config_builder()
        .tls_config(tls)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_mins(1)))
        .build();
    Ok(config.into())
}

/// The text after the first `start` and before the next `end`.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let (_, rest) = text.split_once(start)?;
    rest.split_once(end).map(|(inside, _)| inside)
}

/// `application/x-www-form-urlencoded` value.
fn form_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                for nibble in [byte >> 4, byte & 0x0F] {
                    encoded.push(
                        char::from_digit(u32::from(nibble), 16)
                            .map_or('0', |c| c.to_ascii_uppercase()),
                    );
                }
            }
        }
    }
    encoded
}

/// `Authorization: Basic` value for a proxy in front of Koha (item 7).
fn basic_header(user: &str, password: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = format!("{user}:{password}");
    let mut encoded = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.as_bytes().chunks(3) {
        let bytes = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let group = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (group >> (18 - 6 * index)) & 0x3F;
                let letter = usize::try_from(sextet)
                    .ok()
                    .and_then(|sextet| ALPHABET.get(sextet))
                    .copied()
                    .unwrap_or(b'=');
                encoded.push(char::from(letter));
            } else {
                encoded.push('=');
            }
        }
    }
    format!("Basic {encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_credentials_are_base64() {
        // RFC 7617, section 2.
        assert_eq!(
            basic_header("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
        assert_eq!(basic_header("a", ""), "Basic YTo=");
        assert_eq!(basic_header("ab", "c"), "Basic YWI6Yw==");
    }

    #[test]
    fn form_values_are_encoded() {
        assert_eq!(form_encode("a b&c=ç"), "a+b%26c%3D%C3%A7");
    }
}
