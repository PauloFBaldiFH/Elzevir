// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: bytes from any SIP2 terminal must be framed and read, or answered with a
//! request to send again, never crash (docs/specs/circulacao/sip2.md, items 1 to 13).
#![no_main]

use elzevir_sip2::{Channel, Framer, Received, Response, Terminator};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut framer = Framer::default();
    let mut channel = Channel::new(Terminator::Cr);
    framer.push(data);
    while let Some(Ok(message)) = framer.next_message() {
        if let Received::Request(_) = channel.receive(&message) {
            std::hint::black_box(channel.reply(&Response::Login(false)));
        }
    }
});
