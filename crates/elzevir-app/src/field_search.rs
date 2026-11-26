// SPDX-License-Identifier: GPL-3.0-or-later
//! Search by field: the query tree that the SRU and Z39.50 servers build from CQL or RPN,
//! and the port of the index that answers it (docs/specs/opac/sru.md, items 9 and 10).

use crate::search::{SearchError, SearchResults, SearchScope};

/// What a term is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchField {
    /// Everything, as the OPAC's search box: words and exact codes.
    Any,
    /// The titles of the Instance and of the Work, and the other titles.
    Title,
    /// The contributors.
    Contributor,
    /// The subjects.
    Subject,
    /// The publisher.
    Publisher,
    /// The summary and the notes.
    Note,
    /// The identifiers (ISBN without hyphens, ISSN, others as written).
    Identifier,
    /// The year of publication.
    Year,
    /// A language code.
    Language,
    /// An Item's barcode.
    Barcode,
    /// The Instance's number.
    Record,
}

impl SearchField {
    /// Compared as words; otherwise, as a whole code.
    #[must_use]
    pub const fn is_words(self) -> bool {
        matches!(
            self,
            Self::Any
                | Self::Title
                | Self::Contributor
                | Self::Subject
                | Self::Publisher
                | Self::Note
        )
    }
}

/// How the words of a term must appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WordMatch {
    /// Every word, in any order.
    All,
    /// At least one word.
    Any,
    /// The words together, in order. On a code field, the whole value.
    Phrase,
}

/// One term: a field, how it matches and the text asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldTerm {
    /// The field.
    pub field: SearchField,
    /// How the words match.
    pub matching: WordMatch,
    /// The text, without query syntax.
    pub text: String,
}

/// A query by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldQuery {
    /// One term.
    Term(FieldTerm),
    /// Both.
    And(Box<Self>, Box<Self>),
    /// Either.
    Or(Box<Self>, Box<Self>),
    /// The first without the second.
    Not(Box<Self>, Box<Self>),
}

/// Port: the index searched by field. The search index implements it.
pub trait FieldSearch: Send + Sync {
    /// The Instances matching `query` in `scope`, best first, skipping `offset` and
    /// returning at most `limit`, with the total count.
    ///
    /// # Errors
    ///
    /// When the index cannot be read.
    fn search_fields(
        &self,
        query: &FieldQuery,
        scope: SearchScope,
        offset: usize,
        limit: usize,
    ) -> Result<SearchResults, SearchError>;
}
