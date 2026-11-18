// SPDX-License-Identifier: GPL-3.0-or-later
//! MARC21 and UNIMARC reading and writing (ISO 2709 and MARCXML) with explicit input limits.
//!
//! Reading borrows instead of copying (docs/arquitetura/03-estrategia-reconstrucao.md, §2.2):
//! [`MarcReader`] reads one record at a time into a reused buffer, and the [`RecordRef`] it
//! hands out holds only slices of that buffer, valid until the next record is read. A record
//! is validated once, when it is read; after that, walking its fields cannot fail.
//!
//! Every limit is explicit ([`Limits`]); a hostile or broken file produces an error for the
//! offending record and the reader resynchronizes on the next record terminator.

mod charset;
mod error;
mod iso2709_writer;
mod limits;
mod marcxml;
mod marcxml_reader;
mod owned;
mod publish;
mod reader;
mod record;
mod tag;

pub use charset::{DetectedEncoding, Marc8Text, iso5426_to_utf8, marc8_to_utf8};
pub use error::{ReadError, RecordError, WriteError};
pub use limits::Limits;
pub use marcxml::{MARCXML_COLLECTION_FOOTER, MARCXML_COLLECTION_HEADER};
pub use marcxml_reader::{
    MAX_XML_DEPTH, MAX_XML_ELEMENT_BYTES, MAX_XML_ELEMENTS_PER_RECORD, MAX_XML_INPUT_BYTES,
    MAX_XML_RECORD_BYTES, MAX_XML_RECORDS, MarcXmlError, MarcXmlReader, XmlLimits, XmlProblem,
};
pub use owned::{Field, Record, Subfield};
pub use publish::publish_atomically;
pub use reader::MarcReader;
pub use record::{Encoding, FieldRef, RecordRef, Subfields};
pub use tag::Tag;

/// Length of the leader.
pub const LEADER_LEN: usize = 24;

/// ISO 2709 record terminator.
pub const RECORD_TERMINATOR: u8 = 0x1D;
/// ISO 2709 field terminator.
pub const FIELD_TERMINATOR: u8 = 0x1E;
/// ISO 2709 subfield delimiter.
pub const SUBFIELD_DELIMITER: u8 = 0x1F;
