// SPDX-License-Identifier: GPL-3.0-or-later
//! How long a late return suspends the patron (docs/specs/circulacao/suspensao.md).

use elzevir_data::RuleKind;
use jiff::Zoned;
use jiff::civil::Date;

use crate::calendar::Calendar;
use crate::checkout::perl_whole;
use crate::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use crate::due_date::{DaysMode, LengthUnit, due_day};
use crate::fine::{FinesCalendar, overdue_units};

/// When a patron's suspension ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SuspensionEnd {
    /// The patron may borrow again on this day.
    Day(Date),
    /// Only when the staff lift it (Koha: `9999-12-31`).
    WhenLifted,
}

/// Whether closed days count in a suspension (`SuspensionsCalendar`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspensionsCalendar {
    /// Only open days count (Koha's default, `noSuspensionsWhenClosed`).
    NoSuspensionsWhenClosed,
    /// Every day counts (`ignoreCalendar`).
    Ignore,
}

/// The library's settings for suspensions, with Koha's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuspensionSettings {
    /// `SuspensionsCalendar`.
    pub calendar: SuspensionsCalendar,
    /// `finesCalendar`, which counts the days late.
    pub fines_calendar: FinesCalendar,
    /// `CumulativeRestrictionPeriods` (Koha: off): a new suspension starts when the
    /// current one ends instead of at the return.
    pub cumulative: bool,
}

impl Default for SuspensionSettings {
    fn default() -> Self {
        Self {
            calendar: SuspensionsCalendar::NoSuspensionsWhenClosed,
            fines_calendar: FinesCalendar::NoFinesWhenClosed,
            cumulative: false,
        }
    }
}

/// A late return, as the suspension reads it.
#[derive(Debug, Clone, Copy)]
pub struct LateReturn<'a> {
    /// The circulation rules.
    pub rules: &'a [CirculationRule],
    /// The control branch, the patron's category and the Item's policy.
    pub at: RuleScopeKey,
    /// When the loan was due.
    pub due: &'a Zoned,
    /// When it was returned.
    pub returned: &'a Zoned,
    /// The calendar of the control branch.
    pub calendar: &'a Calendar,
    /// The end of the patron's current suspension, while it lasts.
    pub suspended_until: Option<Date>,
}

/// The day a suspension for `late` ends, if the rules suspend at all.
///
/// The suspension is `finedays` for each day late past the grace (`firstremind`), divided
/// by `suspension_chargeperiod` when it is over 1 and capped by `maxsuspensiondays`;
/// counted from the return, or from the end of the current suspension with
/// `CumulativeRestrictionPeriods`, over open days with `noSuspensionsWhenClosed`.
///
/// Loans in hours do not suspend yet: Koha mixes hours and days there, and that has not
/// been checked against the oracle (spec, "Fora desta parte").
///
/// Koha: `C4::Circulation::_calculate_new_debar_dt`.
#[must_use]
pub fn suspension_until(late: &LateReturn<'_>, settings: &SuspensionSettings) -> Option<Date> {
    let value = |kind| effective_rule(late.rules, kind, &late.at).map(|rule| rule.value.as_str());
    if value(RuleKind::Lengthunit) == Some("hours") {
        return None;
    }
    let fine_days = i64::from(perl_whole(value(RuleKind::Finedays)?));
    if fine_days == 0 {
        return None;
    }
    let units = overdue_units(
        LengthUnit::Days,
        late.due,
        late.returned,
        late.calendar,
        settings.fines_calendar,
    );
    let grace = value(RuleKind::Firstremind).map_or(0, |grace| i64::from(perl_whole(grace)));
    if units - grace <= 0 {
        return None;
    }
    let mut days = units.saturating_mul(fine_days);
    if let Some(period) = value(RuleKind::SuspensionChargeperiod).map(perl_whole)
        && period > 1
    {
        days /= i64::from(period);
    }
    if let Some(max) = value(RuleKind::Maxsuspensiondays).filter(|max| !max.is_empty()) {
        days = days.min(i64::from(perl_whole(max)));
    }
    let start = match late.suspended_until {
        Some(until) if settings.cumulative => until,
        _ => late.returned.date(),
    };
    let days = u32::try_from(days).ok()?;
    match settings.calendar {
        SuspensionsCalendar::NoSuspensionsWhenClosed => {
            due_day(start, days, DaysMode::Calendar, late.calendar).ok()
        }
        SuspensionsCalendar::Ignore => start
            .checked_add(jiff::Span::new().try_days(i64::from(days)).ok()?)
            .ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{Weekday, date};

    fn rule(kind: RuleKind, value: &str) -> CirculationRule {
        CirculationRule {
            kind,
            scope: RuleScopeKey::default(),
            value: value.to_owned(),
        }
    }

    fn on(month: i8, day: i8) -> Zoned {
        date(2026, month, day)
            .at(10, 0, 0, 0)
            .in_tz("America/Sao_Paulo")
            .unwrap()
    }

    fn until(
        rules: &[CirculationRule],
        calendar: &Calendar,
        settings: SuspensionSettings,
    ) -> Option<Date> {
        let (due, returned) = (on(11, 2), on(11, 6));
        suspension_until(
            &LateReturn {
                rules,
                at: RuleScopeKey::default(),
                due: &due,
                returned: &returned,
                calendar,
                suspended_until: Some(date(2026, 11, 30)),
            },
            &settings,
        )
    }

    #[test]
    fn days_late_times_finedays_counted_from_the_return() {
        let open = Calendar::always_open();
        let defaults = SuspensionSettings::default();
        assert_eq!(until(&[], &open, defaults), None);
        let rules = [rule(RuleKind::Finedays, "2")];
        // Four days late, two days each: eight days from the 6th.
        assert_eq!(until(&rules, &open, defaults), Some(date(2026, 11, 14)));
        let graced = [
            rule(RuleKind::Finedays, "2"),
            rule(RuleKind::Firstremind, "4"),
        ];
        assert_eq!(until(&graced, &open, defaults), None);
        let shaped = [
            rule(RuleKind::Finedays, "2"),
            rule(RuleKind::SuspensionChargeperiod, "3"),
            rule(RuleKind::Maxsuspensiondays, "1"),
        ];
        assert_eq!(until(&shaped, &open, defaults), Some(date(2026, 11, 7)));
        let cumulative = SuspensionSettings {
            cumulative: true,
            ..defaults
        };
        assert_eq!(until(&rules, &open, cumulative), Some(date(2026, 12, 8)));
        let hourly = [
            rule(RuleKind::Finedays, "2"),
            rule(RuleKind::Lengthunit, "hours"),
        ];
        assert_eq!(until(&hourly, &open, defaults), None);
    }

    #[test]
    fn closed_days_neither_count_as_late_nor_as_suspended() {
        let sundays = Calendar::always_open().closed_every(Weekday::Sunday);
        let rules = [rule(RuleKind::Finedays, "1")];
        let defaults = SuspensionSettings::default();
        // From Monday the 2nd to Friday the 6th: four days late, no Sunday between; four
        // open days from Friday skip Sunday the 8th.
        assert_eq!(until(&rules, &sundays, defaults), Some(date(2026, 11, 11)));
        let ignore = SuspensionSettings {
            calendar: SuspensionsCalendar::Ignore,
            ..defaults
        };
        assert_eq!(until(&rules, &sundays, ignore), Some(date(2026, 11, 10)));
    }
}
