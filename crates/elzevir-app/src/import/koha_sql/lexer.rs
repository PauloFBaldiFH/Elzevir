// SPDX-License-Identifier: GPL-3.0-or-later
//! The fixed SQL dump grammar consumes ignored literals without collecting them.

use std::collections::VecDeque;
use std::io::{BufRead, Bytes};

use super::{DumpError, DumpProblem, MAX_SQL_DDL_DEPTH, MAX_SQL_IDENTIFIER_BYTES};
use super::{MAX_SQL_STATEMENT_BYTES, MAX_SQL_VALUE_BYTES};
use crate::import::preview::MAX_IMPORT_FILE_BYTES;

pub(super) struct Lexer<R> {
    input: Bytes<R>,
    ahead: VecDeque<u8>,
    pub(super) offset: u64,
    pub(super) statement: Option<u64>,
}

impl<R: BufRead> Lexer<R> {
    pub(super) fn new(input: R) -> Self {
        Self {
            input: input.bytes(),
            ahead: VecDeque::with_capacity(3),
            offset: 0,
            statement: None,
        }
    }

    pub(super) fn error(&self, problem: DumpProblem) -> DumpError {
        DumpError {
            offset: self.offset,
            problem,
        }
    }

    pub(super) fn syntax(&self, expected: &'static str) -> DumpError {
        self.error(DumpProblem::Syntax(expected))
    }

    fn peek(&mut self, position: usize) -> Result<Option<u8>, DumpError> {
        while self.ahead.len() <= position {
            match self.input.next() {
                Some(Ok(byte)) => self.ahead.push_back(byte),
                Some(Err(_)) => return Err(self.error(DumpProblem::Read)),
                None => return Ok(None),
            }
        }
        Ok(self.ahead.get(position).copied())
    }

    fn take(&mut self) -> Result<Option<u8>, DumpError> {
        if self.peek(0)?.is_none() {
            return Ok(None);
        }
        if self.offset == MAX_IMPORT_FILE_BYTES as u64 {
            return Err(self.error(DumpProblem::Limit("input bytes")));
        }
        if self
            .statement
            .is_some_and(|start| self.offset - start == MAX_SQL_STATEMENT_BYTES)
        {
            return Err(self.error(DumpProblem::Limit("statement bytes")));
        }
        self.offset += 1;
        Ok(self.ahead.pop_front())
    }

    pub(super) fn bom(&mut self) -> Result<(), DumpError> {
        if self.peek(0)? == Some(0xef) && self.peek(1)? == Some(0xbb) && self.peek(2)? == Some(0xbf)
        {
            self.take()?;
            self.take()?;
            self.take()?;
        }
        Ok(())
    }

    pub(super) fn head(&mut self) -> Result<Option<u8>, DumpError> {
        loop {
            match self.peek(0)? {
                Some(byte) if byte.is_ascii_whitespace() => {
                    self.take()?;
                }
                Some(b'#') => self.line_comment()?,
                Some(b'-')
                    if self.peek(1)? == Some(b'-')
                        && self.peek(2)?.is_none_or(|byte| {
                            byte.is_ascii_whitespace() || byte.is_ascii_control()
                        }) =>
                {
                    self.line_comment()?;
                }
                Some(b'/') if self.peek(1)? == Some(b'*') => self.block_comment()?,
                next => return Ok(next),
            }
        }
    }

    fn line_comment(&mut self) -> Result<(), DumpError> {
        while self
            .take()?
            .is_some_and(|byte| !matches!(byte, b'\n' | b'\r'))
        {}
        Ok(())
    }

    fn block_comment(&mut self) -> Result<(), DumpError> {
        self.take()?;
        self.take()?;
        loop {
            match self.take()? {
                Some(b'*') if self.peek(0)? == Some(b'/') => {
                    self.take()?;
                    return Ok(());
                }
                Some(_) => {}
                None => return Err(self.syntax("unterminated comment")),
            }
        }
    }

    pub(super) fn accept(&mut self, byte: u8) -> Result<bool, DumpError> {
        if self.head()? == Some(byte) {
            self.take()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(super) fn expect(&mut self, byte: u8, expected: &'static str) -> Result<(), DumpError> {
        self.accept(byte)?
            .then_some(())
            .ok_or_else(|| self.syntax(expected))
    }

    pub(super) fn name(&mut self, identifiers: bool) -> Result<String, DumpError> {
        let quoted = identifiers && self.accept(b'`')?;
        if !quoted {
            self.head()?;
        }
        let mut name = String::new();
        while let Some(byte) = self
            .peek(0)?
            .filter(|byte| byte.is_ascii_alphanumeric() || b"_$".contains(byte))
        {
            if name.len() == MAX_SQL_IDENTIFIER_BYTES {
                return Err(self.error(DumpProblem::Limit("identifier bytes")));
            }
            self.take()?;
            name.push(char::from(byte));
        }
        if name.is_empty() || quoted && self.take()? != Some(b'`') {
            return Err(self.syntax("expected an ASCII SQL identifier"));
        }
        Ok(name)
    }

    pub(super) fn keyword(&mut self) -> Result<String, DumpError> {
        let mut word = self.name(false)?;
        word.make_ascii_lowercase();
        Ok(word)
    }

    pub(super) fn word(&mut self, word: &'static str) -> Result<(), DumpError> {
        if !self.name(false)?.eq_ignore_ascii_case(word) {
            return Err(self.syntax(word));
        }
        Ok(())
    }

    pub(super) fn literal(&mut self, keep: bool) -> Result<Option<Vec<u8>>, DumpError> {
        let mut value = keep.then(Vec::new);
        match self.head()? {
            Some(quote @ (b'\'' | b'"')) => self.quoted(quote, &mut value)?,
            Some(b'n' | b'N') => {
                // A fixed word keeps arbitrary bytes from ignored values out of errors and memory.
                for letter in b"NULL" {
                    if !self
                        .take()?
                        .is_some_and(|byte| byte.eq_ignore_ascii_case(letter))
                    {
                        return Err(self.syntax("expected NULL"));
                    }
                }
                return Ok(None);
            }
            Some(b'0'..=b'9' | b'+' | b'-') => self.decimal(&mut value)?,
            _ => return Err(self.syntax("expected a literal SQL value")),
        }
        Ok(value)
    }

    fn decimal(&mut self, value: &mut Option<Vec<u8>>) -> Result<(), DumpError> {
        if let Some(sign @ (b'+' | b'-')) = self.peek(0)? {
            self.take()?;
            self.collect(value, sign)?;
        }
        self.digits(value)?;
        if self.peek(0)? == Some(b'.') {
            self.take()?;
            self.collect(value, b'.')?;
            self.digits(value)?;
        }
        Ok(())
    }

    fn digits(&mut self, value: &mut Option<Vec<u8>>) -> Result<(), DumpError> {
        if !self.peek(0)?.is_some_and(|byte| byte.is_ascii_digit()) {
            return Err(self.syntax("expected decimal digits"));
        }
        while let Some(byte @ b'0'..=b'9') = self.peek(0)? {
            self.take()?;
            self.collect(value, byte)?;
        }
        Ok(())
    }

    fn quoted(&mut self, quote: u8, value: &mut Option<Vec<u8>>) -> Result<(), DumpError> {
        self.take()?;
        loop {
            let byte = match self.take()? {
                None => return Err(self.syntax("unterminated SQL quote")),
                Some(byte) if byte == quote => {
                    if self.peek(0)? != Some(quote) {
                        return Ok(());
                    }
                    self.take()?;
                    quote
                }
                Some(b'\\') if quote != b'`' => self.escape(value)?,
                Some(byte) => byte,
            };
            self.collect(value, byte)?;
        }
    }

    fn escape(&mut self, value: &mut Option<Vec<u8>>) -> Result<u8, DumpError> {
        let byte = match self.take()? {
            Some(b'0') => 0,
            Some(b'b') => 8,
            Some(b'n') => b'\n',
            Some(b'r') => b'\r',
            Some(b't') => b'\t',
            Some(b'Z') => 26,
            Some(byte @ (b'\\' | b'\'' | b'"')) => byte,
            Some(byte @ (b'%' | b'_')) => {
                self.collect(value, b'\\')?;
                byte
            }
            _ => return Err(self.syntax("unsupported MySQL escape")),
        };
        Ok(byte)
    }

    fn collect(&self, value: &mut Option<Vec<u8>>, byte: u8) -> Result<(), DumpError> {
        if let Some(value) = value {
            if value.len() == MAX_SQL_VALUE_BYTES {
                return Err(self.error(DumpProblem::Limit("retained value bytes")));
            }
            value.push(byte);
        }
        Ok(())
    }

    pub(super) fn definition_end(&mut self) -> Result<(), DumpError> {
        let mut depth = 0;
        loop {
            match self.head()? {
                Some(b',' | b')') if depth == 0 => return Ok(()),
                Some(quote @ (b'\'' | b'"' | b'`')) => self.quoted(quote, &mut None)?,
                Some(b'(') => {
                    depth += 1;
                    if depth > MAX_SQL_DDL_DEPTH {
                        return Err(self.error(DumpProblem::Limit("DDL depth")));
                    }
                    self.take()?;
                }
                Some(b')') => {
                    depth -= 1;
                    self.take()?;
                }
                None | Some(b';') => return Err(self.syntax("unterminated table definition")),
                Some(_) => {
                    self.take()?;
                }
            }
        }
    }
}
