// SPDX-License-Identifier: GPL-3.0-or-later
//! Inspect SQL declarations without exposing or interpreting row values.

use super::{Detection, Evidence, ImportFormat};

/// Returns `None` only when the prefix does not begin as SQL.
pub(super) fn detect(input: &[u8]) -> Option<Detection> {
    let mut tokens = Tokens(input);
    let first = tokens.next()?;
    if ![
        b"CREATE".as_slice(),
        b"INSERT",
        b"SET",
        b"DROP",
        b"LOCK",
        b"UNLOCK",
        b"USE",
        b"ALTER",
    ]
    .iter()
    .any(|keyword| first.is(keyword))
    {
        return None;
    }
    let mut tokens = Tokens(input);
    let mut tables = 0u8;
    while let Some(token) = tokens.next() {
        let clause = if token.is(b"CREATE") {
            b"TABLE".as_slice()
        } else if token.is(b"INSERT") {
            b"INTO"
        } else {
            continue;
        };
        if !tokens.next().is_some_and(|token| token.is(clause)) {
            continue;
        }
        let Some(name) = table_name(&mut tokens) else {
            continue;
        };
        for (bit, table) in [
            b"biblio_metadata".as_slice(),
            b"items",
            b"borrowers",
            b"issues",
            b"systempreferences",
        ]
        .iter()
        .enumerate()
        {
            if name.eq_ignore_ascii_case(table) {
                tables |= 1 << bit;
            }
        }
    }
    let (format, evidence) = if tables == 0b1_1111 {
        (ImportFormat::KohaFullBackup, Evidence::KohaSchemaTables)
    } else if tables & 0b11 != 0 {
        (ImportFormat::KohaSqlDump, Evidence::KohaCatalogTables)
    } else {
        (ImportFormat::Unknown, Evidence::Inconclusive)
    };
    Some(Detection { format, evidence })
}

fn table_name<'a>(tokens: &mut Tokens<'a>) -> Option<&'a [u8]> {
    let mut token = tokens.next()?;
    if token.is(b"IF") {
        if !tokens.next()?.is(b"NOT") || !tokens.next()?.is(b"EXISTS") {
            return None;
        }
        token = tokens.next()?;
    }
    let mut name = token.name()?;
    loop {
        let saved = tokens.0;
        if !matches!(tokens.next(), Some(Token::Symbol(b'.'))) {
            tokens.0 = saved;
            return Some(name);
        }
        name = tokens.next()?.name()?;
    }
}

enum Token<'a> {
    Word(&'a [u8]),
    Quoted(&'a [u8]),
    Symbol(u8),
}

impl<'a> Token<'a> {
    fn is(&self, word: &[u8]) -> bool {
        matches!(self, Self::Word(value) if value.eq_ignore_ascii_case(word))
    }

    fn name(self) -> Option<&'a [u8]> {
        match self {
            Self::Word(name) | Self::Quoted(name) => Some(name),
            Self::Symbol(_) => None,
        }
    }
}

struct Tokens<'a>(&'a [u8]);

impl<'a> Tokens<'a> {
    fn quoted(&mut self, quote: u8) -> Option<&'a [u8]> {
        let mut end = 0;
        while let Some(byte) = self.0.get(end) {
            if *byte == b'\\' || (*byte == quote && self.0.get(end + 1) == Some(&quote)) {
                end += 2;
            } else if *byte == quote {
                let value = self.0.get(..end)?;
                self.0 = self.0.get(end + 1..)?;
                return Some(value);
            } else {
                end += 1;
            }
        }
        self.0 = b"";
        None
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.0 = self.0.trim_ascii_start();
            if self.0.starts_with(b"--") || self.0.starts_with(b"#") {
                let end = self.0.iter().position(|byte| *byte == b'\n')?;
                self.0 = self.0.get(end + 1..)?;
            } else if let Some(comment) = self.0.strip_prefix(b"/*") {
                let end = comment.windows(2).position(|pair| pair == b"*/")?;
                self.0 = comment.get(end + 2..)?;
            } else {
                break;
            }
        }
        let (&byte, rest) = self.0.split_first()?;
        if matches!(byte, b'\'' | b'"' | b'`') {
            self.0 = rest;
            let value = self.quoted(byte)?;
            return Some(if byte == b'\'' {
                Token::Symbol(byte)
            } else {
                Token::Quoted(value)
            });
        }
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            let end = self
                .0
                .iter()
                .position(|byte| !(byte.is_ascii_alphanumeric() || b"_$".contains(byte)))
                .unwrap_or(self.0.len());
            let word = self.0.get(..end)?;
            self.0 = self.0.get(end..)?;
            return Some(Token::Word(word));
        }
        self.0 = rest;
        Some(Token::Symbol(byte))
    }
}
