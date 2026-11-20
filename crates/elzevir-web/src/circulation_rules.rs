// SPDX-License-Identifier: GPL-3.0-or-later
//! The circulation rules screen: the scopes with rules, and the rules of one scope with
//! the value that applies and where it comes from (docs/specs/circulacao/regras.md,
//! items 8 to 13).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::catalog::Named;
use elzevir_app::circulation_rules::{
    CirculationDeskError, CirculationRefusal, RuleBook, RuleChange, RuleKind,
};
use elzevir_app::staff::Permission;
use elzevir_data::RuleScope;
use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_domain::circulation_rules::{CirculationRule, RuleScopeKey, effective_rule};
use elzevir_domain::patron::PatronCategoryId;
use elzevir_web_templates::{
    Choice, I18n, RuleGroupPart, RuleRow, ScopeLine, StaffCirculationRules,
};
use serde::Deserialize;

use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::staff::current_session;
use crate::{AppState, render};

const PATH: &str = "/equipe/admin/regras";

/// The rules the overview shows, one column each (spec, item 9).
const OVERVIEW: [&str; 5] = [
    "issuelength",
    "maxissueqty",
    "renewalsallowed",
    "fine",
    "reservesallowed",
];

/// The groups of the screen, each with its rules by name; every kind is in one group.
const GROUPS: [(&str, &[&str]); 8] = [
    (
        "rules-group-loans",
        &[
            "issuelength",
            "lengthunit",
            "daysmode",
            "hardduedate",
            "hardduedatecompare",
            "maxissueqty",
            "maxonsiteissueqty",
            "patron_maxissueqty",
            "patron_maxonsiteissueqty",
            "decreaseloanholds",
            "rentaldiscount",
            "note",
        ],
    ),
    (
        "rules-group-renewals",
        &[
            "renewalsallowed",
            "renewalperiod",
            "unseen_renewals_allowed",
            "norenewalbefore",
            "auto_renew",
            "noautorenewalbefore",
            "no_auto_renewal_after",
            "no_auto_renewal_after_hard_limit",
        ],
    ),
    (
        "rules-group-fines",
        &[
            "fine",
            "chargeperiod",
            "chargeperiod_charge_at",
            "firstremind",
            "overduefinescap",
            "cap_fine_to_replacement_price",
            "finedays",
            "maxsuspensiondays",
            "suspension_chargeperiod",
        ],
    ),
    (
        "rules-group-holds",
        &[
            "reservesallowed",
            "holds_per_record",
            "holds_per_day",
            "max_holds",
            "holdallowed",
            "hold_fulfillment_policy",
            "onshelfholds",
            "opacitemholds",
            "holds_pickup_period",
            "hold_fee",
            "expire_reserves_charge",
            "waiting_hold_cancellation",
            "fill_other_biblio_holds_policy",
        ],
    ),
    (
        "rules-group-returns",
        &[
            "returnbranch",
            "lostreturn",
            "processingreturn",
            "lost_item_processing_fee",
        ],
    ),
    (
        "rules-group-recalls",
        &[
            "recalls_allowed",
            "recalls_per_record",
            "on_shelf_recalls",
            "recall_due_date_interval",
            "recall_overdue_fine",
            "recall_shelf_time",
        ],
    ),
    (
        "rules-group-bookings",
        &["bookings_lead_period", "bookings_trail_period"],
    ),
    (
        "rules-group-articles",
        &[
            "article_requests",
            "article_request_fee",
            "open_article_requests_limit",
        ],
    ),
];

/// The session and the rule book, or the response that stops the request.
pub(crate) fn open(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(ActiveSession, Arc<dyn RuleBook>), Box<Response>> {
    let Some(session) = current_session(state, headers) else {
        return Err(Box::new(Redirect::to("/equipe/login").into_response()));
    };
    if !session.identity.can(Permission::Administration) {
        return Err(Box::new(StatusCode::FORBIDDEN.into_response()));
    }
    let Some(book) = state.rules.clone() else {
        return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
    };
    Ok((session, book))
}

/// One scope id as the lists send it: `0` or nothing is "any".
pub(crate) fn chosen(id: Option<i64>) -> Option<i64> {
    id.filter(|id| *id != 0)
}

/// Query string of the page.
#[derive(Deserialize)]
pub(crate) struct RulesParams {
    branch: Option<i64>,
    category: Option<i64>,
    policy: Option<i64>,
    saved: Option<String>,
}

impl RulesParams {
    fn scope(&self) -> RuleScopeKey {
        RuleScopeKey {
            branch: chosen(self.branch).map(BranchId),
            category: chosen(self.category).map(PatronCategoryId),
            policy: chosen(self.policy).map(PolicyId),
        }
    }
}

/// The page's address for `scope`.
fn url_of(scope: &RuleScopeKey) -> String {
    format!(
        "{PATH}?branch={}&category={}&policy={}",
        scope.branch.map_or(0, |id| id.0),
        scope.category.map_or(0, |id| id.0),
        scope.policy.map_or(0, |id| id.0),
    )
}

/// `GET /equipe/admin/regras`.
pub(crate) async fn rules(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RulesParams>,
) -> Response {
    match open(&state, &headers) {
        Ok((session, book)) => {
            let shown = Shown {
                scope: params.scope(),
                typed: None,
                saved: params.saved.is_some(),
                refused: None,
            };
            page(&state, &session, book, shown, StatusCode::OK).await
        }
        Err(response) => *response,
    }
}

/// What a value field holds: the text and whether an empty text is kept on purpose.
type Typed = BTreeMap<RuleKind, (String, bool)>;

/// The posted form: the scope, then `v.<rule>` for each value and `b.<rule>` for each
/// empty value kept on purpose.
fn read_form(fields: &[(String, String)]) -> Option<(String, RuleScopeKey, Typed)> {
    let mut csrf = String::new();
    let mut scope = RuleScopeKey::default();
    let mut typed = Typed::new();
    for (name, value) in fields {
        let id = || value.parse::<i64>().ok().map(Some).map(chosen);
        match name.as_str() {
            "csrf" => csrf.clone_from(value),
            "branch" => scope.branch = id()?.map(BranchId),
            "category" => scope.category = id()?.map(PatronCategoryId),
            "policy" => scope.policy = id()?.map(PolicyId),
            _ => {
                if let Some(rule) = name.strip_prefix("v.") {
                    let kind = RuleKind::from_koha_name(rule)?;
                    value
                        .trim()
                        .clone_into(&mut typed.entry(kind).or_default().0);
                } else if let Some(rule) = name.strip_prefix("b.") {
                    let kind = RuleKind::from_koha_name(rule)?;
                    typed.entry(kind).or_default().1 = true;
                }
            }
        }
    }
    Some((csrf, scope, typed))
}

/// The value a field asks for: text, an empty text kept on purpose, or no rule.
fn wanted(kind: RuleKind, (text, blank): &(String, bool)) -> Option<String> {
    if !text.is_empty() || (*blank && kind.info().can_be_blank) {
        Some(text.clone())
    } else {
        None
    }
}

/// Only the rules whose value differs from the one stored at `scope` (spec, item 12).
fn changes(stored: &[CirculationRule], scope: &RuleScopeKey, typed: &Typed) -> RuleChange {
    let values = typed
        .iter()
        .filter(|(kind, _)| settable(**kind, scope))
        .filter_map(|(kind, field)| {
            let now = stored
                .iter()
                .find(|rule| rule.kind == *kind && rule.scope == *scope)
                .map(|rule| rule.value.clone());
            let wanted = wanted(*kind, field);
            (wanted != now).then(|| (kind.koha_name().to_owned(), wanted))
        })
        .collect();
    RuleChange {
        scope: *scope,
        values,
    }
}

/// The first dimension `scope` names that `kind` is not set by.
fn unused_dimension(kind: RuleKind, scope: &RuleScopeKey) -> Option<RuleScope> {
    [
        RuleScope::Branch,
        RuleScope::PatronCategory,
        RuleScope::ItemType,
    ]
    .into_iter()
    .find(|dimension| scope.names(*dimension) && !kind.is_scoped_by(*dimension))
}

fn settable(kind: RuleKind, scope: &RuleScopeKey) -> bool {
    unused_dimension(kind, scope).is_none()
}

/// `POST /equipe/admin/regras`.
pub(crate) async fn set_rules(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let (session, book) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let Some((csrf, scope, typed)) = read_form(&fields) else {
        return (StatusCode::BAD_REQUEST, "unknown field").into_response();
    };
    if !tokens_match(&session.csrf, &csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let actor = session.identity.id;
    let writer = Arc::clone(&book);
    let read = typed.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let change = changes(&writer.rules()?, &scope, &read);
        if change.values.is_empty() {
            return Ok(());
        }
        writer.set_rules(actor, change).map(|_| ())
    })
    .await;
    match outcome {
        Ok(Ok(())) => {
            Redirect::to(&format!("{}&saved=1#rules-scope", url_of(&scope))).into_response()
        }
        Ok(Err(CirculationDeskError::Refused(refusal))) => {
            let message = refusal_text(&state.inner.i18n, &refusal);
            let shown = Shown {
                scope,
                typed: Some(typed),
                saved: false,
                refused: Some(message),
            };
            page(
                &state,
                &session,
                book,
                shown,
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await
        }
        Ok(Err(CirculationDeskError::Busy | CirculationDeskError::Unavailable(_))) | Err(_) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// The refusal as the screen says it, with the rule's label where it names one.
fn refusal_text(t: &I18n, refusal: &CirculationRefusal) -> String {
    let rule = match refusal {
        CirculationRefusal::UnknownRule(rule) | CirculationRefusal::BadValue { rule, .. } => {
            RuleKind::from_koha_name(rule).map_or_else(|| rule.clone(), |kind| label(t, kind))
        }
        _ => String::new(),
    };
    t.get_with(refusal.message_key(), "rule", &rule)
}

fn label(t: &I18n, kind: RuleKind) -> String {
    t.get(&format!("rule-kind-{}", kind.koha_name()))
}

/// What the page shows besides the stored rules.
struct Shown {
    scope: RuleScopeKey,
    /// The fields as typed, after a refusal.
    typed: Option<Typed>,
    saved: bool,
    refused: Option<String>,
}

/// The names of the branches, categories and policies.
pub(crate) struct Names {
    pub(crate) branches: Vec<Named<BranchId>>,
    pub(crate) categories: Vec<(PatronCategoryId, String)>,
    policies: Vec<Named<PolicyId>>,
}

impl Names {
    pub(crate) fn branch(&self, t: &I18n, id: Option<BranchId>) -> String {
        name_of(
            t,
            id,
            self.branches.iter().map(|row| (row.id, row.name.as_str())),
        )
    }

    pub(crate) fn category(&self, t: &I18n, id: Option<PatronCategoryId>) -> String {
        name_of(
            t,
            id,
            self.categories
                .iter()
                .map(|(id, name)| (*id, name.as_str())),
        )
    }

    fn policy(&self, t: &I18n, id: Option<PolicyId>) -> String {
        name_of(
            t,
            id,
            self.policies.iter().map(|row| (row.id, row.name.as_str())),
        )
    }

    fn scope(&self, t: &I18n, scope: &RuleScopeKey) -> [String; 3] {
        [
            self.branch(t, scope.branch),
            self.category(t, scope.category),
            self.policy(t, scope.policy),
        ]
    }
}

fn name_of<'a, Id: PartialEq>(
    t: &I18n,
    id: Option<Id>,
    mut rows: impl Iterator<Item = (Id, &'a str)>,
) -> String {
    match id {
        None => t.get("rules-any"),
        Some(id) => rows
            .find(|(row, _)| *row == id)
            .map_or_else(|| t.get("rules-any"), |(_, name)| name.to_owned()),
    }
}

/// The lists of the chooser, "any" first.
pub(crate) fn choices<Id: Copy + PartialEq>(
    t: &I18n,
    rows: impl Iterator<Item = (Id, i64, String)>,
    chosen: Option<Id>,
) -> Vec<Choice> {
    let mut list = vec![Choice {
        id: 0,
        name: t.get("rules-any"),
        chosen: chosen.is_none(),
    }];
    list.extend(rows.map(|(id, key, name)| Choice {
        id: key,
        name,
        chosen: chosen == Some(id),
    }));
    list
}

pub(crate) async fn names(state: &AppState) -> Option<Names> {
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let codes = tokio::task::spawn_blocking(move || catalog.codes())
        .await
        .ok()?
        .ok()?;
    let categories = match state.patrons.clone() {
        Some(desk) => tokio::task::spawn_blocking(move || desk.categories())
            .await
            .ok()?
            .ok()?
            .into_iter()
            .map(|category| (category.id, category.name))
            .collect(),
        None => Vec::new(),
    };
    Some(Names {
        branches: codes.branches,
        categories,
        policies: codes.policies,
    })
}

async fn page(
    state: &AppState,
    session: &ActiveSession,
    book: Arc<dyn RuleBook>,
    shown: Shown,
    status: StatusCode,
) -> Response {
    let Ok(Ok(stored)) = tokio::task::spawn_blocking(move || book.rules()).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(names) = names(state).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let t = &state.inner.i18n;
    let scope = shown.scope;
    let mut response = render(&StaffCirculationRules {
        t,
        csrf: &session.csrf,
        overview_columns: OVERVIEW
            .iter()
            .map(|name| format!("rule-kind-{name}"))
            .collect(),
        scopes: overview(t, &names, &stored, &scope),
        branches: choices(
            t,
            names
                .branches
                .iter()
                .map(|row| (row.id, row.id.0, row.name.clone())),
            scope.branch,
        ),
        categories: choices(
            t,
            names
                .categories
                .iter()
                .map(|(id, name)| (*id, id.0, name.clone())),
            scope.category,
        ),
        policies: choices(
            t,
            names
                .policies
                .iter()
                .map(|row| (row.id, row.id.0, row.name.clone())),
            scope.policy,
        ),
        groups: groups(t, &names, &stored, &scope, shown.typed.as_ref()),
        saved: shown.saved,
        refused: shown.refused,
    });
    *response.status_mut() = status;
    response
}

/// One line per scope with rules, "any" before a named one (spec, item 9).
fn overview(
    t: &I18n,
    names: &Names,
    stored: &[CirculationRule],
    current: &RuleScopeKey,
) -> Vec<ScopeLine> {
    let mut scopes: BTreeMap<RuleScopeKey, Vec<&CirculationRule>> = BTreeMap::new();
    for rule in stored {
        scopes.entry(rule.scope).or_default().push(rule);
    }
    scopes
        .into_iter()
        .map(|(scope, rules)| {
            let [branch, category, policy] = names.scope(t, &scope);
            ScopeLine {
                url: format!("{}#rules-scope", url_of(&scope)),
                branch,
                category,
                policy,
                values: OVERVIEW
                    .iter()
                    .map(|name| {
                        rules
                            .iter()
                            .find(|rule| rule.kind.koha_name() == *name)
                            .map(|rule| rule.value.clone())
                            .unwrap_or_default()
                    })
                    .collect(),
                count: t.get_numbers(
                    "rules-count",
                    &[("count", u64::try_from(rules.len()).unwrap_or(u64::MAX))],
                ),
                current: scope == *current,
            }
        })
        .collect()
}

/// Every kind at `scope`, in its group, with the value that applies and its origin.
fn groups(
    t: &I18n,
    names: &Names,
    stored: &[CirculationRule],
    scope: &RuleScopeKey,
    typed: Option<&Typed>,
) -> Vec<RuleGroupPart> {
    GROUPS
        .iter()
        .map(|(label_key, members)| RuleGroupPart {
            label_key,
            rows: members
                .iter()
                .filter_map(|name| RuleKind::from_koha_name(name))
                .map(|kind| row(t, names, stored, scope, typed, kind))
                .collect(),
        })
        .collect()
}

fn row(
    t: &I18n,
    names: &Names,
    stored: &[CirculationRule],
    scope: &RuleScopeKey,
    typed: Option<&Typed>,
    kind: RuleKind,
) -> RuleRow {
    let here = stored
        .iter()
        .find(|rule| rule.kind == kind && rule.scope == *scope);
    let (value, blank) = match typed {
        Some(typed) => typed.get(&kind).cloned().unwrap_or_default(),
        None => here.map_or((String::new(), false), |rule| {
            (rule.value.clone(), rule.value.is_empty())
        }),
    };
    let (effective, origin) = match effective_rule(stored, kind, scope) {
        None => (String::new(), t.get("rules-origin-none")),
        Some(rule) => {
            let value = if rule.value.is_empty() {
                t.get("rules-blank")
            } else {
                rule.value.clone()
            };
            let origin = if rule.scope == *scope {
                t.get("rules-origin-here")
            } else {
                let [branch, category, policy] = names.scope(t, &rule.scope);
                t.get_texts(
                    "rules-origin-from",
                    &[
                        ("branch", &branch),
                        ("category", &category),
                        ("policy", &policy),
                    ],
                )
            };
            (value, origin)
        }
    };
    RuleRow {
        name: kind.koha_name(),
        label: label(t, kind),
        value,
        blank,
        can_be_blank: kind.info().can_be_blank,
        not_scoped: unused_dimension(kind, scope).map(|dimension| {
            t.get(match dimension {
                RuleScope::Branch => "rules-not-scoped-branch",
                RuleScope::PatronCategory => "rules-not-scoped-category",
                RuleScope::ItemType => "rules-not-scoped-policy",
            })
        }),
        effective,
        origin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_in_one_group_and_has_a_label() {
        let mut seen: Vec<&str> = GROUPS
            .iter()
            .flat_map(|(_, members)| members.iter().copied())
            .collect();
        seen.sort_unstable();
        let mut all: Vec<&str> = RuleKind::ALL.iter().map(|kind| kind.koha_name()).collect();
        all.sort_unstable();
        assert_eq!(seen, all);
        for t in [I18n::english().unwrap(), I18n::portuguese().unwrap()] {
            for kind in RuleKind::ALL {
                let key = format!("rule-kind-{}", kind.koha_name());
                assert!(t.has(&key), "{key}");
            }
            for (key, _) in GROUPS {
                assert!(t.has(key), "{key}");
            }
        }
    }

    #[test]
    fn every_refusal_has_a_message() {
        let t = I18n::english().unwrap();
        for refusal in [
            CirculationRefusal::UnknownRule("x".to_owned()),
            CirculationRefusal::BadValue {
                rule: "fine".to_owned(),
                error: elzevir_domain::circulation_rules::RuleValueError::NotDecimal,
            },
            CirculationRefusal::BadValue {
                rule: "fine".to_owned(),
                error: elzevir_domain::circulation_rules::RuleValueError::TooLong,
            },
            CirculationRefusal::BadValue {
                rule: "fine".to_owned(),
                error: elzevir_domain::circulation_rules::RuleValueError::OutOfScope(
                    RuleScope::Branch,
                ),
            },
            CirculationRefusal::Repeated,
            CirculationRefusal::UnknownBranch,
            CirculationRefusal::UnknownCategory,
            CirculationRefusal::UnknownPolicy,
            CirculationRefusal::BadHolidayTitle,
            CirculationRefusal::BadWeekday,
            CirculationRefusal::BadDates,
        ] {
            assert!(t.has(refusal.message_key()), "{refusal:?}");
        }
        assert_eq!(
            refusal_text(
                &t,
                &CirculationRefusal::BadValue {
                    rule: "fine".to_owned(),
                    error: elzevir_domain::circulation_rules::RuleValueError::NotDecimal,
                }
            ),
            "Fine amount: write an amount with digits and a dot, such as 1.50."
        );
    }

    #[test]
    fn only_changed_rules_are_sent() {
        let scope = RuleScopeKey {
            branch: Some(BranchId(1)),
            ..RuleScopeKey::default()
        };
        let stored = vec![
            CirculationRule {
                kind: RuleKind::Issuelength,
                scope,
                value: "14".to_owned(),
            },
            CirculationRule {
                kind: RuleKind::Finedays,
                scope,
                value: String::new(),
            },
        ];
        let typed: Typed = [
            (RuleKind::Issuelength, ("14".to_owned(), false)),
            (RuleKind::Finedays, (String::new(), true)),
            (RuleKind::Fine, ("0.50".to_owned(), false)),
            (RuleKind::Renewalsallowed, (String::new(), false)),
            // An empty value cannot be kept for a kind that cannot be blank.
            (RuleKind::Holdallowed, (String::new(), true)),
            (RuleKind::Maxissueqty, (String::new(), true)),
        ]
        .into_iter()
        .collect();
        let change = changes(&stored, &scope, &typed);
        assert_eq!(
            change.values,
            vec![
                ("fine".to_owned(), Some("0.50".to_owned())),
                ("maxissueqty".to_owned(), Some(String::new())),
            ]
        );
    }

    #[test]
    fn the_form_reads_its_scope_and_fields() {
        let fields: Vec<(String, String)> = [
            ("csrf", "token"),
            ("branch", "2"),
            ("category", "0"),
            ("policy", "5"),
            ("v.issuelength", " 7 "),
            ("b.finedays", "1"),
            ("v.finedays", ""),
        ]
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
        let (csrf, scope, typed) = read_form(&fields).unwrap();
        assert_eq!(csrf, "token");
        assert_eq!(scope.branch, Some(BranchId(2)));
        assert_eq!(scope.category, None);
        assert_eq!(scope.policy, Some(PolicyId(5)));
        assert_eq!(typed[&RuleKind::Issuelength], ("7".to_owned(), false));
        assert_eq!(typed[&RuleKind::Finedays], (String::new(), true));
        let unknown = vec![("v.itemtype".to_owned(), "BK".to_owned())];
        assert!(read_form(&unknown).is_none());
    }
}
