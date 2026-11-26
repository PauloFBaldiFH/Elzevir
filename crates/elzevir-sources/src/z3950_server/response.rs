// SPDX-License-Identifier: GPL-3.0-or-later
//! The responses of the Z39.50 server (docs/specs/opac/z3950.md), encoded with
//! [`crate::ber`] in the order of the Z39.50 ASN.1.

use crate::ber::{self, Class, EXTERNAL, INTEGER, context, context_integer, context_of, sequence};
use crate::z3950::Diagnostic;

use super::request::{CLOSE, INIT_RESPONSE, PRESENT_RESPONSE, SEARCH_RESPONSE};

/// The Bib-1 diagnostic set.
const BIB1_DIAGNOSTICS: [u32; 6] = [1, 2, 840, 10003, 4, 1];

/// `presentStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PresentStatus {
    Success = 0,
    Partial = 1,
    Failure = 5,
}

/// One record of a Present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RecordOut {
    /// The record's bytes in `syntax`.
    Record { syntax: Vec<u32>, bytes: Vec<u8> },
    /// A diagnostic in its place.
    Surrogate(Diagnostic),
}

/// The `referenceId` echoed first, when the request had one.
fn reference_part(reference: Option<&[u8]>) -> Vec<Vec<u8>> {
    reference
        .map(|bytes| context(2, bytes))
        .into_iter()
        .collect()
}

fn boolean(number: u32, value: bool) -> Vec<u8> {
    context(number, &[if value { 0xFF } else { 0x00 }])
}

/// The parts of a `DefaultDiagFormat`.
fn diagnostic_parts(diagnostic: &Diagnostic) -> Vec<u8> {
    [
        ber::oid(&BIB1_DIAGNOSTICS),
        ber::element(
            Class::Universal,
            false,
            INTEGER,
            &ber::integer_content(diagnostic.condition),
        ),
        // v2Addinfo, a VisibleString, which every version reads.
        ber::element(
            Class::Universal,
            false,
            26,
            diagnostic.addinfo.as_deref().unwrap_or_default().as_bytes(),
        ),
    ]
    .concat()
}

/// `nonSurrogateDiagnostic [130] IMPLICIT DefaultDiagFormat`.
fn non_surrogate(diagnostic: &Diagnostic) -> Vec<u8> {
    ber::element(Class::Context, true, 130, &diagnostic_parts(diagnostic))
}

/// What the Init agreed (item 5).
pub(super) struct InitAnswer<'a> {
    pub(super) reference: Option<&'a [u8]>,
    pub(super) versions: Vec<u8>,
    pub(super) options: Vec<u8>,
    pub(super) message_size: i64,
    pub(super) record_size: i64,
    pub(super) accepted: bool,
    pub(super) version: &'a str,
}

pub(super) fn init_response(answer: &InitAnswer<'_>) -> Vec<u8> {
    let mut parts = reference_part(answer.reference);
    parts.extend([
        context(3, &answer.versions),
        context(4, &answer.options),
        context_integer(5, answer.message_size),
        context_integer(6, answer.record_size),
        boolean(12, answer.accepted),
        context(110, b"Elzevir"),
        context(111, b"Elzevir"),
        context(112, answer.version.as_bytes()),
    ]);
    context_of(INIT_RESPONSE, &parts)
}

/// The answer to a Search: the total, or the diagnostic (item 12).
pub(super) fn search_response(
    reference: Option<&[u8]>,
    outcome: Result<usize, &Diagnostic>,
) -> Vec<u8> {
    let mut parts = reference_part(reference);
    let total = outcome.as_ref().map_or(0, |total| *total);
    parts.extend([
        context_integer(23, i64::try_from(total).unwrap_or(i64::MAX)),
        context_integer(24, 0),
        context_integer(25, 1),
        boolean(22, outcome.is_ok()),
    ]);
    if let Err(diagnostic) = outcome {
        // resultSetStatus none (3).
        parts.push(context_integer(26, 3));
        parts.push(non_surrogate(diagnostic));
    }
    context_of(SEARCH_RESPONSE, &parts)
}

/// The answer to a Present: the records, or the diagnostic (items 14 to 17).
pub(super) fn present_response(
    reference: Option<&[u8]>,
    database: &str,
    outcome: Result<(&[RecordOut], usize, PresentStatus), &Diagnostic>,
) -> Vec<u8> {
    let mut parts = reference_part(reference);
    match outcome {
        Ok((records, next, status)) => {
            parts.extend([
                context_integer(24, i64::try_from(records.len()).unwrap_or(i64::MAX)),
                context_integer(25, i64::try_from(next).unwrap_or(0)),
                context_integer(27, status as i64),
            ]);
            if !records.is_empty() {
                let named: Vec<Vec<u8>> = records
                    .iter()
                    .map(|record| name_plus_record(database, record))
                    .collect();
                parts.push(context_of(28, &named));
            }
        }
        Err(diagnostic) => {
            parts.extend([
                context_integer(24, 0),
                context_integer(25, 0),
                context_integer(27, PresentStatus::Failure as i64),
                non_surrogate(diagnostic),
            ]);
        }
    }
    context_of(PRESENT_RESPONSE, &parts)
}

/// `NamePlusRecord ::= SEQUENCE { name [0] DatabaseName, record [1] EXPLICIT CHOICE
/// { retrievalRecord [1] EXTERNAL, surrogateDiagnostic [2] DiagRec } }`.
fn name_plus_record(database: &str, record: &RecordOut) -> Vec<u8> {
    let choice = match record {
        RecordOut::Record { syntax, bytes } => {
            let external = ber::element(
                Class::Universal,
                true,
                EXTERNAL,
                &[ber::oid(syntax), context(1, bytes)].concat(),
            );
            context_of(1, &[external])
        }
        RecordOut::Surrogate(diagnostic) => context_of(
            2,
            &[ber::element(
                Class::Universal,
                true,
                ber::SEQUENCE,
                &diagnostic_parts(diagnostic),
            )],
        ),
    };
    sequence(&[context(0, database.as_bytes()), context_of(1, &[choice])])
}

/// The Close with `reason` (item 18).
pub(super) fn close(reference: Option<&[u8]>, reason: i64) -> Vec<u8> {
    let mut parts = reference_part(reference);
    parts.push(context_integer(211, reason));
    context_of(CLOSE, &parts)
}
