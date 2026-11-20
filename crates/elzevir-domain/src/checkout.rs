// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether a checkout may happen: the patron's and the Item's state and the loan limits of
//! the circulation rules (docs/specs/circulacao/emprestimo.md).
//!
//! Each check answers like Koha's `CanBookBeIssued`: a reason that makes the checkout
//! impossible, one the staff may confirm, or an alert that does not stop it.

use elzevir_data::RuleKind;

use crate::account::{DebtCheck, DebtSettings, check_debt};
use crate::catalog::{BranchId, Money, PolicyId};
use crate::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use crate::due_date::{DaysMode, HardDueDate, HardDueDateCompare, LengthUnit, LoanRule};
use crate::patron::PatronCategoryId;
use crate::suspension::SuspensionEnd;

/// One loan the patron has now, as the limits count it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentLoan {
    /// The branch that controls the loan (Koha's `CircControl`): the one whose rules apply
    /// to its Item.
    pub branch: BranchId,
    /// The Item's circulation policy.
    pub policy: PolicyId,
    /// Whether it is an on-site checkout.
    pub on_site: bool,
}

/// The checkout being asked for, as the limits see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitQuery {
    /// The branch that controls this checkout.
    pub branch: BranchId,
    /// The patron's category.
    pub category: PatronCategoryId,
    /// The Item's circulation policy.
    pub policy: PolicyId,
    /// Whether it is an on-site checkout.
    pub on_site: bool,
    /// Whether an on-site checkout of this Item by this patron becomes a normal one
    /// (`SwitchOnSiteCheckouts`), so that it does not count twice.
    pub switching_on_site: bool,
    /// `ConsiderOnSiteCheckoutsAsNormalCheckouts`.
    pub on_site_counts_as_normal: bool,
}

/// Why the loan limits stop a checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooManyReason {
    /// Too many checkouts.
    Checkouts,
    /// Too many on-site checkouts.
    OnSiteCheckouts,
    /// No rule sets a limit, so nothing may be lent (Koha: `NO_RULE_DEFINED`).
    NoRuleDefined,
}

/// The limit a checkout would pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooMany {
    /// Which limit.
    pub reason: TooManyReason,
    /// The loans counted against it; 0 when no rule is defined.
    pub count: u32,
    /// The limit as the rule stores it; `"0"` when no rule is defined.
    pub max_allowed: String,
    /// Where the rule that set the limit applies, when one did.
    pub rule: Option<RuleScopeKey>,
}

impl TooMany {
    /// Whether the limit is zero, which Koha also reports as "this patron cannot borrow
    /// this" (`PATRON_CANT`).
    #[must_use]
    pub fn allows_none(&self) -> bool {
        perl_number_ceiling(&self.max_allowed) == 0
    }
}

/// Perl's numeric reading of a rule value, rounded up, for comparing it with a count:
/// the leading decimal number, or 0 when there is none.
pub(crate) fn perl_number_ceiling(value: &str) -> u64 {
    let text = value.trim_start();
    let whole_end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (whole, rest) = text.split_at(whole_end);
    let whole: u64 = whole.parse().unwrap_or(0);
    let has_fraction = rest.strip_prefix('.').is_some_and(|fraction| {
        fraction
            .chars()
            .take_while(char::is_ascii_digit)
            .any(|digit| digit != '0')
    });
    whole.saturating_add(u64::from(has_fraction))
}

/// The counts and limits of one check (Koha's `_check_max_qty` parameters).
struct Counted<'a> {
    count: u32,
    on_site_count: u32,
    max: Option<&'a str>,
    max_on_site: Option<&'a str>,
    rule: Option<RuleScopeKey>,
    on_site_rule: Option<RuleScopeKey>,
}

/// Koha: `C4::Circulation::_check_max_qty`.
fn check_max(query: &LimitQuery, counted: &Counted<'_>) -> Option<TooMany> {
    if query.on_site
        && let Some(max_on_site) = counted.max_on_site
    {
        if max_on_site.is_empty() {
            return None;
        }
        if u64::from(counted.on_site_count) >= perl_number_ceiling(max_on_site) {
            return Some(TooMany {
                reason: TooManyReason::OnSiteCheckouts,
                count: counted.on_site_count,
                max_allowed: max_on_site.to_owned(),
                rule: counted.on_site_rule,
            });
        }
    }
    // Perl compares an undefined limit as an empty text: no limit.
    let max = counted.max.unwrap_or("");
    if query.on_site_counts_as_normal {
        if max.is_empty() {
            return None;
        }
        let delta = u64::from(query.switching_on_site);
        if u64::from(counted.count) >= perl_number_ceiling(max).saturating_add(delta) {
            return Some(TooMany {
                reason: TooManyReason::Checkouts,
                count: counted.count,
                max_allowed: max.to_owned(),
                rule: counted.rule,
            });
        }
    } else if !query.on_site {
        if max.is_empty() {
            return None;
        }
        let normal = counted.count.saturating_sub(counted.on_site_count);
        if u64::from(normal) >= perl_number_ceiling(max) {
            return Some(TooMany {
                reason: TooManyReason::Checkouts,
                count: normal,
                max_allowed: max.to_owned(),
                rule: counted.rule,
            });
        }
    }
    None
}

fn count(loans: &[&CurrentLoan]) -> (u32, u32) {
    let total = u32::try_from(loans.len()).unwrap_or(u32::MAX);
    let on_site =
        u32::try_from(loans.iter().filter(|loan| loan.on_site).count()).unwrap_or(u32::MAX);
    (total, on_site)
}

/// Whether the patron already has as many loans as the rules allow.
///
/// Loans are counted for the rule `maxissueqty` that applies to the checkout: those of its
/// branch when it names one, and of its policy when it names one. A rule for any policy
/// counts only the loans of policies no `maxissueqty` rule of that branch and category
/// names. Then every loan of the branch counts against `patron_maxissueqty`. With no rule
/// at all, nothing may be lent.
///
/// Koha: `C4::Circulation::TooMany`, with `CircControl` resolved by the caller into the
/// loans' and the checkout's branch, and without parent item types, which Elzevir's
/// circulation policies do not have.
#[must_use]
pub fn too_many(
    rules: &[CirculationRule],
    query: &LimitQuery,
    loans: &[CurrentLoan],
) -> Option<TooMany> {
    let at = RuleScopeKey {
        branch: Some(query.branch),
        category: Some(query.category),
        policy: Some(query.policy),
    };
    let max_rule = effective_rule(rules, RuleKind::Maxissueqty, &at);
    let on_site_rule = effective_rule(rules, RuleKind::Maxonsiteissueqty, &at);
    if let Some(max_rule) = max_rule
        && !max_rule.value.is_empty()
    {
        let counted: Vec<&CurrentLoan> = loans
            .iter()
            .filter(|loan| {
                max_rule
                    .scope
                    .branch
                    .is_none_or(|branch| loan.branch == branch)
            })
            .filter(|loan| match max_rule.scope.policy {
                Some(policy) => loan.policy == policy,
                None => !policy_has_own_limit(rules, max_rule, query.category, loan.policy),
            })
            .collect();
        let (total, on_site) = count(&counted);
        let found = check_max(
            query,
            &Counted {
                count: total,
                on_site_count: on_site,
                max: Some(&max_rule.value),
                max_on_site: on_site_rule.map(|rule| rule.value.as_str()),
                rule: Some(max_rule.scope),
                on_site_rule: on_site_rule.map(|rule| rule.scope),
            },
        );
        if found.is_some() {
            return found;
        }
    }
    let patron_at = RuleScopeKey { policy: None, ..at };
    let patron_max = effective_rule(rules, RuleKind::PatronMaxissueqty, &patron_at);
    let patron_max_on_site = effective_rule(rules, RuleKind::PatronMaxonsiteissueqty, &patron_at);
    if let Some(patron_max) = patron_max
        && !patron_max.value.is_empty()
    {
        let of_branch: Vec<&CurrentLoan> = loans
            .iter()
            .filter(|loan| loan.branch == query.branch)
            .collect();
        let (total, on_site) = count(&of_branch);
        // Koha reads the on-site limit with `|| undef`, so "0" and "" mean no limit.
        let max_on_site = patron_max_on_site
            .map(|rule| rule.value.as_str())
            .filter(|value| !value.is_empty() && *value != "0");
        let found = check_max(
            query,
            &Counted {
                count: total,
                on_site_count: on_site,
                max: Some(&patron_max.value),
                max_on_site,
                rule: Some(patron_max.scope),
                on_site_rule: None,
            },
        );
        if found.is_some() {
            return found;
        }
    }
    if max_rule.is_none() && patron_max.is_none() {
        return Some(TooMany {
            reason: TooManyReason::NoRuleDefined,
            count: 0,
            max_allowed: "0".to_owned(),
            rule: None,
        });
    }
    None
}

/// Whether `policy` has a `maxissueqty` rule of its own at the branch of `general` for its
/// category or the patron's, so that loans of it do not count against `general`.
fn policy_has_own_limit(
    rules: &[CirculationRule],
    general: &CirculationRule,
    category: PatronCategoryId,
    policy: PolicyId,
) -> bool {
    rules.iter().any(|rule| {
        rule.kind == RuleKind::Maxissueqty
            && rule.scope.branch == general.scope.branch
            && (rule.scope.category == general.scope.category
                || rule.scope.category == Some(category))
            && rule.scope.policy == Some(policy)
    })
}

/// Where a lent Item is now, as the checkout sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanHolder {
    /// On loan to the patron asking.
    ThisPatron,
    /// On loan to another patron.
    AnotherPatron,
}

/// The Item's status numbers (0 is normal) and its current loan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemFacts {
    /// Not for loan; any value other than 0, negative ones too, counts.
    pub not_for_loan: i64,
    /// Withdrawn.
    pub withdrawn: i64,
    /// Restricted; only `1` counts, as in Koha.
    pub restricted: i64,
    /// Lost.
    pub lost: i64,
    /// Who has it, if anyone.
    pub on_loan: Option<LoanHolder>,
}

/// What a lost Item does to a checkout (`IssueLostItem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LostItemCheckout {
    /// The staff must confirm.
    Confirm,
    /// The staff are told.
    Alert,
    /// Nothing.
    Nothing,
}

/// The library's settings for checkouts, with Koha's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckoutSettings {
    /// `AllowTooManyOverride` (Koha: on).
    pub allow_too_many_override: bool,
    /// `AllowNotForLoanOverride` (Koha: off).
    pub allow_not_for_loan_override: bool,
    /// `IssueLostItem` (Koha: alert).
    pub lost_item: LostItemCheckout,
    /// `AutoReturnCheckedOutItems` (Koha: off).
    pub auto_return_checked_out: bool,
    /// What debts do.
    pub debt: DebtSettings,
}

impl Default for CheckoutSettings {
    fn default() -> Self {
        Self {
            allow_too_many_override: true,
            allow_not_for_loan_override: false,
            lost_item: LostItemCheckout::Alert,
            auto_return_checked_out: false,
            debt: DebtSettings::default(),
        }
    }
}

/// A reason the checkout cannot happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Impossible {
    /// The patron is suspended (`DEBARRED`, with `USERBLOCKEDWITHENDDATE` or
    /// `USERBLOCKEDNOENDDATE`).
    Suspended(SuspensionEnd),
    /// The patron's registration has expired (`EXPIRED`).
    PatronExpired,
    /// The patron owes more than the limit (`DEBT`).
    Debt(Money),
    /// A loan limit, when it cannot be overridden (`TOO_MANY`).
    TooMany(TooMany),
    /// The Item is not for loan (`NOT_FOR_LOAN`).
    NotForLoan(i64),
    /// The Item is withdrawn (`WTHDRAWN`).
    Withdrawn,
    /// The Item is restricted (`RESTRICTED`).
    Restricted,
}

/// A reason the staff must confirm before the checkout happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// The patron already has the Item: the checkout would renew it (`RENEW_ISSUE`).
    Renew,
    /// Another patron has the Item; it is returned first (`ISSUED_TO_ANOTHER`).
    IssuedToAnother,
    /// The patron owes this (`DEBT`).
    Debt(Money),
    /// The limit is zero (`PATRON_CANT`).
    PatronCannotBorrow,
    /// A loan limit, when it can be overridden (`TOO_MANY`).
    TooMany(TooMany),
    /// The Item is not for loan (`NOT_FOR_LOAN_FORCING`).
    NotForLoan(i64),
    /// The Item is lost (`ITEM_LOST`).
    Lost(i64),
}

/// Something the staff are told without stopping the checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    /// Another patron had the Item; it is returned first (`RETURNED_FROM_ANOTHER`).
    ReturnedFromAnother,
    /// The Item is lost (`ITEM_LOST`).
    Lost(i64),
}

/// Everything a checkout check found, each list in Koha's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckoutCheck {
    /// Reasons it cannot happen.
    pub impossible: Vec<Impossible>,
    /// Reasons to confirm first.
    pub confirm: Vec<Confirm>,
    /// Things to tell.
    pub alerts: Vec<Alert>,
}

impl CheckoutCheck {
    /// Whether the checkout may happen now, or after the staff confirm.
    #[must_use]
    pub fn is_possible(&self) -> bool {
        self.impossible.is_empty()
    }
}

/// What the check reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutFacts {
    /// The patron's suspension, while it holds today.
    pub suspended: Option<SuspensionEnd>,
    /// Whether the patron's registration has expired today.
    pub patron_expired: bool,
    /// What the patron owes: the outstanding debits.
    pub owed: Money,
    /// The Item.
    pub item: ItemFacts,
    /// The loan limit the checkout would pass, from [`too_many`].
    pub too_many: Option<TooMany>,
}

/// Whether a checkout may happen.
///
/// Koha: `C4::Circulation::CanBookBeIssued`, for the patron's suspension, expiry and debt,
/// the Item's current loan, the loan limits and the Item's status. Overdues and holds come
/// with their tasks (doc 03, task 3.8).
#[must_use]
pub fn check_checkout(facts: &CheckoutFacts, settings: &CheckoutSettings) -> CheckoutCheck {
    let mut check = CheckoutCheck::default();
    if let Some(end) = facts.suspended {
        check.impossible.push(Impossible::Suspended(end));
    }
    if facts.patron_expired {
        check.impossible.push(Impossible::PatronExpired);
    }
    match check_debt(facts.owed, &settings.debt) {
        DebtCheck::Impossible(owed) => check.impossible.push(Impossible::Debt(owed)),
        DebtCheck::Confirm(owed) => check.confirm.push(Confirm::Debt(owed)),
        DebtCheck::Clear => {}
    }
    match facts.item.on_loan {
        Some(LoanHolder::ThisPatron) => check.confirm.push(Confirm::Renew),
        Some(LoanHolder::AnotherPatron) if settings.auto_return_checked_out => {
            check.alerts.push(Alert::ReturnedFromAnother);
        }
        Some(LoanHolder::AnotherPatron) => check.confirm.push(Confirm::IssuedToAnother),
        None => {}
    }
    let renewing = facts.item.on_loan == Some(LoanHolder::ThisPatron);
    if let Some(too_many) = facts.too_many.as_ref().filter(|_| !renewing) {
        if too_many.allows_none() {
            check.confirm.push(Confirm::PatronCannotBorrow);
        }
        if settings.allow_too_many_override {
            check.confirm.push(Confirm::TooMany(too_many.clone()));
        } else {
            check.impossible.push(Impossible::TooMany(too_many.clone()));
        }
    }
    let item = &facts.item;
    if item.not_for_loan != 0 {
        if settings.allow_not_for_loan_override {
            check.confirm.push(Confirm::NotForLoan(item.not_for_loan));
        } else {
            check
                .impossible
                .push(Impossible::NotForLoan(item.not_for_loan));
        }
    }
    if item.withdrawn > 0 {
        check.impossible.push(Impossible::Withdrawn);
    }
    if item.restricted == 1 {
        check.impossible.push(Impossible::Restricted);
    }
    if item.lost != 0 {
        match settings.lost_item {
            LostItemCheckout::Confirm => check.confirm.push(Confirm::Lost(item.lost)),
            LostItemCheckout::Alert => check.alerts.push(Alert::Lost(item.lost)),
            LostItemCheckout::Nothing => {}
        }
    }
    check
}

/// The loan period of the rules that apply at `at`.
///
/// Like Koha, a missing `issuelength` is 0 days, a missing `renewalperiod` is 0 (an empty
/// one means "use `issuelength`"), any `lengthunit` but `hours` is days, an empty or
/// missing `daysmode` falls back to `use_days_mode`, and a missing `hardduedatecompare`
/// is 0, so a hard due date then fixes every due date. A `hardduedatecompare` other than
/// -1, 0 or 1 never matches, and a hard due date that is not a day is ignored.
///
/// Koha: `C4::Circulation::GetLoanLength`, `C4::Circulation::GetHardDueDate`,
/// `Koha::CirculationRules::get_effective_daysmode`.
#[must_use]
pub fn loan_rule(rules: &[CirculationRule], at: &RuleScopeKey, use_days_mode: &str) -> LoanRule {
    let value = |kind| effective_rule(rules, kind, at).map(|rule| rule.value.as_str());
    let renewal_period = match value(RuleKind::Renewalperiod) {
        None => Some(0),
        Some("") => None,
        Some(period) => Some(perl_whole(period)),
    };
    let days_mode = value(RuleKind::Daysmode)
        .filter(|mode| !mode.is_empty())
        .unwrap_or(use_days_mode);
    let compare = match value(RuleKind::Hardduedatecompare).map_or(0, perl_integer) {
        -1 => Some(HardDueDateCompare::Before),
        0 => Some(HardDueDateCompare::Exactly),
        1 => Some(HardDueDateCompare::After),
        _ => None,
    };
    let hard_due_date = value(RuleKind::Hardduedate)
        .and_then(|text| text.get(..10))
        .and_then(|day| day.parse().ok())
        .zip(compare)
        .map(|(date, compare)| HardDueDate { date, compare });
    LoanRule {
        issue_length: value(RuleKind::Issuelength).map_or(0, perl_whole),
        renewal_period,
        unit: if value(RuleKind::Lengthunit) == Some("hours") {
            LengthUnit::Hours
        } else {
            LengthUnit::Days
        },
        days_mode: DaysMode::from_koha(days_mode),
        hard_due_date,
    }
}

/// Perl's numeric reading of a whole count: the leading digits, or 0.
pub(crate) fn perl_whole(value: &str) -> u32 {
    let text = value.trim_start();
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text.get(..end)
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(if end == 0 { 0 } else { u32::MAX })
}

/// Perl's numeric reading of a signed whole number: an optional sign and the leading
/// digits, or 0.
fn perl_integer(value: &str) -> i64 {
    let text = value.trim_start();
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest.get(..end)
        .and_then(|digits| digits.parse::<i64>().ok())
        .map_or(0, |number| sign * number)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRANCH: BranchId = BranchId(1);
    const CATEGORY: PatronCategoryId = PatronCategoryId(1);
    const BOOK: PolicyId = PolicyId(1);
    const DVD: PolicyId = PolicyId(2);

    fn rule(kind: RuleKind, policy: Option<PolicyId>, value: &str) -> CirculationRule {
        CirculationRule {
            kind,
            scope: RuleScopeKey {
                branch: Some(BRANCH),
                category: Some(CATEGORY),
                policy,
            },
            value: value.to_owned(),
        }
    }

    fn query(on_site: bool, as_normal: bool) -> LimitQuery {
        LimitQuery {
            branch: BRANCH,
            category: CATEGORY,
            policy: BOOK,
            on_site,
            switching_on_site: false,
            on_site_counts_as_normal: as_normal,
        }
    }

    fn loan(policy: PolicyId, on_site: bool) -> CurrentLoan {
        CurrentLoan {
            branch: BRANCH,
            policy,
            on_site,
        }
    }

    fn reason(found: Option<TooMany>) -> Option<(TooManyReason, u32, String)> {
        found.map(|found| (found.reason, found.count, found.max_allowed))
    }

    // TooMany.t, "no rules exist".
    #[test]
    fn without_rules_nothing_may_be_lent() {
        for on_site in [false, true] {
            let found = too_many(&[], &query(on_site, true), &[]).unwrap();
            assert_eq!(found.reason, TooManyReason::NoRuleDefined);
            assert!(found.allows_none());
        }
    }

    // TooMany.t, "1 Issuingrule exist 0 0: no issue allowed".
    #[test]
    fn a_zero_limit_allows_nothing() {
        let rules = [
            rule(RuleKind::Maxissueqty, None, "0"),
            rule(RuleKind::Maxonsiteissueqty, None, "0"),
        ];
        for as_normal in [false, true] {
            assert_eq!(
                reason(too_many(&rules, &query(false, as_normal), &[])),
                Some((TooManyReason::Checkouts, 0, "0".to_owned()))
            );
            assert_eq!(
                reason(too_many(&rules, &query(true, as_normal), &[])),
                Some((TooManyReason::OnSiteCheckouts, 0, "0".to_owned()))
            );
        }
    }

    // TooMany.t, "1 Issuingrule exist with onsiteissueqty=unlimited".
    #[test]
    fn without_an_on_site_limit_on_site_counts_only_when_it_is_normal() {
        let rules = [rule(RuleKind::Maxissueqty, None, "1")];
        let loans = [loan(BOOK, false)];
        let full = Some((TooManyReason::Checkouts, 1, "1".to_owned()));
        assert_eq!(reason(too_many(&rules, &query(false, false), &loans)), full);
        assert_eq!(reason(too_many(&rules, &query(true, false), &loans)), None);
        assert_eq!(reason(too_many(&rules, &query(false, true), &loans)), full);
        assert_eq!(reason(too_many(&rules, &query(true, true), &loans)), full);
    }

    // TooMany.t, "1 BranchBorrowerCircRule exist: 1 CO allowed, 1 OSCO allowed", second
    // half: one on-site checkout already.
    #[test]
    fn an_on_site_loan_fills_the_on_site_limit() {
        let rules = [
            rule(RuleKind::Maxissueqty, None, "1"),
            rule(RuleKind::Maxonsiteissueqty, None, "1"),
        ];
        let loans = [loan(BOOK, true)];
        assert_eq!(reason(too_many(&rules, &query(false, false), &loans)), None);
        let on_site_full = Some((TooManyReason::OnSiteCheckouts, 1, "1".to_owned()));
        assert_eq!(
            reason(too_many(&rules, &query(true, false), &loans)),
            on_site_full
        );
        assert_eq!(
            reason(too_many(&rules, &query(false, true), &loans)),
            Some((TooManyReason::Checkouts, 1, "1".to_owned()))
        );
        assert_eq!(
            reason(too_many(&rules, &query(true, true), &loans)),
            on_site_full
        );
    }

    // TooMany.t, "empty string means unlimited".
    #[test]
    fn an_empty_limit_is_no_limit() {
        let rules = [rule(RuleKind::Maxissueqty, None, "")];
        assert_eq!(
            reason(too_many(
                &rules,
                &query(false, true),
                &[loan(BOOK, false); 5]
            )),
            None
        );
        let patron = [rule(RuleKind::PatronMaxissueqty, None, "")];
        assert_eq!(reason(too_many(&patron, &query(false, true), &[])), None);
    }

    // TooMany.t, "General vs specific rules limit quantity correctly", without parent types.
    #[test]
    fn a_general_limit_skips_policies_with_their_own() {
        let rules = [
            rule(RuleKind::Maxissueqty, None, "1"),
            rule(RuleKind::Maxissueqty, Some(DVD), "5"),
        ];
        // A DVD on loan does not count against the general limit of a book.
        assert_eq!(
            reason(too_many(&rules, &query(false, true), &[loan(DVD, false)])),
            None
        );
        let dvd = LimitQuery {
            policy: DVD,
            ..query(false, true)
        };
        // A book on loan does not count against the DVD limit.
        assert_eq!(
            reason(too_many(&rules, &dvd, &[loan(BOOK, false); 4])),
            None
        );
        assert_eq!(
            reason(too_many(&rules, &dvd, &[loan(DVD, false); 5])),
            Some((TooManyReason::Checkouts, 5, "5".to_owned()))
        );
    }

    #[test]
    fn the_patron_limit_counts_every_loan_of_the_branch() {
        let rules = [
            rule(RuleKind::Maxissueqty, Some(BOOK), "10"),
            CirculationRule {
                kind: RuleKind::PatronMaxissueqty,
                scope: RuleScopeKey {
                    branch: Some(BRANCH),
                    category: Some(CATEGORY),
                    policy: None,
                },
                value: "2".to_owned(),
            },
        ];
        let loans = [
            loan(BOOK, false),
            loan(DVD, false),
            CurrentLoan {
                branch: BranchId(9),
                policy: BOOK,
                on_site: false,
            },
        ];
        assert_eq!(
            reason(too_many(&rules, &query(false, true), &loans)),
            Some((TooManyReason::Checkouts, 2, "2".to_owned()))
        );
    }

    #[test]
    fn rule_values_read_as_perl_numbers() {
        assert_eq!(perl_number_ceiling("3"), 3);
        assert_eq!(perl_number_ceiling("2.5"), 3);
        assert_eq!(perl_number_ceiling("2.0"), 2);
        assert_eq!(perl_number_ceiling("4 books"), 4);
        assert_eq!(perl_number_ceiling("many"), 0);
    }

    #[test]
    fn checks_follow_koha_order_and_settings() {
        let limit = TooMany {
            reason: TooManyReason::Checkouts,
            count: 3,
            max_allowed: "3".to_owned(),
            rule: None,
        };
        let facts = CheckoutFacts {
            suspended: Some(SuspensionEnd::Day(jiff::civil::date(2026, 12, 1))),
            patron_expired: true,
            owed: Money(0),
            item: ItemFacts {
                not_for_loan: -1,
                withdrawn: 1,
                restricted: 2,
                lost: 1,
                on_loan: Some(LoanHolder::AnotherPatron),
            },
            too_many: Some(limit.clone()),
        };
        let check = check_checkout(&facts, &CheckoutSettings::default());
        assert_eq!(
            check.impossible,
            [
                Impossible::Suspended(SuspensionEnd::Day(jiff::civil::date(2026, 12, 1))),
                Impossible::PatronExpired,
                Impossible::NotForLoan(-1),
                Impossible::Withdrawn
            ]
        );
        assert_eq!(
            check.confirm,
            [Confirm::IssuedToAnother, Confirm::TooMany(limit.clone())]
        );
        assert_eq!(check.alerts, [Alert::Lost(1)]);
        assert!(!check.is_possible());

        let strict = CheckoutSettings {
            allow_too_many_override: false,
            allow_not_for_loan_override: true,
            lost_item: LostItemCheckout::Confirm,
            auto_return_checked_out: true,
            debt: DebtSettings::default(),
        };
        let check = check_checkout(&facts, &strict);
        assert_eq!(
            check.impossible,
            [
                Impossible::Suspended(SuspensionEnd::Day(jiff::civil::date(2026, 12, 1))),
                Impossible::PatronExpired,
                Impossible::TooMany(limit),
                Impossible::Withdrawn
            ]
        );
        assert_eq!(check.confirm, [Confirm::NotForLoan(-1), Confirm::Lost(1)]);
        assert_eq!(check.alerts, [Alert::ReturnedFromAnother]);
    }

    #[test]
    fn a_renewal_skips_the_limits_and_a_zero_limit_asks_twice() {
        let zero = TooMany {
            reason: TooManyReason::NoRuleDefined,
            count: 0,
            max_allowed: "0".to_owned(),
            rule: None,
        };
        let renewing = CheckoutFacts {
            suspended: None,
            patron_expired: false,
            owed: Money(0),
            item: ItemFacts {
                on_loan: Some(LoanHolder::ThisPatron),
                ..ItemFacts::default()
            },
            too_many: Some(zero.clone()),
        };
        assert_eq!(
            check_checkout(&renewing, &CheckoutSettings::default()).confirm,
            [Confirm::Renew]
        );
        let new = CheckoutFacts {
            item: ItemFacts::default(),
            ..renewing
        };
        assert_eq!(
            check_checkout(&new, &CheckoutSettings::default()).confirm,
            [Confirm::PatronCannotBorrow, Confirm::TooMany(zero)]
        );
    }

    #[test]
    fn the_loan_period_reads_rules_like_koha() {
        let at = RuleScopeKey {
            branch: Some(BRANCH),
            category: Some(CATEGORY),
            policy: Some(BOOK),
        };
        let none = loan_rule(&[], &at, "Calendar");
        assert_eq!(none.issue_length, 0);
        assert_eq!(none.renewal_period, Some(0));
        assert_eq!(none.unit, LengthUnit::Days);
        assert_eq!(none.days_mode, DaysMode::Calendar);
        assert_eq!(none.hard_due_date, None);
        let rules = [
            rule(RuleKind::Issuelength, None, "14"),
            rule(RuleKind::Renewalperiod, None, ""),
            rule(RuleKind::Lengthunit, None, "hours"),
            rule(RuleKind::Daysmode, Some(BOOK), ""),
            rule(RuleKind::Hardduedate, None, "2026-12-31"),
        ];
        let read = loan_rule(&rules, &at, "Days");
        assert_eq!(read.issue_length, 14);
        assert_eq!(read.renewal_period, None);
        assert_eq!(read.unit, LengthUnit::Hours);
        assert_eq!(read.days_mode, DaysMode::Days);
        let hard = read.hard_due_date.unwrap();
        assert_eq!(hard.compare, HardDueDateCompare::Exactly);
        assert_eq!(hard.date, jiff::civil::date(2026, 12, 31));
        let before = [
            rule(RuleKind::Hardduedate, None, "2026-12-31"),
            rule(RuleKind::Hardduedatecompare, None, "-1"),
        ];
        let ceiling = loan_rule(&before, &at, "Days").hard_due_date.unwrap();
        assert_eq!(ceiling.compare, HardDueDateCompare::Before);
        let never = [
            rule(RuleKind::Hardduedate, None, "2026-12-31"),
            rule(RuleKind::Hardduedatecompare, None, "2"),
        ];
        assert_eq!(loan_rule(&never, &at, "Days").hard_due_date, None);
        assert_eq!(perl_whole("7 days"), 7);
        assert_eq!(perl_integer(" -1x"), -1);
    }
}
