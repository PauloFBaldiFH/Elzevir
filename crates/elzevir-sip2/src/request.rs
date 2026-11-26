// SPDX-License-Identifier: GPL-3.0-or-later
//! The messages a terminal sends (spec items 9 to 13).
//!
//! Koha: `C4::SIP::Sip::MsgType` (`%handlers` and `_initialize`).

use crate::channel::Version;
use crate::codes::{
    CANCEL, CURRENT_LOCATION, END_ITEM, FEE_ACKNOWLEDGED, INSTITUTION_ID, ITEM_ID, ITEM_PROPERTIES,
    LOCATION_CODE, LOGIN_PASSWORD, LOGIN_USER, PATRON_ID, PATRON_PASSWORD, START_ITEM,
    TERMINAL_PASSWORD, TITLE_ID,
};

/// A message a terminal sent, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `93`.
    Login(Login),
    /// `99`.
    ScStatus(ScStatus),
    /// `97`: send the last response again.
    Resend,
    /// `23`.
    PatronStatus(PatronStatus),
    /// `63`.
    PatronInformation(PatronInformation),
    /// `17`.
    ItemInformation(ItemInformation),
    /// `11`.
    Checkout(Checkout),
    /// `09`.
    Checkin(Checkin),
    /// `29`.
    Renew(Renew),
    /// `35`.
    EndPatronSession(EndPatronSession),
}

/// `93`: the terminal signs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    /// The cipher of the user (`0`: none).
    pub user_algorithm: String,
    /// The cipher of the password (`0`: none).
    pub password_algorithm: String,
    /// `CN`.
    pub user: Option<String>,
    /// `CO`.
    pub password: Option<String>,
    /// `CP`.
    pub location: Option<String>,
}

/// `99`: the terminal's state and protocol version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScStatus {
    /// `0` working, `1` out of paper, `2` shutting down.
    pub status: String,
    /// The width of the terminal's printer.
    pub print_width: String,
    /// `1.00` or `2.00`.
    pub protocol_version: String,
}

/// `23`: a patron's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronStatus {
    /// The language, three digits.
    pub language: String,
    /// The terminal's date.
    pub date: String,
    /// `AO`.
    pub institution: Option<String>,
    /// `AA`.
    pub patron: Option<String>,
    /// `AC`.
    pub terminal_password: Option<String>,
    /// `AD`.
    pub patron_password: Option<String>,
}

/// `63`: a patron's status, counts and one list of items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronInformation {
    /// The language, three digits.
    pub language: String,
    /// The terminal's date.
    pub date: String,
    /// Ten positions; the first `Y` names the list asked for.
    pub summary: String,
    /// `AO`.
    pub institution: Option<String>,
    /// `AA`.
    pub patron: Option<String>,
    /// `AC`.
    pub terminal_password: Option<String>,
    /// `AD`.
    pub patron_password: Option<String>,
    /// `BP`.
    pub start_item: Option<String>,
    /// `BQ`.
    pub end_item: Option<String>,
}

/// The list of items a `63` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryKind {
    /// Holds waiting for the patron (`AS`).
    Holds,
    /// Overdue loans (`AT`).
    Overdue,
    /// Loans (`AU`).
    Charged,
    /// Items with fines (`AV`).
    Fines,
    /// Recalled items (`BU`).
    Recalls,
    /// Holds not yet waiting (`CD`).
    UnavailableHolds,
}

impl PatronInformation {
    /// The list the summary asks for: the first `Y`, when it is in one of the six
    /// positions the standard defines.
    ///
    /// Koha: `C4::SIP::Sip::MsgType::summary_info`.
    #[must_use]
    pub fn summary_kind(&self) -> Option<SummaryKind> {
        match self.summary.find('Y')? {
            0 => Some(SummaryKind::Holds),
            1 => Some(SummaryKind::Overdue),
            2 => Some(SummaryKind::Charged),
            3 => Some(SummaryKind::Fines),
            4 => Some(SummaryKind::Recalls),
            5 => Some(SummaryKind::UnavailableHolds),
            _ => None,
        }
    }
}

/// `17`: an item's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInformation {
    /// The terminal's date.
    pub date: String,
    /// `AO`.
    pub institution: Option<String>,
    /// `AB`.
    pub item: Option<String>,
    /// `AC`.
    pub terminal_password: Option<String>,
}

/// `11`: lend an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    /// Whether the terminal may renew (`Y`/`N`).
    pub renewal_policy: String,
    /// `Y` when the terminal lent it offline.
    pub no_block: String,
    /// The terminal's date.
    pub date: String,
    /// The due date of an offline loan.
    pub no_block_due_date: String,
    /// `AO`.
    pub institution: Option<String>,
    /// `AA`.
    pub patron: Option<String>,
    /// `AB`.
    pub item: Option<String>,
    /// `AC`.
    pub terminal_password: Option<String>,
    /// `CH` (2.00).
    pub item_properties: Option<String>,
    /// `AD` (2.00).
    pub patron_password: Option<String>,
    /// `BO` (2.00).
    pub fee_acknowledged: Option<String>,
    /// `BI` (2.00).
    pub cancel: Option<String>,
}

/// `09`: return an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkin {
    /// `Y` when the terminal took it offline.
    pub no_block: String,
    /// The terminal's date.
    pub date: String,
    /// The date the item came back.
    pub return_date: String,
    /// `AP`.
    pub current_location: Option<String>,
    /// `AO`.
    pub institution: Option<String>,
    /// `AB`.
    pub item: Option<String>,
    /// `AC`.
    pub terminal_password: Option<String>,
    /// `CH` (2.00).
    pub item_properties: Option<String>,
    /// `BI` (2.00).
    pub cancel: Option<String>,
}

/// `29`: renew a loan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renew {
    /// Whether a third party may renew (`Y`/`N`).
    pub third_party: String,
    /// `Y` when the terminal renewed it offline.
    pub no_block: String,
    /// The terminal's date.
    pub date: String,
    /// The due date of an offline renewal.
    pub no_block_due_date: String,
    /// `AO`.
    pub institution: Option<String>,
    /// `AA`.
    pub patron: Option<String>,
    /// `AD`.
    pub patron_password: Option<String>,
    /// `AB`.
    pub item: Option<String>,
    /// `AJ`.
    pub title: Option<String>,
    /// `AC`.
    pub terminal_password: Option<String>,
    /// `CH`.
    pub item_properties: Option<String>,
    /// `BO`.
    pub fee_acknowledged: Option<String>,
}

/// `35`: the patron is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndPatronSession {
    /// The terminal's date.
    pub date: String,
    /// `AO`.
    pub institution: Option<String>,
    /// `AA`.
    pub patron: Option<String>,
    /// `AC`.
    pub terminal_password: Option<String>,
    /// `AD`.
    pub patron_password: Option<String>,
}

/// The fields of the patron messages: `AO`, `AA`, `AC`, `AD`.
const PATRON_FIELDS: [&str; 4] = [
    INSTITUTION_ID,
    PATRON_ID,
    TERMINAL_PASSWORD,
    PATRON_PASSWORD,
];

impl Request {
    /// `message`, without its error detection, read in protocol `version`; `None`
    /// when the Elzevir does not read that message in that version (spec item 13).
    #[must_use]
    pub fn parse(message: &[u8], version: Version) -> Option<Self> {
        let (code, body) = message.split_first_chunk::<2>()?;
        let v2 = version == Version::V2;
        Some(match code {
            b"93" => login(body),
            b"99" => sc_status(body),
            b"97" => Self::Resend,
            b"23" => patron_status(body),
            b"63" if v2 => patron_information(body),
            b"17" if v2 => item_information(body),
            b"11" => checkout(body, v2),
            b"09" => checkin(body, v2),
            b"29" if v2 => renew(body),
            b"35" if v2 => end_patron_session(body),
            _ => return None,
        })
    }
}

fn login(body: &[u8]) -> Request {
    let mut parts = Parts::of(body, &[1, 1], &[LOGIN_USER, LOGIN_PASSWORD, LOCATION_CODE]);
    Request::Login(Login {
        user_algorithm: parts.fixed(),
        password_algorithm: parts.fixed(),
        user: parts.take(LOGIN_USER),
        password: parts.take(LOGIN_PASSWORD),
        location: parts.take(LOCATION_CODE),
    })
}

fn sc_status(body: &[u8]) -> Request {
    let mut parts = Parts::of(body, &[1, 3, 4], &[]);
    Request::ScStatus(ScStatus {
        status: parts.fixed(),
        print_width: parts.fixed(),
        protocol_version: parts.fixed(),
    })
}

fn patron_status(body: &[u8]) -> Request {
    let mut parts = Parts::of(body, &[3, 18], &PATRON_FIELDS);
    Request::PatronStatus(PatronStatus {
        language: parts.fixed(),
        date: parts.fixed(),
        institution: parts.take(INSTITUTION_ID),
        patron: parts.take(PATRON_ID),
        terminal_password: parts.take(TERMINAL_PASSWORD),
        patron_password: parts.take(PATRON_PASSWORD),
    })
}

fn patron_information(body: &[u8]) -> Request {
    let codes = [
        INSTITUTION_ID,
        PATRON_ID,
        TERMINAL_PASSWORD,
        PATRON_PASSWORD,
        START_ITEM,
        END_ITEM,
    ];
    let mut parts = Parts::of(body, &[3, 18, 10], &codes);
    Request::PatronInformation(PatronInformation {
        language: parts.fixed(),
        date: parts.fixed(),
        summary: parts.fixed(),
        institution: parts.take(INSTITUTION_ID),
        patron: parts.take(PATRON_ID),
        terminal_password: parts.take(TERMINAL_PASSWORD),
        patron_password: parts.take(PATRON_PASSWORD),
        start_item: parts.take(START_ITEM),
        end_item: parts.take(END_ITEM),
    })
}

fn item_information(body: &[u8]) -> Request {
    let mut parts = Parts::of(body, &[18], &[INSTITUTION_ID, ITEM_ID, TERMINAL_PASSWORD]);
    Request::ItemInformation(ItemInformation {
        date: parts.fixed(),
        institution: parts.take(INSTITUTION_ID),
        item: parts.take(ITEM_ID),
        terminal_password: parts.take(TERMINAL_PASSWORD),
    })
}

fn checkout(body: &[u8], v2: bool) -> Request {
    let v1_codes = [INSTITUTION_ID, PATRON_ID, ITEM_ID, TERMINAL_PASSWORD];
    let v2_codes = [ITEM_PROPERTIES, PATRON_PASSWORD, FEE_ACKNOWLEDGED, CANCEL];
    let codes: Vec<&str> = v1_codes
        .into_iter()
        .chain(v2_codes.into_iter().filter(|_| v2))
        .collect();
    let mut parts = Parts::of(body, &[1, 1, 18, 18], &codes);
    Request::Checkout(Checkout {
        renewal_policy: parts.fixed(),
        no_block: parts.fixed(),
        date: parts.fixed(),
        no_block_due_date: parts.fixed(),
        institution: parts.take(INSTITUTION_ID),
        patron: parts.take(PATRON_ID),
        item: parts.take(ITEM_ID),
        terminal_password: parts.take(TERMINAL_PASSWORD),
        item_properties: parts.take(ITEM_PROPERTIES),
        patron_password: parts.take(PATRON_PASSWORD),
        fee_acknowledged: parts.take(FEE_ACKNOWLEDGED),
        cancel: parts.take(CANCEL),
    })
}

fn checkin(body: &[u8], v2: bool) -> Request {
    let v1_codes = [CURRENT_LOCATION, INSTITUTION_ID, ITEM_ID, TERMINAL_PASSWORD];
    let v2_codes = [ITEM_PROPERTIES, CANCEL];
    let codes: Vec<&str> = v1_codes
        .into_iter()
        .chain(v2_codes.into_iter().filter(|_| v2))
        .collect();
    let mut parts = Parts::of(body, &[1, 18, 18], &codes);
    Request::Checkin(Checkin {
        no_block: parts.fixed(),
        date: parts.fixed(),
        return_date: parts.fixed(),
        current_location: parts.take(CURRENT_LOCATION),
        institution: parts.take(INSTITUTION_ID),
        item: parts.take(ITEM_ID),
        terminal_password: parts.take(TERMINAL_PASSWORD),
        item_properties: parts.take(ITEM_PROPERTIES),
        cancel: parts.take(CANCEL),
    })
}

fn renew(body: &[u8]) -> Request {
    let codes = [
        INSTITUTION_ID,
        PATRON_ID,
        PATRON_PASSWORD,
        ITEM_ID,
        TITLE_ID,
        TERMINAL_PASSWORD,
        ITEM_PROPERTIES,
        FEE_ACKNOWLEDGED,
    ];
    let mut parts = Parts::of(body, &[1, 1, 18, 18], &codes);
    Request::Renew(Renew {
        third_party: parts.fixed(),
        no_block: parts.fixed(),
        date: parts.fixed(),
        no_block_due_date: parts.fixed(),
        institution: parts.take(INSTITUTION_ID),
        patron: parts.take(PATRON_ID),
        patron_password: parts.take(PATRON_PASSWORD),
        item: parts.take(ITEM_ID),
        title: parts.take(TITLE_ID),
        terminal_password: parts.take(TERMINAL_PASSWORD),
        item_properties: parts.take(ITEM_PROPERTIES),
        fee_acknowledged: parts.take(FEE_ACKNOWLEDGED),
    })
}

fn end_patron_session(body: &[u8]) -> Request {
    let mut parts = Parts::of(body, &[18], &PATRON_FIELDS);
    Request::EndPatronSession(EndPatronSession {
        date: parts.fixed(),
        institution: parts.take(INSTITUTION_ID),
        patron: parts.take(PATRON_ID),
        terminal_password: parts.take(TERMINAL_PASSWORD),
        patron_password: parts.take(PATRON_PASSWORD),
    })
}

/// A message body cut into its fixed fields and its variable fields.
struct Parts {
    fixed: std::vec::IntoIter<String>,
    fields: Vec<(&'static str, String)>,
}

impl Parts {
    /// `body` cut by the fixed `widths`, then the variable fields with one of `codes`:
    /// fields past a body shorter than the widths are absent, an unknown code is
    /// ignored and a repeated code keeps its first value.
    fn of(body: &[u8], widths: &[usize], codes: &[&'static str]) -> Self {
        let mut rest = body;
        let mut fixed = Vec::with_capacity(widths.len());
        for &width in widths {
            let (value, after) = rest.split_at(width.min(rest.len()));
            fixed.push(trimmed(value));
            rest = after;
        }
        let mut fields: Vec<(&'static str, String)> = Vec::new();
        let template_fits = body.len() >= widths.iter().sum();
        let mut pieces: Vec<&[u8]> = rest.split(|&byte| byte == b'|').collect();
        while pieces.last().is_some_and(|piece| piece.is_empty()) {
            pieces.pop();
        }
        for piece in pieces.into_iter().filter(|_| template_fits) {
            let Some((code, value)) = piece.split_first_chunk::<2>() else {
                continue;
            };
            let known = codes.iter().find(|known| known.as_bytes() == code);
            if let Some(&known) = known
                && !fields.iter().any(|(seen, _)| *seen == known)
            {
                fields.push((known, String::from_utf8_lossy(value).into_owned()));
            }
        }
        Self {
            fixed: fixed.into_iter(),
            fields,
        }
    }

    /// The next fixed field; empty past the last.
    fn fixed(&mut self) -> String {
        self.fixed.next().unwrap_or_default()
    }

    /// The value of the field with `code`.
    fn take(&mut self, code: &str) -> Option<String> {
        let position = self.fields.iter().position(|(seen, _)| *seen == code)?;
        Some(self.fields.swap_remove(position).1)
    }
}

/// A fixed field without its trailing spaces and NULs, as Perl's `unpack "A"` reads it.
fn trimmed(value: &[u8]) -> String {
    let end = value
        .iter()
        .rposition(|&byte| !matches!(byte, b' ' | b'\0' | b'\t' | b'\n' | b'\r' | 0x0c))
        .map_or(0, |last| last + 1);
    String::from_utf8_lossy(value.get(..end).unwrap_or_default()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkout_reads_its_fixed_and_variable_fields() {
        let message = b"11YN20240903    134450                  AOCPL|AA23529000035676|AB39999000003697|AC1234|AD1234|BON|XXignored|AAsecond|";
        let Some(Request::Checkout(checkout)) = Request::parse(message, Version::V2) else {
            panic!("not a checkout");
        };
        assert_eq!(checkout.renewal_policy, "Y");
        assert_eq!(checkout.no_block, "N");
        assert_eq!(checkout.date, "20240903    134450");
        assert_eq!(checkout.no_block_due_date, "");
        assert_eq!(checkout.institution.as_deref(), Some("CPL"));
        assert_eq!(checkout.patron.as_deref(), Some("23529000035676"));
        assert_eq!(checkout.item.as_deref(), Some("39999000003697"));
        assert_eq!(checkout.terminal_password.as_deref(), Some("1234"));
        assert_eq!(checkout.patron_password.as_deref(), Some("1234"));
        assert_eq!(checkout.fee_acknowledged.as_deref(), Some("N"));
        assert_eq!(checkout.cancel, None);

        let Some(Request::Checkout(v1)) = Request::parse(message, Version::V1) else {
            panic!("not a checkout");
        };
        assert_eq!((v1.patron_password, v1.fee_acknowledged), (None, None));
    }

    #[test]
    fn a_short_message_has_the_fixed_fields_that_fit_and_no_variable_ones() {
        let Some(Request::PatronStatus(status)) = Request::parse(b"23001201109", Version::V1)
        else {
            panic!("not a patron status");
        };
        assert_eq!(
            (status.language.as_str(), status.date.as_str()),
            ("001", "201109")
        );
        assert_eq!(status.patron, None);
        let Some(Request::Login(login)) =
            Request::parse(b"9300CNterm1|COt\xffp|CPCPL|CN", Version::V1)
        else {
            panic!("not a login");
        };
        assert_eq!(login.user.as_deref(), Some("term1"));
        assert_eq!(login.password.as_deref(), Some("t\u{fffd}p"));
        assert_eq!(login.location.as_deref(), Some("CPL"));
        assert_eq!(
            Request::parse(b"9903022.00", Version::V1),
            Some(Request::ScStatus(ScStatus {
                status: "0".to_owned(),
                print_width: "302".to_owned(),
                protocol_version: "2.00".to_owned(),
            }))
        );
    }

    #[test]
    fn messages_outside_the_version_or_the_elzevir_are_not_read() {
        let information = b"6300120240903    134450Y         AOCPL|AA1|";
        assert_eq!(Request::parse(information, Version::V1), None);
        let Some(Request::PatronInformation(asked)) = Request::parse(information, Version::V2)
        else {
            panic!("not patron information");
        };
        assert_eq!(asked.summary_kind(), Some(SummaryKind::Holds));
        assert_eq!(
            Request::parse(b"9720240903    134450", Version::V1),
            Some(Request::Resend)
        );
        for code in ["01", "15", "19", "25", "37", "65", "xx"] {
            assert_eq!(Request::parse(code.as_bytes(), Version::V2), None);
        }
        assert_eq!(Request::parse(b"9", Version::V2), None);
    }
}
