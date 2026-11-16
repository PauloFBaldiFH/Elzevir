// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff catalog listing: browse and search on one screen
//! (docs/specs/catalogo/listagem.md).

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use elzevir_app::catalog::{CodeLists, ShelfSignal, StatementReader};
use elzevir_app::editing::CatalogEditor;
use elzevir_app::export::{
    ExportOptions, RecordExportError, RecordFormat, RecordSource, export_record,
};
use elzevir_app::items::ItemGridSource;
use elzevir_app::listing::{
    self, CatalogListing, FamilyCursor, FamilyHead, FamilyRow, InstanceRow, ItemPill, Match, Scan,
    WorkRow,
};
use elzevir_app::staff::Permission;
use elzevir_app::suggestions::AuthoritySuggestions;
use elzevir_domain::catalog::InstanceId;
use elzevir_web_templates::{
    AuthorityLine, FamilyLine, HubLine, InstanceLine, PillLine, ScanLine, StaffCatalog, WorkLine,
    url_encode,
};
use serde::Deserialize;

use crate::icons::{
    NO_CONTENT, agent_icon, content_badge, item_icon, material_badges, signal_icon,
};
use crate::staff::current_session;
use crate::{AppState, render};

/// What the staff listing reads.
#[derive(Clone)]
pub struct StaffPorts {
    /// Families of the catalog.
    pub listing: Arc<dyn CatalogListing>,
    /// Stored records, for exporting one.
    pub records: Arc<dyn RecordSource + Send + Sync>,
    /// Edits from the screen (docs/specs/catalogo/edicao.md, items 16 to 24).
    pub editor: Arc<dyn CatalogEditor + Send + Sync>,
    /// Existing agents and subjects suggested while cataloguing.
    pub suggestions: Arc<dyn AuthoritySuggestions + Send + Sync>,
    /// The Items of an Instance as the grid edits them (docs/specs/catalogo/exemplares.md).
    pub items: Arc<dyn ItemGridSource>,
    /// The statements of any entity, for the forms of a Hub and an Item
    /// (docs/specs/catalogo/bibframe-completo.md, step 6c).
    pub statements: Arc<dyn StatementReader + Send + Sync>,
}

/// Query string of `/equipe/acervo`.
#[derive(Deserialize)]
pub(crate) struct CatalogParams {
    #[serde(default)]
    q: String,
    /// Browsing: the family the page starts after.
    after: Option<String>,
    /// Searching: the hit the page starts from.
    from: Option<String>,
}

/// One page, as read.
enum Page {
    Browse(listing::ListingPage),
    /// A search, with what its text was taken for.
    Search(listing::SearchPage, Scan),
}

/// `GET /equipe/acervo?q=&after=&from=`: the listing, or a search when `q` is given. An
/// HTMX request for the next page gets only the rows.
pub(crate) async fn catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CatalogParams>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    let listing = Arc::clone(&state.inner.staff_ports.listing);
    let index = Arc::clone(&state.inner.opac.index);
    let catalog = Arc::clone(&state.inner.opac.catalog);
    let read = tokio::task::spawn_blocking(move || {
        let codes = catalog.codes().map_err(drop)?;
        let page = if params.q.trim().is_empty() {
            // A malformed cursor is read as the first page.
            let after = params.after.as_deref().and_then(FamilyCursor::from_token);
            listing::browse(&*listing, after.as_ref())
                .map(Page::Browse)
                .map_err(drop)
        } else if let Some(from) = params.from {
            // A later page of a text search: the scan rule saw the first one.
            let from = from.parse::<usize>().unwrap_or(0);
            listing::search(&*index, &*listing, &params.q, from)
                .map(|page| Page::Search(page, Scan::Text))
                .map_err(drop)
        } else {
            // One field for everything (docs/specs/catalogo/bipe.md).
            let scan = listing::scan(&*listing, &params.q).map_err(drop)?;
            match &scan {
                Scan::Known(owner) => {
                    listing::scanned_page(&*listing, &params.q, owner).map_err(drop)
                }
                Scan::NewIsbn(_) | Scan::Text => {
                    listing::search(&*index, &*listing, &params.q, 0).map_err(drop)
                }
            }
            .map(|page| Page::Search(page, scan))
        }?;
        Ok::<_, ()>((page, codes))
    })
    .await;
    let Ok(Ok((page, codes))) = read else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let can_catalogue = session.identity.can(Permission::Cataloguing);
    let page = match &page {
        Page::Browse(page) => StaffCatalog {
            t: &state.inner.i18n,
            csrf: &session.csrf,
            can_catalogue,
            query: "",
            total: None,
            scan: None,
            rows: page
                .rows
                .iter()
                .map(|row| family_line(row, &codes))
                .collect(),
            next_url: page
                .next
                .as_ref()
                .map(|next| format!("/equipe/acervo?after={}", url_encode(&next.token()))),
        },
        Page::Search(page, scan) => StaffCatalog {
            t: &state.inner.i18n,
            csrf: &session.csrf,
            can_catalogue,
            query: &page.query,
            total: Some(page.total),
            scan: scan_line(scan, can_catalogue),
            rows: opened(
                page.rows
                    .iter()
                    .map(|row| family_line(row, &codes))
                    .collect(),
            ),
            next_url: page
                .next
                .map(|from| format!("/equipe/acervo?q={}&from={from}", url_encode(&page.query))),
        },
    };
    // The search form swaps the whole list and picks it from a full page; the next-page
    // loader asks for the rows alone.
    let whole_list = headers
        .get("hx-target")
        .is_some_and(|target| target == "families");
    if headers.contains_key("hx-request") && !whole_list {
        match page.as_rows().render() {
            Ok(html) => Html(html).into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
        }
    } else {
        render(&page)
    }
}

/// What the page's script does with a scanned code (bipe.md, items 7 and 8): the grid of
/// a known code's Instance, or a new family for an unknown ISBN, for cataloguers.
fn scan_line(scan: &Scan, can_catalogue: bool) -> Option<ScanLine> {
    match scan {
        Scan::Known(owner) => Some(ScanLine {
            kind: "known",
            instance: Some(owner.instance.0),
            item: owner.item.map(|item| item.0),
            isbn: None,
        }),
        Scan::NewIsbn(isbn) if can_catalogue => Some(ScanLine {
            kind: "new-isbn",
            instance: None,
            item: None,
            isbn: Some(isbn.clone()),
        }),
        Scan::NewIsbn(_) | Scan::Text => None,
    }
}

/// A search opens the first family it found an Instance in (listagem.md, item 32).
fn opened(mut rows: Vec<FamilyLine>) -> Vec<FamilyLine> {
    if let Some(first) = rows.iter_mut().find(|row| {
        row.works
            .iter()
            .any(|work| work.instances.iter().any(|instance| instance.found))
    }) {
        first.open = true;
    }
    rows
}

/// Query string of the one-record export.
#[derive(Deserialize)]
pub(crate) struct ExportParams {
    format: Option<String>,
}

/// `GET /equipe/acervo/instancia/{id}/marc?format=marc21|marcxml`: one record as an
/// attachment (docs/specs/catalogo/listagem.md, items 33 to 36).
pub(crate) async fn export_marc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(params): Query<ExportParams>,
) -> Response {
    if current_session(&state, &headers).is_none() {
        return Redirect::to("/equipe/login").into_response();
    }
    let (format, content_type, extension) = match params.format.as_deref() {
        None | Some("marc21") => (RecordFormat::Marc21, "application/marc", "mrc"),
        Some("marcxml") => (
            RecordFormat::MarcXml,
            "application/marcxml+xml; charset=utf-8",
            "xml",
        ),
        Some(_) => return (StatusCode::BAD_REQUEST, "unknown format").into_response(),
    };
    let records = Arc::clone(&state.inner.staff_ports.records);
    let written = tokio::task::spawn_blocking(move || {
        export_record(
            &*records,
            InstanceId(id),
            format,
            &ExportOptions::default().limits,
        )
    })
    .await;
    match written {
        Ok(Ok(Some(bytes))) => (
            [
                (header::CONTENT_TYPE, content_type.to_owned()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"elzevir-{id}.{extension}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such record").into_response(),
        // Staff only: the reason helps the cataloguer find what to fix in the record.
        Ok(Err(RecordExportError::Failure(failure))) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the record cannot be exported: {failure}"),
        )
            .into_response(),
        Ok(Err(RecordExportError::Read(_))) | Err(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response()
        }
    }
}

fn family_line(row: &FamilyRow, codes: &CodeLists) -> FamilyLine {
    let hub = match (row.cursor.head, &row.hub_title) {
        (FamilyHead::Hub(id), Some(title)) => Some(HubLine {
            id: id.0,
            title: title.clone(),
        }),
        _ => None,
    };
    let mut authorities: Vec<AuthorityLine> = Vec::new();
    let works = row
        .works
        .iter()
        .map(|work| {
            let authority = work.main_contributor.as_ref().map(|(kind, name)| {
                if let Some(known) = authorities.iter().find(|a| &a.name == name) {
                    known.key
                } else {
                    let key = authorities.len();
                    authorities.push(AuthorityLine {
                        key,
                        icon: agent_icon(*kind),
                        name: name.clone(),
                    });
                    key
                }
            });
            work_line(work, authority, codes)
        })
        .collect();
    let instances = || row.works.iter().flat_map(|work| work.instances.iter());
    let first_work = row.works.first();
    FamilyLine {
        head: match row.cursor.head {
            FamilyHead::Hub(id) => format!("h{}", id.0),
            FamilyHead::Work(id) => format!("w{}", id.0),
        },
        title: hub.as_ref().map_or_else(
            || {
                first_work
                    .map(|work| work.title.clone())
                    .unwrap_or_default()
            },
            |hub| hub.title.clone(),
        ),
        icon: instances()
            .find_map(|instance| instance.material.content)
            .map_or(NO_CONTENT, |kind| content_badge(kind).icon),
        classification: first_work.and_then(|work| work.classification.clone()),
        instances: instances().count(),
        items: instances().map(|instance| instance.items).sum(),
        available: instances().map(|instance| instance.available).sum(),
        open: false,
        authorities,
        hub,
        works,
    }
}

fn work_line(work: &WorkRow, authority: Option<usize>, codes: &CodeLists) -> WorkLine {
    WorkLine {
        authority,
        id: work.id.0,
        title: work.title.clone(),
        contributor: work.main_contributor.as_ref().map(|(_, name)| name.clone()),
        classification: work.classification.clone(),
        languages: work.languages.clone(),
        subjects: work.subjects.clone(),
        more_subjects: work.more_subjects,
        instances: work
            .instances
            .iter()
            .map(|instance| instance_line(instance, codes))
            .collect(),
    }
}

fn instance_line(instance: &InstanceRow, codes: &CodeLists) -> InstanceLine {
    let imprint = match (&instance.publisher, &instance.date) {
        (Some(publisher), Some(date)) => Some(format!("{publisher}, {date}")),
        (Some(only), None) | (None, Some(only)) => Some(only.clone()),
        (None, None) => None,
    };
    InstanceLine {
        id: instance.id.0,
        title: instance.title.clone(),
        edition: instance.edition.clone(),
        imprint,
        isbn: instance.isbn.clone(),
        material: material_badges(instance.material),
        icon: item_icon(instance.material),
        found: instance.matched == Match::Found,
        // Neighbouring editions get different colours; the cover itself is a placeholder
        // until real covers arrive (UI spec §6.4).
        cover: u8::try_from(instance.id.0.rem_euclid(4)).unwrap_or(0),
        items: instance.items,
        pills: instance
            .pills
            .iter()
            .map(|pill| pill_line(pill, codes))
            .collect(),
        more_items: instance.more_items,
    }
}

/// One Item in a line, with names in place of codes (exemplares.md, item 29).
fn pill_line(pill: &ItemPill, codes: &CodeLists) -> PillLine {
    let parts = [
        Some(codes.branch(pill.home_branch)),
        pill.location.map(|id| codes.location(id)),
        pill.call_number.as_deref(),
        pill.inventory_number.as_deref(),
    ];
    let (signal_class, signal_key) = signal(pill.signal);
    PillLine {
        text: parts.into_iter().flatten().collect::<Vec<_>>().join(" | "),
        signal_class,
        signal_key,
        signal_icon: signal_icon(pill.signal),
    }
}

/// The CSS class and message key of a signal.
pub(crate) const fn signal(signal: ShelfSignal) -> (&'static str, &'static str) {
    match signal {
        ShelfSignal::Available => ("signal-available", "shelf-signal-available"),
        ShelfSignal::NotForLoan => ("signal-not-for-loan", "shelf-signal-not-for-loan"),
        ShelfSignal::OutOfCirculation => (
            "signal-out-of-circulation",
            "shelf-signal-out-of-circulation",
        ),
    }
}
