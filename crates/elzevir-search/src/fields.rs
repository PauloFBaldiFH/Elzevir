// SPDX-License-Identifier: GPL-3.0-or-later
//! Search by field over the same index (docs/specs/opac/sru.md, items 9 and 10).

use elzevir_app::field_search::{
    FieldQuery, FieldSearch, FieldTerm, SearchField, Truncation, WordMatch,
};
use elzevir_app::search::{SearchError, SearchResults, SearchScope, normalized_identifier};
use elzevir_domain::catalog::IdentifierKind;
use tantivy::Term;
use tantivy::query::{BooleanQuery, EmptyQuery, Occur, Query, QueryParser, RegexQuery, TermQuery};
use tantivy::schema::{Field, IndexRecordOption};
use tantivy::tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer};

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
        let truncation = term.truncation;
        match term.field {
            // A term of every field truncated on the right is the search box with each
            // word completed, codes included (docs/specs/catalogo/busca.md, item 5a).
            SearchField::Any
                if truncation == Truncation::Right && term.matching != WordMatch::Any =>
            {
                self.query(&term.text, scope, true)
            }
            SearchField::Any if truncation != Truncation::None => self.truncated_words(
                &[
                    f.title,
                    f.work_title,
                    f.other_title,
                    f.contributor,
                    f.subject,
                    f.publisher,
                    f.place,
                    f.summary,
                    f.note,
                ],
                term.matching,
                &words,
                truncation,
            ),
            SearchField::Any => match term.matching {
                WordMatch::Any => any_of(
                    words
                        .split_whitespace()
                        .map(|word| self.query(word, scope, false)),
                ),
                WordMatch::All | WordMatch::Phrase if words.trim().is_empty() => {
                    Box::new(EmptyQuery)
                }
                WordMatch::All | WordMatch::Phrase => self.query(&term.text, scope, false),
            },
            SearchField::Title => self.truncated_words(
                &[f.title, f.work_title, f.other_title],
                term.matching,
                &words,
                truncation,
            ),
            SearchField::Contributor => {
                self.truncated_words(&[f.contributor], term.matching, &words, truncation)
            }
            SearchField::Subject => {
                self.truncated_words(&[f.subject], term.matching, &words, truncation)
            }
            SearchField::Publisher => {
                self.truncated_words(&[f.publisher], term.matching, &words, truncation)
            }
            SearchField::Note => {
                self.truncated_words(&[f.summary, f.note], term.matching, &words, truncation)
            }
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
                    .map(|value| exact(f.identifier, &value, truncation)),
                )
            }
            SearchField::Year => exact(f.year, term.text.trim(), truncation),
            SearchField::Language => {
                exact(f.language, &term.text.trim().to_lowercase(), truncation)
            }
            SearchField::Barcode => exact(f.barcode, term.text.trim(), truncation),
            SearchField::CallNumber => exact(
                f.call_number_key,
                &term.text.trim().to_lowercase(),
                truncation,
            ),
            SearchField::Record => match term.text.trim().parse::<u64>() {
                Ok(number) => Box::new(TermQuery::new(
                    Term::from_field_u64(f.instance, number),
                    IndexRecordOption::Basic,
                )),
                Err(_) => Box::new(EmptyQuery),
            },
        }
    }

    /// `words` in `fields`, each truncated as asked: a truncated word finds the indexed
    /// words it starts (right), ends (left) or is inside (both), besides its own stem,
    /// since the index keeps stems. With truncation, a phrase asks for all the words.
    pub(crate) fn truncated_words(
        &self,
        fields: &[Field],
        matching: WordMatch,
        words: &str,
        truncation: Truncation,
    ) -> Box<dyn Query> {
        if truncation == Truncation::None {
            return self.words(fields, matching, words);
        }
        let occur = if matching == WordMatch::Any {
            Occur::Should
        } else {
            Occur::Must
        };
        let clauses: Vec<(Occur, Box<dyn Query>)> = words
            .split_whitespace()
            .map(|word| {
                let folded = fold(word);
                let stem = self.words(fields, WordMatch::All, word);
                let starts = fields
                    .iter()
                    .map(|field| exact(*field, &folded, truncation));
                (occur, any_of(std::iter::once(stem).chain(starts)))
            })
            .collect();
        if clauses.is_empty() {
            return Box::new(EmptyQuery);
        }
        Box::new(BooleanQuery::new(clauses))
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

/// `word` lower-cased and without accents, as the index keeps its words before the stem.
fn fold(word: &str) -> String {
    let mut analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter)
        .build();
    let mut stream = analyzer.token_stream(word);
    let mut folded = String::new();
    while let Some(token) = stream.next() {
        folded.push_str(&token.text);
    }
    folded
}

/// The text with every character that is not a letter or a digit made a space, so the
/// query parser reads words and nothing else.
pub(crate) fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect()
}

/// `value` whole in `field`, or as its start, end or inside with `truncation`.
fn exact(field: Field, value: &str, truncation: Truncation) -> Box<dyn Query> {
    if value.is_empty() {
        return Box::new(EmptyQuery);
    }
    let escaped = escape(value);
    let pattern = match truncation {
        Truncation::None => {
            return Box::new(TermQuery::new(
                Term::from_field_text(field, value),
                IndexRecordOption::Basic,
            ));
        }
        Truncation::Right => format!("{escaped}.*"),
        Truncation::Left => format!(".*{escaped}"),
        Truncation::Both => format!(".*{escaped}.*"),
    };
    // The pattern is the escaped term with fixed wildcards, so it always compiles; if
    // the regex engine refused it anyway, nothing would match rather than everything.
    RegexQuery::from_pattern(&pattern, field).map_or_else(
        |_| Box::new(EmptyQuery) as Box<dyn Query>,
        |query| Box::new(query),
    )
}

/// `value` with every character the regex syntax reads specially escaped.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() * 2);
    for c in value.chars() {
        if "\\.+*?()|[]{}^$#&-~\"".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

fn any_of(queries: impl Iterator<Item = Box<dyn Query>>) -> Box<dyn Query> {
    let clauses: Vec<(Occur, Box<dyn Query>)> =
        queries.map(|query| (Occur::Should, query)).collect();
    if clauses.is_empty() {
        return Box::new(EmptyQuery);
    }
    Box::new(BooleanQuery::new(clauses))
}
