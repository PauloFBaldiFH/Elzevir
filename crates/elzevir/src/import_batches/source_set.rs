// SPDX-License-Identifier: GPL-3.0-or-later
//! Boundaries and digests for immutable members of a private batch.
use std::io::{self, Read, Write};

use elzevir_app::import::batches::{MAX_SOURCE_MEMBERS, SourceInputs, SourceMember, SourceSet};
use elzevir_app::import_detection::{MAX_DETECTION_BYTES, detect_format};
use sha2::{Digest, Sha256};

use super::{HashInput, copy_bounded, io_error};

pub(super) fn capture(
    origin: String,
    inputs: SourceInputs,
    output: &mut impl Write,
    available: u64,
) -> Result<SourceSet, String> {
    if origin.is_empty()
        || origin.len() > 256
        || origin.trim() != origin
        || origin.chars().any(char::is_control)
    {
        return Err("invalid source installation identifier".into());
    }
    let mut members: Vec<SourceMember> = Vec::new();
    let mut offset = 0;
    for input in inputs {
        let mut source = input?;
        if members.len() == MAX_SOURCE_MEMBERS {
            return Err("source member limit exceeded".into());
        }
        if source.name.trim().is_empty()
            || source.name.len() > elzevir_app::import::containers::MAX_ARCHIVE_NAME_BYTES
            || source.name.contains(['/', '\\'])
            || source.name.chars().any(char::is_control)
            || matches!(source.name.as_str(), "." | "..")
        {
            return Err("invalid source member name".into());
        }
        let mut prefix = Vec::new();
        (&mut source.input)
            .take(MAX_DETECTION_BYTES as u64)
            .read_to_end(&mut prefix)
            .map_err(io_error)?;
        let format = if prefix.is_empty() {
            elzevir_app::import_detection::ImportFormat::Unknown
        } else {
            detect_format(&prefix).format
        };
        let mut hashed = HashInput::new(prefix.as_slice().chain(source.input));
        let bytes = copy_bounded(&mut hashed, output, available - offset)?;
        let sha256 = hashed.digest();
        let duplicate_of = members
            .iter()
            .find(|m| m.sha256 == sha256)
            .map(|m| m.position);
        members.push(SourceMember {
            position: members.len() + 1,
            name: source.name,
            archive_path: None,
            offset,
            bytes,
            sha256,
            format,
            duplicate_of,
        });
        offset += bytes;
    }
    if members.is_empty() {
        return Err("select at least one source member".into());
    }
    Ok(SourceSet {
        origin,
        members,
        archive_sha256: None,
    })
}

pub(super) fn stamp(set: &SourceSet) -> Result<String, String> {
    let json = serde_json::to_vec(&("source-set-v1", set))
        .map_err(|_| "cannot encode source inventory")?;
    Ok(format!("{:x}", Sha256::digest(json)))
}

pub(super) fn verify(set: &SourceSet, mut original: std::fs::File) -> Result<(), String> {
    let mut offset = 0;
    for member in &set.members {
        let mut hashed = HashInput::new((&mut original).take(member.bytes));
        let bytes = io::copy(&mut hashed, &mut io::sink()).map_err(io_error)?;
        if member.offset != offset || bytes != member.bytes || hashed.digest() != member.sha256 {
            return Err(format!(
                "source identity changed at member {}",
                member.position
            ));
        }
        offset += bytes;
    }
    if original.read(&mut [0]).map_err(io_error)? != 0 {
        return Err("unexpected bytes after last source member".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_app::import::batches::SourceInput;
    use std::io::Cursor;

    fn inputs(names: &[&str]) -> SourceInputs {
        let names: Vec<_> = names.iter().map(ToString::to_string).collect();
        Box::new(names.into_iter().map(|name| {
            Ok(SourceInput {
                name,
                input: Box::new(Cursor::new(b"123")),
            })
        }))
    }

    #[test]
    fn enforces_count_total_bytes_and_names_across_members() {
        for (names, quota) in [
            (vec![], 1000),
            (vec!["a"; MAX_SOURCE_MEMBERS + 1], 1000),
            (vec!["a", "b"], 5),
            (vec!["../part"], 1000),
        ] {
            assert!(capture("source".into(), inputs(&names), &mut io::sink(), quota).is_err());
        }
        assert!(capture("source".into(), inputs(&["a", "b"]), &mut io::sink(), 6).is_ok());
        assert!(capture(" ".into(), inputs(&["a"]), &mut io::sink(), 6).is_err());
    }
}
