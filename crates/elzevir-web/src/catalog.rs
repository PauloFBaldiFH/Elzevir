// SPDX-License-Identifier: GPL-3.0-or-later
//! The staff catalog listing: browse and search on one screen
//! (docs/specs/catalogo/listagem.md).

use elzevir_app::authorities::AuthorityReader;
use std::sync::Arc;

use elzevir_app::history::HistoryReader;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use elzevir_app::catalog::{CatalogReader, CodeLists, ShelfSignal, StatementReader};
use elzevir_app::editing::CatalogEditor;
use elzevir_app::export::{
    ExportOptions, RecordExportError, RecordFormat, RecordSource, export_record,
};
use elzevir_app::items::ItemGridSource;
use elzevir_app::listing::{
    self, CatalogListing, FamilyCursor, FamilyHead, FamilyRow, InstanceRow, ItemPill, Match, Scan,
    WorkRow,
};
use elzevir_app::rdf::{IriBase, RdfFormat, export_rdf};
use elzevir_app::search::{FacetCounts, Filter, SearchIndex, SearchQuery, SearchScope};
use elzevir_app::staff::Permission;
use elzevir_app::suggestions::AuthoritySuggestions;
use elzevir_app::worklist::{self, SavedList, WorkList, WorkListQuery, WorkListReader};
use elzevir_domain::catalog::InstanceId;
use elzevir_web_templates::{
    AuthorityLine, BatchChoices, Cord, FacetStrip, FamilyLine, HubLine, I18n, InstanceLine,
    PillLine, ScanLine, StaffCatalog, WorkLine, url_encode,
};
use serde::Deserialize;

use crate::facets;
use crate::icons::{
    NO_CONTENT, agent_icon, content_badge, item_icon, material_badges, signal_icon, subject_icon,
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
    /// Who changed a Work or an Instance (docs/specs/catalogo/historico.md).
    pub history: Arc<dyn HistoryReader>,
    /// The agents and subjects the authorities column edits (docs/specs/catalogo/autoridades.md).
    pub authorities: Arc<dyn AuthorityReader>,
    /// The families of the work lists (docs/specs/catalogo/facetas.md, part c).
    pub work_lists: Arc<dyn WorkListReader>,
}

/// Query string of `/equipe/acervo`.
struct CatalogParams {
    q: String,
    /// Browsing: the family the page starts after.
    after: Option<String>,
    /// Searching: the hit the page starts from.
    from: Option<String>,
    /// The filters in force (facetas.md, item 4).
    filters: Vec<Filter>,
    /// The work list asked for, which text and filters override (facetas.md, item 14).
    list: Option<WorkList>,
}

impl CatalogParams {
    /// Reads the parameters, the last of each name winning, and every filter.
    fn from_pairs(pairs: &[(String, String)]) -> Self {
        let last = |name: &str| {
            pairs
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        Self {
            q: last("q").unwrap_or_default(),
            after: last("after"),
            from: last("from"),
            filters: facets::filters_of(pairs),
            list: last(facets::LIST_PARAM)
                .as_deref()
                .and_then(WorkList::from_name),
        }
    }
}

/// What one request of the listing reads.
struct Read {
    page: Page,
    codes: CodeLists,
    counts: Option<FacetCounts>,
    /// The saved lists, for the strip and the selection bar; empty for rows alone.
    saved: Vec<SavedList>,
    params: CatalogParams,
}

/// One page, as read.
enum Page {
    Browse(listing::ListingPage),
    /// A search, with what its text was taken for.
    Search(listing::SearchPage, Scan),
    /// A work list.
    List(worklist::ListPage, WorkList),
}

/// The ports a page reads, borrowed.
struct Readers<'a> {
    listing: &'a dyn CatalogListing,
    index: &'a dyn SearchIndex,
    catalog: &'a dyn CatalogReader,
    work_lists: &'a dyn WorkListReader,
}

impl<'a> Readers<'a> {
    fn of(state: &'a AppState) -> Self {
        Self {
            listing: &*state.inner.staff_ports.listing,
            index: &*state.inner.opac.index,
            catalog: &*state.inner.opac.catalog,
            work_lists: &*state.inner.staff_ports.work_lists,
        }
    }
}

/// The rows of one page: a work list, a stretch of the catalog or a search. Blocking.
fn read_rows(
    readers: &Readers<'_>,
    params: &CatalogParams,
    list: Option<(WorkList, WorkListQuery)>,
    plain: bool,
) -> Result<Page, ()> {
    let Readers {
        listing,
        index,
        work_lists,
        ..
    } = *readers;
    let filters = &params.filters;
    if let Some((list, query)) = list.filter(|_| plain) {
        // A malformed offset is read as the first page.
        let from = params.from.as_deref().and_then(|from| from.parse().ok());
        worklist::page(work_lists, listing, query, from.unwrap_or(0))
            .map(|page| Page::List(page, list))
            .map_err(drop)
    } else if plain {
        // A malformed cursor is read as the first page.
        let after = params.after.as_deref().and_then(FamilyCursor::from_token);
        listing::browse(listing, after.as_ref())
            .map(Page::Browse)
            .map_err(drop)
    } else if let Some(from) = &params.from {
        // A later page of a text search: the scan rule saw the first one.
        let from = from.parse::<usize>().unwrap_or(0);
        listing::search(index, listing, &params.q, filters, from)
            .map(|page| Page::Search(page, Scan::Text))
            .map_err(drop)
    } else if !filters.is_empty() {
        // A filtered search lists what passes; a code typed there is just text.
        listing::search(index, listing, &params.q, filters, 0)
            .map(|page| Page::Search(page, Scan::Text))
            .map_err(drop)
    } else {
        // One field for everything (docs/specs/catalogo/bipe.md).
        let scan = listing::scan(listing, &params.q).map_err(drop)?;
        match &scan {
            Scan::Known(owner) => listing::scanned_page(listing, &params.q, owner).map_err(drop),
            Scan::NewIsbn(_) | Scan::Text => {
                listing::search(index, listing, &params.q, &[], 0).map_err(drop)
            }
        }
        .map(|page| Page::Search(page, scan))
    }
}

/// Reads one page of the listing and, unless only rows are asked for, a code was scanned
/// or the page is a work list, the facets of what it shows (facetas.md, items 11 and 17).
/// `list` is the work list asked for, as the store reads it. Blocking.
fn read_page(
    readers: &Readers<'_>,
    params: CatalogParams,
    list: Option<(WorkList, WorkListQuery)>,
    rows_only: bool,
) -> Result<Read, ()> {
    let Readers {
        index,
        catalog,
        work_lists,
        ..
    } = *readers;
    let codes = catalog.codes().map_err(drop)?;
    let filters = &params.filters;
    let plain = params.q.trim().is_empty() && filters.is_empty();
    // The facets are counted on other cores while the page is read: over a filter that
    // matches most of a large catalog they take as long as the rest of the page
    // (facetas.md, item 12). A work list has none; a scanned code's are dropped.
    let faceted = !(rows_only || plain && list.is_some());
    let asked = SearchQuery {
        text: &params.q,
        filters,
    };
    let (page, counts) = std::thread::scope(|scope| {
        let counting = faceted.then(|| scope.spawn(|| index.facets(&asked, SearchScope::Staff)));
        let page = read_rows(readers, &params, list, plain);
        let counts = match counting.map(std::thread::ScopedJoinHandle::join) {
            Some(Ok(Ok(counts))) => Some(counts),
            Some(_) => return Err(()),
            None => None,
        };
        Ok((page?, counts))
    })?;
    // A scanned code's page is that family alone.
    let counts = counts.filter(|_| !matches!(&page, Page::Search(_, Scan::Known(_))));
    let saved = if rows_only {
        Vec::new()
    } else {
        work_lists.saved_lists().map_err(drop)?
    };
    Ok(Read {
        page,
        codes,
        counts,
        saved,
        params,
    })
}

/// `GET /equipe/acervo?q=&after=&from=&f=`: the listing, or a search when `q` or a filter
/// is given, with the facets strip. An HTMX request for the next page gets only the rows.
pub(crate) async fn catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    let Some(session) = current_session(&state, &headers) else {
        return Redirect::to("/equipe/login").into_response();
    };
    let params = CatalogParams::from_pairs(&pairs);
    let list = params
        .list
        .map(|list| list_query(list, session.identity.id, &state.inner.config.time_zone));
    if list.as_ref().is_some_and(Option::is_none) {
        return (StatusCode::SERVICE_UNAVAILABLE, "clock unavailable").into_response();
    }
    let list = list.flatten();
    let reading = state.clone();
    let rows_only = rows_only(&headers);
    let read = tokio::task::spawn_blocking(move || {
        read_page(&Readers::of(&reading), params, list, rows_only)
    })
    .await;
    let Ok(Ok(Read {
        page,
        codes,
        counts,
        saved,
        params,
    })) = read
    else {
        return (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response();
    };
    let t = &state.inner.i18n;
    let query = params.q.trim();
    let strip = match (&page, counts) {
        (Page::List(list, name), _) if !rows_only => {
            Some(facets::list_strip(*name, list.rows.is_empty()))
        }
        (_, Some(counts)) => Some(facets::strip(query, &params.filters, counts, t, &codes)),
        _ => None,
    };
    let can_catalogue = session.identity.can(Permission::Cataloguing);
    let can_circulate = state.loans.is_some() && session.identity.can(Permission::Circulation);
    let (strip, batch) = with_saved(strip, &page, &saved, &codes, can_catalogue, rows_only);
    let lines = |rows: &[FamilyRow]| -> Vec<FamilyLine> {
        rows.iter().map(|row| family_line(row, &codes, t)).collect()
    };
    let page = match &page {
        Page::Browse(page) => StaffCatalog {
            t: &state.inner.i18n,
            csrf: &session.csrf,
            can_catalogue,
            can_circulate,
            query: "",
            total: None,
            scan: None,
            rows: lines(&page.rows),
            next_url: page
                .next
                .as_ref()
                .map(|next| format!("/equipe/acervo?after={}", url_encode(&next.token()))),
            strip,
            batch,
        },
        Page::Search(page, scan) => StaffCatalog {
            t: &state.inner.i18n,
            csrf: &session.csrf,
            can_catalogue,
            can_circulate,
            query: &page.query,
            total: Some(page.total),
            scan: scan_line(scan, can_catalogue),
            rows: opened(lines(&page.rows)),
            next_url: page.next.map(|from| {
                format!(
                    "{}&from={from}",
                    facets::listing_url(&page.query, &params.filters)
                )
            }),
            strip,
            batch,
        },
        Page::List(page, list) => StaffCatalog {
            t: &state.inner.i18n,
            csrf: &session.csrf,
            can_catalogue,
            can_circulate,
            query: "",
            total: None,
            scan: None,
            rows: lines(&page.rows),
            next_url: page
                .next
                .map(|from| format!("{}&from={from}", facets::list_url(*list))),
            strip,
            batch,
        },
    };
    respond(&page, rows_only)
}

/// The search form swaps the whole list and picks it from a full page; the next-page loader
/// asks for the rows alone.
fn respond(page: &StaffCatalog<'_>, rows_only: bool) -> Response {
    if rows_only {
        match page.as_rows().render() {
            Ok(html) => Html(html).into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
        }
    } else {
        render(page)
    }
}

/// The strip with the saved lists' menu, and the batch actions of a cataloguer on a full
/// page (docs/specs/catalogo/lote.md, items 16, 18 and 20).
fn with_saved(
    strip: Option<FacetStrip>,
    page: &Page,
    saved: &[SavedList],
    codes: &CodeLists,
    can_catalogue: bool,
    rows_only: bool,
) -> (Option<FacetStrip>, Option<BatchChoices>) {
    let shown = match page {
        Page::List(_, list) => Some(*list),
        _ => None,
    };
    let strip = strip.map(|mut strip| {
        facets::with_saved(&mut strip, saved, shown, can_catalogue);
        strip
    });
    // The saved list shown, if it still exists.
    let shown_saved = saved
        .iter()
        .find(|list| shown == Some(WorkList::Saved(list.id)))
        .map(|list| list.id);
    // The batch actions are only built on a full page: the next rows do not carry them.
    let batch =
        (can_catalogue && !rows_only).then(|| crate::batch::choices(codes, saved, shown_saved));
    (strip, batch)
}

/// Whether the request is the next-page loader's, which asks for the rows alone: the
/// search form swaps the whole list and targets `#families`.
fn rows_only(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
        && headers
            .get("hx-target")
            .is_none_or(|target| target != "families")
}

/// What the store reads for `list`, asked by `staff`; `None` only if the clock is out of
/// the calendar's range.
fn list_query(
    list: WorkList,
    staff: elzevir_app::staff::StaffId,
    time_zone: &jiff::tz::TimeZone,
) -> Option<(WorkList, WorkListQuery)> {
    let query = match list {
        WorkList::Recent => WorkListQuery::TouchedBy(staff),
        WorkList::ImportedToday => WorkListQuery::ImportedSince(today_began(time_zone)?),
        WorkList::Saved(saved) => WorkListQuery::Saved(saved),
    };
    Some((list, query))
}

/// When today began in the library's time zone (facetas.md, item 16); `None` only if
/// the clock is out of the calendar's range.
fn today_began(time_zone: &jiff::tz::TimeZone) -> Option<jiff::Timestamp> {
    jiff::Timestamp::now()
        .to_zoned(time_zone.clone())
        .start_of_day()
        .ok()
        .map(|start| start.timestamp())
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

/// `GET /equipe/acervo/instancia/{id}/bibframe?format=turtle|rdfxml`: one record, its Work,
/// Instance and Items, as BIBFRAME (bibframe-completo.md, item 28b).
pub(crate) async fn export_bibframe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(params): Query<ExportParams>,
) -> Response {
    if current_session(&state, &headers).is_none() {
        return Redirect::to("/equipe/login").into_response();
    }
    let (format, content_type, extension) = match params.format.as_deref() {
        None | Some("turtle") => (RdfFormat::Turtle, "text/turtle; charset=utf-8", "ttl"),
        Some("rdfxml") => (
            RdfFormat::RdfXml,
            "application/rdf+xml; charset=utf-8",
            "rdf",
        ),
        Some(_) => return (StatusCode::BAD_REQUEST, "unknown format").into_response(),
    };
    let Some(base) = iri_base(&state, &headers) else {
        return (StatusCode::BAD_REQUEST, "no host to name the IRIs after").into_response();
    };
    let records = Arc::clone(&state.inner.staff_ports.records);
    let written =
        tokio::task::spawn_blocking(move || export_rdf(&*records, InstanceId(id), format, &base))
            .await;
    match written {
        Ok(Ok(Some(text))) => (
            [
                (header::CONTENT_TYPE, content_type.to_owned()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"elzevir-{id}.{extension}\""),
                ),
            ],
            text,
        )
            .into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no such record").into_response(),
        Ok(Err(_)) | Err(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, "catalog unavailable").into_response()
        }
    }
}

/// The configured base of the BIBFRAME IRIs, else the requested host's `/bibframe`. A host
/// with characters no host name has is refused, so it never reaches an IRI.
fn iri_base(state: &AppState, headers: &HeaderMap) -> Option<IriBase> {
    if let Some(base) = &state.inner.config.iri_base {
        return Some(IriBase::new(base));
    }
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let valid = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c));
    let scheme = if state.inner.config.secure_cookies {
        "https"
    } else {
        "http"
    };
    valid.then(|| IriBase::new(&format!("{scheme}://{host}/bibframe")))
}

fn family_line(row: &FamilyRow, codes: &CodeLists, t: &I18n) -> FamilyLine {
    let hub = match (row.cursor.head, &row.hub_title) {
        (FamilyHead::Hub(id), Some(title)) => Some(HubLine {
            id: id.0,
            title: title.clone(),
        }),
        _ => None,
    };
    let mut authorities: Vec<AuthorityLine> = Vec::new();
    // Each agent and subject once in the column, whichever Works name it.
    let mut key_of = |fields: Option<String>, line: AuthorityLine| {
        let known = authorities
            .iter()
            .find(|a| a.subject == line.subject && a.name == line.name && a.fields == fields);
        if let Some(known) = known {
            known.key
        } else {
            let key = authorities.len();
            authorities.push(AuthorityLine {
                key,
                fields,
                ..line
            });
            key
        }
    };
    let works = row
        .works
        .iter()
        .map(|work| {
            let mut cords = Vec::new();
            if let (FamilyHead::Hub(id), Some(_)) = (row.cursor.head, &row.hub_title) {
                cords.push(Cord {
                    node: format!("h{}", id.0),
                    label_key: "staff-cord-work",
                });
            }
            if let Some((kind, name)) = &work.main_contributor {
                let key = key_of(
                    work.main_agent
                        .map(|agent| format!("/equipe/acervo/autoridade/{}/campos", agent.0)),
                    AuthorityLine {
                        key: 0,
                        name: name.clone(),
                        fields: None,
                        pill_key: "staff-badge-agent",
                        subject: false,
                        icon: agent_icon(*kind),
                    },
                );
                cords.push(Cord {
                    node: format!("a{key}"),
                    label_key: "staff-cord-written-by",
                });
            }
            for subject in &work.subjects {
                let key = key_of(
                    Some(format!("/equipe/acervo/assunto/{}/campos", subject.id.0)),
                    AuthorityLine {
                        key: 0,
                        name: subject.heading.clone(),
                        fields: None,
                        pill_key: "staff-badge-subject",
                        subject: true,
                        icon: subject_icon(subject.kind),
                    },
                );
                cords.push(Cord {
                    node: format!("a{key}"),
                    label_key: "staff-cord-about",
                });
            }
            work_line(work, cords, codes, t)
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

fn work_line(work: &WorkRow, cords: Vec<Cord>, codes: &CodeLists, t: &I18n) -> WorkLine {
    WorkLine {
        cords,
        id: work.id.0,
        title: work.title.clone(),
        contributor: work.main_contributor.as_ref().map(|(_, name)| name.clone()),
        classification: work.classification.clone(),
        languages: work.languages.clone(),
        subjects: work
            .subjects
            .iter()
            .map(|subject| subject.heading.clone())
            .collect(),
        more_subjects: work.more_subjects,
        instances: work
            .instances
            .iter()
            .map(|instance| instance_line(instance, codes, t))
            .collect(),
    }
}

fn instance_line(instance: &InstanceRow, codes: &CodeLists, t: &I18n) -> InstanceLine {
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
            .map(|pill| pill_line(pill, codes, t))
            .collect(),
        more_items: instance.more_items,
    }
}

/// One Item in a line, with names in place of codes (exemplares.md, item 29).
pub(crate) fn pill_line(pill: &ItemPill, codes: &CodeLists, t: &I18n) -> PillLine {
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
        item: pill.id.0,
        lendable: pill.signal == ShelfSignal::Available && pill.barcode.is_some(),
        due: pill.due.map(|day| crate::patrons::shown(t, day)),
        refused: Vec::new(),
    }
}

/// The CSS class and message key of a signal.
pub(crate) const fn signal(signal: ShelfSignal) -> (&'static str, &'static str) {
    match signal {
        ShelfSignal::Available => ("signal-available", "shelf-signal-available"),
        ShelfSignal::OnLoan => ("signal-on-loan", "shelf-signal-on-loan"),
        ShelfSignal::NotForLoan => ("signal-not-for-loan", "shelf-signal-not-for-loan"),
        ShelfSignal::OutOfCirculation => (
            "signal-out-of-circulation",
            "shelf-signal-out-of-circulation",
        ),
    }
}
