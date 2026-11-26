// SPDX-License-Identifier: GPL-3.0-or-later
//! The Item messages: information (`17`), checkout (`11`), checkin (`09`) and renewal
//! (`29`), on the circulation desk of the terminal's branch
//! (docs/specs/circulacao/sip2.md, items 25 to 29).
//!
//! Koha: `C4::SIP::ILS::checkout`, `checkin`, `renew`, `C4::SIP::ILS::Item`,
//! `C4::SIP::ILS::Transaction::Checkout`, `Checkin` and `Renew`.

use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationDeskError, CirculationOutput, CirculationRefusal,
};
use elzevir_app::holds::HoldAside;
use elzevir_app::loans::{
    CheckinRefusal, CheckinRequest, CheckoutRefusal, CheckoutRequest, RenewRefusal, RenewRequest,
};
use elzevir_app::self_check::SelfCheckItem;
use elzevir_domain::catalog::BranchId;
use elzevir_domain::checkin::{Destination, LoanOutcome};
use elzevir_domain::checkout::{Confirm, HeldFor, Impossible};
use elzevir_domain::hold::HoldMatch;
use elzevir_domain::renewal::RenewalRefusal;
use jiff::Zoned;
use jiff::civil::DateTime;

use super::patron::{FoundPatron, find, shown};
use super::{Session, Unavailable};
use crate::request::{Checkin, Checkout, ItemInformation, Renew};
use crate::response::{
    CheckedOut, CheckinItem, CheckinResponse, CheckoutRefused, CheckoutResponse, CirculationStatus,
    ItemInformationResponse, KnownItem, RenewRefused, RenewResponse, Renewed, Response,
};

/// A moment as Koha keeps a due date: cut to the minute, in the library's time zone.
fn minute(at: &Zoned) -> DateTime {
    let civil = at.datetime();
    civil
        .with()
        .second(0)
        .subsec_nanosecond(0)
        .build()
        .unwrap_or(civil)
}

fn item_by(
    session: &Session<'_>,
    barcode: Option<&str>,
) -> Result<Option<SelfCheckItem>, Unavailable> {
    let Some(barcode) = barcode.map(str::trim).filter(|barcode| !barcode.is_empty()) else {
        return Ok(None);
    };
    session
        .desk
        .self_check
        .item(barcode)
        .map_err(|_| Unavailable)
}

/// The circulation status of an Item, ranked as Koha ranks it.
///
/// Koha: `C4::SIP::ILS::Item::sip_circulation_status`.
fn circulation_status(item: &SelfCheckItem) -> CirculationStatus {
    if item.in_transit {
        CirculationStatus::InTransit
    } else if item.lost {
        CirculationStatus::Lost
    } else if item.due.is_some() {
        CirculationStatus::Charged
    } else if item.set_aside {
        CirculationStatus::WaitingOnHoldShelf
    } else if item.damaged {
        CirculationStatus::Other
    } else if item.not_for_loan < 0 {
        CirculationStatus::OnOrder
    } else {
        CirculationStatus::Available
    }
}

/// `17`.
pub(crate) fn item_information(
    session: &Session<'_>,
    asked: &ItemInformation,
    now: &Zoned,
) -> Result<Response, Unavailable> {
    let item = item_by(session, asked.item.as_deref())?.map(|item| KnownItem {
        status: circulation_status(&item),
        security_marker: "02".to_owned(),
        fee_type: "01".to_owned(),
        id: item.barcode.clone(),
        title: item.title.clone(),
        media_type: None,
        permanent_location: Some(item.home_branch.clone()),
        current_location: Some(item.holding_branch.clone()),
        properties: None,
        collection: None,
        fee: None,
        owner: Some(item.home_branch.clone()),
        hold_queue_length: item.holds,
        due: item.due.as_ref().map(minute),
        hold_pickup_date: item.waiting_until,
        screen_message: None,
        print_line: None,
    });
    Ok(Response::ItemInformation(ItemInformationResponse {
        now: now.datetime(),
        item_id: asked.item.clone().unwrap_or_default(),
        item,
    }))
}

/// Why a checkout is refused, as the terminal's screen says it; `None` when it goes on.
fn patron_refusal(found: &FoundPatron) -> Option<String> {
    let standing = found.standing;
    if !standing.blocked() {
        return None;
    }
    Some(if standing.suspended {
        "Patron debarred".to_owned()
    } else if standing.expired {
        format!("Patron expired on {}", shown(found.expires()))
    } else {
        "Patron has fines".to_owned()
    })
}

/// `11`.
pub(crate) fn checkout(
    session: &Session<'_>,
    asked: &Checkout,
    now: &Zoned,
) -> Result<Response, Unavailable> {
    let found = find(session, asked.patron.as_deref(), now)?;
    let item = match &found {
        Some(_) => item_by(session, asked.item.as_deref())?,
        None => None,
    };
    let outcome = match (&found, &item) {
        (None, _) => Err(refused(false, "", "Invalid Patron")),
        (Some(_), None) => Err(refused(true, "", "Invalid Item")),
        (Some(found), Some(item)) => lend(session, found, item, now)?,
    };
    Ok(Response::Checkout(CheckoutResponse {
        now: now.datetime(),
        institution: session.terminal.institution.clone(),
        patron_id: asked.patron.clone().unwrap_or_default(),
        item_id: asked.item.clone().unwrap_or_default(),
        outcome,
        fee: None,
    }))
}

fn refused(patron_known: bool, title: &str, message: &str) -> CheckoutRefused {
    CheckoutRefused {
        title: title.to_owned(),
        patron_known,
        screen_message: Some(message.to_owned()),
        print_line: None,
    }
}

fn lend(
    session: &Session<'_>,
    found: &FoundPatron,
    item: &SelfCheckItem,
    now: &Zoned,
) -> Result<Result<CheckedOut, CheckoutRefused>, Unavailable> {
    let title = item.title.as_str();
    if let Some(message) = patron_refusal(found) {
        return Ok(Err(refused(true, title, &message)));
    }
    let borrower = session
        .desk
        .loans
        .borrower(item.id)
        .map_err(|_| Unavailable)?;
    if borrower.is_some_and(|borrower| borrower != found.patron.id) {
        return Ok(Err(refused(
            true,
            title,
            "Item checked out to another patron",
        )));
    }
    let request = |confirmed| {
        CirculationCommand::Checkout(CheckoutRequest {
            patron: found.patron.id,
            barcode: item.barcode.clone(),
            desk: session.terminal.unit,
            on_site: false,
            confirmed,
            at: now.clone(),
        })
    };
    let mut screen_message = None;
    let mut answer = circulate(session, request(false))?;
    if let Err(CirculationRefusal::Checkout(CheckoutRefusal::NeedsConfirmation(confirms))) = &answer
    {
        match confirmation_refusal(confirms) {
            Err(message) => return Ok(Err(refused(true, title, &message))),
            Ok(message) => {
                screen_message = message;
                answer = circulate(session, request(true))?;
            }
        }
    }
    Ok(match answer {
        Ok(CirculationOutput::CheckedOut(loan)) => Ok(CheckedOut {
            renewal: borrower.is_some(),
            magnetic_media: false,
            desensitize: true,
            title: title.to_owned(),
            due: Some(minute(&loan.due)),
            screen_message,
            print_line: None,
            media_type: None,
            properties: None,
        }),
        Ok(_) => Err(refused(true, title, "Checkout failed")),
        Err(refusal) => Err(refused(true, title, &checkout_message(&refusal))),
    })
}

/// Runs `command` as the terminal's staff account; a refusal is an answer, a failure of
/// storage is not.
fn circulate(
    session: &Session<'_>,
    command: CirculationCommand,
) -> Result<Result<CirculationOutput, CirculationRefusal>, Unavailable> {
    match session.desk.loans.circulate(session.staff.id, command) {
        Ok(output) => Ok(Ok(output)),
        Err(CirculationDeskError::Refused(refusal)) => Ok(Err(refusal)),
        Err(CirculationDeskError::Busy | CirculationDeskError::Unavailable(_)) => Err(Unavailable),
    }
}

/// What the confirmations of a checkout do at a terminal: the message of the one that
/// blocks it, or the message to show while it goes on.
///
/// Koha: `C4::SIP::ILS::Transaction::Checkout::do_checkout`.
fn confirmation_refusal(confirms: &[Confirm]) -> Result<Option<String>, String> {
    let mut message = None;
    let mut blocked = None;
    for confirm in confirms {
        match confirm {
            Confirm::Renew => {
                message = Some("Item already checked out to you: renewing item.".to_owned());
            }
            Confirm::Held(HeldFor::Reserved) => {
                blocked = Some("Item is reserved for another patron upon return.".to_owned());
            }
            Confirm::Held(HeldFor::Waiting | HeldFor::InTransit) => {
                blocked = Some("Item is on hold for another patron.".to_owned());
            }
            Confirm::IssuedToAnother => {
                return Err(
                    "Item already checked out to another patron.  Please return item for check-in."
                        .to_owned(),
                );
            }
            Confirm::Debt(_) => return Err("Outstanding Fines block issue".to_owned()),
            other => return Err(format!("Item cannot be issued: {}", confirm_key(other))),
        }
    }
    blocked.map_or(Ok(message), Err)
}

/// The name Koha gives a confirmation.
const fn confirm_key(confirm: &Confirm) -> &'static str {
    match confirm {
        Confirm::Renew => "RENEW_ISSUE",
        Confirm::IssuedToAnother => "ISSUED_TO_ANOTHER",
        Confirm::Debt(_) => "DEBT",
        Confirm::PatronCannotBorrow => "PATRON_CANT",
        Confirm::TooMany(_) => "TOO_MANY",
        Confirm::NotForLoan(_) => "NOT_FOR_LOAN_FORCING",
        Confirm::Lost(_) => "ITEM_LOST",
        Confirm::Held(HeldFor::Waiting) => "RESERVE_WAITING",
        Confirm::Held(HeldFor::InTransit) => "TRANSFERRED",
        Confirm::Held(HeldFor::Reserved) => "RESERVED",
    }
}

/// The screen message of a checkout the desk refused.
fn checkout_message(refusal: &CirculationRefusal) -> String {
    match refusal {
        CirculationRefusal::Checkout(CheckoutRefusal::UnknownBarcode) => "Invalid Item".to_owned(),
        CirculationRefusal::Checkout(CheckoutRefusal::UnknownPatron) => "Invalid Patron".to_owned(),
        CirculationRefusal::Checkout(CheckoutRefusal::OnLoan) => {
            "Item checked out to another patron".to_owned()
        }
        CirculationRefusal::Checkout(CheckoutRefusal::Impossible(reasons)) => format!(
            "Issue failed : {}",
            reasons.first().map_or("UNKNOWN", impossible_key)
        ),
        CirculationRefusal::Checkout(CheckoutRefusal::NeedsConfirmation(confirms)) => {
            confirmation_refusal(confirms)
                .err()
                .unwrap_or_else(|| "Checkout failed".to_owned())
        }
        _ => "Checkout failed".to_owned(),
    }
}

/// The name Koha gives a reason a checkout is impossible.
const fn impossible_key(reason: &Impossible) -> &'static str {
    match reason {
        Impossible::Suspended(_) => "DEBARRED",
        Impossible::PatronExpired => "EXPIRED",
        Impossible::Debt(_) => "DEBT",
        Impossible::TooMany(_) => "TOO_MANY",
        Impossible::NotForLoan(_) => "NOT_FOR_LOAN",
        Impossible::Withdrawn => "WTHDRAWN",
        Impossible::Restricted => "RESTRICTED",
    }
}

/// `09`.
pub(crate) fn checkin(
    session: &Session<'_>,
    asked: &Checkin,
    now: &Zoned,
) -> Result<Response, Unavailable> {
    let mut response = CheckinResponse {
        now: now.datetime(),
        institution: asked.institution.clone().unwrap_or_default(),
        item_id: asked.item.clone().unwrap_or_default(),
        ok: false,
        resensitize: false,
        alert: true,
        item: None,
        sort_bin: None,
        patron_id: None,
        alert_type: Some("99".to_owned()),
        screen_message: Some("Invalid Item".to_owned()),
        print_line: None,
    };
    let Some(item) = item_by(session, asked.item.as_deref())? else {
        return Ok(Response::Checkin(response));
    };
    response.resensitize = true;
    let mut returned = CheckinItem {
        magnetic_media: false,
        permanent_location: item.home_branch.clone(),
        title: Some(item.title.clone()),
        media_type: None,
        properties: None,
        call_number: item.call_number.clone(),
        hold_patron_id: None,
        destination: None,
        hold_patron_name: None,
        collection: None,
    };
    response.alert_type = None;
    response.screen_message = None;
    let command = CirculationCommand::Checkin(CheckinRequest {
        barcode: item.barcode.clone(),
        desk: session.terminal.unit,
        at: now.clone(),
    });
    match circulate(session, command)? {
        Ok(CirculationOutput::CheckedIn(done)) => {
            take_back(session, &item, &done, &mut response, &mut returned, now)?;
        }
        Err(CirculationRefusal::Checkin(CheckinRefusal::Withdrawn)) => {
            response.alert_type = Some("99".to_owned());
            response.screen_message = Some("Item withdrawn, return not allowed".to_owned());
        }
        Err(CirculationRefusal::Checkin(CheckinRefusal::UnknownBarcode)) => {
            response.alert_type = Some("99".to_owned());
            response.screen_message = Some("Invalid Item".to_owned());
        }
        Ok(_) | Err(_) => response.screen_message = Some("Checkin failed".to_owned()),
    }
    response.alert = !response.ok || response.alert_type.is_some();
    response.item = Some(returned);
    Ok(Response::Checkin(response))
}

fn take_back(
    session: &Session<'_>,
    item: &SelfCheckItem,
    done: &elzevir_app::loans::CheckedIn,
    response: &mut CheckinResponse,
    returned: &mut CheckinItem,
    now: &Zoned,
) -> Result<(), Unavailable> {
    let unit = session.terminal.unit;
    let destination = match done.destination {
        Destination::Sent { to, .. } | Destination::WrongBranch { to } => Some((to, false)),
        Destination::Needs { to, .. } => Some((to, true)),
        Destination::Stays | Destination::Arrived { .. } => None,
    };
    if let Some((to, needs)) = destination {
        let name = branch_name(session, to)?;
        response.alert_type = Some("04".to_owned());
        if needs {
            response.screen_message = Some(format!(
                "This item must still be transferred to {name} branch."
            ));
        }
        returned.destination = Some(name);
    }
    if let Some(hold) = done.hold {
        response.alert_type = Some(if hold.pickup == unit { "01" } else { "02" }.to_owned());
        if let HoldMatch::Reserved(id) = hold.found {
            let aside = CirculationCommand::SetHoldAside(HoldAside {
                hold: id,
                barcode: item.barcode.clone(),
                desk: unit,
                at: now.clone(),
            });
            // A hold that cannot be set aside leaves the Item returned all the same.
            let _set_aside = circulate(session, aside)?.is_ok();
        }
        let holder = session
            .desk
            .self_check
            .hold_patron(hold.found.hold())
            .map_err(|_| Unavailable)?;
        if let Some(holder) = holder {
            let contact = match (holder.email.as_deref(), holder.phone.as_deref()) {
                (Some(email), Some(phone)) => format!(" ({email}, {phone})"),
                (Some(one), None) | (None, Some(one)) => format!(" ({one})"),
                (None, None) => String::new(),
            };
            let first = holder
                .first_name
                .map(|first| format!("{first} "))
                .unwrap_or_default();
            returned.hold_patron_name = Some(format!("{first}{}{contact}", holder.surname));
            returned.hold_patron_id = Some(holder.cardnumber);
        }
        returned.destination = Some(branch_name(session, hold.pickup)?);
    }
    match done.loan {
        LoanOutcome::Ended { .. } => {
            response.ok = true;
            if let Some(patron) = done.patron {
                response.patron_id = session
                    .desk
                    .patrons
                    .patron(patron)
                    .map_err(|_| Unavailable)?
                    .map(|patron| patron.fields.cardnumber);
            }
        }
        LoanOutcome::NotOnLoan => {
            response
                .screen_message
                .get_or_insert_with(|| "Item not checked out".to_owned());
        }
        LoanOutcome::KeptLost => {
            response.alert_type = Some("99".to_owned());
            response.screen_message = Some("Item lost, return not allowed".to_owned());
        }
    }
    Ok(())
}

fn branch_name(session: &Session<'_>, branch: BranchId) -> Result<String, Unavailable> {
    Ok(session
        .desk
        .self_check
        .branch_name(branch)
        .map_err(|_| Unavailable)?
        .unwrap_or_default())
}

/// `29`.
pub(crate) fn renew(
    session: &Session<'_>,
    asked: &Renew,
    now: &Zoned,
) -> Result<Response, Unavailable> {
    let asked_ids = || RenewRefused {
        patron_id: asked.patron.clone().unwrap_or_default(),
        item_id: asked.item.clone().unwrap_or_default(),
        title: asked.title.clone().unwrap_or_default(),
    };
    let respond = |outcome, message: Option<String>| {
        Response::Renew(RenewResponse {
            now: now.datetime(),
            institution: session.terminal.institution.clone(),
            outcome,
            fee: None,
            screen_message: message,
            print_line: None,
        })
    };
    let Some(found) = find(session, asked.patron.as_deref(), now)? else {
        return Ok(respond(
            Err(asked_ids()),
            Some("Invalid patron barcode.".to_owned()),
        ));
    };
    let card = found.patron.fields.cardnumber.clone();
    let with_patron = RenewRefused {
        patron_id: card.clone(),
        ..asked_ids()
    };
    if found.standing.blocked() {
        return Ok(respond(
            Err(with_patron),
            Some("Renewals not allowed.".to_owned()),
        ));
    }
    let barcode = asked.item.clone().unwrap_or_default();
    let Some(loan) = found.loans.iter().find(|loan| loan.barcode == barcode) else {
        let message = format!("Item not checked out to {}", found.name());
        return Ok(respond(Err(with_patron), Some(message)));
    };
    let refused_item = RenewRefused {
        patron_id: card.clone(),
        item_id: loan.barcode.clone(),
        title: loan.title.clone(),
    };
    let command = CirculationCommand::Renew(RenewRequest {
        barcode: loan.barcode.clone(),
        override_limit: false,
        at: now.clone(),
        by_patron: None,
    });
    Ok(match circulate(session, command)? {
        Ok(CirculationOutput::Renewed(renewed)) => respond(
            Ok(Renewed {
                patron_id: card,
                item_id: loan.barcode.clone(),
                title: loan.title.clone(),
                due: Some(minute(&renewed.due)),
                magnetic_media: false,
                desensitize: false,
                media_type: String::new(),
                properties: None,
            }),
            None,
        ),
        Ok(_) => respond(Err(refused_item), None),
        Err(refusal) => respond(Err(refused_item), Some(renewal_message(&refusal))),
    })
}

/// The screen message of a renewal the desk refused: Koha's code, translated where
/// Koha translates it.
///
/// Koha: `C4::SIP::ILS::Transaction::Renew::do_renew_for`.
fn renewal_message(refusal: &CirculationRefusal) -> String {
    let code = match refusal {
        CirculationRefusal::Renew(RenewRefusal::Refused(reason)) => match reason {
            RenewalRefusal::OnHold => "Item unavailable due to outstanding holds",
            RenewalRefusal::TooMany => "Item has reached maximum renewals",
            RenewalRefusal::TooSoon { .. } => "too_soon",
            RenewalRefusal::Overdue => "overdue",
            RenewalRefusal::OnSite => "onsite_checkout",
            RenewalRefusal::TooMuchOwing { .. } => "too_much_owing",
        },
        CirculationRefusal::Renew(RenewRefusal::Restricted) => "restriction",
        CirculationRefusal::Renew(RenewRefusal::NotOnLoan) => "no_checkout",
        CirculationRefusal::Renew(RenewRefusal::UnknownBarcode) => "no_item",
        _ => "renewal failed",
    };
    code.to_owned()
}
