// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether a loan may be renewed (docs/specs/circulacao/renovacao.md).
//!
//! Restrictions do not block a renewal (`RestrictionBlockRenewing` is off by default); the
//! OPAC refuses a restricted patron before asking (docs/specs/opac/conta-do-leitor.md).

use elzevir_data::RuleKind;
use jiff::{Span, Zoned};

use crate::catalog::Money;
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
    /// `FineNoRenewals`: a balance above it blocks; `None` never blocks.
    pub fine_limit: Option<Money>,
}

/// Koha's default `FineNoRenewals`, 100 in the currency.
pub const DEFAULT_FINE_LIMIT: Money = Money(10_000);

impl Default for RenewalSettings {
    fn default() -> Self {
        Self {
            overdues: OverduesBlockRenewing::Allow,
            precision: NoRenewalBeforePrecision::ExactTime,
            fine_limit: Some(DEFAULT_FINE_LIMIT),
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
    /// The patron's balance: debits less unused credits (`FineNoRenewalsIncludeCredits` on,
    /// Koha's default).
    pub balance: Money,
    /// Whether a hold waits for this Item, on it or on its record (Koha: `on_reserve`).
    pub on_hold: bool,
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
    /// The patron owes more than `FineNoRenewals` (Koha: `too_much_owing`).
    #[error("the patron owes {owed:?}, more than the renewal limit")]
    TooMuchOwing {
        /// The balance.
        owed: Money,
    },
    /// Someone else is waiting for the Item (Koha: `on_reserve`); not overridden.
    #[error("the item is on hold")]
    OnHold,
    /// It is too soon (Koha: `too_soon`).
    #[error("too soon to renew")]
    TooSoon {
        /// The soonest the loan may be renewed.
        soonest: Zoned,
    },
}

/// How many renewals the rules allow the loan at `at` (`renewalsallowed`, read as a Perl
/// number; missing, empty or zero allows none).
#[must_use]
pub fn renewals_allowed(rules: &[CirculationRule], at: &RuleScopeKey) -> u64 {
    effective_rule(rules, RuleKind::Renewalsallowed, at)
        .map_or(0, |rule| perl_number_ceiling(rule.value.as_str()))
}

/// Whether the loan in `facts` may be renewed; `override_limit` is the staff overriding
/// the count, the overdue and fine blocks and the earliest date, but not a hold.
///
/// Like Koha, a missing, empty or zero `renewalsallowed` allows none, and its value is
/// read as a Perl number.
///
/// # Errors
///
/// [`RenewalRefusal`] with the first reason, in Koha's order.
///
/// Koha: `C4::Circulation::CanBookBeRenewed`, `C4::Circulation::GetSoonestRenewDate`,
/// without recalls, bookings, unseen renewals and automatic renewals.
pub fn can_renew(
    facts: &RenewalFacts<'_>,
    settings: &RenewalSettings,
    override_limit: bool,
) -> Result<(), RenewalRefusal> {
    if facts.on_site {
        return Err(RenewalRefusal::OnSite);
    }
    if !override_limit {
        limits(facts, settings)?;
    }
    if facts.on_hold {
        return Err(RenewalRefusal::OnHold);
    }
    if override_limit {
        return Ok(());
    }
    let value = |kind| effective_rule(facts.rules, kind, &facts.at).map(|rule| rule.value.as_str());
    if let Some(soonest) = soonest(facts, *settings, value(RuleKind::Norenewalbefore))
        && soonest > facts.now
    {
        return Err(RenewalRefusal::TooSoon { soonest });
    }
    Ok(())
}

/// The count, the overdue block and the fine block, which the staff may override.
fn limits(facts: &RenewalFacts<'_>, settings: &RenewalSettings) -> Result<(), RenewalRefusal> {
    if u64::from(facts.renewals) >= renewals_allowed(facts.rules, &facts.at) {
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
    if settings
        .fine_limit
        .is_some_and(|limit| facts.balance.0 > limit.0)
    {
        return Err(RenewalRefusal::TooMuchOwing {
            owed: facts.balance,
        });
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
            balance: Money(0),
            on_hold: false,
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

    #[test]
    fn a_balance_over_the_limit_blocks_and_a_hold_blocks_even_an_override() {
        let rules = [rule(RuleKind::Renewalsallowed, "2")];
        let owing = |cents| RenewalFacts {
            balance: Money(cents),
            ..facts(&rules)
        };
        assert_eq!(check(&owing(10_000)), Ok(()));
        assert_eq!(
            check(&owing(10_001)),
            Err(RenewalRefusal::TooMuchOwing {
                owed: Money(10_001)
            })
        );
        assert_eq!(
            can_renew(&owing(10_001), &RenewalSettings::default(), true),
            Ok(())
        );
        let no_limit = RenewalSettings {
            fine_limit: None,
            ..RenewalSettings::default()
        };
        assert_eq!(can_renew(&owing(90_000), &no_limit, false), Ok(()));
        let held = RenewalFacts {
            on_hold: true,
            ..facts(&rules)
        };
        assert_eq!(check(&held), Err(RenewalRefusal::OnHold));
        assert_eq!(
            can_renew(&held, &RenewalSettings::default(), true),
            Err(RenewalRefusal::OnHold)
        );
        // The count comes before the hold, as in Koha.
        let used = RenewalFacts {
            renewals: 2,
            ..held
        };
        assert_eq!(check(&used), Err(RenewalRefusal::TooMany));
    }

    #[test]
    fn the_rule_says_how_many_renewals_are_allowed() {
        assert_eq!(renewals_allowed(&[], &AT), 0);
        assert_eq!(
            renewals_allowed(&[rule(RuleKind::Renewalsallowed, "3")], &AT),
            3
        );
    }
}
