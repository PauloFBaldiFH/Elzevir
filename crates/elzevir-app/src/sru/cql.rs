// SPDX-License-Identifier: GPL-3.0-or-later
//! CQL 1.2 read into a query by field (docs/specs/opac/sru.md, items 8 to 12).
//!
//! Koha: the CQL indexes of `etc/z3950/pqf.properties`, which `Koha::Z3950Responder`
//! hands to YAZ.

use crate::field_search::{FieldQuery, FieldTerm, SearchField, WordMatch};

use super::{CONTEXT_SETS, Diagnostic, INDEXES};

/// Most clauses in one query (item 12).
const MAX_CLAUSES: usize = 64;

/// Deepest parentheses (item 12).
const MAX_DEPTH: usize = 16;

/// The relations written as words.
const NAMED_RELATIONS: [&str; 6] = ["adj", "all", "any", "exact", "within", "encloses"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Slash,
    /// `=`, `==`, `<>`, `<`, `>`, `<=` or `>=`.
    Symbol(&'static str),
    Word {
        text: String,
        quoted: bool,
        masked: bool,
    },
}

/// The query by field that `query` asks for.
///
/// # Errors
///
/// The SRU diagnostic of the first thing the reader cannot accept.
pub(super) fn parse(query: &str) -> Result<FieldQuery, Diagnostic> {
    let tokens = tokens(query)?;
    let mut reader = Reader {
        tokens: &tokens,
        at: 0,
        clauses: 0,
    };
    let parsed = reader.scoped(0)?;
    match reader.peek() {
        None => Ok(parsed),
        Some(Token::Close) => Err(Diagnostic::bare(13)),
        Some(Token::Word {
            text,
            quoted: false,
            ..
        }) if text.eq_ignore_ascii_case("sortby") => Err(Diagnostic::bare(80)),
        Some(_) => Err(Diagnostic::bare(10)),
    }
}

fn tokens(query: &str) -> Result<Vec<Token>, Diagnostic> {
    let mut tokens = Vec::new();
    let mut chars = query.chars().peekable();
    while let Some(c) = chars.next() {
        let symbol = |next: Option<&char>, pairs: &[(char, &'static str)], alone| {
            pairs
                .iter()
                .find(|(second, _)| next == Some(second))
                .map_or((alone, false), |(_, both)| (*both, true))
        };
        let token = match c {
            c if c.is_whitespace() => continue,
            '(' => Token::Open,
            ')' => Token::Close,
            '/' => Token::Slash,
            '=' | '<' | '>' => {
                let (name, two) = match c {
                    '=' => symbol(chars.peek(), &[('=', "==")], "="),
                    '<' => symbol(chars.peek(), &[('=', "<="), ('>', "<>")], "<"),
                    _ => symbol(chars.peek(), &[('=', ">=")], ">"),
                };
                if two {
                    chars.next();
                }
                Token::Symbol(name)
            }
            '"' => quoted(&mut chars)?,
            first => {
                let mut text = String::from(first);
                let mut masked = matches!(first, '*' | '?');
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace()
                        || matches!(next, '(' | ')' | '=' | '<' | '>' | '"' | '/')
                    {
                        break;
                    }
                    masked |= matches!(next, '*' | '?');
                    text.push(next);
                    chars.next();
                }
                Token::Word {
                    text,
                    quoted: false,
                    masked,
                }
            }
        };
        tokens.push(token);
    }
    Ok(tokens)
}

/// A quoted term, after its opening `"`: `\` keeps the next character as text.
fn quoted(chars: &mut impl Iterator<Item = char>) -> Result<Token, Diagnostic> {
    let mut text = String::new();
    let mut masked = false;
    loop {
        match chars.next() {
            None => return Err(Diagnostic::bare(10)),
            Some('"') => break,
            Some('\\') => match chars.next() {
                Some(escaped) => text.push(escaped),
                None => return Err(Diagnostic::bare(10)),
            },
            Some(c) => {
                masked |= matches!(c, '*' | '?');
                text.push(c);
            }
        }
    }
    Ok(Token::Word {
        text,
        quoted: true,
        masked,
    })
}

struct Reader<'a> {
    tokens: &'a [Token],
    at: usize,
    clauses: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Token> {
        self.tokens.get(self.at + ahead)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    /// Clauses joined by booleans, left to right.
    fn scoped(&mut self, depth: usize) -> Result<FieldQuery, Diagnostic> {
        let mut query = self.clause(depth)?;
        while let Some(Token::Word {
            text,
            quoted: false,
            ..
        }) = self.peek()
        {
            let boolean = text.to_ascii_lowercase();
            if !matches!(boolean.as_str(), "and" | "or" | "not" | "prox") {
                break;
            }
            self.at += 1;
            if boolean == "prox" || self.peek() == Some(&Token::Slash) {
                return Err(Diagnostic::with(37, &boolean));
            }
            let right = Box::new(self.clause(depth)?);
            let left = Box::new(query);
            query = match boolean.as_str() {
                "and" => FieldQuery::And(left, right),
                "or" => FieldQuery::Or(left, right),
                _ => FieldQuery::Not(left, right),
            };
        }
        Ok(query)
    }

    fn clause(&mut self, depth: usize) -> Result<FieldQuery, Diagnostic> {
        match self.next() {
            Some(Token::Open) => {
                if depth >= MAX_DEPTH {
                    return Err(Diagnostic::bare(10));
                }
                let inner = self.scoped(depth + 1)?;
                match self.next() {
                    Some(Token::Close) => Ok(inner),
                    None => Err(Diagnostic::bare(13)),
                    Some(_) => Err(Diagnostic::bare(10)),
                }
            }
            Some(Token::Close) => Err(Diagnostic::bare(13)),
            Some(Token::Word {
                text,
                quoted,
                masked,
            }) => {
                self.clauses += 1;
                if self.clauses > MAX_CLAUSES {
                    return Err(Diagnostic::bare(10));
                }
                match self.relation()? {
                    Some(relation) if !quoted => self.search_clause(&text, &relation),
                    Some(_) => Err(Diagnostic::bare(10)),
                    None => term(SearchField::Any, WordMatch::All, text, masked),
                }
            }
            Some(Token::Slash | Token::Symbol(_)) | None => Err(Diagnostic::bare(10)),
        }
    }

    /// The relation after an index, if the next tokens are a relation and a term.
    fn relation(&mut self) -> Result<Option<String>, Diagnostic> {
        let relation = match self.peek() {
            Some(Token::Symbol(symbol)) => (*symbol).to_owned(),
            Some(Token::Word {
                text,
                quoted: false,
                ..
            }) if NAMED_RELATIONS.contains(&text.to_ascii_lowercase().as_str())
                && matches!(self.peek_at(1), Some(Token::Word { .. } | Token::Slash)) =>
            {
                text.to_ascii_lowercase()
            }
            _ => return Ok(None),
        };
        self.at += 1;
        if self.peek() == Some(&Token::Slash) {
            self.at += 1;
            let modifier = match self.next() {
                Some(Token::Word { text, .. }) => text,
                _ => String::new(),
            };
            return Err(Diagnostic::with(20, &modifier));
        }
        Ok(Some(relation))
    }

    fn search_clause(&mut self, index: &str, relation: &str) -> Result<FieldQuery, Diagnostic> {
        let field = field_of(index)?;
        let Some(Token::Word { text, masked, .. }) = self.next() else {
            return Err(Diagnostic::bare(10));
        };
        let matching = matching(field, relation).ok_or_else(|| Diagnostic::with(19, relation))?;
        term(field, matching, text, masked)
    }
}

fn term(
    field: SearchField,
    matching: WordMatch,
    text: String,
    masked: bool,
) -> Result<FieldQuery, Diagnostic> {
    if text.trim().is_empty() {
        return Err(Diagnostic::bare(27));
    }
    if masked {
        return Err(Diagnostic::bare(28));
    }
    Ok(FieldQuery::Term(FieldTerm {
        field,
        matching,
        text,
    }))
}

/// The field of a CQL index (item 9): names compare without case, and an index without
/// a set is in `dc`.
fn field_of(index: &str) -> Result<SearchField, Diagnostic> {
    let (set, name) = index.split_once('.').unwrap_or(("dc", index));
    if let Some((_, _, field)) = INDEXES.iter().find(|(known_set, known_name, _)| {
        known_set.eq_ignore_ascii_case(set) && known_name.eq_ignore_ascii_case(name)
    }) {
        return Ok(*field);
    }
    if CONTEXT_SETS
        .iter()
        .any(|(known, _)| known.eq_ignore_ascii_case(set))
    {
        Err(Diagnostic::with(16, index))
    } else {
        Err(Diagnostic::with(15, set))
    }
}

/// How `relation` matches on `field` (item 10); `None` when it is not supported there.
fn matching(field: SearchField, relation: &str) -> Option<WordMatch> {
    match (field, relation) {
        (SearchField::Any, "=" | "all" | "adj" | "==" | "exact") => Some(WordMatch::All),
        (SearchField::Any, "any") => Some(WordMatch::Any),
        (SearchField::Any, _) => None,
        (field, relation) if field.is_words() => match relation {
            "=" | "all" => Some(WordMatch::All),
            "any" => Some(WordMatch::Any),
            "adj" | "==" | "exact" => Some(WordMatch::Phrase),
            _ => None,
        },
        (_, "=" | "==" | "exact") => Some(WordMatch::Phrase),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term_of(field: SearchField, matching: WordMatch, text: &str) -> FieldQuery {
        FieldQuery::Term(FieldTerm {
            field,
            matching,
            text: text.to_owned(),
        })
    }

    fn code(query: &str) -> (u16, Option<String>) {
        let diagnostic = parse(query).unwrap_err();
        (diagnostic.code, diagnostic.details)
    }

    #[test]
    fn indexes_relations_and_booleans() {
        assert_eq!(
            parse("dc.title=\"O rio\" and dc.creator=\"Teodora Lemos\"").unwrap(),
            FieldQuery::And(
                Box::new(term_of(SearchField::Title, WordMatch::All, "O rio")),
                Box::new(term_of(
                    SearchField::Contributor,
                    WordMatch::All,
                    "Teodora Lemos"
                )),
            )
        );
        assert_eq!(
            parse("TITLE ADJ \"rio claro\"").unwrap(),
            term_of(SearchField::Title, WordMatch::Phrase, "rio claro")
        );
        assert_eq!(
            parse("bath.isbn == 978-0-00-000000-2").unwrap(),
            term_of(
                SearchField::Identifier,
                WordMatch::Phrase,
                "978-0-00-000000-2"
            )
        );
        assert_eq!(
            parse("cat or (dog not \"hot \\\"dog\\\"\")").unwrap(),
            FieldQuery::Or(
                Box::new(term_of(SearchField::Any, WordMatch::All, "cat")),
                Box::new(FieldQuery::Not(
                    Box::new(term_of(SearchField::Any, WordMatch::All, "dog")),
                    Box::new(term_of(SearchField::Any, WordMatch::All, "hot \"dog\"")),
                )),
            )
        );
        assert_eq!(
            parse("a and b or c").unwrap(),
            FieldQuery::Or(
                Box::new(FieldQuery::And(
                    Box::new(term_of(SearchField::Any, WordMatch::All, "a")),
                    Box::new(term_of(SearchField::Any, WordMatch::All, "b")),
                )),
                Box::new(term_of(SearchField::Any, WordMatch::All, "c")),
            )
        );
        assert_eq!(
            parse("subject any \"rios lagos\"").unwrap(),
            term_of(SearchField::Subject, WordMatch::Any, "rios lagos")
        );
        // A named relation word with nothing after it is a term.
        assert_eq!(
            parse("any").unwrap(),
            term_of(SearchField::Any, WordMatch::All, "any")
        );
        assert_eq!(
            parse("rec.id=12").unwrap(),
            term_of(SearchField::Record, WordMatch::Phrase, "12")
        );
    }

    #[test]
    fn each_refusal_has_its_diagnostic() {
        assert_eq!(code("dc.shelf=x"), (16, Some("dc.shelf".to_owned())));
        assert_eq!(code("marc.245=x"), (15, Some("marc".to_owned())));
        assert_eq!(
            code("koha.itemtype=x"),
            (16, Some("koha.itemtype".to_owned()))
        );
        assert_eq!(code("dc.date < 2000"), (19, Some("<".to_owned())));
        assert_eq!(code("dc.isbn any 1"), (19, Some("any".to_owned())));
        assert_eq!(code("dc.title =/stem rio"), (20, Some("stem".to_owned())));
        assert_eq!(code("a prox b"), (37, Some("prox".to_owned())));
        assert_eq!(
            code("a and/rel.combine=sum b"),
            (37, Some("and".to_owned()))
        );
        assert_eq!(code("dc.title=\"\""), (27, None));
        assert_eq!(code("dc.title=rio*"), (28, None));
        assert_eq!(code("\"ri?o\""), (28, None));
        assert_eq!(code("(a and b"), (13, None));
        assert_eq!(code("a and b)"), (13, None));
        assert_eq!(code("a sortby dc.title"), (80, None));
        assert_eq!(
            code(">dc=\"info:srw/cql-context-set/1/dc-v1.1\" title=x"),
            (10, None)
        );
        assert_eq!(code("a b"), (10, None));
        assert_eq!(code("\"open"), (10, None));
        assert_eq!(code("dc.title="), (10, None));
        assert_eq!(code(""), (10, None));
        // An escaped mask is text.
        assert!(parse("dc.title=\"rio\\*\"").is_ok());
    }

    #[test]
    fn limits_hold() {
        let many = vec!["a"; MAX_CLAUSES + 1].join(" and ");
        assert_eq!(code(&many), (10, None));
        assert!(parse(&vec!["a"; MAX_CLAUSES].join(" and ")).is_ok());
        let deep = format!(
            "{}a{}",
            "(".repeat(MAX_DEPTH + 1),
            ")".repeat(MAX_DEPTH + 1)
        );
        assert_eq!(code(&deep), (10, None));
        let fits = format!("{}a{}", "(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH));
        assert!(parse(&fits).is_ok());
    }
}
