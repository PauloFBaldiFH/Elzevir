// SPDX-License-Identifier: GPL-3.0-or-later
//! Authenticated, read-only review of a versioned relation plan.
use super::{AppState, ImportSession, tokens_match};
use axum::extract::{Form, Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use elzevir_app::import::batches::{BatchCommand, BatchError, BatchReply, RelationPreview};
use elzevir_app::import::relations::{Cardinality, Outcome, Selection};
use elzevir_web_templates::{I18n, ImportView, RelationView, RelationsView};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Review {
    csrf: String,
    revision: u64,
    method: String,
    #[serde(default)]
    plan: String,
    from: u64,
}

pub(super) async fn preview(
    State(state): State<AppState>,
    session: ImportSession,
    Path(id): Path<String>,
    Form(form): Form<Review>,
) -> Response {
    if !session.can_inventory_sources() || !tokens_match(&session.active.csrf, &form.csrf) {
        return session.error(&state, StatusCode::FORBIDDEN, "import-forbidden", None);
    }
    let selection = match form.method.as_str() {
        "folio-v1" if form.plan.trim().is_empty() => Selection::FolioV1,
        "explicit" => match Selection::parse(form.plan.as_bytes()) {
            Ok(plan) => plan,
            Err(err) => return session.failure(&state, BatchError::Invalid(err)),
        },
        _ => return session.error(&state, StatusCode::BAD_REQUEST, "import-invalid", None),
    };
    let command = BatchCommand::Relations(id, form.revision, selection, form.from);
    match session.run(command).await {
        Ok(BatchReply::Relations(report)) => match present(session.translations(&state), *report) {
            Ok(view) => session.page(
                &state,
                ImportView::Relations(Box::new(view)),
                StatusCode::OK,
                None,
                None,
            ),
            Err(err) => session.failure(&state, err),
        },
        Err(err) => session.failure(&state, err),
        Ok(_) => session.error(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            "import-unavailable",
            None,
        ),
    }
}

fn present(t: &I18n, preview: RelationPreview) -> Result<RelationsView, BatchError> {
    let json = serde_json::to_string_pretty(&preview)
        .map_err(|_| BatchError::Unavailable("cannot encode relation evidence".into()))?;
    let report = preview.report;
    let datasets = report
        .plan
        .datasets
        .iter()
        .zip(&report.identities)
        .map(|(d, i)| (d.name.clone(), i.member, i.counts.values().sum()))
        .collect();
    let mut edges = Vec::new();
    for (rule, edge) in report.plan.relations.iter().zip(report.edges) {
        let parent = report
            .plan
            .datasets
            .iter()
            .find(|d| d.name == rule.parent)
            .ok_or_else(|| BatchError::Unavailable("missing reviewed dataset".into()))?;
        edges.push(RelationView {
            binding: format!(
                "{} ({}) → {} ({})",
                rule.child,
                rule.foreign.join(", "),
                rule.parent,
                parent.key.join(", ")
            ),
            cardinality: t.get(match rule.cardinality {
                Cardinality::ManyToOne => "import-rel-many",
                Cardinality::OneToOne => "import-rel-one",
            }),
            counts: edge
                .counts
                .into_iter()
                .map(|(reason, n)| (t.get(reason_key(reason)), n))
                .collect(),
            page: edge
                .page
                .iter()
                .map(|finding| {
                    let coordinate = |pos: &elzevir_app::import::relations::Position| {
                        format!("{} @ {}", pos.line, pos.offset)
                    };
                    let parents = finding
                        .parents
                        .iter()
                        .map(coordinate)
                        .collect::<Vec<_>>()
                        .join(", ");
                    let ancestors = finding
                        .ancestors
                        .iter()
                        .filter_map(|hop| {
                            report.plan.relations.get(hop.rule).map(|step| {
                                let where_to = hop
                                    .target
                                    .as_ref()
                                    .map_or_else(|| t.get(reason_key(hop.outcome)), coordinate);
                                format!("{} ({where_to})", step.parent)
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(" → ");
                    (
                        coordinate(&finding.row),
                        t.get(reason_key(finding.outcome)),
                        parents,
                        ancestors,
                    )
                })
                .collect(),
            parents: edge.linked_parents,
            max_children: edge.max_children,
        });
    }
    Ok(RelationsView {
        batch: preview.batch,
        origin: preview.sources.origin,
        evidence: report.evidence.into(),
        datasets,
        edges,
        json,
    })
}

const fn reason_key(reason: Outcome) -> &'static str {
    match reason {
        Outcome::Resolved => "import-rel-resolved",
        Outcome::Missing => "import-rel-missing",
        Outcome::Null => "import-rel-null",
        Outcome::Empty => "import-rel-empty",
        Outcome::Invalid => "import-rel-invalid",
        Outcome::IdentityConflict => "import-rel-conflict",
        Outcome::DuplicateIdentity => "import-rel-duplicate",
        Outcome::ParentAbsent => "import-rel-absent",
        Outcome::Orphan => "import-rel-orphan",
        Outcome::AmbiguousParent => "import-rel-ambiguous",
        Outcome::CardinalityConflict => "import-rel-cardinality",
    }
}
