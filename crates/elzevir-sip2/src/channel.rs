// SPDX-License-Identifier: GPL-3.0-or-later
//! What a connection remembers between messages: the protocol version, error
//! detection and the last response (spec items 4 and 6 to 8, 12 and 13).
//!
//! Koha: `C4::SIP::Sip::MsgType::handle`, `handle_request_acs_resend`,
//! `C4::SIP::Sip::write_msg`.

use crate::checksum::{checksum, verify_checksum};
use crate::request::Request;
use crate::response::Response;

/// The protocol version of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// SIP 1.00.
    V1,
    /// SIP 2.00.
    V2,
}

/// How a response ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Terminator {
    /// CR and LF, Koha's default.
    #[default]
    CrLf,
    /// CR alone.
    Cr,
}

/// What a message read asks of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// A request for the server to answer with [`Channel::reply`].
    Request(Box<Request>),
    /// Bytes the channel answers by itself (a request to send again, a resend, a
    /// message it does not read): the server writes them as they are.
    Reply(Vec<u8>),
}

/// The codec state of one connection.
#[derive(Debug)]
pub struct Channel {
    terminator: Terminator,
    version: Version,
    error_detection: bool,
    sequence: u8,
    last_response: Option<String>,
}

/// The response that asks the terminal to send again.
const RESEND_PLEASE: &str = "96";
/// The resend requests of error detection, checksum included.
const RESEND_CHECKSUMMED: &[u8] = b"97AZFEF5";
const RESEND_PLEASE_CHECKSUMMED: &str = "96AZFEF6";

impl Channel {
    /// A new connection: protocol 1.00, without error detection.
    #[must_use]
    pub const fn new(terminator: Terminator) -> Self {
        Self {
            terminator,
            version: Version::V1,
            error_detection: false,
            sequence: b'0',
            last_response: None,
        }
    }

    /// Ends the next responses with `terminator` (the terminal signed in).
    pub const fn set_terminator(&mut self, terminator: Terminator) {
        self.terminator = terminator;
    }

    /// The protocol version now.
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// Reads `message`, a message without its CR, already cleaned.
    pub fn receive(&mut self, message: &[u8]) -> Received {
        if message.is_empty() {
            return Received::Reply(raw(RESEND_PLEASE));
        }
        let body = if message == RESEND_CHECKSUMMED {
            self.error_detection = true;
            self.sequence = b'0';
            &b"97"[..]
        } else if let Some((body, sequence)) = checksummed(message) {
            self.error_detection = true;
            if !verify_checksum(message) {
                self.last_response = Some(RESEND_PLEASE_CHECKSUMMED.to_owned());
                return Received::Reply(raw(RESEND_PLEASE_CHECKSUMMED));
            }
            self.sequence = sequence;
            body
        } else {
            self.error_detection = false;
            self.sequence = b'0';
            message
        };
        if body.starts_with(b"93") {
            self.version = Version::V2;
        }
        match Request::parse(body, self.version) {
            Some(Request::Resend) => Received::Reply(self.resend()),
            Some(Request::ScStatus(status)) => {
                self.version = if status.protocol_version.starts_with("2.") {
                    Version::V2
                } else {
                    Version::V1
                };
                Received::Request(Box::new(Request::ScStatus(status)))
            }
            Some(request) => Received::Request(Box::new(request)),
            None => {
                self.last_response = Some(RESEND_PLEASE.to_owned());
                Received::Reply(raw(RESEND_PLEASE))
            }
        }
    }

    /// The bytes of `response` to the message read last, with error detection when it
    /// is on and the terminator.
    pub fn reply(&mut self, response: &Response) -> Vec<u8> {
        let text = response.encode(self.version);
        self.write(text)
    }

    /// `96`, written as Koha writes it outside a handler: when the server cannot answer
    /// the message read (storage failed).
    pub fn ask_again(&mut self) -> Vec<u8> {
        self.last_response = Some(RESEND_PLEASE.to_owned());
        raw(RESEND_PLEASE)
    }

    /// The last response again (`97`).
    fn resend(&mut self) -> Vec<u8> {
        match self.last_response.clone() {
            None => self.write(RESEND_PLEASE.to_owned()),
            Some(last) => match last
                .len()
                .checked_sub(9)
                .and_then(|start| last.get(start..))
            {
                Some(tail) if tail.starts_with("AY") => {
                    let body = last.get(..last.len() - 9).unwrap_or_default().to_owned();
                    self.write(body)
                }
                _ => raw(&last),
            },
        }
    }

    /// `text` with error detection when on, remembered as the last response, and the
    /// terminator.
    fn write(&mut self, mut text: String) -> Vec<u8> {
        if self.error_detection {
            text.push_str("AY");
            text.push(char::from(self.sequence));
            text.push_str("AZ");
            let sum = checksum(text.as_bytes());
            text.push_str(&hexadecimal(sum));
        }
        let mut bytes = text.clone().into_bytes();
        self.last_response = Some(text);
        bytes.extend_from_slice(match self.terminator {
            Terminator::CrLf => b"\r\n",
            Terminator::Cr => b"\r",
        });
        bytes
    }
}

/// The body and sequence number of a message with error detection: longer than 11
/// bytes, with `AY` nine bytes from the end.
fn checksummed(message: &[u8]) -> Option<(&[u8], u8)> {
    if message.len() <= 11 {
        return None;
    }
    let (body, tail) = message.split_at_checked(message.len() - 9)?;
    match tail {
        [b'A', b'Y', sequence, ..] => Some((body, *sequence)),
        _ => None,
    }
}

/// `sum` in four uppercase hexadecimal digits.
fn hexadecimal(sum: u16) -> String {
    format!("{sum:04X}")
}

/// `text` as Koha prints it outside `write_msg`: with CR, without error detection.
fn raw(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(b'\r');
    bytes
}
