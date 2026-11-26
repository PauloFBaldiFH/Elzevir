// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron messages: status (`23`) and information (`63`)
//! (docs/specs/circulacao/sip2.md, items 20 to 23).
//!
//! Koha: `C4::SIP::ILS::Patron::new`, `C4::SIP::Sip::MsgType::handle_patron_status` and
//! `handle_patron_info`.

use elzevir_app::holds::PatronHold;
use elzevir_app::loans::PatronLoan;
use elzevir_app::patrons::Patron;
use elzevir_app::self_check::{Standing, perl_number, two_decimals};
use elzevir_app::staff::verify_password;
use elzevir_domain::catalog::Money;
use elzevir_domain::hold::Found;
use jiff::Zoned;
use jiff::civil::Date;

use super::{Session, Unavailable};
use crate::request::{PatronInformation, PatronStatus, SummaryKind};
use crate::response::{
    KnownPatron, PatronCounts, PatronDetails, PatronFlag, PatronFlags, PatronInformationResponse,
    PatronStatusResponse, Response, Summary,
};

/// The debt limit Koha takes when none is set (`noissuescharge || 5`).
const DEFAULT_LIMIT: Money = Money(500);

/// A patron found by the card, with what the terminal is told about them.
pub(crate) struct FoundPatron {
    pub(crate) patron: Patron,
    pub(crate) password_hash: Option<String>,
    pub(crate) loans: Vec<PatronLoan>,
    pub(crate) holds: Vec<PatronHold>,
    pub(crate) owed: Money,
    pub(crate) balance: Money,
    pub(crate) standing: Standing,
}

impl FoundPatron {
    /// `<first name> <surname>`.
    pub(crate) fn name(&self) -> String {
        let fields = &self.patron.fields;
        format!(
            "{} {}",
            fields.first_name.as_deref().unwrap_or_default(),
            fields.surname
        )
    }

    /// Whether `password` is the patron's OPAC password.
    pub(crate) fn password_matches(&self, password: &str) -> bool {
        self.password_hash
            .as_deref()
            .is_some_and(|hash| verify_password(password, hash))
    }

    /// The last day of the enrolment.
    pub(crate) fn expires(&self) -> Date {
        let fields = &self.patron.fields;
        fields.expires.unwrap_or(fields.enrolled)
    }
}

/// The patron with `card`, read at `now`.
pub(crate) fn find(
    session: &Session<'_>,
    card: Option<&str>,
    now: &Zoned,
) -> Result<Option<FoundPatron>, Unavailable> {
    let desk = session.desk;
    let Some(card) = card.map(str::trim).filter(|card| !card.is_empty()) else {
        return Ok(None);
    };
    let credentials = desk.accounts.credentials(card).map_err(|_| Unavailable)?;
    let Some(credentials) = credentials else {
        return Ok(None);
    };
    let Some(patron) = desk
        .patrons
        .patron(credentials.id)
        .map_err(|_| Unavailable)?
    else {
        return Ok(None);
    };
    let id = credentials.id;
    let today = now.date();
    let restricted = !desk
        .loans
        .restrictions(id, today)
        .map_err(|_| Unavailable)?
        .is_empty();
    let owed = desk.loans.owed(id).map_err(|_| Unavailable)?;
    let balance = desk
        .loans
        .account(id)
        .map_err(|_| Unavailable)?
        .iter()
        .fold(Money(0), |sum, line| Money(sum.0 + line.outstanding.0));
    let loans = desk.loans.loans(id).map_err(|_| Unavailable)?;
    let holds = desk.loans.holds(id).map_err(|_| Unavailable)?;
    let expires = patron.fields.expires.unwrap_or(patron.fields.enrolled);
    let standing = Standing::of(restricted, expires, today, owed, &desk.debt);
    Ok(Some(FoundPatron {
        patron,
        password_hash: credentials.password_hash,
        loans,
        holds,
        owed,
        balance,
        standing,
    }))
}

/// A date as the screen messages write it.
pub(crate) fn shown(date: Date) -> String {
    date.strftime("%d/%m/%Y").to_string()
}

/// What the `24` and the `64` say of a patron found, with `password_ok` as the message
/// checks it.
fn known(session: &Session<'_>, found: &FoundPatron, password_ok: Option<bool>) -> KnownPatron {
    let standing = found.standing;
    let mut flags = PatronFlags::default();
    if standing.blocked() {
        for flag in [
            PatronFlag::ChargeDenied,
            PatronFlag::RenewalDenied,
            PatronFlag::RecallDenied,
            PatronFlag::HoldDenied,
        ] {
            flags = flags.with(flag);
        }
    }
    if standing.over_limit {
        flags = flags
            .with(PatronFlag::ExcessiveFines)
            .with(PatronFlag::ExcessiveFees);
    }
    let mut message = "Greetings from Elzevir. ".to_owned();
    if standing.expired {
        message.push_str("Your account has expired as of ");
        message.push_str(&shown(found.expires()));
    }
    if standing.over_limit {
        message.push_str(" -- Patron blocked by fines");
    }
    if found.owed.0 > 0 {
        message.push_str(" -- Patron owes ");
        message.push_str(&two_decimals(found.owed));
    } else if found.balance.0 < 0 {
        message.push_str(" -- Patron has credit of ");
        message.push_str(&two_decimals(Money(-found.balance.0)));
    }
    KnownPatron {
        flags,
        language: "000".to_owned(),
        id: found.patron.fields.cardnumber.clone(),
        name: found.name(),
        password_ok,
        currency: session.desk.currency.clone(),
        fee_amount: Some(perl_number(found.owed)),
        screen_message: Some(message),
        print_line: None,
    }
}

/// `23`.
pub(crate) fn status(
    session: &Session<'_>,
    asked: &PatronStatus,
    now: &Zoned,
) -> Result<Response, Unavailable> {
    let found = find(session, asked.patron.as_deref(), now)?;
    // Koha checks the password only when one that Perl reads as true was sent.
    let password = asked
        .patron_password
        .as_deref()
        .filter(|password| !password.is_empty() && *password != "0");
    let patron = found.as_ref().map(|found| {
        let ok = password.map(|password| found.password_matches(password));
        known(session, found, ok)
    });
    Ok(Response::PatronStatus(PatronStatusResponse {
        language: asked.language.clone(),
        now: now.datetime(),
        institution: asked.institution.clone().unwrap_or_default(),
        patron_id: asked.patron.clone().unwrap_or_default(),
        patron,
    }))
}

/// `63`.
pub(crate) fn information(
    session: &Session<'_>,
    asked: &PatronInformation,
    now: &Zoned,
) -> Result<Response, Unavailable> {
    let found = find(session, asked.patron.as_deref(), now)?;
    let patron = found.as_ref().map(|found| {
        let ok = asked
            .patron_password
            .as_deref()
            .map(|password| found.password_matches(password));
        (
            known(session, found, ok),
            details(session, found, asked, now),
        )
    });
    Ok(Response::PatronInformation(PatronInformationResponse {
        language: asked.language.clone(),
        now: now.datetime(),
        institution: session.terminal.institution.clone(),
        patron_id: asked.patron.clone().unwrap_or_default(),
        patron,
    }))
}

fn details(
    session: &Session<'_>,
    found: &FoundPatron,
    asked: &PatronInformation,
    now: &Zoned,
) -> PatronDetails {
    let overdue: Vec<String> = found
        .loans
        .iter()
        .filter(|loan| loan.due.timestamp() < now.timestamp())
        .map(|loan| loan.barcode.clone())
        .collect();
    let charged: Vec<String> = found
        .loans
        .iter()
        .map(|loan| loan.barcode.clone())
        .collect();
    let hold_barcode = |hold: &PatronHold| hold.barcode.clone().unwrap_or_default();
    let holds: Vec<String> = found.holds.iter().map(hold_barcode).collect();
    let unavailable: Vec<String> = found
        .holds
        .iter()
        .filter(|hold| hold.found != Some(Found::Waiting))
        .map(hold_barcode)
        .collect();
    let counts = PatronCounts {
        holds: holds.len(),
        overdue: overdue.len(),
        charged: charged.len(),
        fines: 0,
        recalls: 0,
        unavailable_holds: unavailable.len(),
    };
    let summary = asked.summary_kind().map(|kind| {
        let list = match kind {
            SummaryKind::Holds => holds,
            SummaryKind::Overdue => overdue,
            SummaryKind::Charged => charged,
            SummaryKind::UnavailableHolds => unavailable,
            SummaryKind::Fines | SummaryKind::Recalls => Vec::new(),
        };
        Summary {
            kind,
            barcodes: slice(list, asked.start_item.as_deref(), asked.end_item.as_deref()),
        }
    });
    let fields = &found.patron.fields;
    let limit = session
        .desk
        .debt
        .limit
        .filter(|limit| limit.0 != 0)
        .unwrap_or(DEFAULT_LIMIT);
    PatronDetails {
        counts,
        fee_limit: perl_number(limit),
        summary,
        address: fields.address.clone(),
        email: fields.email.clone(),
        phone: fields.phone.clone(),
        birthdate: fields
            .date_of_birth
            .map(|date| date.strftime("%Y%m%d").to_string()),
        category: Some(found.patron.category_name.clone()),
        internet_profile: Some(if found.standing.blocked() { "N" } else { "Y" }.to_owned()),
    }
}

/// The items from `start` to `end`, counted from 1, as Koha's `x_items` cuts a list.
fn slice(list: Vec<String>, start: Option<&str>, end: Option<&str>) -> Vec<String> {
    let number = |text: Option<&str>| text.and_then(|text| text.trim().parse::<usize>().ok());
    let skipped = number(start)
        .filter(|&start| start > 1)
        .map_or(0, |start| start - 1);
    let taken = number(end)
        .filter(|&end| end > 0 && end < list.len())
        .map_or(list.len(), |end| end);
    list.into_iter().take(taken).skip(skipped).collect()
}

#[cfg(test)]
mod tests {
    use super::slice;

    #[test]
    fn a_summary_is_cut_from_the_first_to_the_last_item_asked() {
        let list = || (1..=5).map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(slice(list(), None, None).len(), 5);
        assert_eq!(slice(list(), Some("2"), Some("3")), ["2", "3"]);
        assert_eq!(slice(list(), Some("0"), Some("9")).len(), 5);
        assert_eq!(slice(list(), Some("4"), None), ["4", "5"]);
    }
}
