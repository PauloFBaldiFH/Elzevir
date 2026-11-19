// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether a loan may be renewed (docs/specs/circulacao/renovacao.md).
//!
//! Holds, fines and restrictions that block a renewal come with their tasks (doc 03,
//! tasks 3.7 and 3.8, and the suspensions of task 3.5).

use elzevir_data::RuleKind;
use jiff::{Span, Zoned};

use crate::checkout::{perl_number_ceiling, perl_whole};
use crate::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};

/// When overdue loans block a renewal (`OverduesBlockRenewing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverduesBlockRenewing {
    /// They do not (Koha's default, `allow`).
    Allow,
    /// Any overdue loan of the patron blocks every renewal (`block`).
    Block,
    /// Only the loan itself being overdue blocks it (`blockitem`).
    BlockItem,
}

/// How `norenewalbefore` counts (`NoRenewalBeforePrecision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoRenewalBeforePrecision {
    /// From the due date and time (Koha's default, `exact_time`).
    ExactTime,
    /// From the start of that day, for loans in days (`date`).
    Date,
}

/// The library's settings for renewals, with Koha's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenewalSettings {
    /// `OverduesBlockRenewing`.
    pub overdues: OverduesBlockRenewing,
    /// `NoRenewalBeforePrecision`.
    pub precision: NoRenewalBeforePrecision,
}

impl Default for RenewalSettings {
    fn default() -> Self {
        Self {
            overdues: OverduesBlockRenewing::Allow,
            precision: NoRenewalBeforePrecision::ExactTime,
        }
    }
}

/// The loan to renew, as the check reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewalFacts<'a> {
    /// The circulation rules.
    pub rules: &'a [CirculationRule],
    /// The control branch, the patron's category and the Item's policy.
    pub at: RuleScopeKey,
    /// When the loan is due.
    pub due: Zoned,
    /// How many times it was renewed.
    pub renewals: u32,
    /// Whether it is a loan inside the library.
    pub on_site: bool,
    /// Whether another loan of the patron is overdue.
    pub other_overdue: bool,
    /// When the renewal is asked for.
    pub now: Zoned,
}

/// Why a loan may not be renewed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenewalRefusal {
    /// A loan inside the library is not renewed (Koha: `onsite_checkout`).
    #[error("an on-site loan is not renewed")]
    OnSite,
    /// No more renewals are allowed (Koha: `too_many`).
    #[error("no more renewals are allowed")]
    TooMany,
    /// An overdue loan blocks it (Koha: `overdue`).
    #[error("an overdue loan blocks the renewal")]
    Overdue,
    /// It is too soon (Koha: `too_soon`).
    #[error("too soon to renew")]
    TooSoon {
        /// The soonest the loan may be renewed.
        soonest: Zoned,
    },
}

/// Whether the loan in `facts` may be renewed; `override_limit` is the staff overriding
/// the count, the overdue block and the earliest date.
///
/// Like Koha, a missing, empty or zero `renewalsallowed` allows none, and its value is
/// read as a Perl number.
///
/// # Errors
///
/// [`RenewalRefusal`] with the first reason, in Koha's order.
///
/// Koha: `C4::Circulation::CanBookBeRenewed`, `C4::Circulation::GetSoonestRenewDate`,
/// without holds, recalls, bookings, fines, restrictions and automatic renewals.
pub fn can_renew(
    facts: &RenewalFacts<'_>,
    settings: &RenewalSettings,
    override_limit: bool,
) -> Result<(), RenewalRefusal> {
    if facts.on_site {
        return Err(RenewalRefusal::OnSite);
    }
    if override_limit {
        return Ok(());
    }
    let value = |kind| effective_rule(facts.rules, kind, &facts.at).map(|rule| rule.value.as_str());
    let allowed = value(RuleKind::Renewalsallowed).map_or(0, perl_number_ceiling);
    if u64::from(facts.renewals) >= allowed {
        return Err(RenewalRefusal::TooMany);
    }
    let this_overdue = facts.due < facts.now;
    let blocked = match settings.overdues {
        OverduesBlockRenewing::Allow => false,
        OverduesBlockRenewing::Block => this_overdue || facts.other_overdue,
        OverduesBlockRenewing::BlockItem => this_overdue,
    };
    if blocked {
        return Err(RenewalRefusal::Overdue);
    }
    if let Some(soonest) = soonest(facts, *settings, value(RuleKind::Norenewalbefore))
        && soonest > facts.now
    {
        return Err(RenewalRefusal::TooSoon { soonest });
    }
    Ok(())
}

/// The soonest the loan may be renewed, when `norenewalbefore` is set.
fn soonest(
    facts: &RenewalFacts<'_>,
    settings: RenewalSettings,
    before: Option<&str>,
) -> Option<Zoned> {
    let before = i64::from(perl_whole(before.filter(|value| !value.is_empty())?));
    let hours = effective_rule(facts.rules, RuleKind::Lengthunit, &facts.at)
        .is_some_and(|rule| rule.value == "hours");
    let span = if hours {
        Span::new().try_hours(before).ok()?
    } else {
        Span::new().try_days(before).ok()?
    };
    let soonest = facts.due.checked_sub(span).ok()?;
    if settings.precision == NoRenewalBeforePrecision::Date && !hours {
        return soonest.start_of_day().ok();
    }
    Some(soonest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{BranchId, PolicyId};
    use crate::patron::PatronCategoryId;

    const AT: RuleScopeKey = RuleScopeKey {
        branch: Some(BranchId(1)),
        category: Some(PatronCategoryId(1)),
        policy: Some(PolicyId(1)),
    };

    fn rule(kind: RuleKind, value: &str) -> CirculationRule {
        CirculationRule {
            kind,
            scope: RuleScopeKey::default(),
            value: value.to_owned(),
        }
    }

    fn on(day: i8, hour: i8) -> Zoned {
        jiff::civil::date(2026, 11, day)
            .at(hour, 0, 0, 0)
            .in_tz("America/Sao_Paulo")
            .unwrap()
    }

    fn facts(rules: &[CirculationRule]) -> RenewalFacts<'_> {
        RenewalFacts {
            rules,
            at: AT,
            due: on(26, 23),
            renewals: 0,
            on_site: false,
            other_overdue: false,
            now: on(20, 10),
        }
    }

    fn check(facts: &RenewalFacts<'_>) -> Result<(), RenewalRefusal> {
        can_renew(facts, &RenewalSettings::default(), false)
    }

    #[test]
    fn renewals_are_counted_against_the_rule() {
        assert_eq!(check(&facts(&[])), Err(RenewalRefusal::TooMany));
        for none in ["", "0", "many"] {
            let rules = [rule(RuleKind::Renewalsallowed, none)];
            assert_eq!(check(&facts(&rules)), Err(RenewalRefusal::TooMany));
        }
        let rules = [rule(RuleKind::Renewalsallowed, "2")];
        assert_eq!(check(&facts(&rules)), Ok(()));
        let used = RenewalFacts {
            renewals: 2,
            ..facts(&rules)
        };
        assert_eq!(check(&used), Err(RenewalRefusal::TooMany));
        assert_eq!(can_renew(&used, &RenewalSettings::default(), true), Ok(()));
        let on_site = RenewalFacts {
            on_site: true,
            ..facts(&rules)
        };
        assert_eq!(
            can_renew(&on_site, &RenewalSettings::default(), true),
            Err(RenewalRefusal::OnSite)
        );
    }

    #[test]
    fn overdue_loans_block_as_the_setting_says() {
        let rules = [rule(RuleKind::Renewalsallowed, "2")];
        let late = RenewalFacts {
            now: on(27, 10),
            ..facts(&rules)
        };
        assert_eq!(check(&late), Ok(()));
        let item = RenewalSettings {
            overdues: OverduesBlockRenewing::BlockItem,
            ..RenewalSettings::default()
        };
        assert_eq!(can_renew(&late, &item, false), Err(RenewalRefusal::Overdue));
        let other = RenewalFacts {
            other_overdue: true,
            ..facts(&rules)
        };
        assert_eq!(can_renew(&other, &item, false), Ok(()));
        let block = RenewalSettings {
            overdues: OverduesBlockRenewing::Block,
            ..RenewalSettings::default()
        };
        assert_eq!(
            can_renew(&other, &block, false),
            Err(RenewalRefusal::Overdue)
        );
    }

    #[test]
    fn a_renewal_may_wait_for_its_earliest_date() {
        let rules = [
            rule(RuleKind::Renewalsallowed, "2"),
            rule(RuleKind::Norenewalbefore, "3"),
        ];
        // Due on the 26th at 23:00: renewable from the 23rd at 23:00.
        assert_eq!(
            check(&facts(&rules)),
            Err(RenewalRefusal::TooSoon {
                soonest: on(23, 23)
            })
        );
        let by_date = RenewalSettings {
            precision: NoRenewalBeforePrecision::Date,
            ..RenewalSettings::default()
        };
        let on_the_day = RenewalFacts {
            now: on(23, 10),
            ..facts(&rules)
        };
        assert_eq!(can_renew(&on_the_day, &by_date, false), Ok(()));
        assert!(matches!(
            check(&on_the_day),
            Err(RenewalRefusal::TooSoon { .. })
        ));
        let empty = [
            rule(RuleKind::Renewalsallowed, "2"),
            rule(RuleKind::Norenewalbefore, ""),
        ];
        assert_eq!(check(&facts(&empty)), Ok(()));
    }
}
