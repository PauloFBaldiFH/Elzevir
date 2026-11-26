// SPDX-License-Identifier: GPL-3.0-or-later
//! The arguments of an OAI-PMH request, checked (docs/specs/opac/oai-pmh.md, items 5, 6,
//! 13 and 14).
//!
//! Koha: `HTTP::OAI::Repository::validate_request` and `Koha::OAI::Server::ResumptionToken`.

use std::fmt;

use elzevir_domain::catalog::InstanceId;

/// The longest value of an argument.
const MAX_ARGUMENT: usize = 256;
/// The longest resumption token.
const MAX_TOKEN: usize = 512;

/// The six verbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// `Identify`.
    Identify,
    /// `ListMetadataFormats`.
    ListMetadataFormats,
    /// `ListSets`.
    ListSets,
    /// `GetRecord`.
    GetRecord,
    /// `ListIdentifiers`.
    ListIdentifiers,
    /// `ListRecords`.
    ListRecords,
}

impl Verb {
    const ALL: [Self; 6] = [
        Self::Identify,
        Self::ListMetadataFormats,
        Self::ListSets,
        Self::GetRecord,
        Self::ListIdentifiers,
        Self::ListRecords,
    ];

    /// The verb's name in the protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Identify => "Identify",
            Self::ListMetadataFormats => "ListMetadataFormats",
            Self::ListSets => "ListSets",
            Self::GetRecord => "GetRecord",
            Self::ListIdentifiers => "ListIdentifiers",
            Self::ListRecords => "ListRecords",
        }
    }

    /// The arguments the verb takes besides `verb`.
    const fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Identify => &[],
            Self::ListMetadataFormats => &["identifier"],
            Self::ListSets => &["resumptionToken"],
            Self::GetRecord => &["identifier", "metadataPrefix"],
            Self::ListIdentifiers | Self::ListRecords => {
                &["metadataPrefix", "from", "until", "set", "resumptionToken"]
            }
        }
    }
}

/// The formats a record goes out in (item 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataFormat {
    /// Simple Dublin Core.
    OaiDc,
    /// MARCXML, under Koha's `marc21` prefix.
    Marc21,
    /// MARCXML, under Koha's `marcxml` prefix.
    MarcXml,
}

impl MetadataFormat {
    /// Every format, in the order `ListMetadataFormats` lists them.
    pub const ALL: [Self; 3] = [Self::OaiDc, Self::Marc21, Self::MarcXml];

    /// The `metadataPrefix`.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::OaiDc => "oai_dc",
            Self::Marc21 => "marc21",
            Self::MarcXml => "marcxml",
        }
    }

    /// The schema, as Koha lists it.
    #[must_use]
    pub const fn schema(self) -> &'static str {
        match self {
            Self::OaiDc => "http://www.openarchives.org/OAI/2.0/oai_dc.xsd",
            Self::Marc21 | Self::MarcXml => {
                "http://www.loc.gov/standards/marcxml/schema/MARC21slim.xsd"
            }
        }
    }

    /// The namespace, as Koha lists it.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::OaiDc => "http://www.openarchives.org/OAI/2.0/oai_dc/",
            Self::Marc21 | Self::MarcXml => {
                "http://www.loc.gov/MARC21/slim http://www.loc.gov/standards/marcxml/schema/MARC21slim"
            }
        }
    }

    fn from_prefix(prefix: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.prefix() == prefix)
    }
}

/// A UTC time to the second, `AAAA-MM-DDThh:mm:ssZ`: text that sorts as time does.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Datestamp(String);

impl Datestamp {
    /// `text` when it is a time to the second in UTC.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (day, time) = text.strip_suffix('Z')?.split_once('T')?;
        let valid = day.len() == 10
            && time.len() == 8
            && jiff::civil::Date::strptime("%Y-%m-%d", day).is_ok()
            && jiff::civil::Time::strptime("%H:%M:%S", time).is_ok();
        valid.then(|| Self(text.to_owned()))
    }

    /// The time `timestamp` names, to the second.
    #[must_use]
    pub fn of(timestamp: jiff::Timestamp) -> Self {
        Self(timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Datestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The OAI-PMH error codes the repository answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OaiErrorCode {
    /// `badArgument`.
    BadArgument,
    /// `badResumptionToken`.
    BadResumptionToken,
    /// `badVerb`.
    BadVerb,
    /// `cannotDisseminateFormat`.
    CannotDisseminateFormat,
    /// `idDoesNotExist`.
    IdDoesNotExist,
    /// `noRecordsMatch`.
    NoRecordsMatch,
    /// `noSetHierarchy`.
    NoSetHierarchy,
}

impl OaiErrorCode {
    /// The code in the protocol.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::BadArgument => "badArgument",
            Self::BadResumptionToken => "badResumptionToken",
            Self::BadVerb => "badVerb",
            Self::CannotDisseminateFormat => "cannotDisseminateFormat",
            Self::IdDoesNotExist => "idDoesNotExist",
            Self::NoRecordsMatch => "noRecordsMatch",
            Self::NoSetHierarchy => "noSetHierarchy",
        }
    }
}

/// Where a list stands: the first page, or the state a resumption token carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListState {
    /// The format of the records.
    pub format: MetadataFormat,
    /// How many records earlier pages gave.
    pub cursor: u64,
    /// The earliest datestamp, inclusive.
    pub from: Option<Datestamp>,
    /// The latest datestamp, inclusive.
    pub until: Option<Datestamp>,
    /// The first Instance id still to give.
    pub next_id: i64,
}

impl ListState {
    /// The token of the page after this one, starting at `next_id` after `given` more
    /// records, in Koha's form `<prefix>/<cursor>/<from>/<until>/<set>/<next id>`.
    #[must_use]
    pub fn token(&self, given: u64, next_id: i64) -> String {
        let text = |datestamp: &Option<Datestamp>| {
            datestamp
                .as_ref()
                .map(Datestamp::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        format!(
            "{}/{}/{}/{}//{next_id}",
            self.format.prefix(),
            self.cursor.saturating_add(given),
            text(&self.from),
            text(&self.until),
        )
    }

    /// The state a token carries; `None` when it is not one this repository gave.
    fn from_token(token: &str) -> Option<Self> {
        if token.len() > MAX_TOKEN {
            return None;
        }
        let parts: Vec<&str> = token.split('/').collect();
        let [prefix, cursor, from, until, set, next_id] = parts.as_slice() else {
            return None;
        };
        let bound = |text: &str| {
            if text.is_empty() {
                Some(None)
            } else {
                Datestamp::parse(text).map(Some)
            }
        };
        let next_id: i64 = next_id.parse().ok()?;
        (set.is_empty() && next_id >= 0).then_some(())?;
        Some(Self {
            format: MetadataFormat::from_prefix(prefix)?,
            cursor: cursor.parse().ok()?,
            from: bound(from)?,
            until: bound(until)?,
            next_id,
        })
    }
}

/// A request whose arguments passed the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `Identify`.
    Identify,
    /// `ListMetadataFormats`, for one record or for the repository.
    ListMetadataFormats {
        /// The record asked about.
        identifier: Option<String>,
    },
    /// `ListSets`.
    ListSets,
    /// `GetRecord`.
    GetRecord {
        /// The record.
        identifier: String,
        /// Its format.
        format: MetadataFormat,
    },
    /// `ListIdentifiers` (`records` false) or `ListRecords`.
    List {
        /// Whether the records go out whole.
        records: bool,
        /// Where the list stands.
        state: ListState,
        /// Whether the request carried a resumption token.
        resumed: bool,
        /// Whether a set was asked for.
        set: bool,
    },
}

impl Request {
    /// The verb.
    #[must_use]
    pub const fn verb(&self) -> Verb {
        match self {
            Self::Identify => Verb::Identify,
            Self::ListMetadataFormats { .. } => Verb::ListMetadataFormats,
            Self::ListSets => Verb::ListSets,
            Self::GetRecord { .. } => Verb::GetRecord,
            Self::List { records: false, .. } => Verb::ListIdentifiers,
            Self::List { records: true, .. } => Verb::ListRecords,
        }
    }
}

/// One error, with its message.
pub(super) type OaiError = (OaiErrorCode, String);

/// The arguments checked: the request, or every error found.
///
/// # Errors
///
/// The `badVerb`, `badArgument`, `badResumptionToken` and `cannotDisseminateFormat`
/// errors of the arguments, all of them.
pub fn parse(arguments: &[(String, String)]) -> Result<Request, Vec<OaiError>> {
    let value = |name: &str| {
        arguments
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let verbs = arguments.iter().filter(|(key, _)| key == "verb").count();
    let verb =
        value("verb").and_then(|name| Verb::ALL.into_iter().find(|verb| verb.name() == name));
    let (Some(verb), 1) = (verb, verbs) else {
        return Err(vec![(
            OaiErrorCode::BadVerb,
            "Illegal OAI verb or verb repeated".to_owned(),
        )]);
    };
    let mut errors = argument_errors(arguments, verb);
    if let Some(token) = value("resumptionToken")
        && arguments.len() > 2
    {
        errors.push(bad_argument(format!(
            "resumptionToken {token} cannot go with other arguments"
        )));
    }
    let format = value("metadataPrefix").map(|prefix| {
        MetadataFormat::from_prefix(prefix).ok_or_else(|| {
            (
                OaiErrorCode::CannotDisseminateFormat,
                format!("Dissemination as '{prefix}' is not supported"),
            )
        })
    });
    if let Some(Err(error)) = &format {
        errors.push(error.clone());
    }
    let format = format.and_then(Result::ok);
    let request = match verb {
        Verb::Identify => Some(Request::Identify),
        Verb::ListMetadataFormats => Some(Request::ListMetadataFormats {
            identifier: value("identifier").map(str::to_owned),
        }),
        Verb::ListSets => Some(Request::ListSets),
        Verb::GetRecord => match (value("identifier"), format) {
            (Some(identifier), Some(format)) => Some(Request::GetRecord {
                identifier: identifier.to_owned(),
                format,
            }),
            _ => None,
        },
        Verb::ListIdentifiers | Verb::ListRecords => list(verb, value, format, &mut errors),
    };
    match request {
        Some(request) if errors.is_empty() => Ok(request),
        _ => Err(errors),
    }
}

/// Repeated, unknown, missing and too long arguments.
fn argument_errors(arguments: &[(String, String)], verb: Verb) -> Vec<OaiError> {
    let allowed = verb.arguments();
    let mut errors = Vec::new();
    for (index, (name, value)) in arguments.iter().enumerate() {
        if name == "verb" {
            continue;
        }
        if !allowed.contains(&name.as_str()) {
            errors.push(bad_argument(format!(
                "Illegal argument {name} for {}",
                verb.name()
            )));
        } else if arguments
            .iter()
            .take(index)
            .any(|(earlier, _)| earlier == name)
        {
            errors.push(bad_argument(format!("Argument {name} repeated")));
        } else if value.chars().count() > MAX_ARGUMENT {
            errors.push(bad_argument(format!("Argument {name} is too long")));
        }
    }
    let has = |name: &str| arguments.iter().any(|(key, _)| key == name);
    let required: &[&str] = match verb {
        Verb::GetRecord => &["identifier", "metadataPrefix"],
        Verb::ListIdentifiers | Verb::ListRecords if !has("resumptionToken") => &["metadataPrefix"],
        _ => &[],
    };
    for name in required {
        if !has(name) {
            errors.push(bad_argument(format!("Missing argument {name}")));
        }
    }
    errors
}

/// The list a `ListIdentifiers` or `ListRecords` asks for.
fn list<'a>(
    verb: Verb,
    value: impl Fn(&str) -> Option<&'a str>,
    format: Option<MetadataFormat>,
    errors: &mut Vec<OaiError>,
) -> Option<Request> {
    let records = verb == Verb::ListRecords;
    if let Some(token) = value("resumptionToken") {
        let Some(state) = ListState::from_token(token) else {
            errors.push((
                OaiErrorCode::BadResumptionToken,
                "The resumptionToken is invalid".to_owned(),
            ));
            return None;
        };
        return Some(Request::List {
            records,
            state,
            resumed: true,
            set: false,
        });
    }
    let (from, until) = bounds(value("from"), value("until"), errors)?;
    Some(Request::List {
        records,
        state: ListState {
            format: format?,
            cursor: 0,
            from,
            until,
            next_id: 0,
        },
        resumed: false,
        set: value("set").is_some(),
    })
}

/// `from` and `until`, a day widened to its first and last second.
fn bounds(
    from: Option<&str>,
    until: Option<&str>,
    errors: &mut Vec<OaiError>,
) -> Option<(Option<Datestamp>, Option<Datestamp>)> {
    let read = |text: Option<&str>, time: &str| match text {
        None => Ok(None),
        Some(text) if text.len() == 10 => Datestamp::parse(&format!("{text}T{time}Z"))
            .map(|datestamp| Some((datestamp, true)))
            .ok_or(()),
        Some(text) => Datestamp::parse(text)
            .map(|datestamp| Some((datestamp, false)))
            .ok_or(()),
    };
    let (Ok(from), Ok(until)) = (read(from, "00:00:00"), read(until, "23:59:59")) else {
        errors.push(bad_argument(
            "from and until are YYYY-MM-DD or YYYY-MM-DDThh:mm:ssZ".to_owned(),
        ));
        return None;
    };
    if let (Some((from, from_day)), Some((until, until_day))) = (&from, &until) {
        if from_day != until_day {
            errors.push(bad_argument(
                "from and until have different granularities".to_owned(),
            ));
            return None;
        }
        if from > until {
            errors.push(bad_argument("from is after until".to_owned()));
            return None;
        }
    }
    Some((
        from.map(|(datestamp, _)| datestamp),
        until.map(|(datestamp, _)| datestamp),
    ))
}

fn bad_argument(message: String) -> OaiError {
    (OaiErrorCode::BadArgument, message)
}

/// The Instance an identifier names, when it has the repository's prefix.
#[must_use]
pub(super) fn instance_of(identifier: &str, repository_identifier: &str) -> Option<InstanceId> {
    let id = identifier
        .strip_prefix(repository_identifier)?
        .strip_prefix(':')?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    id.parse().ok().filter(|id| *id > 0).map(InstanceId)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn codes(pairs: &[(&str, &str)]) -> Vec<OaiErrorCode> {
        parse(&arguments(pairs))
            .unwrap_err()
            .into_iter()
            .map(|(code, _)| code)
            .collect()
    }

    #[test]
    fn a_missing_repeated_or_unknown_verb_is_bad() {
        assert_eq!(codes(&[]), [OaiErrorCode::BadVerb]);
        assert_eq!(codes(&[("verb", "Delete")]), [OaiErrorCode::BadVerb]);
        assert_eq!(
            codes(&[("verb", "Identify"), ("verb", "Identify")]),
            [OaiErrorCode::BadVerb]
        );
    }

    #[test]
    fn every_bad_argument_is_reported() {
        assert_eq!(
            codes(&[("verb", "Identify"), ("identifier", "x")]),
            [OaiErrorCode::BadArgument]
        );
        assert_eq!(
            codes(&[("verb", "GetRecord"), ("metadataPrefix", "mods")]),
            [
                OaiErrorCode::BadArgument,
                OaiErrorCode::CannotDisseminateFormat
            ]
        );
        assert_eq!(
            codes(&[
                ("verb", "ListRecords"),
                ("metadataPrefix", "oai_dc"),
                ("metadataPrefix", "oai_dc")
            ]),
            [OaiErrorCode::BadArgument]
        );
        assert_eq!(
            codes(&[
                ("verb", "ListRecords"),
                ("resumptionToken", "oai_dc/0////1"),
                ("from", "2026-01-01")
            ]),
            [OaiErrorCode::BadArgument]
        );
    }

    #[test]
    fn from_and_until_take_a_day_or_a_second() {
        let state = |pairs: &[(&str, &str)]| match parse(&arguments(pairs)).unwrap() {
            Request::List { state, .. } => state,
            other => panic!("{other:?}"),
        };
        let listed = state(&[
            ("verb", "ListIdentifiers"),
            ("metadataPrefix", "marc21"),
            ("from", "2026-11-01"),
            ("until", "2026-11-26"),
        ]);
        assert_eq!(listed.from.unwrap().as_str(), "2026-11-01T00:00:00Z");
        assert_eq!(listed.until.unwrap().as_str(), "2026-11-26T23:59:59Z");
        for (from, until) in [
            ("2026-11-01", "2026-11-26T10:00:00Z"),
            ("2026-11-27", "2026-11-26"),
            ("2026-13-01", "2026-11-26"),
            ("2026-11-01T10:00:00", "2026-11-26T10:00:00Z"),
        ] {
            assert_eq!(
                codes(&[
                    ("verb", "ListIdentifiers"),
                    ("metadataPrefix", "marc21"),
                    ("from", from),
                    ("until", until)
                ]),
                [OaiErrorCode::BadArgument],
                "{from} {until}"
            );
        }
    }

    #[test]
    fn a_token_carries_the_list_and_nothing_else() {
        let state = ListState {
            format: MetadataFormat::OaiDc,
            cursor: 50,
            from: Datestamp::parse("2026-11-01T00:00:00Z"),
            until: None,
            next_id: 0,
        };
        let token = state.token(50, 131);
        assert_eq!(token, "oai_dc/100/2026-11-01T00:00:00Z///131");
        assert_eq!(
            ListState::from_token(&token).unwrap(),
            ListState {
                cursor: 100,
                next_id: 131,
                ..state
            }
        );
        for bad in [
            "oai_dc/1////",
            "mods/0////1",
            "oai_dc/0///set/1",
            "oai_dc/-1////1",
            "oai_dc/0////-1",
            "oai_dc/0/yesterday///1",
            "oai_dc/0////1/2",
        ] {
            assert_eq!(ListState::from_token(bad), None, "{bad}");
        }
    }

    #[test]
    fn an_identifier_names_an_instance_of_this_repository() {
        assert_eq!(instance_of("ELZ:42", "ELZ"), Some(InstanceId(42)));
        for other in [
            "ELZ:0", "ELZ:-1", "ELZ:4a", "ELZ:", "KOHA:42", "ELZ42", "ELZ:+4",
        ] {
            assert_eq!(instance_of(other, "ELZ"), None, "{other}");
        }
    }
}
