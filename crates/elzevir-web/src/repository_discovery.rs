// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{AppState, render};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Response,
};
use elzevir_app::repository::{DigitalCollectionId, discover};
use elzevir_web_templates::{DiscoveryResult, RepositorySearch, url_encode};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Default, Deserialize)]
pub(super) struct Params {
    #[serde(default)]
    q: String,
    collection: Option<String>,
    page: Option<usize>,
}

pub(super) async fn search(
    State(state): State<AppState>,
    Query(params): Query<Params>,
) -> Result<Response, StatusCode> {
    let collection = params
        .collection
        .as_deref()
        .map(crate::repository::id)
        .transpose()?
        .map(DigitalCollectionId);
    let access = state.repository.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let permit = access.slot()?;
    let port = Arc::clone(&access.port);
    let index = Arc::clone(&state.inner.opac.index);
    let page = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        discover(
            index.as_ref(),
            port.as_ref(),
            &params.q,
            collection,
            params.page.unwrap_or(1),
        )
    })
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let base = format!("/repository?q={}", url_encode(&page.query));
    let selected = collection.map(|id| format!("{:032x}", id.0));
    let filtered = selected
        .as_ref()
        .map_or_else(|| base.clone(), |id| format!("{base}&collection={id}"));
    let rows = page
        .rows
        .into_iter()
        .map(|(deposit, title)| DiscoveryResult {
            title,
            id: format!("{:032x}", deposit.id.0),
            collections: deposit
                .collections
                .into_iter()
                .map(|(id, name)| (format!("{:032x}", id.0), name))
                .collect(),
        })
        .collect();
    Ok(render(&RepositorySearch {
        t: &state.inner.i18n,
        query: page.query,
        selected,
        clear_url: base,
        rows,
        previous: (page.page > 1).then(|| format!("{filtered}&page={}", page.page - 1)),
        next: page
            .has_next
            .then(|| format!("{filtered}&page={}", page.page + 1)),
    }))
}
