// SPDX-License-Identifier: GPL-3.0-or-later
//! HTTP layer: axum routes for the OPAC and the staff interface, HTMX fragments.
//!
//! OPAC at `/`, staff interface at `/equipe` on the same listener by default
//! (docs/arquitetura/04-decisoes.md, §5). The web layer reaches data only through
//! `elzevir-app` ports; it never opens the database.

mod access;
mod assets;
mod authorities;
mod batch;
mod board;
mod calendar;
mod catalog;
mod checkout_slip;
mod circulation;
mod circulation_rules;
mod creating;
mod described;
mod editing;
mod facets;
mod form_sections;
mod history;
mod icons;
mod import_cells;
mod import_rules;
mod import_sheets;
mod importing;
mod items;
mod new_record;
mod notice_rules;
mod opac;
mod opac_account;
mod opac_holds;
mod patron_account;
mod patron_card;
mod patron_form;
mod patron_holds;
mod patron_login;
mod patron_password;
mod patrons;
mod pill_loans;
mod print_notices;
mod record_fields;
mod record_form;
mod registries;
mod security;
mod session;
mod staff;
mod staff_bar;

use elzevir_domain::call_number::AuthorTable;
use std::sync::Arc;
use std::time::Duration;

use askama::Template;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use elzevir_app::memory::MemoryProbe;
use elzevir_app::staff::StaffDirectory;
use elzevir_web_templates::I18n;

pub use access::{AccessError, AccessPolicy, normalize_hostname, restrict_access};
pub use catalog::StaffPorts;
pub use opac::OpacPorts;
pub use print_notices::notice_text;
pub use session::Sessions;

/// Web settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebConfig {
    /// Mark cookies `Secure`. Off only for plain-HTTP development and tests.
    pub secure_cookies: bool,
    /// How long a staff session lasts without use.
    pub session_idle: Duration,
    /// Where the IRIs of the BIBFRAME export start; the requested host's `/bibframe` when
    /// `None` (bibframe-completo.md, item 28b).
    pub iri_base: Option<String>,
    /// The library's author table, which the item grid's suggested call number reads
    /// (docs/specs/catalogo/numero-de-chamada.md); no suggestion without one.
    pub author_table: Option<Arc<AuthorTable>>,
    /// The library's time zone, where "today" of a work list begins
    /// (docs/specs/catalogo/facetas.md, item 16).
    pub time_zone: jiff::tz::TimeZone,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            secure_cookies: true,
            session_idle: Duration::from_hours(8),
            iri_base: None,
            author_table: None,
            time_zone: jiff::tz::TimeZone::UTC,
        }
    }
}

/// Shared state of the web layer.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
    imports: Option<Arc<importing::Imports>>,
    patrons: Option<Arc<dyn elzevir_app::patrons::PatronDesk>>,
    loans: Option<Arc<dyn elzevir_app::loans::LoanDesk>>,
    rules: Option<Arc<dyn elzevir_app::circulation_rules::RuleBook>>,
    calendar: Option<Arc<dyn elzevir_app::calendar::CalendarBook>>,
    notices: Option<Arc<dyn elzevir_app::notices::NoticeQueue>>,
    board: Option<Arc<dyn elzevir_app::board::NoticeBoard>>,
    pending: Option<Arc<dyn elzevir_app::pending::PendingReader>>,
    accounts: Option<Arc<dyn elzevir_app::patron_account::PatronAccounts>>,
}

struct Inner {
    staff: Arc<dyn StaffDirectory>,
    memory: Arc<dyn MemoryProbe>,
    opac: OpacPorts,
    staff_ports: StaffPorts,
    records: opac::RecordCache,
    sessions: Sessions,
    patron_sessions: session::SessionStore<elzevir_app::patron_account::PatronIdentity>,
    attempts: patron_login::Attempts,
    i18n: I18n,
    config: WebConfig,
}

impl AppState {
    /// Number of unexpired staff sessions, for local operational confirmation only.
    #[must_use]
    pub fn staff_session_count(&self) -> usize {
        self.inner.sessions.count()
    }

    /// Builds the state from its ports.
    #[must_use]
    pub fn new(
        staff: Arc<dyn StaffDirectory>,
        memory: Arc<dyn MemoryProbe>,
        opac: OpacPorts,
        staff_ports: StaffPorts,
        i18n: I18n,
        config: WebConfig,
    ) -> Self {
        Self {
            imports: None,
            patrons: None,
            loans: None,
            rules: None,
            calendar: None,
            notices: None,
            board: None,
            pending: None,
            accounts: None,
            inner: Arc::new(Inner {
                staff,
                memory,
                opac,
                staff_ports,
                records: opac::RecordCache::default(),
                sessions: Sessions::new(config.session_idle),
                patron_sessions: session::SessionStore::new(patron_login::PATRON_IDLE),
                attempts: patron_login::Attempts::default(),
                i18n,
                config,
            }),
        }
    }

    /// Connects the prepared imports for this institution.
    ///
    /// # Errors
    /// Returns an error if the embedded import translations cannot load.
    pub fn with_imports(
        mut self,
        port: Arc<dyn elzevir_app::import::batches::BatchPort>,
    ) -> Result<Self, elzevir_web_templates::I18nError> {
        self.imports = Some(Arc::new(importing::Imports::new(port)?));
        Ok(self)
    }

    /// Connects the patrons behind the wall (docs/specs/circulacao/leitores.md). Without
    /// them the patron pages answer 404.
    #[must_use]
    pub fn with_patrons(mut self, desk: Arc<dyn elzevir_app::patrons::PatronDesk>) -> Self {
        self.patrons = Some(desk);
        self
    }

    /// Connects the circulation desk (interface §18.3). Without it the patron's record and
    /// the checkin desk answer 404.
    #[must_use]
    pub fn with_loans(mut self, desk: Arc<dyn elzevir_app::loans::LoanDesk>) -> Self {
        self.loans = Some(desk);
        self
    }

    /// Connects the circulation rules (docs/specs/circulacao/regras.md). Without them the
    /// rules screen answers 404.
    #[must_use]
    pub fn with_rules(mut self, book: Arc<dyn elzevir_app::circulation_rules::RuleBook>) -> Self {
        self.rules = Some(book);
        self
    }

    /// Connects the branch calendars (docs/specs/circulacao/calendario.md). Without them
    /// the calendar screen answers 404.
    #[must_use]
    pub fn with_calendar(mut self, book: Arc<dyn elzevir_app::calendar::CalendarBook>) -> Self {
        self.calendar = Some(book);
        self
    }

    /// Connects the queue of notices (docs/specs/circulacao/avisos.md). Without it the
    /// printed notices screen answers 404.
    #[must_use]
    pub fn with_notices(mut self, queue: Arc<dyn elzevir_app::notices::NoticeQueue>) -> Self {
        self.notices = Some(queue);
        self
    }

    /// Connects the notice board (docs/specs/interface/mural.md). Without it both homes
    /// show their welcome note and the note pages answer 404.
    #[must_use]
    pub fn with_board(mut self, board: Arc<dyn elzevir_app::board::NoticeBoard>) -> Self {
        self.board = Some(board);
        self
    }

    /// Connects the counts of the desk's pending work (interface spec, §22 item 14).
    /// Without them the module cards show no counts.
    #[must_use]
    pub fn with_pending(mut self, reader: Arc<dyn elzevir_app::pending::PendingReader>) -> Self {
        self.pending = Some(reader);
        self
    }

    /// Connects the patron accounts of the OPAC (docs/specs/opac/conta-do-leitor.md).
    /// Without them the sign-in panel stays a placeholder and `/conta` answers 404.
    #[must_use]
    pub fn with_patron_accounts(
        mut self,
        accounts: Arc<dyn elzevir_app::patron_account::PatronAccounts>,
    ) -> Self {
        self.accounts = Some(accounts);
        self
    }
}

/// All routes, with security headers on every response.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(opac::home))
        .route("/search", get(opac::search))
        .route("/record/{id}", get(opac::record))
        .route("/equipe/barra", get(staff_bar::bar))
        .route("/equipe/busca", get(staff_bar::search))
        .route("/equipe", get(staff::home))
        .route("/equipe/acervo", get(catalog::catalog))
        .route("/equipe/leitores", get(patrons::search))
        .route(
            "/equipe/leitores/categorias",
            get(patrons::categories).post(patrons::change_category),
        )
        .route(
            "/equipe/leitores/novo",
            get(patron_form::new_patron).post(patron_form::add_patron),
        )
        .route(
            "/equipe/leitores/{id}/editar",
            get(patron_form::edit_patron).post(patron_form::update_patron),
        )
        .route("/equipe/acervo/lote/marc", get(batch::export_marc))
        .route("/equipe/acervo/lote/exemplares", post(batch::change_items))
        .route("/equipe/acervo/listas", post(batch::change_list))
        .route(
            "/equipe/acervo/instancia/{id}/marc",
            get(catalog::export_marc),
        )
        .route(
            "/equipe/acervo/instancia/{id}/bibframe",
            get(catalog::export_bibframe),
        )
        .route(
            "/equipe/acervo/sugestoes/agentes",
            get(editing::suggest_agents),
        )
        .route(
            "/equipe/acervo/sugestoes/assuntos",
            get(editing::suggest_subjects),
        )
        .route("/equipe/acervo/isbn", get(editing::check_isbn_field))
        .route(
            "/equipe/acervo/nova",
            get(new_record::new_family).post(creating::create_family),
        )
        .route(
            "/equipe/acervo/instancia/{id}/exemplares",
            get(items::item_grid).post(items::save_items),
        )
        .route(
            "/equipe/acervo/instancia/{id}/nova",
            get(new_record::new_instance),
        )
        .route("/equipe/acervo/obra/{id}", post(editing::edit_work))
        .route(
            "/equipe/acervo/obra/{id}/historico",
            get(history::work_history),
        )
        .route(
            "/equipe/acervo/instancia/{id}/historico",
            get(history::instance_history),
        )
        .route(
            "/equipe/acervo/obra/{id}/instancias",
            post(creating::create_instance),
        )
        .route(
            "/equipe/acervo/instancia/{id}",
            post(editing::edit_instance),
        )
        .route(
            "/equipe/acervo/instancia/{id}/campos",
            get(record_fields::record_fields),
        )
        .route("/equipe/acervo/matriz/{id}", post(editing::edit_hub))
        .route(
            "/equipe/acervo/matriz/{id}/campos",
            get(described::hub_form),
        )
        .route("/equipe/acervo/exemplar/{id}", post(editing::edit_item))
        .route(
            "/equipe/acervo/exemplar/{id}/campos",
            get(described::item_form),
        )
        .route("/equipe/login", get(staff::login_form).post(staff::log_in))
        .route("/equipe/logout", post(staff::log_out))
        .merge(static_routes())
        .merge(board_routes())
        .merge(patron_routes())
        .merge(admin_routes())
        .merge(authority_routes())
        .merge(importing::routes())
        .merge(circulation::routes())
        .layer(axum::middleware::map_response(security::headers))
        .with_state(state)
}

/// The patron's side of the OPAC and the staff setting their password
/// (docs/specs/opac/conta-do-leitor.md).
fn patron_routes() -> Router<AppState> {
    Router::new()
        .route("/conta", get(opac_account::account))
        .route("/conta/renovar", post(opac_account::renew))
        .route(
            "/conta/reservar/{id}",
            get(opac_holds::hold_form).post(opac_holds::place_hold),
        )
        .route(
            "/conta/reservas/{hold}/cancelar",
            post(opac_holds::cancel_hold),
        )
        .route("/conta/historico", post(opac_holds::set_history))
        .route(
            "/conta/entrar",
            get(patron_login::login_form).post(patron_login::log_in_patron),
        )
        .route("/conta/sair", post(patron_login::log_out_patron))
        .route(
            "/equipe/leitores/{id}/senha",
            post(patron_password::set_password),
        )
}

/// The notice board's form (docs/specs/interface/mural.md, item 7).
fn board_routes() -> Router<AppState> {
    Router::new()
        .route("/equipe/mural", post(board::add_note))
        .route("/equipe/mural/nova", get(board::new_note))
        .route(
            "/equipe/mural/{id}",
            get(board::edit_note).post(board::update_note),
        )
        .route("/equipe/mural/{id}/remover", post(board::remove_note))
}

/// The stylesheet, scripts and images, all served from the binary.
fn static_routes() -> Router<AppState> {
    Router::new()
        .route("/static/elzevir.css", get(assets::stylesheet))
        .route("/static/htmx.min.js", get(assets::htmx))
        .route("/static/elzevir-catalog.js", get(assets::catalog_script))
        .route("/static/elzevir-import.js", get(assets::import_script))
        .route("/static/elzevir-icons.svg", get(assets::icons))
        .route("/static/elzevir-mark.svg", get(assets::mark))
        .route("/static/elzevir-theme.js", get(assets::theme_script))
        .route("/static/elzevir-staff.js", get(assets::staff_script))
        .route("/static/elzevir-opac.js", get(assets::opac_script))
}

/// Administration: memory report, registries, circulation rules and calendars.
fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/equipe/admin/memory", get(staff::memory))
        .route(
            "/equipe/admin/calendario",
            get(calendar::calendar).post(calendar::change_calendar),
        )
        .route(
            "/equipe/admin/regras",
            get(circulation_rules::rules).post(circulation_rules::set_rules),
        )
        .route(
            "/equipe/admin/avisos",
            get(notice_rules::notices).post(notice_rules::set_overdue_rule),
        )
        .route(
            "/equipe/admin/avisos/reservas",
            post(notice_rules::set_hold_notices),
        )
        .route("/equipe/admin/avisos/textos", post(notice_rules::set_text))
        .route(
            "/equipe/admin/cadastros",
            get(registries::registries).post(registries::change_registry),
        )
}

/// The authorities column: agents and subjects edited once for every record, and linked
/// to Works by role (docs/specs/catalogo/autoridades.md).
fn authority_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/equipe/acervo/assunto/{id}/campos",
            get(authorities::subject_form),
        )
        .route(
            "/equipe/acervo/assunto/{id}",
            post(authorities::edit_subject),
        )
        .route(
            "/equipe/acervo/obra/{id}/autoridade/nova",
            get(authorities::link_form),
        )
        .route(
            "/equipe/acervo/obra/{id}/autoridades",
            post(authorities::link_authority),
        )
        .route(
            "/equipe/acervo/autoridade/{id}",
            post(authorities::edit_agent),
        )
        .route(
            "/equipe/acervo/autoridade/{id}/campos",
            get(authorities::agent_form),
        )
}

/// Renders a template, or answers 500 without leaking the error to the visitor.
fn render(page: &impl Template) -> Response {
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}
