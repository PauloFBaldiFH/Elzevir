// SPDX-License-Identifier: GPL-3.0-or-later
//! The call number the item grid suggests for an Instance: its classification and the
//! author notation from the library's table (docs/specs/catalogo/numero-de-chamada.md).

use elzevir_domain::call_number::{AuthorTable, MainEntry, NotationError};
use elzevir_domain::catalog::AgentKind;

use crate::catalog::NewRecord;

/// A suggested call number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallNumberSuggestion {
    /// The whole call number, such as `869.3 A848d 2. ed.`.
    pub call_number: String,
    /// The entry of the table the notation comes from, as printed, to check in the book.
    pub entry: String,
}

/// The call number of `record`: its classification, the author notation of its main
/// entry from `table`, and the edition from the second on (`2. ed.`).
///
/// Koha: `cataloguing/value_builder/callnumber.pl` builds the call number from the
/// classification; Koha has no author table, the notation follows the PHA explanation.
///
/// # Errors
///
/// [`NotationError`] when the table gives no notation for the main entry.
pub fn suggest(
    record: &NewRecord,
    table: &AuthorTable,
) -> Result<CallNumberSuggestion, NotationError> {
    let main = record
        .contributions
        .iter()
        .find(|contribution| contribution.primary);
    let entry = match main {
        Some(main) if matches!(main.kind, AgentKind::Person | AgentKind::Family) => {
            MainEntry::Person(&main.heading)
        }
        Some(main) => MainEntry::Corporate(&main.heading),
        None => MainEntry::Title,
    };
    // The title proper: the subtitle follows ` : ` and the statement ` / `.
    let title = record
        .instance
        .title
        .split([':', '/'])
        .next()
        .unwrap_or_default();
    let notation = table.notation(entry, title)?;
    let edition = record.instance.edition.as_deref().and_then(edition_number);
    let parts = [
        record.classification.as_deref().map(str::trim),
        Some(notation.notation.as_str()),
        edition.as_deref(),
    ];
    Ok(CallNumberSuggestion {
        call_number: parts
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        entry: notation.entry,
    })
}

/// `2. ed.` for an edition statement that starts with 2 or more; the first is not marked.
fn edition_number(statement: &str) -> Option<String> {
    let digits: String = statement
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let number: u32 = digits.get(..digits.len().min(3))?.parse().ok()?;
    (number > 1).then(|| format!("{number}. ed."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{NewContribution, NewInstance};
    use elzevir_domain::statement::Description;

    fn record(contributions: Vec<NewContribution>, edition: Option<&str>) -> NewRecord {
        NewRecord {
            work_title: "Dom Casmurro".to_owned(),
            classification: Some("869.3".to_owned()),
            contributions,
            instance: NewInstance {
                title: "Dom Casmurro : romance / Machado de Assis".to_owned(),
                edition: edition.map(str::to_owned),
                ..NewInstance::default()
            },
            work_description: Description::default(),
            hub: None,
            relations: Vec::new(),
            classification_scheme: Some("ddc".to_owned()),
            summary: None,
            audience: None,
            languages: Vec::new(),
            subjects: Vec::new(),
            items: crate::catalog::RecordItems::default(),
            source: None,
        }
    }

    fn agent(kind: AgentKind, heading: &str, primary: bool) -> NewContribution {
        NewContribution {
            kind,
            heading: heading.to_owned(),
            role: None,
            role_term: None,
            primary,
            name_parts: Vec::new(),
            identifiers: Vec::new(),
        }
    }

    fn table() -> AuthorTable {
        AuthorTable::parse("Assi 848\nAst 849\nDo 72\nEditora 33\n")
            .unwrap()
            .0
    }

    #[test]
    fn class_notation_and_edition() {
        let assis = agent(AgentKind::Person, "Assis, Machado de, 1839-1908", true);
        let suggestion = suggest(&record(vec![assis.clone()], Some("2. ed. rev.")), &table());
        assert_eq!(
            suggestion,
            Ok(CallNumberSuggestion {
                call_number: "869.3 A848d 2. ed.".to_owned(),
                entry: "Assi".to_owned(),
            })
        );
        let first = suggest(&record(vec![assis], Some("1. ed.")), &table()).unwrap();
        assert_eq!(first.call_number, "869.3 A848d");
    }

    #[test]
    fn without_a_main_entry_the_title_and_an_institution_by_its_first_word() {
        let added = agent(AgentKind::Person, "Assis, Machado de", false);
        assert_eq!(
            suggest(&record(vec![added], None), &table())
                .unwrap()
                .call_number,
            "869.3 D72"
        );
        let body = agent(AgentKind::Organization, "Editora Garnier", true);
        assert_eq!(
            suggest(&record(vec![body], None), &table())
                .unwrap()
                .call_number,
            "869.3 E33d"
        );
    }
}
