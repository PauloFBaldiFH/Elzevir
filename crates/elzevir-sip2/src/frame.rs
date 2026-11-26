// SPDX-License-Identifier: GPL-3.0-or-later
//! Messages out of the byte stream (spec items 1 and 2).

/// Most bytes of a message before its CR.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024;

/// A message longer than [`MAX_MESSAGE_BYTES`]: the connection is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("SIP2 message longer than {MAX_MESSAGE_BYTES} bytes")]
pub struct FrameTooLong;

/// Collects the bytes a terminal sends and gives back each message ended by CR.
#[derive(Debug, Default)]
pub struct Framer {
    buffer: Vec<u8>,
}

impl Framer {
    /// Adds bytes read from the connection.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next whole message, cleaned by [`clean`]; `None` until a CR arrives.
    ///
    /// # Errors
    ///
    /// When a message, or the bytes waiting for a CR, pass [`MAX_MESSAGE_BYTES`].
    pub fn next_message(&mut self) -> Option<Result<Vec<u8>, FrameTooLong>> {
        let Some(end) = self.buffer.iter().position(|&byte| byte == b'\r') else {
            return (self.buffer.len() > MAX_MESSAGE_BYTES).then_some(Err(FrameTooLong));
        };
        if end > MAX_MESSAGE_BYTES {
            return Some(Err(FrameTooLong));
        }
        let message: Vec<u8> = self.buffer.drain(..=end).collect();
        let raw = message.strip_suffix(b"\r").unwrap_or(&message);
        Some(Ok(clean(raw)))
    }
}

/// `raw` without the junk terminals leave around a message: leading bytes that are
/// not ASCII letters (`A` to `z`) or digits, trailing control bytes and spaces, and
/// line feeds anywhere.
///
/// Koha: `C4::SIP::SIPServer::read_request`.
#[must_use]
pub fn clean(raw: &[u8]) -> Vec<u8> {
    let start = raw
        .iter()
        .position(|&byte| byte.is_ascii_digit() || (b'A'..=b'z').contains(&byte))
        .unwrap_or(raw.len());
    let rest = raw.get(start..).unwrap_or_default();
    let end = rest
        .iter()
        .rposition(|&byte| byte > b' ' && byte != 0x7f)
        .map_or(0, |last| last + 1);
    rest.get(..end)
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|&byte| byte != b'\n')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_end_in_cr_and_lose_the_junk_around_them() {
        let mut framer = Framer::default();
        framer.push(b"\n9300CNterm|COpw|\r\n  990");
        assert_eq!(
            framer.next_message(),
            Some(Ok(b"9300CNterm|COpw|".to_vec()))
        );
        assert_eq!(framer.next_message(), None);
        framer.push(b"302.00 \t\r");
        assert_eq!(framer.next_message(), Some(Ok(b"990302.00".to_vec())));
        assert_eq!(
            clean(b"\x02\xc3 23001\n20240903    134450AOx|\x00"),
            b"2300120240903    134450AOx|"
        );
        assert_eq!(clean(b" \t\x03"), b"");
    }

    #[test]
    fn a_message_too_long_is_refused() {
        let mut framer = Framer::default();
        framer.push(&vec![b'a'; MAX_MESSAGE_BYTES + 1]);
        assert_eq!(framer.next_message(), Some(Err(FrameTooLong)));
        let mut framer = Framer::default();
        framer.push(&vec![b'a'; MAX_MESSAGE_BYTES]);
        framer.push(b"\r");
        assert_eq!(
            framer
                .next_message()
                .map(|message| message.map(|m| m.len())),
            Some(Ok(MAX_MESSAGE_BYTES))
        );
    }
}
