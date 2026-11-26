// SPDX-License-Identifier: GPL-3.0-or-later
//! SIP2 for self-check machines (docs/specs/circulacao/sip2.md): the codec that turns
//! the terminal's lines into typed requests and typed responses back into lines, as
//! Koha's SIP server writes them. The codec does no I/O.
#![forbid(unsafe_code)]

mod channel;
mod checksum;
pub mod codes;
mod frame;
mod request;
mod response;
mod server;
mod text;

pub use channel::{Channel, Received, Terminator, Version};
pub use checksum::{checksum, verify_checksum};
pub use frame::{FrameTooLong, Framer, MAX_MESSAGE_BYTES, clean};
pub use request::{
    Checkin, Checkout, EndPatronSession, ItemInformation, Login, PatronInformation, PatronStatus,
    Renew, Request, ScStatus, SummaryKind,
};
pub use response::{
    AcsStatus, CheckedOut, CheckinItem, CheckinResponse, CheckoutRefused, CheckoutResponse,
    CirculationStatus, EndSessionResponse, Fee, ItemInformationResponse, KnownItem, KnownPatron,
    PatronCounts, PatronDetails, PatronFlag, PatronFlags, PatronInformationResponse,
    PatronStatusResponse, RenewRefused, RenewResponse, Renewed, Response, SUPPORTED_MESSAGES,
    Summary,
};
pub use server::{Desk, SipEvent, Terminal, serve};
