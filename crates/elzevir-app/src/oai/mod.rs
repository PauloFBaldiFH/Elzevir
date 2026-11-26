// SPDX-License-Identifier: GPL-3.0-or-later
//! The OAI-PMH repository of the public catalog (docs/specs/opac/oai-pmh.md): the request
//! rules, the resumption token, Dublin Core and the response, over a read port.
//!
//! Koha: `Koha::OAI::Server::Repository` and the verbs under `Koha::OAI::Server`.

mod dublin_core;
mod request;
mod response;

use elzevir_domain::catalog::InstanceId;
use elzevir_marc::{Limits, Record, Tag};

use crate::export::{RecordExportError, RecordSource, marc_record};

pub use dublin_core::{DcElement, dublin_core};
pub use request::{Datestamp, ListState, MetadataFormat, OaiErrorCode, Request, Verb, parse};
pub use response::{Answer, respond};

/// `[oai]` of `elzevir.toml`, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OaiSettings {
    /// The prefix of the identifiers (`OAI-PMH:archiveID`).
    pub repository_identifier: String,
    /// The public address of `/oai`.
    pub base_url: String,
    /// The repository's name.
    pub repository_name: String,
    /// At least one address.
    pub admin_emails: Vec<String>,
    /// Records per response (`OAI-PMH:MaxCount`).
    pub max_count: usize,
}

/// The header of one record: its Instance, when it last changed, and whether the OPAC
/// hides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OaiHeader {
    /// The Instance.
    pub instance: InstanceId,
    /// When its version last moved, in UTC.
    pub datestamp: Datestamp,
    /// Hidden from the OPAC: the record goes out as deleted.
    pub hidden: bool,
}

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("catalog unavailable: {0}")]
pub struct OaiUnavailable(pub String);

/// Port: the records of the repository, with the stored records the export reads. The
/// store implements it.
pub trait OaiCatalog: RecordSource + Send + Sync {
    /// The earliest datestamp; `None` with no records.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn earliest(&self) -> Result<Option<Datestamp>, OaiUnavailable>;

    /// The header of `instance`; `None` when it does not exist.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn header(&self, instance: InstanceId) -> Result<Option<OaiHeader>, OaiUnavailable>;

    /// Up to `limit` headers from the Instance `from_id` on, in Instance order, with a
    /// datestamp between `from` and `until`, both inclusive.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn headers(
        &self,
        from_id: i64,
        from: Option<&Datestamp>,
        until: Option<&Datestamp>,
        limit: usize,
    ) -> Result<Vec<OaiHeader>, OaiUnavailable>;
}

/// The tag of Koha's Items, left out of the records (`include_items` off, item 15).
const ITEMS: Tag = Tag::new(*b"952");

/// The MARC record of `instance`, as the export writes it, without the Items; `None`
/// when it does not exist.
///
/// # Errors
///
/// [`OaiRecordError::Unavailable`] when storage fails, [`OaiRecordError::Invalid`] when
/// the record cannot be written.
pub fn marc(
    catalog: &dyn OaiCatalog,
    instance: InstanceId,
) -> Result<Option<Record>, OaiRecordError> {
    match marc_record(catalog, instance, &Limits::default()) {
        Ok(record) => Ok(record.map(|mut record| {
            record.fields.retain(|field| field.tag() != ITEMS);
            record
        })),
        Err(RecordExportError::Read(error)) => Err(OaiUnavailable(error.to_string()).into()),
        Err(RecordExportError::Failure(error)) => Err(OaiRecordError::Invalid(error.to_string())),
    }
}

/// Why a record's metadata could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OaiRecordError {
    /// Storage failed.
    #[error(transparent)]
    Unavailable(#[from] OaiUnavailable),
    /// The record cannot be written as MARC.
    #[error("record cannot be written: {0}")]
    Invalid(String),
}
