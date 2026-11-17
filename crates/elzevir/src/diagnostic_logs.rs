// SPDX-License-Identifier: GPL-3.0-or-later
//! A bounded projection of operational events, never raw log text.

use elzevir_panel::{LogEntry, LogEvent, LogFeed, LogState};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const BYTE_LIMIT: u64 = 65_536;

pub(crate) fn read(directory: &Path) -> LogFeed {
    match inspect(&directory.join("logs")) {
        Ok(feed) => feed,
        Err(error) => LogFeed {
            state: if error.kind() == std::io::ErrorKind::NotFound {
                LogState::Missing
            } else {
                LogState::Unavailable
            },
            ..LogFeed::default()
        },
    }
}

fn inspect(directory: &Path) -> std::io::Result<LogFeed> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return Err(std::io::Error::other("log directory refused"));
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut feed = LogFeed::default();
    for (count, entry) in std::fs::read_dir(directory)?.enumerate() {
        if count >= 64 {
            feed.limited = true;
            break;
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(date) = name
            .strip_prefix("elzevir.")
            .and_then(|n| n.strip_suffix(".jsonl"))
        else {
            continue;
        };
        if date.len() != 10 || date.parse::<jiff::civil::Date>().is_err() {
            continue;
        }
        paths.push(entry.path());
    }
    paths.sort();
    feed.limited |= paths.len() > 7;
    let mut budget = BYTE_LIMIT;
    for path in paths.into_iter().rev().take(7) {
        feed.state = LogState::Ready;
        if budget == 0 {
            feed.limited = true;
            break;
        }
        tail(&path, &mut budget, &mut feed)?;
    }
    Ok(feed)
}

fn tail(path: &Path, budget: &mut u64, feed: &mut LogFeed) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(std::io::Error::other("log file refused"));
    }
    let mut file = std::fs::File::open(path)?;
    let offset = metadata.len().saturating_sub(*budget);
    feed.limited |= offset > 0;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    let count = file.take(*budget).read_to_end(&mut bytes)?;
    *budget = budget.saturating_sub(u64::try_from(count).unwrap_or(u64::MAX));
    let mut entries = Vec::new();
    for line in bytes
        .split_inclusive(|b| *b == b'\n')
        .skip(usize::from(offset > 0))
    {
        if line.last() != Some(&b'\n') {
            continue;
        }
        if let Some(entry) = project(line) {
            entries.push(entry);
        } else {
            feed.omitted += 1;
        }
    }
    for entry in entries.into_iter().rev() {
        if feed.entries.len() < 40 {
            feed.entries.push(entry);
        } else {
            feed.limited = true;
        }
    }
    Ok(())
}

fn project(line: &[u8]) -> Option<LogEntry> {
    if line.len() > 4096 {
        return None;
    }
    // Parsing/unknown fields are intentionally discarded; no raw text leaves this module.
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    if value.get("target")?.as_str()? != "elzevir::ops" {
        return None;
    }
    let fields = value.get("fields")?;
    if fields.get("institution")?.as_str()? != "local" {
        return None;
    }
    let instance = fields.get("instance")?.as_str()?;
    if instance.len() != 32 || !instance.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let event = match fields.get("event")?.as_str()? {
        "server.started" => LogEvent::ServerStarted,
        "server.stopped" => LogEvent::ServerStopped,
        "server.failed" => LogEvent::ServerFailed,
        "indexer.committed" => LogEvent::IndexerCommitted,
        "indexer.failed" => LogEvent::IndexerFailed,
        "indexer.panicked" => LogEvent::IndexerPanicked,
        "admin.handler_failed" => LogEvent::AdminHandlerFailed,
        "runtime.cleanup_failed" => LogEvent::RuntimeCleanupFailed,
        _ => return None,
    };
    if value.get("level")?.as_str()? != event.level() {
        return None;
    }
    let timestamp = value.get("timestamp")?.as_str()?;
    if timestamp.len() > 40 || !timestamp.ends_with('Z') {
        return None;
    }
    let timestamp = timestamp.parse::<jiff::Timestamp>().ok()?.to_string();
    Some(LogEntry { timestamp, event })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(code: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-11-16T12:00:00Z","level":"INFO","target":"elzevir::ops","fields":{{"institution":"local","instance":"00000000000000000000000000000000","event":"{code}","message":"do-not-export-secret","password":"do-not-export-secret"}}}}"#
        )
    }

    #[test]
    fn projection_discards_payloads_partial_lines_and_unknown_events() {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::write(
            logs.join("elzevir.2026-11-16.jsonl"),
            format!(
                "{}\n{}\ninvalid\n{}",
                event("server.started"),
                event("secret.unknown"),
                event("server.stopped")
            ),
        )
        .unwrap();
        let result = read(root.path());
        assert_eq!(result.entries.len(), 1);
        assert_eq!(
            result.entries.first().unwrap().event,
            LogEvent::ServerStarted
        );
        assert_eq!(result.omitted, 2);
        assert!(!format!("{result:?}").contains("do-not-export-secret"));
    }

    #[test]
    fn large_files_are_tailed_and_windows_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        let mut contents = "x".repeat(100_000);
        contents.push('\n');
        for _ in 0..100 {
            contents.push_str(&event("server.started"));
            contents.push('\n');
        }
        std::fs::write(logs.join("elzevir.2026-11-16.jsonl"), contents).unwrap();
        let result = read(root.path());
        assert_eq!(result.entries.len(), 40);
        assert!(result.limited);
        assert_eq!(result.state, LogState::Ready);
    }

    #[cfg(unix)]
    #[test]
    fn log_symlinks_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("logs")).unwrap();
        let file = outside.path().join("private");
        std::fs::write(&file, event("server.started") + "\n").unwrap();
        std::os::unix::fs::symlink(file, root.path().join("logs/elzevir.2026-11-16.jsonl"))
            .unwrap();
        assert_eq!(read(root.path()).state, LogState::Unavailable);
    }
}
