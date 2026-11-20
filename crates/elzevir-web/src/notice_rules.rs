// SPDX-License-Identifier: GPL-3.0-or-later
//! The notices screen: the overdue notice rules by branch and patron category, and how
//! each category hears of a waiting hold (docs/specs/circulacao/avisos.md, items 28 to 30).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use elzevir_app::circulation_rules::{
    CirculationDeskError, NoticeChange, NoticeRules, OverdueNoticeRule, RuleBook,
};
use elzevir_domain::catalog::BranchId;
use elzevir_domain::notice::{NoticeLevel, OverdueLevel, Transports};
use elzevir_domain::patron::PatronCategoryId;
use elzevir_web_templates::{
    HoldNoticeRow, I18n, NoticeLevelRow, NoticeRuleLine, StaffNoticeRules,
};
use serde::Deserialize;

use crate::circulation_rules::{Names, choices, chosen, names, open};
use crate::security::tokens_match;
use crate::session::ActiveSession;
use crate::{AppState, render};

const PATH: &str = "/equipe/admin/avisos";

const LEVELS: [NoticeLevel; 3] = [NoticeLevel::First, NoticeLevel::Second, NoticeLevel::Third];

/// The fields of one level as the form holds them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LevelFields {
    delay: String,
    transports: Transports,
    restrict: bool,
}

/// The overdue rule being shown: its scope and, after a refusal, the levels as typed.
struct Shown {
    branch: Option<BranchId>,
    category: Option<PatronCategoryId>,
    typed: Option<[LevelFields; 3]>,
    saved: bool,
    refused: Option<String>,
}

/// Query string of the page.
#[derive(Deserialize)]
pub(crate) struct NoticeParams {
    branch: Option<i64>,
    category: Option<i64>,
    saved: Option<String>,
}

fn url_of(branch: Option<BranchId>, category: Option<PatronCategoryId>) -> String {
    format!(
        "{PATH}?branch={}&category={}",
        branch.map_or(0, |id| id.0),
        category.map_or(0, |id| id.0),
    )
}

/// `GET /equipe/admin/avisos`.
pub(crate) async fn notices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<NoticeParams>,
) -> Response {
    match open(&state, &headers) {
        Ok((session, book)) => {
            let shown = Shown {
                branch: chosen(params.branch).map(BranchId),
                category: chosen(params.category).map(PatronCategoryId),
                typed: None,
                saved: params.saved.is_some(),
                refused: None,
            };
            page(&state, &session, book, shown, StatusCode::OK).await
        }
        Err(response) => *response,
    }
}

/// The posted overdue rule: the scope, then `d1`..`d3` (days) and `e`, `p`, `r` with the
/// level's number for e-mail, print and restriction.
fn read_rule(
    fields: &[(String, String)],
) -> Option<(String, Option<BranchId>, PatronCategoryId, [LevelFields; 3])> {
    let mut csrf = String::new();
    let mut branch = None;
    let mut category = None;
    let mut levels: [LevelFields; 3] = Default::default();
    for (name, value) in fields {
        let id = || value.parse::<i64>().ok();
        let (field, number) = name.split_at(name.len().saturating_sub(1));
        let level = number
            .parse::<usize>()
            .ok()
            .and_then(|number| levels.get_mut(number.checked_sub(1)?));
        match (name.as_str(), field, level) {
            ("csrf", ..) => csrf.clone_from(value),
            ("branch", ..) => branch = chosen(Some(id()?)).map(BranchId),
            ("category", ..) => category = chosen(Some(id()?)).map(PatronCategoryId),
            (_, "d", Some(level)) => value.trim().clone_into(&mut level.delay),
            (_, "e", Some(level)) => level.transports.email = true,
            (_, "p", Some(level)) => level.transports.print = true,
            (_, "r", Some(level)) => level.restrict = true,
            _ => return None,
        }
    }
    Some((csrf, branch, category?, levels))
}

/// The levels with days; `None` when some days are not a whole number above zero.
fn levels_of(typed: &[LevelFields; 3]) -> Option<Vec<OverdueLevel>> {
    LEVELS
        .iter()
        .zip(typed)
        .filter(|(_, fields)| !fields.delay.is_empty())
        .map(|(level, fields)| {
            let delay = fields.delay.parse::<u16>().ok().filter(|days| *days > 0)?;
            Some(OverdueLevel {
                level: *level,
                delay,
                transports: fields.transports,
                restrict: fields.restrict,
            })
        })
        .collect()
}

/// `POST /equipe/admin/avisos`.
pub(crate) async fn set_overdue_rule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let (session, book) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let Some((csrf, branch, category, typed)) = read_rule(&fields) else {
        return (StatusCode::BAD_REQUEST, "unknown field").into_response();
    };
    if !tokens_match(&session.csrf, &csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let refusal = match levels_of(&typed) {
        None => Some(state.inner.i18n.get("notices-error-days")),
        Some(levels) => {
            let change = NoticeChange::Overdue(OverdueNoticeRule {
                branch,
                category,
                levels,
            });
            match apply(&book, &session, change).await {
                Ok(()) => None,
                Err(CirculationDeskError::Refused(refusal)) => {
                    Some(state.inner.i18n.get(refusal.message_key()))
                }
                Err(CirculationDeskError::Busy | CirculationDeskError::Unavailable(_)) => {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
        }
    };
    if refusal.is_none() {
        let url = format!("{}&saved=1#notice-rule", url_of(branch, Some(category)));
        return Redirect::to(&url).into_response();
    }
    let shown = Shown {
        branch,
        category: Some(category),
        typed: Some(typed),
        saved: false,
        refused: refusal,
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

async fn apply(
    book: &Arc<dyn RuleBook>,
    session: &ActiveSession,
    change: NoticeChange,
) -> Result<(), CirculationDeskError> {
    let book = Arc::clone(book);
    let actor = session.identity.id;
    tokio::task::spawn_blocking(move || book.set_notice_rule(actor, change).map(|_| ()))
        .await
        .map_err(|error| CirculationDeskError::Unavailable(error.to_string()))?
}

/// `POST /equipe/admin/avisos/reservas`: `e.<category>` and `p.<category>` for each
/// transport checked; only the categories that changed are sent (spec, item 30).
pub(crate) async fn set_hold_notices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let (session, book) = match open(&state, &headers) {
        Ok(opened) => opened,
        Err(response) => return *response,
    };
    let csrf = fields
        .iter()
        .find(|(name, _)| name == "csrf")
        .map_or("", |(_, value)| value.as_str());
    if !tokens_match(&session.csrf, csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let checked = |prefix: &str, category: PatronCategoryId| {
        let name = format!("{prefix}.{}", category.0);
        fields.iter().any(|(field, _)| *field == name)
    };
    let Some(names) = names(&state).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(stored) = stored(&book).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    for (category, _) in &names.categories {
        let wanted = Transports {
            email: checked("e", *category),
            print: checked("p", *category),
        };
        if wanted == hold_transports(&stored, *category) {
            continue;
        }
        let change = NoticeChange::Hold {
            category: *category,
            transports: wanted,
        };
        if apply(&book, &session, change).await.is_err() {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }
    Redirect::to(&format!("{PATH}?saved=1#notice-holds")).into_response()
}

fn hold_transports(stored: &NoticeRules, category: PatronCategoryId) -> Transports {
    stored
        .holds
        .iter()
        .find(|(id, _)| *id == category)
        .map(|(_, transports)| *transports)
        .unwrap_or_default()
}

async fn stored(book: &Arc<dyn RuleBook>) -> Option<NoticeRules> {
    let book = Arc::clone(book);
    tokio::task::spawn_blocking(move || book.notice_rules())
        .await
        .ok()?
        .ok()
}

async fn page(
    state: &AppState,
    session: &ActiveSession,
    book: Arc<dyn RuleBook>,
    shown: Shown,
    status: StatusCode,
) -> Response {
    let (Some(stored), Some(names)) = (stored(&book).await, names(state).await) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let t = &state.inner.i18n;
    let category = shown
        .category
        .or_else(|| names.categories.first().map(|(id, _)| *id));
    let typed = shown.typed.unwrap_or_else(|| {
        let levels = stored
            .overdue
            .iter()
            .find(|rule| rule.branch == shown.branch && Some(rule.category) == category)
            .map_or(&[][..], |rule| rule.levels.as_slice());
        fields_of(levels)
    });
    let mut response = render(&StaffNoticeRules {
        t,
        csrf: &session.csrf,
        rules: stored
            .overdue
            .iter()
            .map(|rule| line(t, &names, rule, shown.branch, category))
            .collect(),
        branches: choices(
            t,
            names
                .branches
                .iter()
                .map(|row| (row.id, row.id.0, row.name.clone())),
            shown.branch,
        ),
        categories: choices(
            t,
            names
                .categories
                .iter()
                .map(|(id, name)| (*id, id.0, name.clone())),
            category,
        )
        .into_iter()
        .skip(1)
        .collect(),
        levels: LEVELS
            .iter()
            .zip(typed)
            .map(|(level, fields)| NoticeLevelRow {
                number: level.number(),
                delay: fields.delay,
                email: fields.transports.email,
                print: fields.transports.print,
                restrict: fields.restrict,
            })
            .collect(),
        holds: names
            .categories
            .iter()
            .map(|(id, name)| {
                let transports = hold_transports(&stored, *id);
                HoldNoticeRow {
                    category: id.0,
                    name: name.clone(),
                    email: transports.email,
                    print: transports.print,
                }
            })
            .collect(),
        saved: shown.saved,
        refused: shown.refused,
    });
    *response.status_mut() = status;
    response
}

/// The form's fields for stored levels.
fn fields_of(levels: &[OverdueLevel]) -> [LevelFields; 3] {
    LEVELS.map(|wanted| {
        levels
            .iter()
            .find(|level| level.level == wanted)
            .map(|level| LevelFields {
                delay: level.delay.to_string(),
                transports: level.transports,
                restrict: level.restrict,
            })
            .unwrap_or_default()
    })
}

/// One rule of the list, its levels in words (spec, item 28).
fn line(
    t: &I18n,
    names: &Names,
    rule: &OverdueNoticeRule,
    branch: Option<BranchId>,
    category: Option<PatronCategoryId>,
) -> NoticeRuleLine {
    let levels = rule
        .levels
        .iter()
        .map(|level| {
            let mut parts = vec![t.get_numbers(
                "notices-level-days",
                &[
                    ("level", u64::from(level.level.number())),
                    ("days", u64::from(level.delay)),
                ],
            )];
            for (on, key) in [
                (level.transports.email, "notices-email"),
                (level.transports.print, "notices-print"),
                (level.restrict, "notices-restrict"),
            ] {
                if on {
                    parts.push(t.get(key));
                }
            }
            parts.join(", ")
        })
        .collect();
    NoticeRuleLine {
        url: format!("{}#notice-rule", url_of(rule.branch, Some(rule.category))),
        branch: names.branch(t, rule.branch),
        category: names.category(t, Some(rule.category)),
        levels,
        current: rule.branch == branch && Some(rule.category) == category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn the_form_reads_its_scope_and_levels() {
        let (csrf, branch, category, levels) = read_rule(&fields(&[
            ("csrf", "t"),
            ("branch", "0"),
            ("category", "4"),
            ("d1", " 3 "),
            ("e1", "1"),
            ("r1", "1"),
            ("d2", ""),
            ("d3", "10"),
            ("p3", "1"),
        ]))
        .unwrap();
        assert_eq!(
            (csrf.as_str(), branch, category),
            ("t", None, PatronCategoryId(4))
        );
        let levels = levels_of(&levels).unwrap();
        assert_eq!(
            levels
                .iter()
                .map(|level| (level.level.number(), level.delay, level.restrict))
                .collect::<Vec<_>>(),
            [(1, 3, true), (3, 10, false)]
        );
        assert!(levels[0].transports.email && !levels[0].transports.print);
        assert!(read_rule(&fields(&[("csrf", "t"), ("category", "4"), ("x9", "1")])).is_none());
        assert!(read_rule(&fields(&[("csrf", "t")])).is_none());
    }

    #[test]
    fn days_must_be_a_whole_number_above_zero() {
        for delay in ["0", "-1", "2.5", "três"] {
            let mut typed: [LevelFields; 3] = Default::default();
            delay.clone_into(&mut typed[0].delay);
            assert_eq!(levels_of(&typed), None, "{delay}");
        }
    }

    #[test]
    fn stored_levels_fill_their_rows() {
        let typed = fields_of(&[OverdueLevel {
            level: NoticeLevel::Second,
            delay: 7,
            transports: Transports {
                email: true,
                print: false,
            },
            restrict: true,
        }]);
        assert_eq!(typed[0], LevelFields::default());
        assert_eq!(typed[1].delay, "7");
        assert!(typed[1].restrict && typed[1].transports.email);
    }
}
