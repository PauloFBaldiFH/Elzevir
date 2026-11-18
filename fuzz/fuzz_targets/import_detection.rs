// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: arbitrary prefixes must be bounded and never panic or execute input.
#![no_main]

use elzevir_app::import_detection::{MAX_DETECTION_BYTES, detect_format};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let prefix = data.get(..MAX_DETECTION_BYTES).unwrap_or(data);
    assert_eq!(detect_format(data), detect_format(prefix));
});
