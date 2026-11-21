// SPDX-License-Identifier: GPL-3.0-or-later
//! The CQL of a draft query, by Koha's rule, and URL encoding.

use std::collections::HashMap;
use std::hash::BuildHasher;

use elzevir_app::drafts::{DraftField, DraftQuery};

/// The CQL of `query` for a server whose indexes are `fields` (spec items 8 and 9).
///
/// Koha: `C4::Breeding::_build_query` and `_translate_query`.
#[must_use]
pub fn cql<S: BuildHasher>(query: &DraftQuery, fields: &HashMap<DraftField, String, S>) -> String {
    let any = fields.get(&DraftField::Any);
    query
        .fields()
        .map(|(field, value)| {
            let value = quoted(value);
            match fields.get(&field).or(any) {
                Some(index) => format!("{index}={value}"),
                None => value,
            }
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// `"value"` with `"` and `\` escaped, so the text never changes the query's shape.
fn quoted(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// Percent-encoding of a query-string value: every byte but the unreserved ones.
pub(crate) fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            for nibble in [byte >> 4, byte & 0x0f] {
                encoded.extend(
                    char::from_digit(u32::from(nibble), 16).map(|c| c.to_ascii_uppercase()),
                );
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use elzevir_app::drafts::TypedQuery;

    use super::*;

    fn query(typed: &TypedQuery<'_>) -> DraftQuery {
        DraftQuery::new(typed).unwrap()
    }

    #[test]
    fn fields_map_to_the_servers_indexes_or_fall_back_to_any() {
        let typed = TypedQuery {
            isbn: "978-85-359-0277-1",
            title: "Dom \"Casmurro\" \\",
            author: "Assis",
            ..TypedQuery::default()
        };
        let mut fields = HashMap::from([
            (DraftField::Isbn, "bath.isbn".to_owned()),
            (DraftField::Title, "dc.title".to_owned()),
        ]);
        assert_eq!(
            cql(&query(&typed), &fields),
            r#""Assis" and bath.isbn="9788535902771" and dc.title="Dom \"Casmurro\" \\""#
        );
        fields.insert(DraftField::Any, "cql.anywhere".to_owned());
        assert_eq!(
            cql(&query(&typed), &fields),
            r#"cql.anywhere="Assis" and bath.isbn="9788535902771" and dc.title="Dom \"Casmurro\" \\""#
        );
    }

    #[test]
    fn values_are_percent_encoded() {
        assert_eq!(
            encode(r#"dc.title="São Paulo""#),
            "dc.title%3D%22S%C3%A3o%20Paulo%22"
        );
    }
}
