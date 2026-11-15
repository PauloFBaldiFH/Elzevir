// SPDX-License-Identifier: GPL-3.0-or-later
//! The form of the Work (`380`, docs/specs/catalogo/bibframe-completo.md, item 29, part
//! 5f), after the Library of Congress conversion (`marc2bibframe2`: `ConvSpec-3XX.xsl`).
//!
//! The first `$a` of each `380` becomes a `bf:genreForm` of the Work: a `bf:GenreForm` with
//! the term as its label, as the LC writes it without a source. The LC way back has no
//! `380` and writes such a term as a `655`; here it goes back to `380`, where it came from,
//! since the subjects of `655` have their own field. A repeated `$a` and the source of
//! `$2` stay in the source record.

use elzevir_domain::statement::{Resource, Statement, Value};

use super::titles::Owner;
use super::{DataField, Marc, text};

/// The fields this module reads.
pub(super) const TAGS: [&[u8; 3]; 1] = [b"380"];

/// The statements of the Work, in record order.
pub(super) fn of_work(marc: &Marc<'_>) -> Vec<Statement> {
    marc.data_in(&TAGS)
        .filter_map(|(tag, field)| of_field(*tag, field))
        .map(|(_, statement)| statement)
        .collect()
}

/// The statement one `380` gives; `None` without a term.
pub(super) fn of_field(tag: [u8; 3], field: DataField<'_>) -> Option<(Owner, Statement)> {
    if tag != *b"380" {
        return None;
    }
    let term = field.values(b'a').find_map(text::trimmed)?;
    Some((
        Owner::Work,
        Statement {
            property: "genreForm".to_owned(),
            value: Value::Resource(Resource {
                class: "GenreForm".to_owned(),
                label: Some(term),
                statements: Vec::new(),
            }),
        },
    ))
}
