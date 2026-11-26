// SPDX-License-Identifier: GPL-3.0-or-later
//! The SRU server of the public catalog (docs/specs/opac/sru.md): CQL, the request rules
//! and the response, over the search by field and the stored records.
//!
//! Koha: `Koha::Z3950Responder` and `etc/z3950/pqf.properties`, served by YAZ.

mod cql;
mod request;
mod response;

use crate::field_search::SearchField;

pub use request::{Packing, RecordSchema, Version};
pub use response::{SruAnswer, respond};

/// `[sru]` of `elzevir.toml`, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SruSettings {
    /// The host of the public address, for `explain`.
    pub host: String,
    /// Its port.
    pub port: u16,
    /// Its path without the leading `/`: the database.
    pub database: String,
    /// The catalog's name.
    pub title: String,
    /// Records per response when the client asks for none.
    pub default_records: usize,
    /// Most records per response.
    pub max_records: usize,
}

/// The context sets the server knows, by short name (item 9).
pub(crate) const CONTEXT_SETS: [(&str, &str); 5] = [
    ("cql", "info:srw/cql-context-set/1/cql-v1.1"),
    ("dc", "info:srw/cql-context-set/1/dc-v1.1"),
    ("bath", "http://zing.z3950.org/cql/bath/2.0/"),
    ("rec", "info:srw/cql-context-set/2/rec-1.0"),
    (
        "koha",
        "https://wiki.koha-community.org/wiki/Making_more_indexes_searchable_with_Zebra_SRU",
    ),
];

/// Every CQL index and its field (item 9), in the order `explain` lists them.
pub(crate) const INDEXES: [(&str, &str, SearchField); 32] = [
    ("cql", "serverChoice", SearchField::Any),
    ("dc", "any", SearchField::Any),
    ("dc", "title", SearchField::Title),
    ("bath", "keyTitle", SearchField::Title),
    ("bath", "uniformTitle", SearchField::Title),
    ("dc", "creator", SearchField::Contributor),
    ("dc", "author", SearchField::Contributor),
    ("dc", "editor", SearchField::Contributor),
    ("dc", "pname", SearchField::Contributor),
    ("bath", "name", SearchField::Contributor),
    ("bath", "personalName", SearchField::Contributor),
    ("bath", "corporateName", SearchField::Contributor),
    ("bath", "conferenceName", SearchField::Contributor),
    ("dc", "subject", SearchField::Subject),
    ("bath", "topicalSubject", SearchField::Subject),
    ("bath", "geographicName", SearchField::Subject),
    ("bath", "genreForm", SearchField::Subject),
    ("dc", "publisher", SearchField::Publisher),
    ("dc", "description", SearchField::Note),
    ("dc", "note", SearchField::Note),
    ("bath", "notes", SearchField::Note),
    ("dc", "identifier", SearchField::Identifier),
    ("dc", "isbn", SearchField::Identifier),
    ("dc", "issn", SearchField::Identifier),
    ("bath", "isbn", SearchField::Identifier),
    ("bath", "issn", SearchField::Identifier),
    ("dc", "date", SearchField::Year),
    ("dc", "language", SearchField::Language),
    ("dc", "barcode", SearchField::Barcode),
    ("koha", "barcode", SearchField::Barcode),
    ("rec", "id", SearchField::Record),
    ("dc", "resourceIdentifier", SearchField::Record),
];

/// An SRU diagnostic of `info:srw/diagnostic/1` (item 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The number.
    pub code: u16,
    /// What it is about: a parameter, an index, a relation.
    pub details: Option<String>,
}

impl Diagnostic {
    pub(crate) const fn bare(code: u16) -> Self {
        Self {
            code,
            details: None,
        }
    }

    pub(crate) fn with(code: u16, details: &str) -> Self {
        Self {
            code,
            details: Some(details.to_owned()),
        }
    }

    /// The description from the SRU list.
    #[must_use]
    pub const fn message(&self) -> &'static str {
        match self.code {
            1 => "General system error",
            4 => "Unsupported operation",
            5 => "Unsupported version",
            6 => "Unsupported parameter value",
            7 => "Mandatory parameter not supplied",
            8 => "Unsupported Parameter",
            10 => "Query syntax error",
            13 => "Invalid or unsupported use of parentheses",
            15 => "Unsupported context set",
            16 => "Unsupported index",
            19 => "Unsupported relation",
            20 => "Unsupported relation modifier",
            27 => "Empty term unsupported",
            28 => "Masking character not supported",
            37 => "Unsupported boolean operator",
            61 => "First record position out of range",
            64 => "Record temporarily unavailable",
            66 => "Unknown schema for retrieval",
            71 => "Unsupported record packing",
            80 => "Sort not supported",
            _ => "Diagnostic",
        }
    }
}
