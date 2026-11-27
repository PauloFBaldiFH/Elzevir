// SPDX-License-Identifier: GPL-3.0-or-later
//! The index schema: one document per Instance (docs/specs/catalogo/busca.md, §1).

use elzevir_app::search::Facet;
use tantivy::Index;
use tantivy::schema::{
    FAST, INDEXED, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions,
};
use tantivy::tokenizer::{
    AsciiFoldingFilter, Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer,
    TextAnalyzer,
};

/// Words lower-cased, stemmed as Portuguese and without accents, so `coração` finds
/// `Coracao` and `histórias` finds `história`. Folding runs before stemming so that a
/// word typed without accents gets the same stem as the accented one.
pub(crate) const FOLDED: &str = "elzevir_folded";

/// The fields of the schema.
pub(crate) struct Fields {
    pub(crate) repository_membership: tantivy::schema::Field,
    pub(crate) digital_collection: tantivy::schema::Field,
    pub(crate) instance: tantivy::schema::Field,
    pub(crate) work: tantivy::schema::Field,
    pub(crate) hub: tantivy::schema::Field,
    pub(crate) title: tantivy::schema::Field,
    pub(crate) work_title: tantivy::schema::Field,
    pub(crate) other_title: tantivy::schema::Field,
    pub(crate) main_contributor: tantivy::schema::Field,
    pub(crate) contributor: tantivy::schema::Field,
    pub(crate) subject: tantivy::schema::Field,
    pub(crate) publisher: tantivy::schema::Field,
    pub(crate) place: tantivy::schema::Field,
    pub(crate) summary: tantivy::schema::Field,
    pub(crate) note: tantivy::schema::Field,
    pub(crate) date: tantivy::schema::Field,
    pub(crate) content: tantivy::schema::Field,
    pub(crate) carrier: tantivy::schema::Field,
    pub(crate) classification: tantivy::schema::Field,
    pub(crate) identifier: tantivy::schema::Field,
    pub(crate) call_number: tantivy::schema::Field,
    pub(crate) call_number_key: tantivy::schema::Field,
    pub(crate) location: tantivy::schema::Field,
    pub(crate) barcode: tantivy::schema::Field,
    pub(crate) inventory_number: tantivy::schema::Field,
    pub(crate) language: tantivy::schema::Field,
    /// `1` for an Instance hidden from the OPAC, `0` otherwise.
    pub(crate) opac_hidden: tantivy::schema::Field,
    /// The family: the Hub (with [`HUB_FAMILY`] set) or else the Work.
    pub(crate) family: tantivy::schema::Field,
    pub(crate) branch: tantivy::schema::Field,
    pub(crate) genre: tantivy::schema::Field,
    pub(crate) audience: tantivy::schema::Field,
    /// Each subject heading whole, for its facet; `subject` holds its words.
    pub(crate) subject_heading: tantivy::schema::Field,
    pub(crate) year: tantivy::schema::Field,
}

/// Set in [`Fields::family`] for a Hub, so a Hub and a Work with the same number are
/// different families.
pub(crate) const HUB_FAMILY: u64 = 1 << 63;

/// The fast field each facet is counted on (docs/specs/catalogo/facetas.md, item 1).
pub(crate) const fn facet_field(facet: Facet) -> &'static str {
    match facet {
        Facet::Branch => "branch",
        Facet::Content => "content",
        Facet::Carrier => "carrier",
        Facet::Genre => "genre",
        Facet::Audience => "audience",
        Facet::Language => "language",
        Facet::Subject => "subject_heading",
        Facet::Year => "year",
    }
}

pub(crate) fn build() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let indexed_words = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(FOLDED)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let words = indexed_words.clone().set_stored();
    let code = STRING | STORED;
    // A facet is a whole value counted from its column.
    let facet = STRING | FAST;
    let fields = Fields {
        instance: builder.add_u64_field("instance", INDEXED | STORED | FAST),
        work: builder.add_u64_field("work", STORED | FAST),
        hub: builder.add_u64_field("hub", STORED | FAST),
        title: builder.add_text_field("title", words.clone()),
        work_title: builder.add_text_field("work_title", words.clone()),
        other_title: builder.add_text_field("other_title", words.clone()),
        main_contributor: builder.add_text_field("main_contributor", STORED),
        contributor: builder.add_text_field("contributor", words.clone()),
        subject: builder.add_text_field("subject", words.clone()),
        publisher: builder.add_text_field("publisher", words.clone()),
        place: builder.add_text_field("place", words.clone()),
        summary: builder.add_text_field("summary", words.clone()),
        // Opening an index that predates note visibility must discard its private terms.
        note: builder.add_text_field("public_note", words),
        date: builder.add_text_field("date", STORED),
        content: builder.add_text_field("content", code.clone() | FAST),
        carrier: builder.add_text_field("carrier", code.clone() | FAST),
        classification: builder.add_text_field("classification", code.clone()),
        identifier: builder.add_text_field("identifier", code.clone()),
        call_number: builder.add_text_field("call_number", STORED),
        call_number_key: builder.add_text_field("call_number_key", STRING),
        location: builder.add_text_field("location", code.clone()),
        barcode: builder.add_text_field("barcode", code.clone()),
        inventory_number: builder.add_text_field("inventory_number", code.clone()),
        language: builder.add_text_field("language", code | FAST),
        opac_hidden: builder.add_u64_field("opac_hidden", INDEXED),
        family: builder.add_u64_field("family", FAST),
        branch: builder.add_text_field("branch", facet.clone()),
        genre: builder.add_text_field("genre", facet.clone()),
        audience: builder.add_text_field("audience", facet.clone()),
        subject_heading: builder.add_text_field("subject_heading", facet.clone()),
        year: builder.add_text_field("year", facet),
        // Keep repository-only fields after the catalog query schema.
        repository_membership: builder.add_text_field("repository_membership", STRING),
        digital_collection: builder.add_text_field("digital_collection", indexed_words),
    };
    (builder.build(), fields)
}

/// Tokenizers are not saved in the index, so every open registers them again.
pub(crate) fn register_tokenizers(index: &Index) {
    index.tokenizers().register(
        FOLDED,
        TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser)
            .filter(AsciiFoldingFilter)
            .filter(Stemmer::new(Language::Portuguese))
            .build(),
    );
}
