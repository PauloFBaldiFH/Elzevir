// SPDX-License-Identifier: GPL-3.0-or-later
//! SQLite storage: the single writer thread, the read pool, audit log and effect outbox.
//!
//! One [`Store`] serves one institution's database (docs/arquitetura/04-decisoes.md, §4).
//!
//! - **Writes** go through one dedicated thread that owns the only writable connection.
//!   Callers submit typed commands ([`CommandHandler::Command`]) to a bounded queue and get
//!   the outcome back on a reply channel (docs/arquitetura/05-definicoes-de-implementacao.md,
//!   §1). The writer groups queued commands into one transaction, gives each command its own
//!   savepoint, records it in `audit_log`, and replies only after the commit.
//! - **Reads** use a small pool of read-only connections; WAL lets them run while the writer
//!   commits.
//! - **Effects** that must happen after a commit (reindexing, notices) are written to
//!   `effect_outbox` inside the same transaction as the change that causes them.

mod authorities;
mod backup;
mod batch;
mod calendar;
mod catalog;
mod circulation_rules;
mod codes;
mod config;
mod digest;
mod editing;
mod editor;
mod effects;
mod elzevir;
mod error;
mod export;
mod history;
mod import;
mod items;
mod koha_types;
mod listing;
mod loan_desk;
mod loans;
mod matching;
mod native_items;
mod patron_desk;
mod patrons;
mod places;
mod preview;
mod readers;
mod registries;
mod renewals;
mod restrictions;
mod returns;
mod saved_lists;
mod schema;
mod statements;
mod suggestions;
mod worklist;
mod writer;

pub use backup::{
    SnapshotInfo, inspect_snapshot, inspect_snapshot_controlled, snapshot_offline,
    supported_schema_version,
};
pub use config::StoreConfig;
pub use elzevir::{ElzevirHandler, ElzevirStore};
pub use error::{CommandFailed, StoreError, SubmitError};
pub use import::PendingAdd;
pub use preview::{preview_import, preview_sheet, preview_table_source};
pub use readers::ReadPool;
pub use writer::{CommandHandler, Pending, WriteTx};

use std::path::Path;

/// An open institution database: the writer thread plus the read pool.
///
/// Dropping the store closes the queue, lets the writer finish what is already queued,
/// and waits for it to optimize and checkpoint the database.
pub struct Store<H: CommandHandler> {
    writer: writer::WriterHandle<H>,
    readers: ReadPool,
}

impl<H: CommandHandler> Store<H> {
    /// Drains queued commands, closes readers and checkpoints on the writer thread.
    ///
    /// # Errors
    /// Reports a writer panic, SQLite maintenance error or busy checkpoint.
    pub fn shutdown(self) -> Result<(), StoreError> {
        let Self {
            mut writer,
            readers,
        } = self;
        drop(readers);
        writer.shutdown()
    }

    /// Opens (creating if needed) the database at `path`, applies pending migrations and
    /// starts the writer thread with `handler`.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be opened, a migration fails, or the thread cannot start.
    pub fn open(path: &Path, config: &StoreConfig, handler: H) -> Result<Self, StoreError> {
        let connection = schema::open_writer(path)?;
        let writer = writer::WriterHandle::start(connection, config, handler)?;
        let readers = ReadPool::open(path, config.readers)?;
        Ok(Self { writer, readers })
    }

    /// Queues `command` for the writer.
    ///
    /// # Errors
    ///
    /// [`SubmitError::Busy`] when the queue is full (answer 503 with `Retry-After`), and
    /// [`SubmitError::Closed`] when the writer has stopped. Never blocks.
    pub fn submit(&self, command: H::Command) -> Result<Pending<H>, SubmitError> {
        self.writer.submit(None, command)
    }

    /// Queues `command` on behalf of a staff account, which the audit log records
    /// (docs/specs/catalogo/edicao.md, item 18).
    ///
    /// # Errors
    ///
    /// As [`Self::submit`].
    pub fn submit_as(&self, actor: i64, command: H::Command) -> Result<Pending<H>, SubmitError> {
        self.writer.submit(Some(actor), command)
    }

    /// Queues `command`, waiting while the queue is full. For batch work (an import) that
    /// runs on its own thread and should slow down to the writer's pace instead of failing.
    /// Must not be called from async code: it blocks the thread.
    ///
    /// # Errors
    ///
    /// [`SubmitError::Closed`] when the writer has stopped.
    pub fn submit_waiting(&self, command: H::Command) -> Result<Pending<H>, SubmitError> {
        self.writer.submit_waiting(None, command)
    }

    /// A digest of every table's content, for comparing two databases built from the same
    /// commands (docs/arquitetura/03-estrategia-reconstrucao.md, §1.2 item 8).
    ///
    /// # Errors
    ///
    /// Fails if a read fails.
    pub fn digest(&self) -> Result<String, StoreError> {
        self.readers.read(digest::database)
    }

    /// The read-only connection pool.
    #[must_use]
    pub fn readers(&self) -> &ReadPool {
        &self.readers
    }
}
