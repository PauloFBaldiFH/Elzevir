// SPDX-License-Identifier: GPL-3.0-or-later
//! Just enough BER (X.690) for Z39.50, with the limits of docs/specs/catalogo/cliente-z3950.md
//! (items 1 to 3): definite lengths only, bounded size, depth, tag numbers and integers.

/// Largest message read or written: the same bound as an SRU response.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
/// Deepest nesting read.
const MAX_DEPTH: usize = 32;
/// Longest integer read, in bytes.
const MAX_INTEGER_BYTES: usize = 8;
/// Most bytes of a high tag number (7 bits each).
const MAX_TAG_BYTES: usize = 4;

/// Why bytes are not a readable BER element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BerError {
    /// The bytes end before the element does.
    #[error("truncated element")]
    Truncated,
    /// An indefinite length, which Z39.50 servers do not send.
    #[error("indefinite length")]
    Indefinite,
    /// A length or a message over [`MAX_MESSAGE_BYTES`].
    #[error("element too large")]
    TooLarge,
    /// Nesting deeper than the limit.
    #[error("nesting too deep")]
    TooDeep,
    /// A tag number or an integer longer than the limit.
    #[error("number too long")]
    NumberTooLong,
    /// Content that does not fit the type read.
    #[error("malformed content")]
    Malformed,
}

/// The class of a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Universal types (`INTEGER`, `SEQUENCE`).
    Universal,
    /// Application tags.
    Application,
    /// Context-specific tags, which Z39.50 uses for almost everything.
    Context,
    /// Private tags.
    Private,
}

impl Class {
    const fn bits(self) -> u8 {
        match self {
            Self::Universal => 0x00,
            Self::Application => 0x40,
            Self::Context => 0x80,
            Self::Private => 0xC0,
        }
    }

    const fn of(byte: u8) -> Self {
        match byte & 0xC0 {
            0x00 => Self::Universal,
            0x40 => Self::Application,
            0x80 => Self::Context,
            _ => Self::Private,
        }
    }
}

/// Universal tag of `BOOLEAN`.
pub const BOOLEAN: u32 = 1;
/// Universal tag of `INTEGER`.
pub const INTEGER: u32 = 2;
/// Universal tag of `OCTET STRING`.
pub const OCTET_STRING: u32 = 4;
/// Universal tag of `OBJECT IDENTIFIER`.
pub const OBJECT_IDENTIFIER: u32 = 6;
/// Universal tag of `EXTERNAL`.
pub const EXTERNAL: u32 = 8;
/// Universal tag of `SEQUENCE`.
pub const SEQUENCE: u32 = 16;

/// One element read from a message, borrowing its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element<'a> {
    /// The tag's class.
    pub class: Class,
    /// Whether the content is made of elements.
    pub constructed: bool,
    /// The tag number.
    pub number: u32,
    /// The content octets.
    pub content: &'a [u8],
    depth: usize,
}

impl<'a> Element<'a> {
    /// Whether the element has this context-specific tag.
    #[must_use]
    pub const fn is_context(&self, number: u32) -> bool {
        matches!(self.class, Class::Context) && self.number == number
    }

    /// Whether the element has this universal tag.
    #[must_use]
    pub const fn is_universal(&self, number: u32) -> bool {
        matches!(self.class, Class::Universal) && self.number == number
    }

    /// The elements inside a constructed element, in order.
    ///
    /// # Errors
    ///
    /// When the element is primitive, too deep, or its content is not whole elements.
    pub fn children(&self) -> Result<Vec<Element<'a>>, BerError> {
        if !self.constructed {
            return Err(BerError::Malformed);
        }
        let depth = self.depth + 1;
        if depth > MAX_DEPTH {
            return Err(BerError::TooDeep);
        }
        let mut rest = self.content;
        let mut children = Vec::new();
        while !rest.is_empty() {
            let (child, after) = read_at(rest, depth)?;
            children.push(child);
            rest = after;
        }
        Ok(children)
    }

    /// The only element inside an explicitly tagged element.
    ///
    /// # Errors
    ///
    /// When the element does not hold exactly one element.
    pub fn inner(&self) -> Result<Element<'a>, BerError> {
        let mut children = self.children()?.into_iter();
        match (children.next(), children.next()) {
            (Some(only), None) => Ok(only),
            _ => Err(BerError::Malformed),
        }
    }

    /// The content as a signed integer.
    ///
    /// # Errors
    ///
    /// When the content is empty, constructed or longer than eight bytes.
    pub fn integer(&self) -> Result<i64, BerError> {
        if self.constructed || self.content.is_empty() {
            return Err(BerError::Malformed);
        }
        if self.content.len() > MAX_INTEGER_BYTES {
            return Err(BerError::NumberTooLong);
        }
        let negative = self.content.first().is_some_and(|first| first & 0x80 != 0);
        let mut value: i64 = if negative { -1 } else { 0 };
        for byte in self.content {
            value = (value << 8) | i64::from(*byte);
        }
        Ok(value)
    }

    /// The content as a boolean: any non-zero byte is true.
    ///
    /// # Errors
    ///
    /// When the content is not one primitive byte.
    pub fn boolean(&self) -> Result<bool, BerError> {
        match (self.constructed, self.content) {
            (false, [byte]) => Ok(*byte != 0),
            _ => Err(BerError::Malformed),
        }
    }

    /// The content as an object identifier's arcs.
    ///
    /// # Errors
    ///
    /// When the content is constructed, empty, or an arc does not end or overflows.
    pub fn oid(&self) -> Result<Vec<u32>, BerError> {
        if self.constructed || self.content.is_empty() {
            return Err(BerError::Malformed);
        }
        let mut arcs = Vec::new();
        let mut value: u32 = 0;
        let mut pending = false;
        for byte in self.content {
            if value > (u32::MAX >> 7) {
                return Err(BerError::NumberTooLong);
            }
            value = (value << 7) | u32::from(byte & 0x7F);
            pending = byte & 0x80 != 0;
            if !pending {
                if arcs.is_empty() {
                    let first = (value / 40).min(2);
                    arcs.push(first);
                    arcs.push(value - first * 40);
                } else {
                    arcs.push(value);
                }
                value = 0;
            }
        }
        if pending {
            return Err(BerError::Truncated);
        }
        Ok(arcs)
    }

    /// The content as text: UTF-8 where it is, with replacement characters elsewhere.
    ///
    /// # Errors
    ///
    /// When the element is constructed.
    pub fn text(&self) -> Result<String, BerError> {
        if self.constructed {
            return Err(BerError::Malformed);
        }
        Ok(String::from_utf8_lossy(self.content).into_owned())
    }
}

/// Reads the first element of `input`, and what follows it.
///
/// # Errors
///
/// When `input` does not start with a whole, bounded element.
pub fn read(input: &[u8]) -> Result<(Element<'_>, &[u8]), BerError> {
    if input.len() > MAX_MESSAGE_BYTES {
        return Err(BerError::TooLarge);
    }
    read_at(input, 0)
}

/// The full length of the element `input` starts with, once its header has arrived:
/// `None` while more bytes are needed to know it. Lets a stream be cut into messages.
///
/// # Errors
///
/// When the header is invalid, indefinite, or announces more than [`MAX_MESSAGE_BYTES`].
pub fn frame_length(input: &[u8]) -> Result<Option<usize>, BerError> {
    match header(input) {
        Ok((_, _, _, header_length, content_length)) => {
            let total = header_length
                .checked_add(content_length)
                .ok_or(BerError::TooLarge)?;
            if total > MAX_MESSAGE_BYTES {
                return Err(BerError::TooLarge);
            }
            Ok(Some(total))
        }
        Err(BerError::Truncated) => Ok(None),
        Err(error) => Err(error),
    }
}

fn read_at(input: &[u8], depth: usize) -> Result<(Element<'_>, &[u8]), BerError> {
    let (class, constructed, number, header_length, content_length) = header(input)?;
    let end = header_length
        .checked_add(content_length)
        .ok_or(BerError::TooLarge)?;
    let content = input.get(header_length..end).ok_or(BerError::Truncated)?;
    let rest = input.get(end..).ok_or(BerError::Truncated)?;
    Ok((
        Element {
            class,
            constructed,
            number,
            content,
            depth,
        },
        rest,
    ))
}

/// Class, constructed bit, tag number, header length and content length.
fn header(input: &[u8]) -> Result<(Class, bool, u32, usize, usize), BerError> {
    let first = *input.first().ok_or(BerError::Truncated)?;
    let class = Class::of(first);
    let constructed = first & 0x20 != 0;
    let mut at = 1;
    let mut number = u32::from(first & 0x1F);
    if number == 0x1F {
        number = 0;
        loop {
            let byte = *input.get(at).ok_or(BerError::Truncated)?;
            at += 1;
            if at - 1 > MAX_TAG_BYTES {
                return Err(BerError::NumberTooLong);
            }
            number = (number << 7) | u32::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                break;
            }
        }
    }
    let first_length = *input.get(at).ok_or(BerError::Truncated)?;
    at += 1;
    let length = if first_length & 0x80 == 0 {
        usize::from(first_length)
    } else {
        let count = usize::from(first_length & 0x7F);
        if count == 0 {
            return Err(BerError::Indefinite);
        }
        if count > 4 {
            return Err(BerError::TooLarge);
        }
        let mut length: usize = 0;
        for _ in 0..count {
            let byte = *input.get(at).ok_or(BerError::Truncated)?;
            at += 1;
            length = (length << 8) | usize::from(byte);
        }
        length
    };
    if length > MAX_MESSAGE_BYTES {
        return Err(BerError::TooLarge);
    }
    Ok((class, constructed, number, at, length))
}

/// Encodes one element with `content`.
#[must_use]
pub fn element(class: Class, constructed: bool, number: u32, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 8);
    let first = class.bits() | if constructed { 0x20 } else { 0 };
    if number < 0x1F {
        // A low tag number fits in the identifier byte; the guard proves the cast.
        out.push(first | u8::try_from(number).unwrap_or(0x1F));
    } else {
        out.push(first | 0x1F);
        out.extend(base128(number));
    }
    let length = content.len();
    if length < 0x80 {
        out.push(u8::try_from(length).unwrap_or(0));
    } else {
        let bytes = length.to_be_bytes();
        let skip = bytes.iter().take_while(|byte| **byte == 0).count();
        let significant = bytes.get(skip..).unwrap_or_default();
        out.push(0x80 | u8::try_from(significant.len()).unwrap_or(0));
        out.extend_from_slice(significant);
    }
    out.extend_from_slice(content);
    out
}

/// A context-specific primitive element.
#[must_use]
pub fn context(number: u32, content: &[u8]) -> Vec<u8> {
    element(Class::Context, false, number, content)
}

/// A context-specific constructed element holding `parts`, in order.
#[must_use]
pub fn context_of(number: u32, parts: &[Vec<u8>]) -> Vec<u8> {
    element(Class::Context, true, number, &parts.concat())
}

/// A `SEQUENCE` of `parts`.
#[must_use]
pub fn sequence(parts: &[Vec<u8>]) -> Vec<u8> {
    element(Class::Universal, true, SEQUENCE, &parts.concat())
}

/// The minimal two's-complement content of `value`.
#[must_use]
pub fn integer_content(value: i64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let mut start = 0;
    while start + 1 < bytes.len() {
        let (Some(this), Some(next)) = (bytes.get(start), bytes.get(start + 1)) else {
            break;
        };
        let redundant = (*this == 0x00 && next & 0x80 == 0) || (*this == 0xFF && next & 0x80 != 0);
        if !redundant {
            break;
        }
        start += 1;
    }
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// A context-specific `[number] IMPLICIT INTEGER`.
#[must_use]
pub fn context_integer(number: u32, value: i64) -> Vec<u8> {
    context(number, &integer_content(value))
}

/// The content of an object identifier.
#[must_use]
pub fn oid_content(arcs: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut arcs = arcs.iter();
    let first = arcs.next().copied().unwrap_or(0);
    let second = arcs.next().copied().unwrap_or(0);
    out.extend(base128(first.saturating_mul(40).saturating_add(second)));
    for arc in arcs {
        out.extend(base128(*arc));
    }
    out
}

/// A universal `OBJECT IDENTIFIER`.
#[must_use]
pub fn oid(arcs: &[u32]) -> Vec<u8> {
    element(
        Class::Universal,
        false,
        OBJECT_IDENTIFIER,
        &oid_content(arcs),
    )
}

fn base128(value: u32) -> Vec<u8> {
    let mut groups = vec![u8::try_from(value & 0x7F).unwrap_or(0)];
    let mut rest = value >> 7;
    while rest > 0 {
        groups.push(0x80 | u8::try_from(rest & 0x7F).unwrap_or(0));
        rest >>= 7;
    }
    groups.reverse();
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_and_oids_round_trip_in_their_minimal_form() {
        for (value, content) in [
            (0, vec![0x00]),
            (127, vec![0x7F]),
            (128, vec![0x00, 0x80]),
            (-1, vec![0xFF]),
            (-129, vec![0xFF, 0x7F]),
            (8 * 1024 * 1024, vec![0x00, 0x80, 0x00, 0x00]),
        ] {
            assert_eq!(integer_content(value), content, "{value}");
            let encoded = context_integer(23, value);
            let (element, rest) = read(&encoded).unwrap();
            assert!(rest.is_empty());
            assert!(element.is_context(23));
            assert_eq!(element.integer().unwrap(), value);
        }
        let usmarc = [1, 2, 840, 10003, 5, 10];
        let encoded = oid(&usmarc);
        assert_eq!(
            encoded,
            [0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x13, 0x05, 0x0A]
        );
        assert_eq!(read(&encoded).unwrap().0.oid().unwrap(), usmarc);
    }

    #[test]
    fn high_tags_and_long_lengths_are_read_back() {
        let content = vec![b'x'; 300];
        let encoded = context(211, &content);
        assert_eq!(encoded.get(..5), Some(&[0x9F, 0x81, 0x53, 0x82, 0x01][..]));
        let (element, _) = read(&encoded).unwrap();
        assert!(element.is_context(211));
        assert_eq!(element.content.len(), 300);
        assert_eq!(frame_length(&encoded).unwrap(), Some(encoded.len()));
        assert_eq!(frame_length(encoded.get(..3).unwrap()).unwrap(), None);
    }

    #[test]
    fn what_breaks_the_limits_is_refused() {
        assert_eq!(read(&[0x30, 0x80, 0x00, 0x00]), Err(BerError::Indefinite));
        assert_eq!(read(&[0x04, 0x05, b'a']), Err(BerError::Truncated));
        assert_eq!(
            read(&[0x04, 0x84, 0x7F, 0xFF, 0xFF, 0xFF]),
            Err(BerError::TooLarge)
        );
        assert_eq!(
            read(&[0x9F, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x00]),
            Err(BerError::NumberTooLong)
        );
        let long = context(1, &[1; 9]);
        assert_eq!(
            read(&long).unwrap().0.integer(),
            Err(BerError::NumberTooLong)
        );
        // 40 nested sequences: the reader stops at 32.
        let mut nested = sequence(&[]);
        for _ in 0..40 {
            nested = sequence(&[nested]);
        }
        let mut element = read(&nested).unwrap().0;
        let deepest = loop {
            match element.inner() {
                Ok(inner) => element = inner,
                Err(error) => break error,
            }
        };
        assert_eq!(deepest, BerError::TooDeep);
    }
}
