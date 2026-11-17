// SPDX-License-Identifier: GPL-3.0-or-later
//! Elzevir entry point: one binary, several roles (`serve`, `staff`, `import`, `export`,
//! and later `worker` and `panel`).
#![expect(
    clippy::print_stderr,
    reason = "the command line reports errors on the terminal"
)]

mod catalog_cli;
mod config;
mod health;
mod init;
mod koha_codes;
mod local_disk;
mod logging;
mod memory;
mod runtime_info;
mod server_config;
mod status;

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
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serves the OPAC and the staff interface.
    Serve(server_config::ServeArgs),
    /// Initializes a new institution and its first administrator.
    Init(init::InitArgs),
    /// Reports local server status without opening its database.
    Status(status::StatusArgs),
    /// Manages staff accounts.
    #[command(subcommand)]
    Staff(StaffAction),
    /// Imports a MARC21 file (ISO 2709) into the catalog.
    Import(catalog_cli::ImportArgs),
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
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Serve(args) => {
            args.resolve()
                .map_err(|error| error.to_string())
                .and_then(|settings| {
                    serve(
                        &settings.data_dir,
                        settings.listen,
                        settings.insecure_cookies,
                        settings.iri_base,
                    )
                })
        }
        Command::Init(args) => init::run(&args),
        Command::Status(args) => status::run(args),
        Command::Staff(StaffAction::Add {
            data_dir,
            login,
            admin,
            cataloguing,
        }) => add_staff(&data_dir, &login, admin, cataloguing),
        Command::Import(args) => catalog_cli::import(&args),
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
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("cannot create {}: {e}", data_dir.display()))?;
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
    let store = ElzevirStore::open(
        &data_dir.join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .map_err(|e| e.to_string())?;
    Ok(DataDir { store, _lock: lock })
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

fn serve(
    data_dir: &Path,
    listen: SocketAddr,
    insecure_cookies: bool,
    iri_base: Option<String>,
) -> Result<(), String> {
    let loaded = config::load(data_dir)?;
    if let Some((path, report)) = &loaded.table_report {
        println!("{}", config::summary(path, report));
    }
    let DataDir { store, _lock } = open_store(data_dir)?;
    let store = Arc::new(store);
    let _logs = logging::start(data_dir)?;
    let probe = Arc::new(health::Probe::new(Arc::clone(&store))?);
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
        let i18n = I18n::english().map_err(|e| e.to_string())?;
        let config = WebConfig {
            secure_cookies: !insecure_cookies,
            iri_base,
            author_table: loaded.author_table,
            time_zone: loaded.time_zone,
            ..WebConfig::default()
        };
        let opac = OpacPorts {
            catalog: Arc::clone(&store) as _,
            index,
        };
        let staff_ports = StaffPorts {
            listing: Arc::clone(&store) as _,
            records: Arc::clone(&store) as _,
            editor: Arc::clone(&store) as _,
            suggestions: Arc::clone(&store) as _,
            items: Arc::clone(&store) as _,
            statements: Arc::clone(&store) as _,
            history: Arc::clone(&store) as _,
            authorities: Arc::clone(&store) as _,
            work_lists: Arc::clone(&store) as _,
        };
        let app = elzevir_web::router(AppState::new(
            store,
            Arc::new(memory::AllocatorProbe),
            opac,
            staff_ports,
            i18n,
            config,
        ));
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let result = runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind(listen)
                .await
                .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
            let address = listener.local_addr().map_err(|error| error.to_string())?;
            let health_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|error| format!("cannot bind local health listener: {error}"))?;
            let health_address = health_listener
                .local_addr()
                .map_err(|error| error.to_string())?;
            let _metadata =
                runtime_info::RuntimeFile::publish(data_dir, probe.instance(), health_address)?;
            logging::emit(probe.instance(), logging::Event::ServerStarted);
            println!("elzevir: listening on http://{address}");
            println!("elzevir: health on http://{health_address}/health");
            serve_http(listener, app, health_listener, probe).await
        });
        drop(indexer);
        result
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

async fn serve_http(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    health_listener: tokio::net::TcpListener,
    probe: Arc<health::Probe>,
) -> Result<(), String> {
    let health_app = axum::Router::new()
        .route("/health", axum::routing::get(health::endpoint))
        .with_state(probe);
    let (stop, receiver) = tokio::sync::watch::channel(false);
    let catalog = axum::serve(listener, app)
        .with_graceful_shutdown(wait_shutdown(receiver.clone()))
        .into_future();
    let readiness = axum::serve(health_listener, health_app)
        .with_graceful_shutdown(wait_shutdown(receiver))
        .into_future();
    let servers = async {
        tokio::try_join!(catalog, readiness)
            .map(drop)
            .map_err(|error| error.to_string())
    };
    let signal = async {
        // Preserve serving when the platform cannot install the signal handler.
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    tokio::pin!(servers);
    tokio::select! {
        outcome = &mut servers => outcome,
        () = signal => {
            stop.send_replace(true);
            servers.await
        }
    }
}

async fn wait_shutdown(mut receiver: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

fn add_staff(data_dir: &Path, login: &str, admin: bool, cataloguing: bool) -> Result<(), String> {
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
    if admin {
        permissions.insert(Permission::Administration);
    }
    if cataloguing {
        permissions.insert(Permission::Cataloguing);
    }
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
