// SPDX-License-Identifier: GPL-3.0-or-later
//! The responses the server sends, written in Koha's field order (spec items 14 and 15).
//!
//! Koha: `C4::SIP::Sip::MsgType` (`handle_*`, `build_patron_status`, `send_acs_status`).

use jiff::civil::DateTime;

use crate::channel::Version;
use crate::codes::{
    CHARGED_ITEMS, CURRENCY, EMAIL, FEE_AMOUNT, FEE_LIMIT, FEE_TYPE, FINE_ITEMS, HOLD_ITEMS,
    HOME_ADDRESS, HOME_PHONE, INSTITUTION_ID, INTERNET_PROFILE, OVERDUE_ITEMS, PATRON_BIRTHDATE,
    PATRON_CLASS, PATRON_ID, PERSONAL_NAME, PRINT_LINE, RECALL_ITEMS, SCREEN_MESSAGE,
    TRANSACTION_ID, UNAVAILABLE_HOLD_ITEMS, VALID_PATRON, VALID_PATRON_PASSWORD,
};
use crate::request::SummaryKind;
use crate::text::Writer;

mod items;

pub use items::{
    CheckedOut, CheckinItem, CheckinResponse, CheckoutRefused, CheckoutResponse,
    EndSessionResponse, ItemInformationResponse, KnownItem, RenewRefused, RenewResponse, Renewed,
};

/// The messages the Elzevir answers, in the order of the `BX` field: patron status,
/// checkout, checkin, block patron, ACS status, resend, login, patron information, end
/// patron session, fee paid, item information, item status update, patron enable,
/// hold, renew, renew all (spec item 13).
pub const SUPPORTED_MESSAGES: &str = "YYYNYYYYYNYNNNYN";

/// The screen message of a card nobody has.
const INVALID_CARD: &str = "Invalid cardnumber";
/// What the screen message gains when the patron's password is wrong.
const INVALID_PASSWORD: &str = " -- Invalid password";

/// A response to a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// `94`: whether the login worked.
    Login(bool),
    /// `98`.
    AcsStatus(AcsStatus),
    /// `24`.
    PatronStatus(PatronStatusResponse),
    /// `64`.
    PatronInformation(PatronInformationResponse),
    /// `18`.
    ItemInformation(ItemInformationResponse),
    /// `12`.
    Checkout(CheckoutResponse),
    /// `10`.
    Checkin(CheckinResponse),
    /// `30`.
    Renew(RenewResponse),
    /// `36`.
    EndPatronSession(EndSessionResponse),
}

impl Response {
    /// The response as the terminal reads it, without error detection or terminator.
    #[must_use]
    pub fn encode(&self, version: Version) -> String {
        match self {
            Self::Login(ok) => Writer::new("94")
                .fixed(if *ok { "1" } else { "0" })
                .finish(),
            Self::AcsStatus(status) => status.encode(version),
            Self::PatronStatus(status) => status.encode(version),
            Self::PatronInformation(information) => information.encode(),
            Self::ItemInformation(information) => information.encode(),
            Self::Checkout(checkout) => checkout.encode(version),
            Self::Checkin(checkin) => checkin.encode(version),
            Self::Renew(renew) => renew.encode(),
            Self::EndPatronSession(end) => end.encode(),
        }
    }
}

/// `98`: what the server allows the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the message is a fixed row of yes/no answers, one per field"
)]
pub struct AcsStatus {
    /// Returns are allowed.
    pub checkin_ok: bool,
    /// Loans are allowed.
    pub checkout_ok: bool,
    /// The terminal may renew.
    pub renewal_ok: bool,
    /// The terminal may change an item's properties.
    pub status_update_ok: bool,
    /// The terminal may work offline.
    pub offline_ok: bool,
    /// The transaction timeout, in tenths of a second.
    pub timeout: u16,
    /// How many times the terminal may retry.
    pub retries: u16,
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The institution of the terminal's account.
    pub institution: String,
    /// `AF`.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
}

impl AcsStatus {
    fn encode(&self, version: Version) -> String {
        let mut writer = Writer::new("98");
        writer
            .yes_no(true)
            .yes_no(self.checkin_ok)
            .yes_no(self.checkout_ok)
            .yes_no(self.renewal_ok)
            .yes_no(self.status_update_ok)
            .yes_no(self.offline_ok)
            .fixed(&three_digits(self.timeout))
            .fixed(&three_digits(self.retries))
            .timestamp(self.now)
            .fixed(match version {
                Version::V1 => "1.00",
                Version::V2 => "2.00",
            })
            .field(INSTITUTION_ID, &self.institution);
        if version == Version::V2 {
            writer.field(crate::codes::SUPPORTED_MESSAGES, SUPPORTED_MESSAGES);
        }
        writer
            .maybe(SCREEN_MESSAGE, self.screen_message.as_deref())
            .maybe(PRINT_LINE, self.print_line.as_deref())
            .finish()
    }
}

/// `value` in three digits; `000` when it does not fit, as Koha writes it.
fn three_digits(value: u16) -> String {
    if value > 999 {
        "000".to_owned()
    } else {
        format!("{value:03}")
    }
}

/// One of the fourteen positions of a patron's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatronFlag {
    /// The patron may not borrow.
    ChargeDenied,
    /// The patron may not renew.
    RenewalDenied,
    /// The patron may not recall.
    RecallDenied,
    /// The patron may not place holds.
    HoldDenied,
    /// The card is lost.
    CardLost,
    /// Too many loans.
    TooManyCharged,
    /// Too many overdue loans.
    TooManyOverdue,
    /// Too many renewals.
    TooManyRenewals,
    /// Too many items claimed returned.
    TooManyClaimsReturned,
    /// Too many items lost.
    TooManyLost,
    /// Fines over the limit.
    ExcessiveFines,
    /// Fees over the limit.
    ExcessiveFees,
    /// A recalled item is overdue.
    RecallOverdue,
    /// Too many items billed.
    TooManyBilled,
}

/// The patron's status: each flag set is written `Y`, each other one a space.
///
/// Koha: `C4::SIP::Sip::MsgType::patron_status_string`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PatronFlags(u16);

impl PatronFlags {
    /// The status of a card nobody has: the four "denied" positions.
    pub const UNKNOWN: Self = Self(0b1111);

    /// These flags and `flag`.
    #[must_use]
    pub const fn with(self, flag: PatronFlag) -> Self {
        Self(self.0 | (1 << flag as u16))
    }

    /// Whether `flag` is set.
    #[must_use]
    pub const fn contains(self, flag: PatronFlag) -> bool {
        self.0 & (1 << flag as u16) != 0
    }

    fn write(self, writer: &mut Writer) {
        for position in 0..14 {
            writer.yes_blank(self.0 & (1 << position) != 0);
        }
    }
}

/// What the `24` and the `64` say of a patron found by the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPatron {
    /// The status.
    pub flags: PatronFlags,
    /// The patron's language, three digits (`000` unspecified).
    pub language: String,
    /// The card.
    pub id: String,
    /// The name.
    pub name: String,
    /// Whether the password the terminal sent is right; `None` when it sent none
    /// that the message checks.
    pub password_ok: Option<bool>,
    /// `BH`.
    pub currency: Option<String>,
    /// `BV`: what the patron owes.
    pub fee_amount: Option<String>,
    /// `AF`, before the note of a wrong password.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
}

impl KnownPatron {
    /// The screen message, with the note of a wrong password.
    fn screen_message(&self) -> Option<String> {
        let base = self.screen_message.as_deref();
        if self.password_ok == Some(false) {
            Some(format!("{}{INVALID_PASSWORD}", base.unwrap_or_default()))
        } else {
            base.map(str::to_owned)
        }
    }
}

/// `24`: a patron's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronStatusResponse {
    /// The language of the request.
    pub language: String,
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The institution of the request.
    pub institution: String,
    /// The card the terminal sent.
    pub patron_id: String,
    /// The patron; `None` when nobody has the card.
    pub patron: Option<KnownPatron>,
}

impl PatronStatusResponse {
    fn encode(&self, version: Version) -> String {
        let v2 = version == Version::V2;
        let mut writer = Writer::new("24");
        let Some(patron) = &self.patron else {
            PatronFlags::UNKNOWN.write(&mut writer);
            writer
                .fixed(&self.language)
                .timestamp(self.now)
                .field(PERSONAL_NAME, "")
                .field(PATRON_ID, &self.patron_id);
            if v2 {
                writer.field(VALID_PATRON, "N");
            }
            return writer
                .field(SCREEN_MESSAGE, INVALID_CARD)
                .field(INSTITUTION_ID, &self.institution)
                .finish();
        };
        patron.flags.write(&mut writer);
        writer
            .fixed(&self.language)
            .timestamp(self.now)
            .field(PERSONAL_NAME, &patron.name)
            .field(PATRON_ID, &patron.id);
        if v2 {
            writer
                .field(VALID_PATRON, "Y")
                .yes_no_field(VALID_PATRON_PASSWORD, patron.password_ok == Some(true))
                .maybe(CURRENCY, patron.currency.as_deref())
                .maybe(FEE_AMOUNT, patron.fee_amount.as_deref());
        }
        writer
            .maybe(SCREEN_MESSAGE, patron.screen_message().as_deref())
            .maybe(PRINT_LINE, patron.print_line.as_deref())
            .field(INSTITUTION_ID, &self.institution)
            .finish()
    }
}

/// The six counts of a `64`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PatronCounts {
    /// Holds waiting.
    pub holds: usize,
    /// Overdue loans.
    pub overdue: usize,
    /// Loans.
    pub charged: usize,
    /// Items with fines.
    pub fines: usize,
    /// Recalled items.
    pub recalls: usize,
    /// Holds not yet waiting.
    pub unavailable_holds: usize,
}

/// The list of items a `64` sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Which list.
    pub kind: SummaryKind,
    /// The barcodes, in order.
    pub barcodes: Vec<String>,
}

/// What only the `64` says of a patron.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatronDetails {
    /// The counts.
    pub counts: PatronCounts,
    /// `CC`: the most the patron may owe.
    pub fee_limit: String,
    /// The list asked for.
    pub summary: Option<Summary>,
    /// `BD`.
    pub address: Option<String>,
    /// `BE`.
    pub email: Option<String>,
    /// `BF`.
    pub phone: Option<String>,
    /// `PB`.
    pub birthdate: Option<String>,
    /// `PC`.
    pub category: Option<String>,
    /// `PI`.
    pub internet_profile: Option<String>,
}

/// `64`: a patron's status, counts and one list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronInformationResponse {
    /// The language of the request.
    pub language: String,
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The institution of the terminal.
    pub institution: String,
    /// The card the terminal sent.
    pub patron_id: String,
    /// The patron; `None` when nobody has the card.
    pub patron: Option<(KnownPatron, PatronDetails)>,
}

impl PatronInformationResponse {
    fn encode(&self) -> String {
        let mut writer = Writer::new("64");
        let Some((patron, details)) = &self.patron else {
            PatronFlags::UNKNOWN.write(&mut writer);
            return writer
                .fixed(&self.language)
                .timestamp(self.now)
                .fixed(&"0000".repeat(6))
                .field(INSTITUTION_ID, &self.institution)
                .field(PATRON_ID, &self.patron_id)
                .field(PERSONAL_NAME, "")
                .field(VALID_PATRON, "N")
                .field(SCREEN_MESSAGE, INVALID_CARD)
                .finish();
        };
        patron.flags.write(&mut writer);
        let language = if self.language.len() == 3 {
            &self.language
        } else {
            &patron.language
        };
        let counts = details.counts;
        writer
            .fixed(language)
            .timestamp(self.now)
            .count(counts.holds)
            .count(counts.overdue)
            .count(counts.charged)
            .count(counts.fines)
            .count(counts.recalls)
            .count(counts.unavailable_holds)
            .field(INSTITUTION_ID, &self.institution)
            .field(PATRON_ID, &patron.id)
            .field(PERSONAL_NAME, &patron.name)
            .field(VALID_PATRON, "Y");
        if let Some(ok) = patron.password_ok {
            writer.yes_no_field(VALID_PATRON_PASSWORD, ok);
        }
        writer
            .maybe(CURRENCY, patron.currency.as_deref())
            .maybe(FEE_AMOUNT, patron.fee_amount.as_deref())
            .field(FEE_LIMIT, &details.fee_limit);
        if let Some(summary) = &details.summary {
            writer.each(summary_code(summary.kind), &summary.barcodes);
        }
        writer
            .maybe(HOME_ADDRESS, details.address.as_deref())
            .maybe(EMAIL, details.email.as_deref())
            .maybe(HOME_PHONE, details.phone.as_deref())
            .maybe(PATRON_BIRTHDATE, details.birthdate.as_deref())
            .maybe(PATRON_CLASS, details.category.as_deref())
            .maybe(INTERNET_PROFILE, details.internet_profile.as_deref())
            .maybe(SCREEN_MESSAGE, patron.screen_message().as_deref())
            .maybe(PRINT_LINE, patron.print_line.as_deref())
            .finish()
    }
}

/// The field code of each list of a `64`.
const fn summary_code(kind: SummaryKind) -> &'static str {
    match kind {
        SummaryKind::Holds => HOLD_ITEMS,
        SummaryKind::Overdue => OVERDUE_ITEMS,
        SummaryKind::Charged => CHARGED_ITEMS,
        SummaryKind::Fines => FINE_ITEMS,
        SummaryKind::Recalls => RECALL_ITEMS,
        SummaryKind::UnavailableHolds => UNAVAILABLE_HOLD_ITEMS,
    }
}

/// The circulation status of an item, by the standard's codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CirculationStatus {
    /// `01`: other (Koha: damaged).
    Other,
    /// `02`.
    OnOrder,
    /// `03`.
    Available,
    /// `04`.
    Charged,
    /// `05`.
    ChargedRecalled,
    /// `06`.
    InProcess,
    /// `07`.
    Recalled,
    /// `08`.
    WaitingOnHoldShelf,
    /// `09`.
    WaitingToBeReshelved,
    /// `10`.
    InTransit,
    /// `11`.
    ClaimedReturned,
    /// `12`.
    Lost,
    /// `13`.
    Missing,
}

impl CirculationStatus {
    /// The two digits of the status.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Other => "01",
            Self::OnOrder => "02",
            Self::Available => "03",
            Self::Charged => "04",
            Self::ChargedRecalled => "05",
            Self::InProcess => "06",
            Self::Recalled => "07",
            Self::WaitingOnHoldShelf => "08",
            Self::WaitingToBeReshelved => "09",
            Self::InTransit => "10",
            Self::ClaimedReturned => "11",
            Self::Lost => "12",
            Self::Missing => "13",
        }
    }
}

/// An amount an item or a transaction costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fee {
    /// `BV`.
    pub amount: String,
    /// `BH`.
    pub currency: Option<String>,
    /// `BT`.
    pub fee_type: Option<String>,
    /// `BK`.
    pub transaction_id: Option<String>,
}

impl Fee {
    fn write(&self, writer: &mut Writer) {
        writer
            .field(FEE_AMOUNT, &self.amount)
            .maybe(CURRENCY, self.currency.as_deref())
            .maybe(FEE_TYPE, self.fee_type.as_deref())
            .maybe(TRANSACTION_ID, self.transaction_id.as_deref());
    }
}
