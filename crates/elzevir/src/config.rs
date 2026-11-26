// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir.toml`, the typed configuration of a data directory (doc 04, decision 8, and
//! docs/specs/catalogo/numero-de-chamada.md). Every key is optional; an unknown key is an
//! error, so a typo is not silently ignored.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use elzevir_app::drafts::DraftField;
use elzevir_domain::call_number::{AuthorTable, MAX_TABLE_LINES, TableReport};
use elzevir_domain::long_overdue::{DEFAULT_MAX_DAYS, LongOverdueSettings};
use elzevir_sources::{
    DEFAULT_TIMEOUT, DEFAULT_Z3950_PORT, DraftServer, MAX_NAME_CHARS, RecordCharset, SruServer,
    SruVersion, Z3950Server,
};
use jiff::tz::TimeZone;
use serde::Deserialize;

use crate::notice_mail::{MailSettings, Security};

/// Most bytes of `elzevir.toml`.
const MAX_CONFIG: u64 = 1 << 20;

/// Most bytes of the author table: the printed tables are a few hundred kilobytes.
const MAX_TABLE: u64 = 8 << 20;

/// What `elzevir.toml` says.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    /// How call numbers are suggested.
    #[serde(default)]
    call_number: CallNumber,
    /// The library itself.
    #[serde(default)]
    library: Library,
    /// Circulation settings that are not rules.
    #[serde(default)]
    circulation: Circulation,
    /// How notices go out (docs/specs/circulacao/avisos.md).
    #[serde(default)]
    notices: Notices,
    /// Other catalogues to copy records from (docs/specs/catalogo/fontes-de-rascunho.md).
    #[serde(default)]
    draft_sources: Vec<DraftSourceEntry>,
}

/// One `[[draft_sources]]` entry (fontes-de-rascunho.md, items 16 and 17, and
/// cliente-z3950.md, items 15 and 16).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftSourceEntry {
    /// Shown in the origin, `sru:<name>` or `z3950:<name>`.
    name: String,
    /// `sru` or `z3950`.
    kind: String,
    /// From 1 to 60; 10 when absent.
    timeout_seconds: Option<u64>,
    /// Asked when absent.
    enabled: Option<bool>,
    /// `bibliographic` (when absent) or `authority` (fontes-de-autoridade.md, item 1).
    records: Option<String>,
    /// SRU: base URL, `http` or `https`, without a query string.
    url: Option<String>,
    /// SRU: `1.2` (when absent) or `2.0`.
    version: Option<String>,
    /// SRU: `marcxml` when absent.
    record_schema: Option<String>,
    /// SRU: the server's CQL index of each query field.
    fields: Option<DraftFields>,
    /// Z39.50: host name or address, without scheme or port.
    host: Option<String>,
    /// Z39.50: 210 when absent.
    port: Option<u16>,
    /// Z39.50: one or more databases, separated by commas.
    database: Option<String>,
    /// Z39.50: `auto` (when absent), `utf-8` or `marc-8`.
    encoding: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftFields {
    isbn: Option<String>,
    issn: Option<String>,
    title: Option<String>,
    author: Option<String>,
    any: Option<String>,
    personal_name: Option<String>,
    corporate_name: Option<String>,
    meeting_name: Option<String>,
    name: Option<String>,
    subject: Option<String>,
    uniform_title: Option<String>,
}

impl DraftFields {
    /// Each key with its field.
    fn entries(&self) -> Vec<(DraftField, Option<&str>)> {
        vec![
            (DraftField::Isbn, self.isbn.as_deref()),
            (DraftField::Issn, self.issn.as_deref()),
            (DraftField::Title, self.title.as_deref()),
            (DraftField::Author, self.author.as_deref()),
            (DraftField::Any, self.any.as_deref()),
            (DraftField::PersonalName, self.personal_name.as_deref()),
            (DraftField::CorporateName, self.corporate_name.as_deref()),
            (DraftField::MeetingName, self.meeting_name.as_deref()),
            (DraftField::Name, self.name.as_deref()),
            (DraftField::Subject, self.subject.as_deref()),
            (DraftField::UniformTitle, self.uniform_title.as_deref()),
        ]
    }
}

/// What a draft server answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Records {
    Bibliographic,
    Authority,
}

/// Most characters of a Z39.50 database name.
const MAX_DATABASE_CHARS: usize = 64;

impl DraftSourceEntry {
    /// What the server answers with, or why that cannot be.
    fn records(&self) -> Result<Records, String> {
        match self.records.as_deref().unwrap_or("bibliographic") {
            "bibliographic" => Ok(Records::Bibliographic),
            "authority" => Ok(Records::Authority),
            other => Err(format!(
                "records must be bibliographic or authority, not {other}"
            )),
        }
    }

    /// The server, or why it cannot be.
    fn server(&self) -> Result<DraftServer, String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(format!("name must have 1 to {MAX_NAME_CHARS} characters"));
        }
        let timeout = match self.timeout_seconds {
            None => DEFAULT_TIMEOUT,
            Some(seconds @ 1..=60) => Duration::from_secs(seconds),
            Some(other) => return Err(format!("timeout_seconds must be 1 to 60, not {other}")),
        };
        match self.kind.as_str() {
            "sru" => self.sru(name, timeout).map(DraftServer::Sru),
            "z3950" => self.z3950(name, timeout).map(DraftServer::Z3950),
            other => Err(format!("kind must be sru or z3950, not {other}")),
        }
    }

    /// The SRU server; Z39.50 keys are refused.
    fn sru(&self, name: &str, timeout: Duration) -> Result<SruServer, String> {
        refuse_keys(
            "sru",
            &[
                ("host", self.host.is_some()),
                ("port", self.port.is_some()),
                ("database", self.database.is_some()),
                ("encoding", self.encoding.is_some()),
            ],
        )?;
        let url = self.url.as_deref().unwrap_or_default().trim();
        if !(url.starts_with("https://") || url.starts_with("http://"))
            || url.contains(['?', '#'])
            || url.chars().any(char::is_whitespace)
        {
            return Err(format!(
                "url must be http or https, without a query string: {url}"
            ));
        }
        let version = match self.version.as_deref().unwrap_or("1.2") {
            "1.2" => SruVersion::V1_2,
            "2.0" => SruVersion::V2_0,
            other => return Err(format!("version must be 1.2 or 2.0, not {other}")),
        };
        let authority = self.records()? == Records::Authority;
        let entries = self
            .fields
            .as_ref()
            .map(DraftFields::entries)
            .unwrap_or_default();
        // `any` serves both kinds of server; every other key belongs to one.
        if let Some((field, _)) = entries.iter().find(|(field, index)| {
            index.is_some() && *field != DraftField::Any && field.is_authority() != authority
        }) {
            let kind = if authority {
                "an authority"
            } else {
                "a bibliographic"
            };
            return Err(format!(
                "fields.{} does not apply to {kind} server",
                field.name()
            ));
        }
        let fields = entries
            .into_iter()
            .filter_map(|(field, index)| {
                let index = index?.trim();
                (!index.is_empty()).then(|| (field, index.to_owned()))
            })
            .collect();
        Ok(SruServer {
            name: name.to_owned(),
            url: url.trim_end_matches('/').to_owned(),
            version,
            record_schema: self
                .record_schema
                .as_deref()
                .map_or("marcxml", str::trim)
                .to_owned(),
            timeout,
            fields,
        })
    }

    /// The Z39.50 server; SRU keys are refused.
    fn z3950(&self, name: &str, timeout: Duration) -> Result<Z3950Server, String> {
        refuse_keys(
            "z3950",
            &[
                ("url", self.url.is_some()),
                ("version", self.version.is_some()),
                ("record_schema", self.record_schema.is_some()),
                ("fields", self.fields.is_some()),
            ],
        )?;
        let host = self.host.as_deref().unwrap_or_default().trim();
        // A colon belongs only to an IPv6 address; a port has its own key.
        if host.is_empty()
            || host.contains(['/', '@', '[', ']'])
            || host.chars().any(char::is_whitespace)
            || (host.contains(':') && host.parse::<std::net::Ipv6Addr>().is_err())
        {
            return Err(format!(
                "host must be a name or address, without scheme or port: {host}"
            ));
        }
        let port = match self.port {
            None => DEFAULT_Z3950_PORT,
            Some(0) => return Err("port must be 1 to 65535, not 0".to_owned()),
            Some(port) => port,
        };
        let databases: Vec<String> = self
            .database
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(|database| database.trim().to_owned())
            .collect();
        if databases
            .iter()
            .any(|database| database.is_empty() || database.chars().count() > MAX_DATABASE_CHARS)
        {
            return Err(format!(
                "database must name one or more databases of 1 to {MAX_DATABASE_CHARS} characters"
            ));
        }
        let charset = match self.encoding.as_deref().unwrap_or("auto") {
            "auto" => RecordCharset::Auto,
            "utf-8" => RecordCharset::Utf8,
            "marc-8" => RecordCharset::Marc8,
            other => {
                return Err(format!(
                    "encoding must be auto, utf-8 or marc-8, not {other}"
                ));
            }
        };
        Ok(Z3950Server {
            name: name.to_owned(),
            host: host.to_owned(),
            port,
            databases,
            charset,
            timeout,
        })
    }
}

/// An error naming the first of `keys` that is set, which `kind` does not take.
fn refuse_keys(kind: &str, keys: &[(&str, bool)]) -> Result<(), String> {
    match keys.iter().find(|(_, set)| *set) {
        Some((key, _)) => Err(format!("{key} does not apply to kind {kind}")),
        None => Ok(()),
    }
}

/// The enabled servers of `[[draft_sources]]`, in order, with unique names: the
/// bibliographic ones and the authority ones (fontes-de-autoridade.md, items 1 and 2).
type Servers = (Vec<DraftServer>, Vec<DraftServer>);

fn draft_servers(entries: &[DraftSourceEntry]) -> Result<Servers, String> {
    let mut servers: Vec<DraftServer> = Vec::new();
    let mut authorities: Vec<DraftServer> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let server = entry
            .server()
            .map_err(|e| format!("draft_sources[{index}]: {e}"))?;
        let records = entry
            .records()
            .map_err(|e| format!("draft_sources[{index}]: {e}"))?;
        if entries
            .iter()
            .take(index)
            .any(|other| other.name.trim() == server.name())
        {
            return Err(format!(
                "draft_sources[{index}]: the name {} is repeated",
                server.name()
            ));
        }
        if entry.enabled.unwrap_or(true) {
            match records {
                Records::Bibliographic => servers.push(server),
                Records::Authority => authorities.push(server),
            }
        }
    }
    Ok((servers, authorities))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Notices {
    /// The mail server; no e-mail goes out when absent.
    email: Option<Email>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Email {
    /// The sender's address, such as `Biblioteca <biblioteca@example.org>`.
    from: String,
    /// The mail server.
    host: String,
    /// Its port; 465 with `tls`, 587 with `starttls`, 25 with `none` when absent.
    port: Option<u16>,
    /// `tls`, `starttls` (when absent) or `none`.
    security: Option<String>,
    /// The user to sign in as; no sign-in when absent.
    user: Option<String>,
    /// The environment variable that holds the password, so it is not written here.
    password_env: Option<String>,
}

impl Email {
    /// The settings, or why they cannot be; reads the password from its variable.
    fn settings(&self) -> Result<MailSettings, String> {
        let security = match self.security.as_deref().unwrap_or("starttls") {
            "tls" => Security::Tls,
            "starttls" => Security::StartTls,
            "none" => Security::None,
            other => {
                return Err(format!(
                    "security must be tls, starttls or none, not {other}"
                ));
            }
        };
        let from = self
            .from
            .parse()
            .map_err(|_| format!("from is not an e-mail address: {}", self.from))?;
        let credentials = match (&self.user, &self.password_env) {
            (None, None) => None,
            (Some(user), Some(variable)) => {
                let password = std::env::var(variable)
                    .map_err(|_| format!("the variable {variable} of password_env is not set"))?;
                Some((user.clone(), password))
            }
            _ => return Err("user and password_env go together".to_owned()),
        };
        Ok(MailSettings {
            from,
            host: self.host.clone(),
            port: self.port.unwrap_or(match security {
                Security::Tls => 465,
                Security::StartTls => 587,
                Security::None => 25,
            }),
            security,
            credentials,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Circulation {
    /// Declaring long overdue Items lost; off when absent (docs/specs/circulacao/conta.md).
    long_overdue: Option<LongOverdue>,
    /// Holds (docs/specs/circulacao/reservas.md).
    #[serde(default)]
    holds: Holds,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Holds {
    /// Cancel the holds waiting past their last day to pick up; off when absent (Koha:
    /// `ExpireReservesMaxPickUpDelay`).
    expire_waiting: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LongOverdue {
    /// Days past due after which the Item is lost.
    days: u32,
    /// The lost status it gets; 1 when absent.
    lost_status: Option<i64>,
    /// Charge the patron and end the loan; off when absent.
    charge: Option<bool>,
    /// Due dates older than this many days are left alone; 366 when absent.
    max_days: Option<u32>,
    /// End the loan even without charging; off when absent.
    mark_returned: Option<bool>,
}

impl LongOverdue {
    /// The settings, or why they cannot be.
    fn settings(&self) -> Result<LongOverdueSettings, &'static str> {
        let lost_status = self.lost_status.unwrap_or(1);
        // Koha skips a range whose lost value is zero, since zero is "not lost".
        if lost_status <= 0 {
            return Err("lost_status must be above zero");
        }
        Ok(LongOverdueSettings {
            days: self.days,
            max_days: self.max_days.unwrap_or(DEFAULT_MAX_DAYS),
            lost_status,
            charge: self.charge.unwrap_or(false),
            mark_returned: self.mark_returned.unwrap_or(false),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Library {
    /// The IANA name of its time zone, such as `America/Sao_Paulo`; the system's when
    /// absent (docs/specs/catalogo/facetas.md, item 16).
    time_zone: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallNumber {
    /// The library's author table (PHA, Cutter-Sanborn), relative to the data directory.
    author_table: Option<PathBuf>,
}

/// The configuration as the server uses it.
#[derive(Debug)]
pub(crate) struct Loaded {
    /// The author table, when one is configured.
    pub(crate) author_table: Option<Arc<AuthorTable>>,
    /// What reading the table found, to show at start.
    pub(crate) table_report: Option<(PathBuf, TableReport)>,
    /// The library's time zone.
    pub(crate) time_zone: TimeZone,
    /// How long overdue Items are declared lost, when they are.
    pub(crate) long_overdue: Option<LongOverdueSettings>,
    /// Whether holds waiting past their last day are cancelled.
    pub(crate) expire_waiting_holds: bool,
    /// The mail server, when notices go out by e-mail.
    pub(crate) mail: Option<MailSettings>,
    /// The enabled bibliographic draft sources, in priority order.
    pub(crate) draft_sources: Vec<DraftServer>,
    /// The enabled authority sources, in priority order.
    pub(crate) authority_sources: Vec<DraftServer>,
}

/// Reads `data_dir/elzevir.toml` and what it points to; no file is no configuration.
pub(crate) fn load(data_dir: &Path) -> Result<Loaded, String> {
    let path = data_dir.join("elzevir.toml");
    let config: Config = match read_limited(&path, MAX_CONFIG)? {
        None => Config::default(),
        Some(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?,
    };
    let time_zone = match &config.library.time_zone {
        Some(name) => TimeZone::get(name)
            .map_err(|e| format!("{}: library.time_zone: {e}", path.display()))?,
        None => TimeZone::system(),
    };
    let long_overdue = match &config.circulation.long_overdue {
        Some(section) => Some(
            section
                .settings()
                .map_err(|e| format!("{}: circulation.long_overdue: {e}", path.display()))?,
        ),
        None => None,
    };
    let expire_waiting_holds = config.circulation.holds.expire_waiting.unwrap_or(false);
    let mail = match &config.notices.email {
        Some(section) => Some(
            section
                .settings()
                .map_err(|e| format!("{}: notices.email: {e}", path.display()))?,
        ),
        None => None,
    };
    let (draft_sources, authority_sources) =
        draft_servers(&config.draft_sources).map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(table) = config.call_number.author_table else {
        return Ok(Loaded {
            author_table: None,
            table_report: None,
            time_zone,
            long_overdue,
            expire_waiting_holds,
            mail,
            draft_sources,
            authority_sources,
        });
    };
    let table_path = data_dir.join(table);
    let text = read_limited(&table_path, MAX_TABLE)?
        .ok_or_else(|| format!("{}: the author table does not exist", table_path.display()))?;
    let (table, report) = AuthorTable::parse(&text).map_err(|e| {
        format!(
            "{}: {e} (at most {MAX_TABLE_LINES} lines)",
            table_path.display()
        )
    })?;
    Ok(Loaded {
        author_table: Some(Arc::new(table)),
        table_report: Some((table_path, report)),
        time_zone,
        long_overdue,
        expire_waiting_holds,
        mail,
        draft_sources,
        authority_sources,
    })
}

/// The file as text, `None` when it does not exist.
fn read_limited(path: &Path, limit: u64) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut text = String::new();
    file.take(limit + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("{}: larger than {limit} bytes", path.display()));
    }
    Ok(Some(text))
}

/// One line on what the table gave, and the lines to check against the book.
pub(crate) fn summary(path: &Path, report: &TableReport) -> String {
    let lines = |list: &[usize]| {
        list.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = format!(
        "elzevir: author table {}: {} entries from {} lines",
        path.display(),
        report.entries,
        report.lines
    );
    for (what, list) in [
        ("not understood", &report.not_understood),
        ("numbers out of order", &report.out_of_order),
        ("repeated", &report.repeated),
    ] {
        if !list.is_empty() {
            out.push_str("; ");
            out.push_str(what);
            out.push_str(" at lines ");
            out.push_str(&lines(list));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mail_server_is_read_and_its_password_comes_from_a_variable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().mail.is_none());
        let config = dir.path().join("elzevir.toml");
        let write = |text: &str| std::fs::write(&config, text).unwrap();
        write(
            "[notices.email]\nfrom = \"Biblioteca <b@example.org>\"\nhost = \"smtp.example.org\"\n",
        );
        let mail = load(dir.path()).unwrap().mail.unwrap();
        assert_eq!((mail.port, mail.security), (587, Security::StartTls));
        assert!(mail.credentials.is_none());
        // `PATH` is always set; its value stands in for a password.
        write(
            "[notices.email]\nfrom = \"b@example.org\"\nhost = \"h\"\nsecurity = \"tls\"\n\
             user = \"u\"\npassword_env = \"PATH\"\n",
        );
        let mail = load(dir.path()).unwrap().mail.unwrap();
        assert_eq!((mail.port, mail.security), (465, Security::Tls));
        assert_eq!(mail.credentials.map(|(user, _)| user).as_deref(), Some("u"));
        for wrong in [
            "from = \"b@example.org\"\nhost = \"h\"\nsecurity = \"ssl\"\n",
            "from = \"nobody\"\nhost = \"h\"\n",
            "from = \"b@example.org\"\nhost = \"h\"\nuser = \"u\"\n",
            "from = \"b@example.org\"\nhost = \"h\"\nuser = \"u\"\npassword_env = \"ELZEVIR_NO_SUCH_VARIABLE_FOR_TESTS\"\n",
            "from = \"b@example.org\"\nhost = \"h\"\npassword = \"x\"\n",
        ] {
            write(&format!("[notices.email]\n{wrong}"));
            assert!(load(dir.path()).is_err(), "{wrong}");
        }
    }

    #[test]
    fn the_table_is_read_relative_to_the_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().author_table.is_none());
        std::fs::write(
            dir.path().join("elzevir.toml"),
            "[call_number]\nauthor_table = \"autores.txt\"\n",
        )
        .unwrap();
        assert!(load(dir.path()).unwrap_err().contains("does not exist"));
        std::fs::write(dir.path().join("autores.txt"), "Assi 848\nAb 12\nAc 11\n").unwrap();
        let loaded = load(dir.path()).unwrap();
        assert!(loaded.author_table.is_some());
        let (path, report) = loaded.table_report.unwrap();
        assert!(
            summary(&path, &report)
                .ends_with("3 entries from 3 lines; numbers out of order at lines 3")
        );
        std::fs::write(
            dir.path().join("elzevir.toml"),
            "[call_number]\nauthor_tabel = \"x\"\n",
        )
        .unwrap();
        assert!(load(dir.path()).unwrap_err().contains("unknown field"));
    }

    #[test]
    fn the_time_zone_is_named_or_the_systems() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap().time_zone, TimeZone::system());
        let config = dir.path().join("elzevir.toml");
        std::fs::write(&config, "[library]\ntime_zone = \"America/Sao_Paulo\"\n").unwrap();
        let zone = load(dir.path()).unwrap().time_zone;
        assert_eq!(zone.iana_name(), Some("America/Sao_Paulo"));
        std::fs::write(&config, "[library]\ntime_zone = \"Mars/Olympus\"\n").unwrap();
        assert!(load(dir.path()).unwrap_err().contains("library.time_zone"));
    }

    #[test]
    fn waiting_holds_expire_only_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!load(dir.path()).unwrap().expire_waiting_holds);
        std::fs::write(
            dir.path().join("elzevir.toml"),
            "[circulation.holds]\nexpire_waiting = true\n",
        )
        .unwrap();
        assert!(load(dir.path()).unwrap().expire_waiting_holds);
    }

    #[test]
    fn long_overdue_is_off_unless_its_days_are_given() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap().long_overdue, None);
        let config = dir.path().join("elzevir.toml");
        std::fs::write(&config, "[circulation.long_overdue]\ndays = 60\n").unwrap();
        assert_eq!(
            load(dir.path()).unwrap().long_overdue,
            Some(LongOverdueSettings {
                days: 60,
                max_days: 366,
                lost_status: 1,
                charge: false,
                mark_returned: false,
            })
        );
        std::fs::write(
            &config,
            "[circulation.long_overdue]\ndays = 60\nlost_status = 2\ncharge = true\n",
        )
        .unwrap();
        let settings = load(dir.path()).unwrap().long_overdue.unwrap();
        assert_eq!((settings.lost_status, settings.charge), (2, true));
        std::fs::write(
            &config,
            "[circulation.long_overdue]\ndays = 60\nlost_status = 0\n",
        )
        .unwrap();
        assert!(load(dir.path()).unwrap_err().contains("lost_status"));
        std::fs::write(&config, "[circulation.long_overdue]\nlost_status = 1\n").unwrap();
        assert!(load(dir.path()).unwrap_err().contains("days"));
    }

    #[test]
    fn authority_sources_are_kept_apart_from_draft_sources() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("elzevir.toml");
        std::fs::write(
            &config,
            "[[draft_sources]]\nname = \"Livros\"\nkind = \"sru\"\nurl = \"https://a\"\n\
             [[draft_sources]]\nname = \"Nomes\"\nkind = \"sru\"\nurl = \"https://b\"\n\
             records = \"authority\"\n\
             [draft_sources.fields]\npersonal_name = \"local.personalName\"\nany = \"cql.anywhere\"\n\
             [[draft_sources]]\nname = \"Assuntos\"\nkind = \"z3950\"\nhost = \"z\"\n\
             database = \"auth\"\nrecords = \"authority\"\n",
        )
        .unwrap();
        let loaded = load(dir.path()).unwrap();
        let names = |servers: &[DraftServer]| -> Vec<String> {
            servers
                .iter()
                .map(|server| server.name().to_owned())
                .collect()
        };
        assert_eq!(names(&loaded.draft_sources), ["Livros"]);
        assert_eq!(names(&loaded.authority_sources), ["Nomes", "Assuntos"]);
        let Some(DraftServer::Sru(authority)) = loaded.authority_sources.first() else {
            panic!("{:?}", loaded.authority_sources);
        };
        assert_eq!(
            authority
                .fields
                .get(&DraftField::PersonalName)
                .map(String::as_str),
            Some("local.personalName")
        );
        for (wrong, says) in [
            (
                "[[draft_sources]]\nname = \"A\"\nkind = \"sru\"\nurl = \"https://a\"\n\
                 records = \"authority\"\n[draft_sources.fields]\nisbn = \"bath.isbn\"\n",
                "fields.isbn",
            ),
            (
                "[[draft_sources]]\nname = \"A\"\nkind = \"sru\"\nurl = \"https://a\"\n\
                 [draft_sources.fields]\nsubject = \"dc.subject\"\n",
                "fields.subject",
            ),
            (
                "[[draft_sources]]\nname = \"A\"\nkind = \"sru\"\nurl = \"https://a\"\n\
                 records = \"holdings\"\n",
                "records",
            ),
        ] {
            std::fs::write(&config, wrong).unwrap();
            let error = load(dir.path()).unwrap_err();
            assert!(error.contains(says), "{wrong}: {error}");
        }
    }

    #[test]
    fn z3950_sources_are_read_with_their_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("elzevir.toml"),
            "[[draft_sources]]\nname = \"Z\"\nkind = \"z3950\"\nhost = \" z.example.org \"\n\
             database = \"livros, periodicos\"\n\
             [[draft_sources]]\nname = \"V6\"\nkind = \"z3950\"\nhost = \"::1\"\nport = 2100\n\
             database = \"db\"\nencoding = \"marc-8\"\ntimeout_seconds = 3\n",
        )
        .unwrap();
        let servers = load(dir.path()).unwrap().draft_sources;
        assert_eq!(
            servers,
            [
                DraftServer::Z3950(Z3950Server {
                    name: "Z".to_owned(),
                    host: "z.example.org".to_owned(),
                    port: 210,
                    databases: vec!["livros".to_owned(), "periodicos".to_owned()],
                    charset: RecordCharset::Auto,
                    timeout: DEFAULT_TIMEOUT,
                }),
                DraftServer::Z3950(Z3950Server {
                    name: "V6".to_owned(),
                    host: "::1".to_owned(),
                    port: 2100,
                    databases: vec!["db".to_owned()],
                    charset: RecordCharset::Marc8,
                    timeout: Duration::from_secs(3),
                }),
            ]
        );
    }

    #[test]
    fn draft_sources_are_read_in_order_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().draft_sources.is_empty());
        let config = dir.path().join("elzevir.toml");
        let write = |text: &str| std::fs::write(&config, text).unwrap();
        write(
            "[[draft_sources]]\nname = \"Exemplo\"\nkind = \"sru\"\n\
             url = \"https://sru.example.org/cat/\"\ntimeout_seconds = 5\n\
             [draft_sources.fields]\nisbn = \"bath.isbn\"\nany = \" \"\n\
             [[draft_sources]]\nname = \"Desligada\"\nkind = \"sru\"\n\
             url = \"http://off.example.org\"\nenabled = false\n\
             [[draft_sources]]\nname = \"Segunda\"\nkind = \"sru\"\n\
             url = \"http://dois.example.org/sru\"\nversion = \"2.0\"\n",
        );
        let servers: Vec<SruServer> = load(dir.path())
            .unwrap()
            .draft_sources
            .into_iter()
            .map(|server| match server {
                DraftServer::Sru(server) => server,
                DraftServer::Z3950(other) => panic!("{other:?}"),
            })
            .collect();
        let names: Vec<_> = servers.iter().map(|server| server.name.as_str()).collect();
        assert_eq!(names, ["Exemplo", "Segunda"]);
        let first = &servers[0];
        assert_eq!(first.url, "https://sru.example.org/cat");
        assert_eq!(first.timeout, Duration::from_secs(5));
        assert_eq!(first.record_schema, "marcxml");
        assert_eq!(
            first.fields,
            std::collections::HashMap::from([(DraftField::Isbn, "bath.isbn".to_owned())])
        );
        assert_eq!(
            (servers[1].version, servers[1].timeout),
            (SruVersion::V2_0, DEFAULT_TIMEOUT)
        );
        let entry = |extra: &str| format!("[[draft_sources]]\nname = \"A\"\n{extra}");
        for (wrong, says) in [
            (entry("kind = \"wais\"\nurl = \"https://a\"\n"), "kind"),
            (entry("kind = \"z3950\"\nurl = \"https://a\"\n"), "url"),
            (entry("kind = \"z3950\"\ndatabase = \"a\"\n"), "host"),
            (
                entry("kind = \"z3950\"\nhost = \"z.example.org:210\"\ndatabase = \"a\"\n"),
                "host",
            ),
            (
                entry("kind = \"z3950\"\nhost = \"z3950://z.example.org\"\ndatabase = \"a\"\n"),
                "host",
            ),
            (
                entry("kind = \"z3950\"\nhost = \"z.example.org\"\ndatabase = \"a,\"\n"),
                "database",
            ),
            (
                entry("kind = \"z3950\"\nhost = \"z.example.org\"\n"),
                "database",
            ),
            (
                entry("kind = \"z3950\"\nhost = \"z\"\ndatabase = \"a\"\nport = 0\n"),
                "port",
            ),
            (
                entry("kind = \"z3950\"\nhost = \"z\"\ndatabase = \"a\"\nencoding = \"latin1\"\n"),
                "encoding",
            ),
            (entry("kind = \"sru\"\nurl = \"ftp://a\"\n"), "url"),
            (entry("kind = \"sru\"\nurl = \"https://a?x=1\"\n"), "url"),
            (
                entry("kind = \"sru\"\nurl = \"https://a\"\nversion = \"1.1\"\n"),
                "version",
            ),
            (
                entry("kind = \"sru\"\nurl = \"https://a\"\ntimeout_seconds = 61\n"),
                "timeout",
            ),
            (
                entry("kind = \"sru\"\nurl = \"https://a\"\nport = 80\n"),
                "port",
            ),
            (
                format!("{0}{0}", entry("kind = \"sru\"\nurl = \"https://a\"\n")),
                "repeated",
            ),
            (
                "[[draft_sources]]\nname = \" \"\nkind = \"sru\"\nurl = \"https://a\"\n".to_owned(),
                "name",
            ),
        ] {
            write(&wrong);
            let error = load(dir.path()).unwrap_err();
            assert!(error.contains(says), "{wrong}: {error}");
        }
    }
}
