// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron's card and loan cards on the staff side of the wall, and the zoom the page
//! opens in (interface §24).

use axum::http::HeaderMap;
use elzevir_app::loans::{LoanUrgency, PatronLoan};
use elzevir_app::patrons::{Patron, PatronRow, ReadingHistory};
use elzevir_domain::catalog::Money;
use elzevir_domain::patron::is_expired;
use elzevir_web_templates::{I18n, LoanLine, PatronHeader, PatronLine, StatusChip};
use jiff::Zoned;

use crate::circulation::{due_text, money};
use crate::patrons::{full_name, shown};
use crate::staff::cookie;
use crate::staff_bar::query_value;

/// The screens that remember their zoom, each in its own cookie.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ZoomScreen {
    /// The patron's record, which opens full.
    PatronRecord,
    /// The patron search, which opens compact.
    PatronList,
}

impl ZoomScreen {
    const fn cookie(self) -> &'static str {
        match self {
            Self::PatronRecord => "elzevir_zoom_patron",
            Self::PatronList => "elzevir_zoom_patrons",
        }
    }

    const fn default(self) -> &'static str {
        match self {
            Self::PatronRecord => "full",
            Self::PatronList => "compact",
        }
    }
}

/// The zoom `screen` opens in: what this browser chose last, else the screen's own.
pub(crate) fn zoom(headers: &HeaderMap, screen: ZoomScreen) -> &'static str {
    match cookie(headers, screen.cookie()).as_deref() {
        Some("full") => "full",
        Some("compact") => "compact",
        _ => screen.default(),
    }
}

/// Up to two initials: the first name's and the surname's last word's.
fn initials(surname: &str, first_name: Option<&str>) -> String {
    let first_letter = |word: &str| word.chars().next().into_iter().flat_map(char::to_uppercase);
    let words = surname.split_whitespace();
    let last = words.clone().next_back().unwrap_or_default();
    match first_name.and_then(|name| name.split_whitespace().next()) {
        Some(first) => first_letter(first).chain(first_letter(last)).collect(),
        None => words.take(2).flat_map(first_letter).collect(),
    }
}

/// How the patron stands: overdue loans, what they owe and an expired enrolment, or a
/// single "in good standing". Restrictions keep their own line with the lift button.
fn status(t: &I18n, overdue: usize, owed: Money, expired: bool) -> Vec<StatusChip> {
    let mut chips = Vec::new();
    if overdue > 0 {
        chips.push(StatusChip {
            tone: "late",
            icon: "circ-alert",
            text: t.get_numbers(
                "patron-status-overdue",
                &[("count", u64::try_from(overdue).unwrap_or(u64::MAX))],
            ),
        });
    }
    if owed.0 > 0 {
        chips.push(StatusChip {
            tone: "soon",
            icon: "circ-owed",
            text: t.get_with("patron-status-owes", "amount", &money(t, owed)),
        });
    }
    if expired {
        chips.push(StatusChip {
            tone: "late",
            icon: "circ-clock",
            text: t.get("patron-status-expired"),
        });
    }
    if chips.is_empty() {
        chips.push(StatusChip {
            tone: "ok",
            icon: "circ-ok",
            text: t.get("patron-status-clear"),
        });
    }
    chips
}

/// Rows with a value only.
fn present<const N: usize>(
    rows: [(&'static str, Option<String>); N],
) -> Vec<(&'static str, String)> {
    rows.into_iter()
        .filter_map(|(label, value)| value.map(|value| (label, value)))
        .collect()
}

/// The patron's card: name, marks and the three columns of the full view.
pub(crate) fn header(
    t: &I18n,
    patron: &Patron,
    loans: &[PatronLoan],
    now: &Zoned,
    (owed, expired, reveal_cpf): (Money, bool, bool),
) -> PatronHeader {
    let fields = &patron.fields;
    let overdue = loans
        .iter()
        .filter(|loan| loan.urgency(now) == LoanUrgency::Overdue)
        .count();
    let display_name = match &fields.first_name {
        Some(first) => format!("{first} {}", fields.surname),
        None => fields.surname.clone(),
    };
    PatronHeader {
        initials: initials(&fields.surname, fields.first_name.as_deref()),
        display_name,
        cardnumber: fields.cardnumber.clone(),
        category: patron.category_name.clone(),
        branch: patron.branch_name.clone(),
        expires: fields.expires.map(|day| shown(t, day)),
        status: status(t, overdue, owed, expired),
        contact: present([
            ("patron-email", fields.email.clone()),
            ("patron-phone", fields.phone.clone()),
            ("patron-address", fields.address.clone()),
            (
                "patron-date-of-birth",
                fields.date_of_birth.map(|day| shown(t, day)),
            ),
        ]),
        usage: present([
            ("patron-enrolled", Some(shown(t, fields.enrolled))),
            ("patron-expires", fields.expires.map(|day| shown(t, day))),
            (
                "patron-reading-history",
                Some(t.get(match fields.reading_history {
                    ReadingHistory::Kept => "patron-history-kept",
                    ReadingHistory::NotKept => "patron-history-not-kept",
                })),
            ),
        ]),
        cpf_hidden: fields.cpf.is_some() && !reveal_cpf,
        identity: present([
            ("patron-cardnumber", Some(fields.cardnumber.clone())),
            (
                "patron-cpf",
                fields.cpf.as_ref().map(|cpf| {
                    if reveal_cpf {
                        cpf.formatted()
                    } else {
                        cpf.masked()
                    }
                }),
            ),
            ("patron-staff-note", fields.staff_note.clone()),
        ]),
    }
}

/// One patron of the search: a line in the compact view, a card in the full one. Nothing
/// personal beyond the name and card number, and no titles (interface §24.6).
pub(crate) fn patron_line(t: &I18n, row: PatronRow, now: &Zoned) -> PatronLine {
    let overdue = row
        .loans_due
        .iter()
        .filter(|due| LoanUrgency::of(due, now) == LoanUrgency::Overdue)
        .count();
    let expired = is_expired(row.expires, now.date());
    let count = |key: &str, count: u64| t.get_numbers(key, &[("count", count)]);
    let loans = row.loans_due.len();
    PatronLine {
        id: row.id.0,
        initials: initials(&row.surname, row.first_name.as_deref()),
        name: full_name(&row.surname, row.first_name.as_deref()),
        status: status(t, overdue, row.owed, expired),
        loans,
        loans_text: count(
            "patron-list-loans",
            u64::try_from(loans).unwrap_or(u64::MAX),
        ),
        next_due: row.loans_due.iter().min().map(|due| due_text(t, due)),
        holds: row.holds,
        holds_text: count("patron-list-holds", u64::from(row.holds)),
        waiting: (row.holds_waiting > 0)
            .then(|| count("patron-list-waiting", u64::from(row.holds_waiting))),
        owed: if row.owed.0 > 0 {
            money(t, row.owed)
        } else {
            "—".to_owned()
        },
        enrolled: shown(t, row.enrolled),
        history_key: match row.reading_history {
            ReadingHistory::Kept => "patron-history-kept",
            ReadingHistory::NotKept => "patron-history-not-kept",
        },
        cardnumber: row.cardnumber,
        category: row.category_name,
        branch: row.branch_name,
        expires: shown(t, row.expires),
    }
}

/// One loan card.
pub(crate) fn loan_line(t: &I18n, loan: &PatronLoan, now: &Zoned, lent: Option<&str>) -> LoanLine {
    let date = due_text(t, &loan.due);
    let (urgency, key) = match loan.urgency(now) {
        LoanUrgency::Overdue => ("late", "loan-due-late"),
        LoanUrgency::DueSoon => ("soon", "loan-due-soon"),
        LoanUrgency::OnTime => ("ok", "loan-due-ok"),
    };
    LoanLine {
        barcode: loan.barcode.clone(),
        title: loan.title.clone(),
        item: loan.item.0,
        catalog_url: format!("/equipe/acervo?q={}", query_value(&loan.barcode)),
        due: t.get_with(key, "date", &date),
        urgency,
        place: match &loan.call_number {
            Some(call_number) => format!("{call_number} · {}", loan.branch_name),
            None => loan.branch_name.clone(),
        },
        renewals: (loan.renewals > 0)
            .then(|| t.get_numbers("loans-renewals", &[("count", u64::from(loan.renewals))])),
        renewals_left: t.get_numbers(
            "opac-renewals-left",
            &[("count", u64::from(loan.renewals_left))],
        ),
        new: lent == Some(loan.barcode.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_name_and_the_last_surname() {
        assert_eq!(initials("Duarte Lopes", Some("Marina Clara")), "ML");
        assert_eq!(initials("da Costa", Some("ana")), "AC");
        assert_eq!(initials("Biblioteca Municipal", None), "BM");
        assert_eq!(initials("Silva", None), "S");
        assert_eq!(initials("", None), "");
    }
}
