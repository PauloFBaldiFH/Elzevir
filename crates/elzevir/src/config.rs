// SPDX-License-Identifier: GPL-3.0-or-later
//! `elzevir.toml`, the typed configuration of a data directory (doc 04, decision 8, and
//! docs/specs/catalogo/numero-de-chamada.md). Every key is optional; an unknown key is an
//! error, so a typo is not silently ignored.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use elzevir_domain::call_number::{AuthorTable, MAX_TABLE_LINES, TableReport};
use elzevir_domain::long_overdue::{DEFAULT_MAX_DAYS, LongOverdueSettings};
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
    let Some(table) = config.call_number.author_table else {
        return Ok(Loaded {
            author_table: None,
            table_report: None,
            time_zone,
            long_overdue,
            expire_waiting_holds,
            mail,
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
}
