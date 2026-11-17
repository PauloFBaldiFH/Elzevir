// SPDX-License-Identifier: GPL-3.0-or-later
//! Small stored ZIP32 archives with fixed entry names, published without replacement.

use std::{
    io::{self, Seek, Write},
    path::{Path, PathBuf},
};

pub(crate) fn publish(report: &[u8], events: &[u8], output: Option<&Path>) -> io::Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let parent = output
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(&cwd);
    let mut file = tempfile::Builder::new()
        .prefix(".elzevir-diagnostics-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    write(
        file.as_file_mut(),
        [("report.json", report), ("events.jsonl", events)],
    )?;
    file.as_file().sync_all()?;
    let path = if let Some(output) = output {
        cwd.join(output)
    } else {
        let name = file
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix('.'))
            .and_then(|n| n.strip_suffix(".tmp"))
            .ok_or_else(|| io::Error::other("invalid archive filename"))?;
        cwd.join(format!("{name}.zip"))
    };
    drop(file.persist_noclobber(&path).map_err(|error| error.error)?);
    Ok(path)
}

fn number(writer: &mut impl Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
fn short(writer: &mut impl Write, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
fn size(value: usize) -> io::Result<u32> {
    u32::try_from(value).map_err(|_| io::Error::other("archive entry too large"))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn write(writer: &mut (impl Write + Seek), entries: [(&str, &[u8]); 2]) -> io::Result<()> {
    let mut central = Vec::new();
    for (name, bytes) in entries {
        let length = size(bytes.len())?;
        let name_length =
            u16::try_from(name.len()).map_err(|_| io::Error::other("archive name too long"))?;
        let offset = u32::try_from(writer.stream_position()?)
            .map_err(|_| io::Error::other("archive too large"))?;
        let crc = crc32(bytes);
        number(writer, 0x0403_4b50)?;
        for value in [20, 0x0800, 0, 0, 33] {
            short(writer, value)?;
        }
        for value in [crc, length, length] {
            number(writer, value)?;
        }
        short(writer, name_length)?;
        short(writer, 0)?;
        writer.write_all(name.as_bytes())?;
        writer.write_all(bytes)?;
        number(&mut central, 0x0201_4b50)?;
        for value in [0x0314, 20, 0x0800, 0, 0, 33] {
            short(&mut central, value)?;
        }
        for value in [crc, length, length] {
            number(&mut central, value)?;
        }
        for value in [name_length, 0, 0, 0, 0] {
            short(&mut central, value)?;
        }
        number(&mut central, 0o100_600 << 16)?;
        number(&mut central, offset)?;
        central.write_all(name.as_bytes())?;
    }
    let offset = u32::try_from(writer.stream_position()?)
        .map_err(|_| io::Error::other("archive too large"))?;
    writer.write_all(&central)?;
    number(writer, 0x0605_4b50)?;
    for value in [0, 0, 2, 2] {
        short(writer, value)?;
    }
    number(writer, size(central.len())?)?;
    number(writer, offset)?;
    short(writer, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crc_matches_the_zip_standard_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }
}
