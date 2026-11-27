// SPDX-License-Identifier: GPL-3.0-or-later
//! Koha's system preferences as an `elzevir.toml`: the ones with a field go to it, the
//! others that differ from Koha's default go to `[koha_legacy]` for review
//! (docs/specs/migracao/preferencias-do-koha.md).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::BufRead;

use elzevir_data::{KOHA_SYSPREFS, KohaSyspref, koha_syspref};

use crate::import::koha_sql::{self, DumpError, DumpProblem, MAX_DATABASE_DUMP_BYTES, Target};

/// Most rows read from `systempreferences`; Koha 26.05 has fewer than a thousand.
const MAX_PREFERENCES: usize = 10_000;

/// One row of `systempreferences`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KohaPreference {
    /// `variable`, as written in the dump.
    pub name: String,
    /// `value`, empty for `NULL`.
    pub value: String,
}

/// The kept columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Variable,
    Value,
}

/// The only table read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Preferences;

#[derive(Default)]
struct PreferencesTarget {
    rows: Vec<KohaPreference>,
    /// Names already read, lowercase: the column does not tell case apart.
    seen: BTreeSet<String>,
    declared: bool,
}

impl Target for PreferencesTarget {
    type Table = Preferences;
    type Column = Column;
    const WHOLE_DATABASE: bool = true;
    const MAX_BYTES: u64 = MAX_DATABASE_DUMP_BYTES;

    fn table(&mut self, name: &str, declared: bool) -> Result<Option<Preferences>, DumpProblem> {
        let ours = name == "systempreferences";
        self.declared |= declared && ours;
        Ok(ours.then_some(Preferences))
    }

    fn column(&self, _table: Preferences, name: &str) -> Option<Column> {
        match name {
            "variable" => Some(Column::Variable),
            "value" => Some(Column::Value),
            _ => None,
        }
    }

    fn has_keys(&self, _table: Preferences, layout: &[Option<Column>]) -> bool {
        layout.contains(&Some(Column::Variable))
    }

    fn push(
        &mut self,
        _table: Preferences,
        offset: u64,
        cells: Vec<(Column, Option<Vec<u8>>)>,
    ) -> Result<(), DumpError> {
        let error = |problem| DumpError { offset, problem };
        if self.rows.len() == MAX_PREFERENCES {
            return Err(error(DumpProblem::Limit("system preferences")));
        }
        let mut name = None;
        let mut value = String::new();
        for (column, cell) in cells {
            let text = cell
                .map(String::from_utf8)
                .transpose()
                .map_err(|_| error(DumpProblem::Syntax("a system preference is not UTF-8")))?;
            match column {
                Column::Variable => name = text,
                Column::Value => value = text.unwrap_or_default(),
            }
        }
        let name = name
            .filter(|name| !name.is_empty())
            .ok_or_else(|| error(DumpProblem::Identifier("variable")))?;
        if !self.seen.insert(name.to_lowercase()) {
            return Err(error(DumpProblem::Duplicate("system preference")));
        }
        self.rows.push(KohaPreference { name, value });
        Ok(())
    }

    fn finish(&self, _any: bool) -> Result<(), DumpProblem> {
        if self.declared {
            Ok(())
        } else {
            Err(DumpProblem::MissingPreferences)
        }
    }
}

/// Reads `systempreferences` from a Koha dump, the whole database or the table alone
/// (items 1 and 2).
///
/// # Errors
/// A located, value-free grammar or budget failure, a repeated preference, or a dump
/// without the table.
pub fn read_preferences(input: impl BufRead) -> Result<Vec<KohaPreference>, DumpError> {
    Ok(koha_sql::read(input, PreferencesTarget::default())?.rows)
}

/// What became of the preferences, by name only (item 8).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PrefsReport {
    /// Rows read.
    pub read: usize,
    /// Left out for holding Koha's default.
    pub at_default: usize,
    /// Written to a field: the preference and the field.
    pub placed: Vec<(String, &'static str)>,
    /// Koha 26.05 preferences in `[koha_legacy]`.
    pub legacy: Vec<String>,
    /// Preferences Koha 26.05 does not have, in `[koha_legacy]`.
    pub unknown: Vec<String>,
    /// Passwords and keys not at their default, not copied.
    pub secrets: Vec<String>,
    /// Whether `[oai]` was written turned off, to review and turn on.
    pub oai_to_review: bool,
}

/// The `elzevir.toml` text and its report.
#[derive(Debug)]
pub struct PrefsConfig {
    /// The configuration, as TOML.
    pub toml: String,
    /// What went where.
    pub report: PrefsReport,
}

/// One value written to a field.
enum Setting {
    Integer(i64),
    Boolean(bool),
    Text(String),
    Texts(Vec<String>),
}

/// One table of the configuration.
struct Section {
    name: &'static str,
    /// A comment above it.
    note: Option<&'static str>,
    settings: Vec<(&'static str, Setting)>,
}

impl Section {
    const fn new(name: &'static str, settings: Vec<(&'static str, Setting)>) -> Self {
        Self {
            name,
            note: None,
            settings,
        }
    }
}

/// The preferences as the importer looks them up: by Koha's own spelling of the name.
struct Values<'a> {
    by_name: BTreeMap<&'a str, &'a str>,
    placed: Vec<(String, &'static str)>,
}

impl<'a> Values<'a> {
    fn new(preferences: &'a [KohaPreference]) -> Self {
        let by_name = preferences
            .iter()
            .map(|preference| {
                let name = known(&preference.name).map_or(preference.name.as_str(), |s| s.name);
                (name, preference.value.as_str())
            })
            .collect();
        Self {
            by_name,
            placed: Vec::new(),
        }
    }

    /// The value of `name`, empty when the dump lacks it.
    fn get(&self, name: &str) -> &'a str {
        self.by_name.get(name).copied().unwrap_or_default()
    }

    /// Notes that `name` went to its field.
    fn place(&mut self, name: &str) {
        if let Some(field) = koha_syspref(name).and_then(|syspref| syspref.field) {
            self.placed.push((name.to_owned(), field));
        }
    }

    /// The value of `name`, trimmed, noted as placed, unless empty.
    fn take(&mut self, name: &str) -> Option<&'a str> {
        let value = self.get(name).trim();
        if value.is_empty() {
            return None;
        }
        self.place(name);
        Some(value)
    }

    fn is_placed(&self, name: &str) -> bool {
        self.placed.iter().any(|(placed, _)| placed == name)
    }
}

/// The Koha 26.05 preference spelled `name` in any case.
fn known(name: &str) -> Option<&'static KohaSyspref> {
    koha_syspref(name).or_else(|| {
        KOHA_SYSPREFS
            .iter()
            .find(|syspref| syspref.name.eq_ignore_ascii_case(name))
    })
}

/// Perl's truth: anything but empty and `0`.
fn is_on(value: &str) -> bool {
    !matches!(value, "" | "0")
}

/// `[circulation.long_overdue]`, when Koha's cron job would run without `--lost`.
///
/// Koha: `misc/cronjobs/longoverdue.pl`, which charges only when the charge value equals
/// the lost value.
fn long_overdue(values: &mut Values) -> Option<Vec<(&'static str, Setting)>> {
    let days = values.get("DefaultLongOverdueDays").trim();
    let lost = values.get("DefaultLongOverdueLostValue");
    let days: u32 = days.parse().ok()?;
    let lost_status: i64 = lost.trim().parse().ok().filter(|status| *status > 0)?;
    let charge = values.get("DefaultLongOverdueChargeValue") == lost;
    values.place("DefaultLongOverdueDays");
    values.place("DefaultLongOverdueLostValue");
    values.place("DefaultLongOverdueChargeValue");
    Some(vec![
        ("days", Setting::Integer(i64::from(days))),
        ("lost_status", Setting::Integer(lost_status)),
        ("charge", Setting::Boolean(charge)),
    ])
}

/// `[oai]`, turned off for review, when Koha had the repository on.
fn oai(values: &mut Values) -> Option<Vec<(&'static str, Setting)>> {
    if !is_on(values.get("OAI-PMH")) {
        return None;
    }
    values.place("OAI-PMH");
    let mut settings = vec![("enabled", Setting::Boolean(false))];
    if let Some(identifier) = values.take("OAI-PMH:archiveID") {
        settings.push((
            "repository_identifier",
            Setting::Text(identifier.to_owned()),
        ));
    }
    if let Some(base) = values.take("OPACBaseURL") {
        let base = base.trim_end_matches('/');
        settings.push(("base_url", Setting::Text(format!("{base}/oai"))));
    }
    if let Some(name) = values.take("LibraryName") {
        settings.push(("repository_name", Setting::Text(name.to_owned())));
    }
    if let Some(email) = values.take("KohaAdminEmailAddress") {
        settings.push(("admin_email", Setting::Texts(vec![email.to_owned()])));
    }
    if let Ok(count) = values.get("OAI-PMH:MaxCount").trim().parse::<u32>() {
        values.place("OAI-PMH:MaxCount");
        settings.push(("max_count", Setting::Integer(i64::from(count))));
    }
    Some(settings)
}

/// The `elzevir.toml` for these preferences (items 4 to 8).
#[must_use]
pub fn prefs_config(preferences: &[KohaPreference]) -> PrefsConfig {
    let mut values = Values::new(preferences);
    let mut sections = Vec::new();
    // `date_due` is the Elzevir's default too; only `now` is written.
    if values.get("ManualRenewalPeriodBase").trim() == "now" {
        values.place("ManualRenewalPeriodBase");
        let settings = vec![("renewal_base", Setting::Text("now".to_owned()))];
        sections.push(Section::new("circulation", settings));
    }
    if let Some(settings) = long_overdue(&mut values) {
        sections.push(Section::new("circulation.long_overdue", settings));
    }
    let mut holds = Vec::new();
    if is_on(values.get("ExpireReservesMaxPickUpDelay")) {
        values.place("ExpireReservesMaxPickUpDelay");
        holds.push(("expire_waiting", Setting::Boolean(true)));
    }
    // On is the Elzevir's default too; only Koha's 0 is written.
    if values.get("SuspendHoldsOpac").trim() == "0" {
        values.place("SuspendHoldsOpac");
        holds.push(("patrons_suspend", Setting::Boolean(false)));
    }
    if !holds.is_empty() {
        sections.push(Section::new("circulation.holds", holds));
    }
    // Completing words is the Elzevir's default too; only Koha's 0 is written.
    if values.get("QueryAutoTruncate").trim() == "0" {
        values.place("QueryAutoTruncate");
        let settings = vec![("complete_words", Setting::Boolean(false))];
        sections.push(Section::new("search", settings));
    }
    if let Ok(days) = values.get("PatronAnonymizeDelay").trim().parse::<u32>() {
        values.place("PatronAnonymizeDelay");
        let settings = vec![("anonymize_expired_days", Setting::Integer(i64::from(days)))];
        sections.push(Section::new("privacy", settings));
    }
    let oai = oai(&mut values);
    let oai_to_review = oai.is_some();
    if let Some(settings) = oai {
        let note = "Koha had OAI-PMH on. The repository now answers at /oai: check these \
                    values, tell the harvesters and set enabled = true.";
        sections.push(Section {
            name: "oai",
            note: Some(note),
            settings,
        });
    }
    let mut report = PrefsReport {
        read: preferences.len(),
        oai_to_review,
        ..PrefsReport::default()
    };
    let mut legacy = String::new();
    for (&name, &value) in &values.by_name {
        if values.is_placed(name) {
            continue;
        }
        match koha_syspref(name) {
            Some(syspref) if syspref.default == value => report.at_default += 1,
            Some(syspref) if syspref.secret => {
                report.secrets.push(name.to_owned());
                legacy_secret(&mut legacy, name);
            }
            Some(syspref) => {
                report.legacy.push(name.to_owned());
                legacy_entry(&mut legacy, syspref, value);
            }
            None => {
                report.unknown.push(name.to_owned());
                legacy_unknown(&mut legacy, name, value);
            }
        }
    }
    report.placed = values.placed;
    PrefsConfig {
        toml: toml_text(&sections, &legacy),
        report,
    }
}

// Writing to a `String` cannot fail, hence the ignored `fmt::Result`s below.
fn legacy_entry(out: &mut String, syspref: &KohaSyspref, value: &str) {
    let field = syspref.field.map_or(String::new(), |field| {
        format!("; its field, {field}, does not take this value")
    });
    let _written = writeln!(
        out,
        "# Koha default: {}{field}\n{} = {}",
        string(syspref.default),
        key(syspref.name),
        string(value)
    );
}

fn legacy_secret(out: &mut String, name: &str) {
    let _written = writeln!(out, "# {}: a password or a key, not copied", key(name));
}

fn legacy_unknown(out: &mut String, name: &str, value: &str) {
    let _written = writeln!(
        out,
        "# Not a Koha 26.05 preference\n{} = {}",
        key(name),
        string(value)
    );
}

/// The text of one setting's value.
fn setting(value: &Setting) -> String {
    match value {
        Setting::Integer(number) => number.to_string(),
        Setting::Boolean(on) => on.to_string(),
        Setting::Text(text) => string(text),
        Setting::Texts(texts) => {
            let texts: Vec<String> = texts.iter().map(|text| string(text)).collect();
            format!("[{}]", texts.join(", "))
        }
    }
}

fn toml_text(sections: &[Section], legacy: &str) -> String {
    let mut out = String::from(
        "# Written by `elzevir import-koha-prefs` from Koha's system preferences\n\
         # (docs/specs/migracao/preferencias-do-koha.md).\n",
    );
    for section in sections {
        out.push('\n');
        if let Some(note) = section.note {
            let _written = writeln!(out, "# {note}");
        }
        let _written = writeln!(out, "[{}]", section.name);
        for (name, value) in &section.settings {
            let _written = writeln!(out, "{name} = {}", setting(value));
        }
    }
    if !legacy.is_empty() {
        out.push_str(
            "\n# Koha preferences that differ from Koha's default and have no field here, for\n\
             # review; docs/specs/migracao/sysprefs-do-koha.md says where each one goes. The\n\
             # server does not read this section.\n[koha_legacy]\n",
        );
        out.push_str(legacy);
    }
    out
}

/// A TOML key: bare when it can be, quoted otherwise.
fn key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if bare { name.to_owned() } else { string(name) }
}

/// A TOML basic string, on one line.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _written = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preferences(rows: &[(&str, &str)]) -> Vec<KohaPreference> {
        rows.iter()
            .map(|(name, value)| KohaPreference {
                name: (*name).to_owned(),
                value: (*value).to_owned(),
            })
            .collect()
    }

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn defaults_are_left_out() {
        let config = prefs_config(&preferences(&[
            ("ExpireReservesMaxPickUpDelay", "0"),
            ("SuspendHoldsOpac", "1"),
            ("ManualRenewalPeriodBase", "date_due"),
            ("QueryAutoTruncate", "1"),
            ("OAI-PMH", "0"),
            ("OAI-PMH:archiveID", "KOHA-OAI-TEST"),
            (
                "1PageOrderPDFText",
                "Order number must appear on all related correspondence, shipping papers and invoices. Notify us immediately if \n you are unable to supply item(s).",
            ),
        ]));
        assert_eq!(config.report.at_default, 7);
        assert!(config.report.placed.is_empty());
        assert_eq!(table(&config.toml), toml::Table::new());
    }

    #[test]
    fn fields_get_their_values() {
        let config = prefs_config(&preferences(&[
            ("DefaultLongOverdueDays", "30"),
            ("DefaultLongOverdueLostValue", "2"),
            ("DefaultLongOverdueChargeValue", "2"),
            ("ExpireReservesMaxPickUpDelay", "1"),
            ("SuspendHoldsOpac", "0"),
            ("ManualRenewalPeriodBase", "now"),
            ("PatronAnonymizeDelay", "365"),
            ("QueryAutoTruncate", "0"),
        ]));
        let toml = table(&config.toml);
        assert_eq!(
            toml["circulation"]["long_overdue"],
            table("days = 30\nlost_status = 2\ncharge = true").into()
        );
        assert_eq!(toml["circulation"]["holds"]["expire_waiting"], true.into());
        assert_eq!(
            toml["circulation"]["holds"]["patrons_suspend"],
            false.into()
        );
        assert_eq!(toml["privacy"]["anonymize_expired_days"], 365.into());
        assert_eq!(toml["circulation"]["renewal_base"], "now".into());
        assert_eq!(toml["search"]["complete_words"], false.into());
        assert_eq!(config.report.placed.len(), 8);
        assert!(!toml.contains_key("koha_legacy"));
    }

    #[test]
    fn koha_charges_only_at_the_same_lost_value() {
        let config = prefs_config(&preferences(&[
            ("DefaultLongOverdueDays", "30"),
            ("DefaultLongOverdueLostValue", "1"),
            ("DefaultLongOverdueChargeValue", "2"),
        ]));
        assert_eq!(
            table(&config.toml)["circulation"]["long_overdue"]["charge"],
            false.into()
        );
    }

    #[test]
    fn half_of_the_long_overdue_pair_is_left_for_review() {
        let config = prefs_config(&preferences(&[
            ("DefaultLongOverdueDays", "30"),
            ("DefaultLongOverdueLostValue", ""),
            ("PatronAnonymizeDelay", "a year"),
        ]));
        let toml = table(&config.toml);
        assert!(!toml.contains_key("circulation"));
        assert!(!toml.contains_key("privacy"));
        assert_eq!(
            toml["koha_legacy"],
            table("DefaultLongOverdueDays = \"30\"\nPatronAnonymizeDelay = \"a year\"").into()
        );
        assert_eq!(
            config.report.legacy,
            ["DefaultLongOverdueDays", "PatronAnonymizeDelay"]
        );
        assert!(config.toml.contains(
            "# Koha default: \"\"; its field, [privacy] anonymize_expired_days, does not take this value\n"
        ));
    }

    #[test]
    fn oai_is_written_turned_off() {
        let config = prefs_config(&preferences(&[
            ("OAI-PMH", "1"),
            ("OAI-PMH:archiveID", "biblioteca"),
            ("OAI-PMH:MaxCount", "100"),
            ("LibraryName", "Biblioteca Pública"),
            ("KohaAdminEmailAddress", "biblioteca@example.org"),
            ("OPACBaseURL", "https://opac.example.org/"),
        ]));
        let expected = table(
            "enabled = false\nrepository_identifier = \"biblioteca\"\n\
             base_url = \"https://opac.example.org/oai\"\nrepository_name = \"Biblioteca Pública\"\n\
             admin_email = [\"biblioteca@example.org\"]\nmax_count = 100",
        );
        assert_eq!(table(&config.toml)["oai"], expected.into());
        assert!(config.report.oai_to_review);
        assert_eq!(config.report.placed.len(), 6);
    }

    #[test]
    fn without_oai_its_values_are_reviewed() {
        let config = prefs_config(&preferences(&[("LibraryName", "Biblioteca")]));
        assert_eq!(
            table(&config.toml)["koha_legacy"]["LibraryName"],
            "Biblioteca".into()
        );
    }

    #[test]
    fn unknown_names_secrets_and_case() {
        let config = prefs_config(&preferences(&[
            ("LocalPreference", "x"),
            ("SMSSendPassword", "hunter2"),
            ("opacuserjs", "alert(\"oi\");\n\\"),
            ("expirereservesmaxpickupdelay", "1"),
        ]));
        let toml = table(&config.toml);
        assert_eq!(toml["koha_legacy"]["LocalPreference"], "x".into());
        assert_eq!(
            toml["koha_legacy"]["OPACUserJS"],
            "alert(\"oi\");\n\\".into()
        );
        assert_eq!(toml["circulation"]["holds"]["expire_waiting"], true.into());
        assert!(!config.toml.contains("hunter2"));
        assert!(
            config
                .toml
                .contains("# SMSSendPassword: a password or a key, not copied\n")
        );
        assert_eq!(config.report.unknown, ["LocalPreference"]);
        assert_eq!(config.report.secrets, ["SMSSendPassword"]);
        assert_eq!(config.report.legacy, ["OPACUserJS"]);
    }

    #[test]
    fn keys_and_strings_are_escaped() {
        assert_eq!(key("OAI-PMH:archiveID"), "\"OAI-PMH:archiveID\"");
        assert_eq!(key("OPACUserJS"), "OPACUserJS");
        let text = "a\"b\\c\nd\u{1}";
        let toml = table(&format!("x = {}", string(text)));
        assert_eq!(toml["x"], text.into());
    }

    #[test]
    fn the_table_is_read_from_a_dump() {
        let dump = "CREATE TABLE `systempreferences` (`variable` varchar(50) NOT NULL, \
                    `value` mediumtext, `options` mediumtext);\n\
                    INSERT INTO `systempreferences` VALUES ('OAI-PMH','1',NULL),('LibraryName',NULL,NULL);\n";
        let rows = read_preferences(dump.as_bytes()).unwrap();
        assert_eq!(rows, preferences(&[("OAI-PMH", "1"), ("LibraryName", "")]));
        let repeated =
            format!("{dump}INSERT INTO `systempreferences` VALUES ('oai-pmh','0',NULL);\n");
        assert_eq!(
            read_preferences(repeated.as_bytes()).unwrap_err().problem,
            DumpProblem::Duplicate("system preference")
        );
        assert_eq!(
            read_preferences("CREATE TABLE `items` (`itemnumber` int);\n".as_bytes())
                .unwrap_err()
                .problem,
            DumpProblem::MissingPreferences
        );
    }
}
