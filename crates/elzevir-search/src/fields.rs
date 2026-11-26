// SPDX-License-Identifier: GPL-3.0-or-later
//! Search by field over the same index (docs/specs/opac/sru.md, items 9 and 10).

use elzevir_app::field_search::{FieldQuery, FieldSearch, FieldTerm, SearchField, WordMatch};
use elzevir_app::search::{SearchError, SearchResults, SearchScope, normalized_identifier};
use elzevir_domain::catalog::IdentifierKind;
use tantivy::Term;
use tantivy::query::{BooleanQuery, EmptyQuery, Occur, Query, QueryParser, TermQuery};
use tantivy::schema::{Field, IndexRecordOption};

use crate::TantivyIndex;

impl FieldSearch for TantivyIndex {
    fn search_fields(
        &self,
        query: &FieldQuery,
        scope: SearchScope,
        offset: usize,
        limit: usize,
    ) -> Result<SearchResults, SearchError> {
        let built = self.scoped(self.tree(query, scope), scope);
        self.run(&*built, false, offset, limit)
    }
}

impl TantivyIndex {
    fn tree(&self, query: &FieldQuery, scope: SearchScope) -> Box<dyn Query> {
        let both = |occur: Occur, left: &FieldQuery, right: &FieldQuery| -> Box<dyn Query> {
            Box::new(BooleanQuery::new(vec![
                (Occur::Must, self.tree(left, scope)),
                (occur, self.tree(right, scope)),
            ]))
        };
        match query {
            FieldQuery::Term(term) => self.term(term, scope),
            FieldQuery::And(left, right) => both(Occur::Must, left, right),
            FieldQuery::Not(left, right) => both(Occur::MustNot, left, right),
            FieldQuery::Or(left, right) => Box::new(BooleanQuery::new(vec![
                (Occur::Should, self.tree(left, scope)),
                (Occur::Should, self.tree(right, scope)),
            ])),
        }
    }

    fn term(&self, term: &FieldTerm, scope: SearchScope) -> Box<dyn Query> {
        let f = &self.fields;
        let words = plain(&term.text);
        match term.field {
            SearchField::Any => match term.matching {
                WordMatch::Any => {
                    any_of(words.split_whitespace().map(|word| self.query(word, scope)))
                }
                WordMatch::All | WordMatch::Phrase if words.trim().is_empty() => {
                    Box::new(EmptyQuery)
                }
                WordMatch::All | WordMatch::Phrase => self.query(&term.text, scope),
            },
            SearchField::Title => self.words(
                &[f.title, f.work_title, f.other_title],
                term.matching,
                &words,
            ),
            SearchField::Contributor => self.words(&[f.contributor], term.matching, &words),
            SearchField::Subject => self.words(&[f.subject], term.matching, &words),
            SearchField::Publisher => self.words(&[f.publisher], term.matching, &words),
            SearchField::Note => self.words(&[f.summary, f.note], term.matching, &words),
            SearchField::Identifier => {
                let typed = term.text.trim();
                any_of(
                    [
                        normalized_identifier(IdentifierKind::Isbn, typed),
                        normalized_identifier(IdentifierKind::Issn, typed),
                        typed.to_owned(),
                    ]
                    .into_iter()
                    .filter(|value| !value.is_empty())
                    .map(|value| exact(f.identifier, &value)),
                )
            }
            SearchField::Year => exact(f.year, term.text.trim()),
            SearchField::Language => exact(f.language, &term.text.trim().to_lowercase()),
            SearchField::Barcode => exact(f.barcode, term.text.trim()),
            SearchField::Record => match term.text.trim().parse::<u64>() {
                Ok(number) => Box::new(TermQuery::new(
                    Term::from_field_u64(f.instance, number),
                    IndexRecordOption::Basic,
                )),
                Err(_) => Box::new(EmptyQuery),
            },
        }
    }

    /// `words` in `fields`: all of them, any of them, or as a phrase.
    fn words(&self, fields: &[Field], matching: WordMatch, words: &str) -> Box<dyn Query> {
        if words.trim().is_empty() {
            return Box::new(EmptyQuery);
        }
        let mut parser = QueryParser::new(
            self.catalog_query_schema.clone(),
            fields.to_vec(),
            self.index.tokenizers().clone(),
        );
        if matching == WordMatch::All {
            parser.set_conjunction_by_default();
        }
        let text = if matching == WordMatch::Phrase {
            format!("\"{words}\"")
        } else {
            words.to_owned()
        };
        // `plain` left only letters, digits and spaces, so nothing is dropped here.
        let (query, _ignored) = parser.parse_query_lenient(&text);
        query
    }
}

/// The text with every character that is not a letter or a digit made a space, so the
/// query parser reads words and nothing else.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect()
}

fn exact(field: Field, value: &str) -> Box<dyn Query> {
    if value.is_empty() {
        return Box::new(EmptyQuery);
    }
    Box::new(TermQuery::new(
        Term::from_field_text(field, value),
        IndexRecordOption::Basic,
    ))
}

fn any_of(queries: impl Iterator<Item = Box<dyn Query>>) -> Box<dyn Query> {
    let clauses: Vec<(Occur, Box<dyn Query>)> =
        queries.map(|query| (Occur::Should, query)).collect();
    if clauses.is_empty() {
        return Box::new(EmptyQuery);
    }
    Box::new(BooleanQuery::new(clauses))
}
