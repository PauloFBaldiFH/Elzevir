// SPDX-License-Identifier: GPL-3.0-or-later
//! Path input stays data. Filesystem probes are serialized off the rendering thread.
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

pub(super) fn single(value: &str) -> bool {
    !value.is_empty() && value.chars().count() <= 512 && !value.chars().any(char::is_control)
}

#[derive(Default)]
pub(super) struct Checked {
    raw: String,
    candidates: Vec<String>,
    choice: Option<usize>,
}

impl Checked {
    fn inspect(raw: String) -> Self {
        let mut candidates = Vec::new();
        if single(&raw) {
            let readable = |path: &str| {
                std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
                    && std::fs::File::open(Path::new(path)).is_ok()
            };
            if readable(&raw) {
                candidates.push(raw.clone());
            }
            // Terminals may append a separator after the dropped filename.
            let token = raw.trim_end_matches(' ');
            let decoded = ['\'', '"']
                .into_iter()
                .find_map(|quote| {
                    let inner = token.strip_prefix(quote)?.strip_suffix(quote)?;
                    (!inner.contains(quote)).then(|| inner.to_owned())
                })
                .unwrap_or_else(|| {
                    if cfg!(unix) {
                        token.replace("\\ ", " ")
                    } else {
                        token.to_owned()
                    }
                });
            if decoded != raw && readable(&decoded) {
                candidates.push(decoded);
            }
        }
        let choice = (candidates.len() == 1).then_some(0);
        Self {
            raw,
            candidates,
            choice,
        }
    }
    pub(super) fn choose(&mut self, decoded: bool) {
        if self.candidates.len() == 2 {
            self.choice = Some(usize::from(decoded));
        }
    }
    pub(super) fn resolved(&self, current: &str) -> Option<String> {
        if self.raw != current {
            return None;
        }
        self.candidates.get(self.choice?).cloned()
    }
    pub(super) fn status(&self, current: &str) -> &'static str {
        if current.is_empty() {
            "restore-empty"
        } else if current != self.raw {
            "restore-checking"
        } else if self.choice.is_some() {
            "restore-readable"
        } else if self.candidates.len() == 2 {
            "restore-ambiguous"
        } else {
            "restore-unreadable"
        }
    }
}

#[derive(Default)]
pub(super) struct Probe {
    last: Option<(Vec<String>, Instant)>,
    pending: Option<mpsc::Receiver<[Checked; 2]>>,
}

impl Probe {
    pub(super) fn poll(&mut self, form: Option<&mut crate::Form>) {
        let Some(form) = form.filter(|f| matches!(f.operation, crate::Operation::Restore { .. }))
        else {
            self.last = None;
            return;
        };
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(checks) => {
                    for ((current, previous), c) in
                        form.values.iter().zip(&mut form.checks).zip(checks)
                    {
                        if *current == c.raw && previous.raw != c.raw {
                            *previous = c;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
            self.pending = None;
        }
        if form
            .values
            .iter()
            .zip(&form.checks)
            .all(|(v, c)| *v == c.raw)
        {
            self.last = None;
            return;
        }
        if self.last.as_ref().is_none_or(|(v, _)| v != &form.values) {
            self.last = Some((form.values.clone(), Instant::now()));
        } else if self
            .last
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= Duration::from_millis(180))
        {
            let [input, key] = form.values.as_slice() else {
                return;
            };
            let paths = [input.clone(), key.clone()];
            let (tx, rx) = mpsc::sync_channel(1);
            self.pending = Some(rx);
            std::thread::spawn(move || {
                drop(tx.send(paths.map(Checked::inspect)));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dragging_one_file_or_multiple_items() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = dir.path().join("a b.age");
        std::fs::write(&fixture, []).unwrap();
        let path = fixture.to_str().unwrap();
        for raw in [
            path.to_owned(),
            format!("'{path}'"),
            format!("'{path}' "),
            format!("\"{path}\""),
        ] {
            assert_eq!(
                Checked::inspect(raw.clone()).resolved(&raw).as_deref(),
                Some(path)
            );
        }
        #[cfg(unix)]
        {
            let raw = path.replace(' ', "\\ ");
            assert_eq!(
                Checked::inspect(raw.clone()).resolved(&raw).as_deref(),
                Some(path)
            );
            std::fs::write(&raw, []).unwrap();
            let mut check = Checked::inspect(raw.clone());
            assert_eq!(check.resolved(&raw), None);
            check.choose(true);
            assert_eq!(check.resolved(&raw).as_deref(), Some(path));
        }
        for raw in [format!("{path}\n{path}"), format!("'{path}' '{path}'")] {
            assert_eq!(Checked::inspect(raw.clone()).resolved(&raw), None);
        }
    }
}
