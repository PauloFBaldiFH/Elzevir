// SPDX-License-Identifier: GPL-3.0-or-later
//! Elzevir entry point: one binary, several roles (`serve`, `staff`, `import`, `export`,
//! and later `worker` and `panel`).
#![expect(
    clippy::print_stderr,
    reason = "the command line reports errors on the terminal"
)]

mod access_config;
mod addresses;
mod admin;
mod backup_archive;
mod backup_cli;
mod broker;
mod broker_cli;
mod broker_protocol;
mod cancellation;
mod catalog_cli;
mod cloud;
mod cloud_cli;
mod cloud_plans;
mod cloudflare;
mod config;
mod diagnostic_logs;
mod diagnostics;
mod file_owner;
mod first_use;
mod health;
mod host_jobs;
mod http_client;
mod import_batch_cli;
mod import_batches;
mod import_service;
mod init;
mod installation;
mod koha_codes;
mod local_artifacts;
mod local_disk;
mod logging;
mod managed_restore;
mod managed_tool_update;
mod managed_tools;
mod memory;
mod operations;
mod operations_cli;
mod operations_config;
mod panel_cli;
mod personal_update;
mod private_temp;
mod process_runner;
mod release;
mod restore_journal;
mod runtime_info;
mod server_config;
mod service;
mod shutdown;
mod status;
mod support_archive;
mod tools_cli;
mod tunnel_cli;
mod tunnel_observe;
mod update;
mod update_journal;
mod update_recovery;

use std::future::IntoFuture;
use std::io::BufRead;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use elzevir_app::commands::{StaffCommand, WriteCommand, WriteOutput};
use elzevir_app::parallelism::Parallelism;
use elzevir_app::search::{IndexOptions, index_pending};
use elzevir_app::staff::{Permission, hash_password};
use elzevir_search::TantivyIndex;
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};
use elzevir_web::{AppState, OpacPorts, StaffPorts, WebConfig};
use elzevir_web_templates::I18n;

/// Elzevir library system.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Keep update ownership with the installer or package manager.
    #[arg(long, global = true)]
    no_self_update: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(hide = true)]
    HostJob(host_jobs::Args),
    /// Install or inspect optional operational tools.
    Tools(tools_cli::Args),
    /// Manage a free address through an explicitly compatible broker.
    Address(broker_cli::Args),
    /// Configure cloud connections through the institution worker.
    Cloud(cloud_cli::Args),
    /// Configure and observe the managed public tunnel.
    Tunnel(tunnel_cli::Args),
    /// Install a verified release with backup and automatic rollback.
    Update(update::Args),
    /// Recover an update interrupted between backup, executable swap and restart.
    RecoverUpdate(update_recovery::Args),
    /// Serves the OPAC and the staff interface.
    Serve(server_config::ServeArgs),
    /// Runs the local web server and the terminal panel together.
    Start(server_config::ServeArgs),
    /// Initializes a new institution and its first administrator.
    Init(init::InitArgs),
    /// Reports local server status without opening its database.
    Status(status::StatusArgs),
    /// Native administrative terminal panel.
    Panel(panel_cli::PanelArgs),
    /// Safe diagnostics and optional redacted support archive.
    Diagnose(diagnostics::DiagnoseArgs),
    /// Configured and observed local addresses.
    Addresses(addresses::AddressArgs),
    /// Create, verify, export and restore encrypted snapshots.
    Backup(backup_cli::BackupArgs),
    /// Observe jobs and configure operational schedules.
    Operations(operations_cli::Args),
    /// Manage the operating-system service.
    Service(service::ServiceArgs),
    /// Manages staff accounts.
    #[command(subcommand)]
    Staff(StaffAction),
    /// Imports a MARC21 file (ISO 2709) into the catalog, or stages, edits, previews
    /// and confirms a private import batch of any supported format.
    Import(import_batch_cli::ImportCommand),
    /// Writes the versioned BIBFRAME spreadsheet header.
    ImportModel {
        /// Use tab-separated values.
        #[arg(long)]
        tsv: bool,
    },
    /// Exports the catalog as MARC21 (ISO 2709, UTF-8).
    Export(catalog_cli::ExportArgs),
    /// Names branches, item types, locations and collections as a Koha names them.
    KohaCodes(koha_codes::KohaCodesArgs),
    /// Sets how this library's Koha item types translate into BIBFRAME on import.
    KohaTypes(catalog_cli::KohaTypesArgs),
}

#[derive(Subcommand)]
enum StaffAction {
    /// Creates a staff account. The password is read from `ELZEVIR_STAFF_PASSWORD` or, if
    /// unset, from the first line of standard input.
    Add {
        /// Institution data directory.
        #[arg(long)]
        data_dir: PathBuf,
        /// Login name.
        #[arg(long)]
        login: String,
        /// Also grant administration.
        #[arg(long)]
        admin: bool,
        /// Also grant cataloguing: adding and editing records and Items.
        #[arg(long)]
        cataloguing: bool,
        /// Also grant circulation: opening patron records and lending.
        #[arg(long)]
        circulation: bool,
        /// Also grant patron management: enrolling and editing patrons.
        #[arg(long)]
        patrons: bool,
    },
}

fn main() -> ExitCode {
    let Cli {
        command,
        no_self_update,
    } = Cli::parse();
    let result = match command {
        Command::HostJob(args) => host_jobs::run(&args),
        Command::Tools(args) => tools_cli::run(args),
        Command::Address(args) => broker_cli::run(args),
        Command::Cloud(args) => cloud_cli::run(args),
        Command::Tunnel(args) => tunnel_cli::run(args),
        Command::Update(args) => update::run(args, no_self_update),
        Command::RecoverUpdate(args) => update_recovery::run(&args),
        Command::Serve(args) => first_use::resolve(args).and_then(|(settings, _)| serve(&settings)),
        Command::Start(args) => first_use::start(args, no_self_update),
        Command::Init(args) => init::run(&args),
        Command::Status(args) => status::run(args),
        Command::Panel(args) => panel_cli::run(args, no_self_update),
        Command::Diagnose(args) => diagnostics::run(args),
        Command::Addresses(args) => addresses::run(args),
        Command::Service(args) => service::run(args),
        Command::Backup(args) => backup_cli::run(args),
        Command::Operations(args) => operations_cli::run(args),
        Command::Staff(StaffAction::Add {
            data_dir,
            login,
            admin,
            cataloguing,
            circulation,
            patrons,
        }) => add_staff(
            &data_dir,
            &login,
            [
                (admin, Permission::Administration),
                (cataloguing, Permission::Cataloguing),
                (circulation, Permission::Circulation),
                (patrons, Permission::Patrons),
            ],
        ),
        Command::Import(args) => import_batch_cli::run(args),
        Command::ImportModel { tsv } => import_batch_cli::model(tsv),
        Command::Export(args) => catalog_cli::export(&args),
        Command::KohaCodes(args) => koha_codes::koha_codes(&args),
        Command::KohaTypes(args) => catalog_cli::koha_types(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("elzevir: {message}");
            ExitCode::FAILURE
        }
    }
}

/// An open data directory: the store, and the lock that keeps other Elzevir processes out
/// while it is open, so that there is only ever one writer (CLAUDE.md, single writer).
/// Fields drop in order, so the store closes before the lock is released.
pub(crate) struct DataDir {
    pub(crate) store: ElzevirStore,
    _lock: std::fs::File,
}

pub(crate) fn open_store(data_dir: &Path) -> Result<DataDir, String> {
    if restore_journal::pending(data_dir) {
        return Err(
            "interrupted restore requires backup recover-restore before startup".to_owned(),
        );
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("cannot create {}: {e}", data_dir.display()))?;
    let lock = lock_data(data_dir)?;
    let store = ElzevirStore::open(
        &data_dir.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .map_err(|e| e.to_string())?;
    Ok(DataDir { store, _lock: lock })
}

fn lock_data(data_dir: &Path) -> Result<std::fs::File, String> {
    let lock_path = data_dir.join("elzevir.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("cannot open {}: {e}", lock_path.display()))?;
    lock.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => format!(
            "another Elzevir process is using {}; stop it first",
            data_dir.display()
        ),
        std::fs::TryLockError::Error(e) => format!("cannot lock {}: {e}", lock_path.display()),
    })?;
    Ok(lock)
}

/// How often the indexer looks for new effects in the outbox.
const INDEX_INTERVAL: Duration = Duration::from_secs(1);

/// Effects read per indexing batch.
const INDEX_BATCH: NonZeroUsize = NonZeroUsize::new(1024).unwrap();

/// Opens the search index in `data_dir/index`.
fn open_index(data_dir: &Path) -> Result<(TantivyIndex, Parallelism), String> {
    let directory = data_dir.join("index");
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
    let cores = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let switch = std::env::var("ELZEVIR_SERIAL").ok();
    let parallelism = Parallelism::from_serial_switch(switch.as_deref(), "index", cores);
    let index = TantivyIndex::open(&directory, parallelism).map_err(|e| e.to_string())?;
    Ok((index, parallelism))
}

/// Keeps the index up to date with the outbox, in its own thread, for as long as the
/// process runs. A failed run is reported and tried again on the next tick: the cursor
/// only moves with a successful commit, so nothing is lost.
fn spawn_indexer(
    store: Arc<ElzevirStore>,
    index: Arc<TantivyIndex>,
    parallelism: Parallelism,
    probe: Arc<health::Probe>,
) -> Result<Indexer, String> {
    let options = IndexOptions {
        parallelism,
        batch: INDEX_BATCH,
    };
    let (stop, receiver) = std::sync::mpsc::channel();
    let instance = probe.instance().to_owned();
    let task = std::thread::Builder::new()
        .name("elzevir-indexer".to_owned())
        .spawn(move || {
            loop {
                if let Ok(report) = index_pending(&*store, &*store, &*index, &options) {
                    probe.indexed(true);
                    if report.effects > 0 {
                        logging::emit(
                            probe.instance(),
                            logging::Event::IndexerCommitted {
                                effects: report.effects,
                            },
                        );
                    }
                } else {
                    probe.indexed(false);
                    logging::emit(probe.instance(), logging::Event::IndexerFailed);
                }
                match receiver.recv_timeout(INDEX_INTERVAL) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    _ => break,
                }
            }
        })
        .map_err(|error| format!("cannot start the indexer: {error}"))?;
    Ok(Indexer {
        instance,
        stop: Some(stop),
        task: Some(task),
    })
}

struct Indexer {
    instance: String,
    stop: Option<std::sync::mpsc::Sender<()>>,
    task: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Indexer {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(task) = self.task.take()
            && task.join().is_err()
        {
            logging::emit(&self.instance, logging::Event::IndexerPanicked);
        }
    }
}

struct WebRuntime {
    router: axum::Router,
    state: AppState,
}

fn web_app(
    store: &Arc<ElzevirStore>,
    index: Arc<TantivyIndex>,
    data_dir: &Path,
    config: WebConfig,
    language: &str,
) -> Result<WebRuntime, String> {
    let i18n = if language == "pt-BR" {
        I18n::portuguese()
    } else {
        I18n::english()
    }
    .map_err(|error| error.to_string())?;
    let opac = OpacPorts {
        catalog: Arc::clone(store) as _,
        index,
    };
    let staff_ports = StaffPorts {
        listing: Arc::clone(store) as _,
        records: Arc::clone(store) as _,
        editor: Arc::clone(store) as _,
        suggestions: Arc::clone(store) as _,
        items: Arc::clone(store) as _,
        statements: Arc::clone(store) as _,
        history: Arc::clone(store) as _,
        authorities: Arc::clone(store) as _,
        work_lists: Arc::clone(store) as _,
    };
    let state = AppState::new(
        Arc::clone(store) as _,
        Arc::new(memory::AllocatorProbe),
        opac,
        staff_ports,
        i18n,
        config,
    )
    .with_imports(Arc::new(import_service::WebImports {
        data_dir: data_dir.to_owned(),
        store: Arc::clone(store),
    }))
    .map_err(|error| error.to_string())?
    .with_patrons(Arc::clone(store) as _)
    .with_rules(Arc::clone(store) as _);
    Ok(WebRuntime {
        router: elzevir_web::router(state.clone()),
        state,
    })
}

fn serve(settings: &server_config::Settings) -> Result<(), String> {
    serve_controlled(settings, &shutdown::Control::default(), || Ok(()))
}

fn serve_controlled(
    settings: &server_config::Settings,
    control: &shutdown::Control,
    ready: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if control.requested() {
        return Ok(());
    }
    cloudflare::require_host_policy(settings)?;
    broker::require_host_policy(settings)?;
    let data_dir = &settings.data_dir;
    let loaded = config::load(data_dir)?;
    if let Some((path, report)) = &loaded.table_report {
        println!("{}", config::summary(path, report));
    }
    let DataDir { store, _lock } = open_store(data_dir)?;
    let store = Arc::new(store);
    let _logs = logging::start(data_dir)?;
    let probe = Arc::new(health::Probe::new(Arc::clone(&store))?);
    let operations = operations::Worker::start(
        data_dir.to_owned(),
        server_config::snapshot(settings).map_err(|_| "cannot snapshot operational settings")?,
        Arc::clone(&store),
    )?;
    let instance = probe.instance().to_owned();
    let result = (|| {
        let (index, parallelism) = open_index(data_dir)?;
        let index = Arc::new(index);
        let indexer = spawn_indexer(
            Arc::clone(&store),
            Arc::clone(&index),
            parallelism,
            Arc::clone(&probe),
        )?;
        let app = web_app(
            &store,
            index,
            data_dir,
            WebConfig {
                secure_cookies: !settings.insecure_cookies,
                iri_base: settings.iri_base.clone(),
                author_table: loaded.author_table,
                time_zone: loaded.time_zone,
                ..WebConfig::default()
            },
            &settings.lang,
        )?;
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let result = runtime.block_on(run_http(
            settings,
            control,
            ready,
            app,
            Arc::clone(&probe),
            operations.client(),
        ));
        drop(indexer);
        // Runtime drop waits for blocking reads; keep the institution lock until all
        // readers are gone and the existing writer has reported its final checkpoint.
        drop(runtime);
        operations.finish()?;
        drop(probe);
        let closed = Arc::try_unwrap(store)
            .map_err(|_| "database still in use during shutdown".to_owned())
            .and_then(|store| {
                store
                    .shutdown()
                    .map_err(|_| "database shutdown/checkpoint failed".to_owned())
            });
        result.and(closed)
    })();
    logging::emit(
        &instance,
        if result.is_ok() {
            logging::Event::ServerStopped
        } else {
            logging::Event::ServerFailed
        },
    );
    result
}

async fn run_http(
    settings: &server_config::Settings,
    control: &shutdown::Control,
    ready: impl FnOnce() -> Result<(), String>,
    web: WebRuntime,
    probe: Arc<health::Probe>,
    operations: operations::Client,
) -> Result<(), String> {
    let app = web.router;
    let data_dir = &settings.data_dir;
    let listen = settings.listen;
    let catalog_policy = settings
        .staff
        .catalog_policy()
        .map_err(|error| error.to_string())?;

    let mut signals =
        shutdown::Signals::register(control).map_err(|_| "cannot register shutdown signals")?;
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    let address = listener
        .local_addr()
        .map_err(|e| format!("cannot read listener address: {e}"))?;
    let staff_binding = bind_staff(&settings.staff).await?;
    let staff_address = staff_binding
        .as_ref()
        .map_or(address, |(_, address, _)| *address);
    let catalog_policy = catalog_policy.with_listener_addresses(address, staff_address);
    let health_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "cannot bind local health listener")?;
    let health_address = health_listener
        .local_addr()
        .map_err(|_| "cannot read health listener address")?;
    let admin_listener = admin::transport::Bound::bind(probe.instance())
        .map_err(|_| "cannot bind private administrative endpoint")?;
    let staff_mode = settings
        .staff
        .observed(staff_binding.as_ref().map(|(_, address, _)| *address));
    let _metadata = runtime_info::RuntimeFile::publish(
        data_dir,
        probe.instance(),
        health_address,
        Some(admin_listener.endpoint.clone()),
        Some(address),
        Some(staff_mode),
    )?;
    let health_app = axum::Router::new()
        .route("/health", axum::routing::get(health::endpoint))
        .with_state(Arc::clone(&probe));
    let receiver = control.receiver();
    let admin_server = admin::serve(
        admin_listener,
        Arc::clone(&probe),
        operations,
        web.state,
        receiver.clone(),
    );
    let health_server = axum::serve(health_listener, health_app)
        .with_graceful_shutdown(wait_shutdown(receiver.clone()))
        .into_future();
    let staff_app = app.clone();
    let staff_server = async move {
        if let Some((listener, _, policy)) = staff_binding {
            axum::serve(
                listener,
                elzevir_web::restrict_access(
                    staff_app,
                    policy.with_listener_addresses(address, staff_address),
                )
                .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(wait_shutdown(receiver))
            .await
        } else {
            Ok(())
        }
    };
    logging::emit(probe.instance(), logging::Event::ServerStarted);
    println!("elzevir: listening on http://{address}");
    println!("elzevir: health on http://{health_address}/health");
    let server = axum::serve(
        listener,
        elzevir_web::restrict_access(app, catalog_policy)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(wait_shutdown(control.receiver()))
    .into_future();
    if !control.requested() {
        ready()?;
    }
    let servers = async {
        tokio::try_join!(server, staff_server, health_server, admin_server)
            .map(drop)
            .map_err(|_| "server listener failed".to_owned())
    };
    tokio::pin!(servers);
    tokio::select! {
        result = &mut servers => result,
        () = signals.wait(control) => {
            tokio::time::timeout(Duration::from_secs(30), &mut servers).await.map_err(|_| "HTTP shutdown deadline exceeded".to_owned())?
        }
    }
}

async fn bind_staff(
    mode: &access_config::StaffMode,
) -> Result<
    Option<(
        tokio::net::TcpListener,
        SocketAddr,
        elzevir_web::AccessPolicy,
    )>,
    String,
> {
    let Some((bind, policy)) = mode
        .staff_policy()
        .map_err(|_| "invalid staff listener policy")?
    else {
        return Ok(None);
    };
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| "cannot bind staff listener")?;
    let address = listener
        .local_addr()
        .map_err(|_| "cannot read staff listener address")?;
    Ok(Some((listener, address, policy)))
}

async fn wait_shutdown(receiver: tokio::sync::watch::Receiver<bool>) {
    shutdown::wait(receiver).await;
}

fn add_staff(data_dir: &Path, login: &str, granted: [(bool, Permission); 4]) -> Result<(), String> {
    let password = if let Ok(password) = std::env::var("ELZEVIR_STAFF_PASSWORD") {
        password
    } else {
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| format!("cannot read the password: {e}"))?;
        line.trim_end_matches(['\r', '\n']).to_owned()
    };
    if password.chars().count() < 8 {
        return Err("the password must have at least 8 characters".to_owned());
    }
    let mut permissions = [Permission::StaffInterface]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    permissions.extend(
        granted
            .into_iter()
            .filter_map(|(asked, permission)| asked.then_some(permission)),
    );
    let password_hash = hash_password(&password).map_err(|e| e.to_string())?;
    let data = open_store(data_dir)?;
    let outcome = data
        .store
        .submit(WriteCommand::Staff(StaffCommand::Create {
            login: login.to_owned(),
            password_hash,
            permissions,
        }))
        .map_err(|e| e.to_string())?
        .wait()
        .map_err(|e| e.to_string())?;
    let WriteOutput::StaffCreated(id) = outcome else {
        return Err(format!("unexpected answer from the writer: {outcome:?}"));
    };
    println!("elzevir: staff account `{login}` created (id {})", id.0);
    Ok(())
}

fn panel_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(character,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod indexer_tests {
    use super::*;

    #[test]
    fn stopping_the_indexer_releases_its_store_references() {
        let root = tempfile::tempdir().unwrap();
        let data = open_store(root.path()).unwrap();
        let store = Arc::new(data.store);
        let weak = Arc::downgrade(&store);
        let (index, parallelism) = open_index(root.path()).unwrap();
        let probe = Arc::new(health::Probe::new(Arc::clone(&store)).unwrap());
        {
            let _worker = spawn_indexer(store, Arc::new(index), parallelism, probe).unwrap();
        }
        assert!(weak.upgrade().is_none());
    }
}
