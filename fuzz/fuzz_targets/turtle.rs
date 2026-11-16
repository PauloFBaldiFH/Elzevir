// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: reading a BIBFRAME Turtle document back into records must handle any
//! input without panicking, looping forever or nesting past the limits.
#![no_main]

use elzevir_app::rdf::{IriBase, read_turtle, records_from};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(nodes) = read_turtle(text) else { return };
    let base = IriBase::new("https://biblioteca.example/bibframe");
    std::hint::black_box(records_from(&nodes, &base));
});
