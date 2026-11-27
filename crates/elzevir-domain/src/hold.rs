// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether a patron may place a hold, and where it may be picked up
//! (docs/specs/circulacao/reservas.md).

use elzevir_data::RuleKind;
use jiff::civil::Date;

use crate::catalog::{BranchId, InstanceId, ItemId, Money, PolicyId};
use crate::checkout::perl_whole;
use crate::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use crate::patron::PatronCategoryId;

/// One hold.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct HoldId(pub i64);

/// A hold set aside for its patron.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// Waiting at the pickup branch (`W`).
    Waiting,
    /// In transit to the pickup branch (`T`).
    InTransit,
}

impl Found {
    /// The code stored, Koha's.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Waiting => "W",
            Self::InTransit => "T",
        }
    }

    /// The state of a stored code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        [Self::Waiting, Self::InTransit]
            .into_iter()
            .find(|found| found.code() == code)
    }
}

/// Where a hold stands in its queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueState {
    /// Waiting for an Item.
    Queued,
    /// Keeping its place, but no Item fills it (Koha: `suspend`). A hold set aside is never
    /// suspended.
    Suspended,
    /// Set aside for its patron.
    Found(Found),
}

/// A hold in an Instance's queue, as `hold_for_item` sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueuedHold {
    /// Which hold.
    pub id: HoldId,
    /// The branch of its patron, whose rules apply.
    pub patron_branch: BranchId,
    /// Where it is picked up.
    pub pickup: BranchId,
    /// Its place in the queue; 0 when set aside.
    pub priority: u32,
    /// Queued, suspended or set aside.
    pub state: QueueState,
    /// The Item it asks for or was set aside with.
    pub item: Option<ItemId>,
}

/// The hold an Item answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldMatch {
    /// The Item is waiting for this hold.
    Waiting(HoldId),
    /// The Item is in transit for this hold.
    InTransit(HoldId),
    /// The Item can fill this hold, the first in the queue it may fill.
    Reserved(HoldId),
}

impl HoldMatch {
    /// The hold.
    #[must_use]
    pub const fn hold(self) -> HoldId {
        match self {
            Self::Waiting(hold) | Self::InTransit(hold) | Self::Reserved(hold) => hold,
        }
    }
}

/// The hold the Item answers, if any: a hold already set aside with it, or else the first
/// hold in the queue the rules let it fill, passing over suspended holds. `queue` holds the
/// Instance's holds.
///
/// Koha: `C4::Reserves::CheckReserves` and `_Findgroupreserve` (`suspend = 0`; without the
/// holds queue, `LocalHoldsPriority`, item groups and hold groups; `TrapHoldsOnOrder` on).
#[must_use]
pub fn hold_for_item(
    rules: &[CirculationRule],
    queue: &[QueuedHold],
    item: &HoldItem,
    not_for_loan: i64,
) -> Option<HoldMatch> {
    if not_for_loan > 0 {
        return None;
    }
    let mut candidates: Vec<&QueuedHold> = queue
        .iter()
        .filter(|hold| hold.item.is_none() || hold.item == Some(item.id))
        .collect();
    candidates.sort_by_key(|hold| (hold.priority, hold.id));
    for hold in &candidates {
        match hold.state {
            QueueState::Found(Found::Waiting) => return Some(HoldMatch::Waiting(hold.id)),
            QueueState::Found(Found::InTransit) => return Some(HoldMatch::InTransit(hold.id)),
            QueueState::Queued | QueueState::Suspended => {}
        }
    }
    candidates
        .into_iter()
        .filter(|hold| hold.state == QueueState::Queued && hold.priority > 0)
        .find(|hold| may_fill(rules, hold, item))
        .map(|hold| HoldMatch::Reserved(hold.id))
}

/// Whether the rules let the Item fill the hold. Without hold groups, no Item fills a hold
/// whose rules ask for one.
fn may_fill(rules: &[CirculationRule], hold: &QueuedHold, item: &HoldItem) -> bool {
    let scope = RuleScopeKey {
        branch: Some(hold.patron_branch),
        category: None,
        policy: Some(item.policy),
    };
    let allowed = rule_value(rules, RuleKind::Holdallowed, &scope).unwrap_or("from_any_library");
    let fulfillment = rule_value(rules, RuleKind::HoldFulfillmentPolicy, &scope).unwrap_or("any");
    let allowed_ok = match allowed {
        "not_allowed" | "from_local_hold_group" => false,
        "from_home_library" => item.home == hold.patron_branch,
        _ => true,
    };
    let pickup_ok = match fulfillment {
        "holdgroup" => false,
        "homebranch" => hold.pickup == item.home,
        "holdingbranch" => hold.pickup == item.holding,
        _ => true,
    };
    allowed_ok && pickup_ok
}

/// How many days a waiting hold waits: the rule `holds_pickup_period`, or
/// `ReservesMaxPickUpDelay` (7).
///
/// Koha: `Koha::Hold::set_waiting` (`ExcludeHolidaysFromMaxPickUpDelay` off).
#[must_use]
pub fn pickup_days(rules: &[CirculationRule], at: &RuleScopeKey) -> u32 {
    rule_value(rules, RuleKind::HoldsPickupPeriod, at)
        .filter(|value| !value.is_empty())
        .map_or(7, perl_whole)
}

/// Why a hold cannot be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HoldRefusal {
    /// The pickup branch does not serve the Item, or any Item of the Instance.
    #[error("the pickup branch is not valid")]
    PickupNotValid,
    /// The patron is suspended (`restricted`).
    #[error("the patron is restricted")]
    Restricted,
    /// The patron owes more than the limit (`debt_limit`).
    #[error("the patron owes {owed:?}, more than {limit:?}")]
    DebtLimit {
        /// The balance.
        owed: Money,
        /// `maxoutstanding`.
        limit: Money,
    },
    /// The patron has as many holds as allowed (`hold_limit`).
    #[error("the patron has {limit} holds already")]
    HoldLimit {
        /// `maxreserves`.
        limit: u32,
    },
    /// The patron already has a hold on this Item (`itemAlreadyOnHold`).
    #[error("the patron already holds this item")]
    ItemAlreadyOnHold,
    /// The rules allow no holds (`noReservesAllowed`).
    #[error("no holds are allowed")]
    NoReservesAllowed,
    /// As many holds on this Instance as allowed (`tooManyHoldsForThisRecord`).
    #[error("too many holds on this record")]
    TooManyHoldsForThisRecord {
        /// `holds_per_record`.
        limit: u32,
    },
    /// As many holds placed today as allowed (`tooManyReservesToday`).
    #[error("too many holds today")]
    TooManyReservesToday {
        /// `holds_per_day`.
        limit: u32,
    },
    /// As many holds as allowed (`tooManyReserves`).
    #[error("too many holds")]
    TooManyReserves {
        /// `reservesallowed` or `max_holds`.
        limit: u32,
    },
    /// Holds on this Item are not allowed (`notReservable`).
    #[error("this item cannot be held")]
    NotReservable,
    /// Only patrons of the Item's home branch may hold it (`cannotReserveFromOtherBranches`).
    #[error("only patrons of the item's branch may hold it")]
    CannotReserveFromOtherBranches,
    /// Only patrons of the Item's hold group may hold it (`branchNotInHoldGroup`).
    #[error("the patron's branch is not in the item's hold group")]
    BranchNotInHoldGroup,
}

impl HoldRefusal {
    /// Koha's name of the reason, as its REST API gives it.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::PickupNotValid => "pickup",
            Self::Restricted => "restricted",
            Self::DebtLimit { .. } => "debt_limit",
            Self::HoldLimit { .. } => "hold_limit",
            Self::ItemAlreadyOnHold => "itemAlreadyOnHold",
            Self::NoReservesAllowed => "noReservesAllowed",
            Self::TooManyHoldsForThisRecord { .. } => "tooManyHoldsForThisRecord",
            Self::TooManyReservesToday { .. } => "tooManyReservesToday",
            Self::TooManyReserves { .. } => "tooManyReserves",
            Self::NotReservable => "notReservable",
            Self::CannotReserveFromOtherBranches => "cannotReserveFromOtherBranches",
            Self::BranchNotInHoldGroup => "branchNotInHoldGroup",
        }
    }
}

/// The library's settings for holds, with Koha's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldSettings {
    /// `maxoutstanding`: a balance above it blocks holds; `None` does not limit.
    pub max_outstanding: Option<Money>,
    /// `maxreserves`: the most holds a patron may have; `None` does not limit.
    pub max_reserves: Option<u32>,
}

impl Default for HoldSettings {
    fn default() -> Self {
        Self {
            max_outstanding: Some(Money(500)),
            max_reserves: Some(50),
        }
    }
}

/// The patron asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldPatron {
    /// The patron's branch, whose rules apply (`ReservesControlBranch` `PatronLibrary`).
    pub branch: BranchId,
    /// The patron's category.
    pub category: PatronCategoryId,
    /// Suspended today.
    pub suspended: bool,
    /// Debits and credits still outstanding, credits counting against debits.
    pub balance: Money,
}

/// A hold the patron already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentHold {
    /// On which Instance.
    pub instance: InstanceId,
    /// The Item of an Item hold.
    pub item: Option<ItemId>,
    /// The policy of that Item.
    pub policy: Option<PolicyId>,
    /// The day it was placed.
    pub placed_on: Date,
}

/// An Item of the Instance asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldItem {
    /// Which Item.
    pub id: ItemId,
    /// Its home branch.
    pub home: BranchId,
    /// Where it is now.
    pub holding: BranchId,
    /// Its circulation policy.
    pub policy: PolicyId,
}

/// What is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldTarget {
    /// This Item.
    Item(ItemId),
    /// Any Item of the Instance.
    Record,
}

/// Everything the check reads.
#[derive(Debug, Clone, Copy)]
pub struct HoldRequestFacts<'a> {
    /// The rules.
    pub rules: &'a [CirculationRule],
    /// The patron.
    pub patron: &'a HoldPatron,
    /// The patron's holds.
    pub holds: &'a [CurrentHold],
    /// The Instance.
    pub instance: InstanceId,
    /// Its Items, in the order they were added.
    pub items: &'a [HoldItem],
    /// The branches there are.
    pub branches: &'a [BranchId],
    /// Today, in the library's time zone.
    pub today: Date,
}

/// Whether a hold of `target` for pickup at `pickup` can be placed.
///
/// # Errors
///
/// The first [`HoldRefusal`], in the order Koha's REST API checks: the pickup branch, the
/// patron, then the rules.
///
/// Koha: `Koha::REST::V1::Holds::add`.
pub fn check_hold(
    facts: &HoldRequestFacts<'_>,
    target: HoldTarget,
    pickup: BranchId,
    settings: &HoldSettings,
) -> Result<(), HoldRefusal> {
    let asked: Vec<&HoldItem> = match target {
        HoldTarget::Item(id) => facts.items.iter().filter(|item| item.id == id).collect(),
        HoldTarget::Record => facts.items.iter().collect(),
    };
    let serves = asked.iter().any(|item| {
        pickup_locations(facts.rules, facts.patron, item, facts.branches).contains(&pickup)
    });
    if !serves {
        return Err(HoldRefusal::PickupNotValid);
    }
    check_patron(facts.patron, facts.holds.len(), settings)?;
    // `serves` above proves at least one Item is asked for, so `last` is always replaced.
    let mut last = HoldRefusal::PickupNotValid;
    for item in asked {
        match check_item(facts, item) {
            Ok(()) => return Ok(()),
            Err(refusal) => last = refusal,
        }
    }
    Err(last)
}

/// Whether the patron may place holds at all.
///
/// Koha: `Koha::Patron::can_place_holds` (with `BlockExpiredPatronOpacActions` empty).
fn check_patron(
    patron: &HoldPatron,
    holds: usize,
    settings: &HoldSettings,
) -> Result<(), HoldRefusal> {
    if patron.suspended {
        return Err(HoldRefusal::Restricted);
    }
    if let Some(limit) = settings.max_outstanding
        && limit.0 != 0
        && patron.balance > limit
    {
        return Err(HoldRefusal::DebtLimit {
            owed: patron.balance,
            limit,
        });
    }
    if let Some(limit) = settings.max_reserves
        && limit != 0
        && holds >= usize::try_from(limit).unwrap_or(usize::MAX)
    {
        return Err(HoldRefusal::HoldLimit { limit });
    }
    Ok(())
}

/// The value of a rule, `None` when there is no rule.
fn rule_value<'a>(
    rules: &'a [CirculationRule],
    kind: RuleKind,
    at: &RuleScopeKey,
) -> Option<&'a str> {
    effective_rule(rules, kind, at).map(|rule| rule.value.as_str())
}

/// Whether `count` reaches a limit written in a rule; an empty value does not limit.
fn reaches(value: Option<&str>, count: usize) -> Option<u32> {
    let limit = value.filter(|text| !text.is_empty()).map(perl_whole)?;
    (count >= usize::try_from(limit).unwrap_or(usize::MAX)).then_some(limit)
}

/// `holdallowed` and `hold_fulfillment_policy` for the Item.
///
/// Koha: `C4::Circulation::GetBranchItemRule`, with its defaults `from_any_library` and
/// `any`.
fn branch_item_rule<'a>(
    rules: &'a [CirculationRule],
    patron: &HoldPatron,
    item: &HoldItem,
) -> (&'a str, &'a str) {
    let scope = RuleScopeKey {
        branch: Some(patron.branch),
        category: None,
        policy: Some(item.policy),
    };
    (
        rule_value(rules, RuleKind::Holdallowed, &scope).unwrap_or("from_any_library"),
        rule_value(rules, RuleKind::HoldFulfillmentPolicy, &scope).unwrap_or("any"),
    )
}

/// The refusal `holdallowed` makes, if any. Without hold groups, a local hold group is
/// the Item's home branch alone.
fn hold_allowed_refusal(
    allowed: &str,
    patron: &HoldPatron,
    item: &HoldItem,
) -> Option<HoldRefusal> {
    match allowed {
        "not_allowed" => Some(HoldRefusal::NotReservable),
        "from_home_library" if patron.branch != item.home => {
            Some(HoldRefusal::CannotReserveFromOtherBranches)
        }
        "from_local_hold_group" if patron.branch != item.home => {
            Some(HoldRefusal::BranchNotInHoldGroup)
        }
        _ => None,
    }
}

/// Where a hold on the Item may be picked up.
///
/// Koha: `Koha::Item::pickup_locations` (every branch is a pickup location, no hold groups,
/// no transfer limits).
#[must_use]
pub fn pickup_locations(
    rules: &[CirculationRule],
    patron: &HoldPatron,
    item: &HoldItem,
    branches: &[BranchId],
) -> Vec<BranchId> {
    let (allowed, fulfillment) = branch_item_rule(rules, patron, item);
    if matches!(
        hold_allowed_refusal(allowed, patron, item),
        Some(HoldRefusal::CannotReserveFromOtherBranches | HoldRefusal::BranchNotInHoldGroup)
    ) {
        return Vec::new();
    }
    match fulfillment {
        "homebranch" => vec![item.home],
        "holdingbranch" => vec![item.holding],
        "holdgroup" | "patrongroup" => Vec::new(),
        _ => branches.to_vec(),
    }
}

/// Whether the rules let the patron hold the Item.
///
/// Koha: `C4::Reserves::CanItemBeReserved` (`IndependentBranches` off,
/// `AllowHoldsOnDamagedItems` and `AllowHoldsOnPatronsPossessions` on, no age restriction,
/// no recalls, no hold groups).
fn check_item(facts: &HoldRequestFacts<'_>, item: &HoldItem) -> Result<(), HoldRefusal> {
    let holds = facts.holds;
    if holds.iter().any(|hold| hold.item == Some(item.id)) {
        return Err(HoldRefusal::ItemAlreadyOnHold);
    }
    let scope = RuleScopeKey {
        branch: Some(facts.patron.branch),
        category: Some(facts.patron.category),
        policy: Some(item.policy),
    };
    let per_record = rule_value(facts.rules, RuleKind::HoldsPerRecord, &scope).unwrap_or("1");
    if !per_record.is_empty() {
        if perl_whole(per_record) == 0 {
            return Err(HoldRefusal::NoReservesAllowed);
        }
        let on_record = holds
            .iter()
            .filter(|hold| hold.instance == facts.instance)
            .count();
        if let Some(limit) = reaches(Some(per_record), on_record) {
            return Err(HoldRefusal::TooManyHoldsForThisRecord { limit });
        }
    }
    let today = holds
        .iter()
        .filter(|hold| hold.placed_on == facts.today)
        .count();
    if let Some(limit) = reaches(
        rule_value(facts.rules, RuleKind::HoldsPerDay, &scope),
        today,
    ) {
        return Err(HoldRefusal::TooManyReservesToday { limit });
    }
    // No rule is zero holds allowed; an empty value allows any number.
    let allowed = effective_rule(facts.rules, RuleKind::Reservesallowed, &scope);
    let allowed_value = allowed.map_or("0", |rule| rule.value.as_str());
    if !allowed_value.is_empty() {
        if perl_whole(allowed_value) == 0 {
            return Err(HoldRefusal::NoReservesAllowed);
        }
        let policy = allowed.and_then(|rule| rule.scope.policy);
        let counted = holds
            .iter()
            .filter(|hold| policy.is_none() || hold.policy == policy)
            .count();
        if let Some(limit) = reaches(Some(allowed_value), counted) {
            return Err(HoldRefusal::TooManyReserves { limit });
        }
    }
    let total_scope = RuleScopeKey {
        policy: None,
        ..scope
    };
    if let Some(limit) = reaches(
        rule_value(facts.rules, RuleKind::MaxHolds, &total_scope),
        holds.len(),
    ) {
        return Err(HoldRefusal::TooManyReserves { limit });
    }
    let (hold_allowed, _) = branch_item_rule(facts.rules, facts.patron, item);
    hold_allowed_refusal(hold_allowed, facts.patron, item).map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTRAL: BranchId = BranchId(1);
    const ANNEX: BranchId = BranchId(2);
    const BOOK: PolicyId = PolicyId(1);
    const DVD: PolicyId = PolicyId(2);
    const RECORD: InstanceId = InstanceId(10);

    fn rule(kind: RuleKind, policy: Option<PolicyId>, value: &str) -> CirculationRule {
        CirculationRule {
            kind,
            scope: RuleScopeKey {
                branch: None,
                category: None,
                policy,
            },
            value: value.to_owned(),
        }
    }

    fn patron() -> HoldPatron {
        HoldPatron {
            branch: CENTRAL,
            category: PatronCategoryId(1),
            suspended: false,
            balance: Money(0),
        }
    }

    fn item(id: i64, home: BranchId, policy: PolicyId) -> HoldItem {
        HoldItem {
            id: ItemId(id),
            home,
            holding: home,
            policy,
        }
    }

    fn today() -> Date {
        Date::constant(2026, 11, 20)
    }

    fn hold(instance: i64, item: Option<(i64, PolicyId)>, placed_on: Date) -> CurrentHold {
        CurrentHold {
            instance: InstanceId(instance),
            item: item.map(|(id, _)| ItemId(id)),
            policy: item.map(|(_, policy)| policy),
            placed_on,
        }
    }

    fn check(
        rules: &[CirculationRule],
        patron: &HoldPatron,
        holds: &[CurrentHold],
        items: &[HoldItem],
        target: HoldTarget,
    ) -> Result<(), HoldRefusal> {
        let facts = HoldRequestFacts {
            rules,
            patron,
            holds,
            instance: RECORD,
            items,
            branches: &[CENTRAL, ANNEX],
            today: today(),
        };
        check_hold(&facts, target, CENTRAL, &HoldSettings::default())
    }

    fn queued(id: i64, priority: u32, found: Option<Found>, item: Option<i64>) -> QueuedHold {
        QueuedHold {
            id: HoldId(id),
            patron_branch: CENTRAL,
            pickup: CENTRAL,
            priority,
            state: found.map_or(QueueState::Queued, QueueState::Found),
            item: item.map(ItemId),
        }
    }

    #[test]
    fn a_suspended_hold_keeps_its_place_but_no_item_fills_it() {
        let book = item(1, CENTRAL, BOOK);
        let suspended = |hold: QueuedHold| QueuedHold {
            state: QueueState::Suspended,
            ..hold
        };
        let queue = [
            suspended(queued(1, 1, None, None)),
            queued(2, 2, None, None),
        ];
        assert_eq!(
            hold_for_item(&[], &queue, &book, 0),
            Some(HoldMatch::Reserved(HoldId(2)))
        );
        // Only suspended holds: the Item answers none, as Koha's `CheckReserves`.
        assert_eq!(hold_for_item(&[], &queue[..1], &book, 0), None);
        let item_level = [suspended(queued(3, 1, None, Some(1)))];
        assert_eq!(hold_for_item(&[], &item_level, &book, 0), None);
    }

    #[test]
    fn an_item_answers_its_own_set_aside_hold_first_then_the_queue() {
        let book = item(1, CENTRAL, BOOK);
        let queue = [
            queued(1, 1, None, Some(2)),
            queued(2, 2, None, None),
            queued(3, 3, None, Some(1)),
        ];
        assert_eq!(
            hold_for_item(&[], &queue, &book, 0),
            Some(HoldMatch::Reserved(HoldId(2)))
        );
        let waiting = [queued(4, 0, Some(Found::Waiting), Some(1)), queue[1]];
        assert_eq!(
            hold_for_item(&[], &waiting, &book, 0),
            Some(HoldMatch::Waiting(HoldId(4)))
        );
        assert_eq!(hold_for_item(&[], &queue, &book, 1), None, "not for loan");
        assert_eq!(
            hold_for_item(&[], &queue, &book, -1).map(HoldMatch::hold),
            Some(HoldId(2))
        );
    }

    #[test]
    fn the_rules_skip_holds_the_item_may_not_fill() {
        let annex_book = item(1, ANNEX, BOOK);
        let queue = [
            queued(1, 1, None, None),
            QueuedHold {
                patron_branch: ANNEX,
                pickup: ANNEX,
                ..queued(2, 2, None, None)
            },
        ];
        let home_only = [rule(RuleKind::Holdallowed, None, "from_home_library")];
        assert_eq!(
            hold_for_item(&home_only, &queue, &annex_book, 0),
            Some(HoldMatch::Reserved(HoldId(2)))
        );
        let at_home = [rule(RuleKind::HoldFulfillmentPolicy, None, "homebranch")];
        assert_eq!(
            hold_for_item(&at_home, &queue, &annex_book, 0),
            Some(HoldMatch::Reserved(HoldId(2)))
        );
        let none = [rule(RuleKind::Holdallowed, None, "not_allowed")];
        assert_eq!(hold_for_item(&none, &queue, &annex_book, 0), None);
    }

    #[test]
    fn a_hold_waits_seven_days_unless_a_rule_says_otherwise() {
        let scope = RuleScopeKey::default();
        assert_eq!(pickup_days(&[], &scope), 7);
        assert_eq!(
            pickup_days(&[rule(RuleKind::HoldsPickupPeriod, None, "3")], &scope),
            3
        );
    }

    #[test]
    fn without_a_reservesallowed_rule_no_hold_is_allowed() {
        let items = [item(1, CENTRAL, BOOK)];
        assert_eq!(
            check(&[], &patron(), &[], &items, HoldTarget::Record),
            Err(HoldRefusal::NoReservesAllowed)
        );
        let rules = [rule(RuleKind::Reservesallowed, None, "")];
        assert_eq!(
            check(&rules, &patron(), &[], &items, HoldTarget::Record),
            Ok(())
        );
    }

    #[test]
    fn the_rules_are_checked_in_koha_order() {
        let items = [item(1, CENTRAL, BOOK), item(2, ANNEX, DVD)];
        let base = rule(RuleKind::Reservesallowed, None, "2");
        let on_item = [hold(10, Some((1, BOOK)), today())];
        assert_eq!(
            check(
                std::slice::from_ref(&base),
                &patron(),
                &on_item,
                &items,
                HoldTarget::Item(ItemId(1))
            ),
            Err(HoldRefusal::ItemAlreadyOnHold)
        );
        let on_record = [hold(10, None, today())];
        assert_eq!(
            check(
                std::slice::from_ref(&base),
                &patron(),
                &on_record,
                &items,
                HoldTarget::Record
            ),
            Err(HoldRefusal::TooManyHoldsForThisRecord { limit: 1 })
        );
        let elsewhere = [hold(11, None, today()), hold(12, None, today())];
        let per_day = [base.clone(), rule(RuleKind::HoldsPerDay, None, "2")];
        assert_eq!(
            check(&per_day, &patron(), &elsewhere, &items, HoldTarget::Record),
            Err(HoldRefusal::TooManyReservesToday { limit: 2 })
        );
        assert_eq!(
            check(
                std::slice::from_ref(&base),
                &patron(),
                &elsewhere,
                &items,
                HoldTarget::Record
            ),
            Err(HoldRefusal::TooManyReserves { limit: 2 })
        );
        let max = [
            rule(RuleKind::Reservesallowed, None, ""),
            rule(RuleKind::MaxHolds, None, "1"),
        ];
        assert_eq!(
            check(&max, &patron(), &elsewhere[..1], &items, HoldTarget::Record),
            Err(HoldRefusal::TooManyReserves { limit: 1 })
        );
    }

    #[test]
    fn a_policy_rule_counts_only_holds_on_items_of_that_policy() {
        let items = [item(1, CENTRAL, DVD)];
        let rules = [
            rule(RuleKind::Reservesallowed, None, ""),
            rule(RuleKind::Reservesallowed, Some(DVD), "1"),
        ];
        let record_holds = [hold(11, None, today()), hold(12, Some((5, BOOK)), today())];
        assert_eq!(
            check(&rules, &patron(), &record_holds, &items, HoldTarget::Record),
            Ok(())
        );
        let dvd = [hold(12, Some((5, DVD)), today())];
        assert_eq!(
            check(&rules, &patron(), &dvd, &items, HoldTarget::Record),
            Err(HoldRefusal::TooManyReserves { limit: 1 })
        );
    }

    #[test]
    fn a_record_hold_needs_one_item_the_rules_allow() {
        let items = [item(1, CENTRAL, BOOK), item(2, CENTRAL, DVD)];
        let rules = [
            rule(RuleKind::Reservesallowed, None, ""),
            rule(RuleKind::Holdallowed, Some(BOOK), "not_allowed"),
        ];
        assert_eq!(
            check(&rules, &patron(), &[], &items, HoldTarget::Record),
            Ok(())
        );
        assert_eq!(
            check(&rules, &patron(), &[], &items[..1], HoldTarget::Record),
            Err(HoldRefusal::NotReservable)
        );
        assert_eq!(
            check(&rules, &patron(), &[], &[], HoldTarget::Record),
            Err(HoldRefusal::PickupNotValid)
        );
    }

    #[test]
    fn the_home_branch_and_fulfillment_rules_limit_who_and_where() {
        let annex_item = item(1, ANNEX, BOOK);
        let home_only = [
            rule(RuleKind::Reservesallowed, None, ""),
            rule(RuleKind::Holdallowed, None, "from_home_library"),
        ];
        assert!(pickup_locations(&home_only, &patron(), &annex_item, &[CENTRAL, ANNEX]).is_empty());
        assert_eq!(
            check(
                &home_only,
                &patron(),
                &[],
                &[annex_item],
                HoldTarget::Record
            ),
            Err(HoldRefusal::PickupNotValid)
        );
        let at_home = [
            rule(RuleKind::Reservesallowed, None, ""),
            rule(RuleKind::HoldFulfillmentPolicy, None, "homebranch"),
        ];
        assert_eq!(
            pickup_locations(&at_home, &patron(), &annex_item, &[CENTRAL, ANNEX]),
            vec![ANNEX]
        );
        let group = [rule(RuleKind::HoldFulfillmentPolicy, None, "holdgroup")];
        assert!(pickup_locations(&group, &patron(), &annex_item, &[CENTRAL]).is_empty());
    }

    #[test]
    fn the_patron_is_checked_before_the_rules() {
        let items = [item(1, CENTRAL, BOOK)];
        let suspended = HoldPatron {
            suspended: true,
            ..patron()
        };
        assert_eq!(
            check(&[], &suspended, &[], &items, HoldTarget::Record),
            Err(HoldRefusal::Restricted)
        );
        let rules = [rule(RuleKind::Reservesallowed, None, "")];
        let at_limit = HoldPatron {
            balance: Money(500),
            ..patron()
        };
        assert_eq!(
            check(&rules, &at_limit, &[], &items, HoldTarget::Record),
            Ok(())
        );
        let over = HoldPatron {
            balance: Money(501),
            ..patron()
        };
        assert_eq!(
            check(&rules, &over, &[], &items, HoldTarget::Record),
            Err(HoldRefusal::DebtLimit {
                owed: Money(501),
                limit: Money(500)
            })
        );
        let many: Vec<CurrentHold> = (0..50).map(|n| hold(100 + n, None, today())).collect();
        assert_eq!(
            check(&rules, &patron(), &many, &items, HoldTarget::Record),
            Err(HoldRefusal::HoldLimit { limit: 50 })
        );
    }
}
