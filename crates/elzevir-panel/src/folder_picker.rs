// SPDX-License-Identifier: GPL-3.0-or-later
//! Operator-side directory browsing. Directory enumeration runs outside the input loop.
use crate::{Messages, dashboard::text};
use ratatui::{
    Frame,
    crossterm::event::KeyCode,
    widgets::{Clear, Paragraph, Wrap},
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Directory,
    Archive,
    Key,
}
pub(super) struct Picker {
    path: String,
    managed: PathBuf,
    kind: Kind,
    owned: Vec<String>,
    catalog: bool,
    all: bool,
    entries: Vec<String>,
    pending: Option<Receiver<std::io::Result<Vec<String>>>>,
    selected: usize,
    focus: usize,
    notice: &'static str,
}

impl Picker {
    pub(super) fn new(managed: PathBuf) -> Self {
        Self::start(managed, Kind::Directory, Vec::new())
    }
    pub(super) fn files(managed: PathBuf, archive: bool, ids: &[String]) -> Self {
        Self::start(
            managed,
            if archive { Kind::Archive } else { Kind::Key },
            ids.iter()
                .rev()
                .map(|id| format!("elzevir-{id}.age"))
                .collect(),
        )
    }
    fn start(managed: PathBuf, kind: Kind, owned: Vec<String>) -> Self {
        let mut picker = Self {
            path: String::new(),
            managed: managed.clone(),
            kind,
            owned,
            catalog: kind == Kind::Archive,
            all: false,
            entries: Vec::new(),
            pending: None,
            selected: 0,
            focus: 0,
            notice: "",
        };
        if kind == Kind::Key {
            picker.open(managed.parent().unwrap_or(&managed).to_owned());
        } else {
            picker.open(managed);
        }
        picker
    }

    fn open(&mut self, path: PathBuf) {
        let Some(value) = path.to_str() else {
            self.notice = "folder-error";
            return;
        };
        value.clone_into(&mut self.path);
        self.entries.clear();
        self.selected = 0;
        self.focus = 0;
        self.notice = "folder-loading";
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let files = self.kind != Kind::Directory;
        let allow_all = self.all || self.kind != Kind::Archive;
        let catalog = self.catalog;
        let owned = catalog.then(|| self.owned.clone());
        std::thread::spawn(move || {
            let result = (|| {
                if let Some(names) = owned {
                    return Ok(names
                        .into_iter()
                        .filter(|name| {
                            std::fs::symlink_metadata(path.join(name)).is_ok_and(|m| m.is_file())
                        })
                        .collect());
                }
                let mut names = Vec::new();
                for entry in std::fs::read_dir(path)?.take(4096) {
                    let entry = entry?;
                    let kind = entry.file_type()?;
                    if let Ok(mut name) = entry.file_name().into_string() {
                        if kind.is_dir() {
                            if files {
                                name.push('/');
                            }
                            names.push(name);
                        } else if files
                            && kind.is_file()
                            && (allow_all || name.to_ascii_lowercase().ends_with(".age"))
                        {
                            names.push(name);
                        }
                    }
                }
                names.sort();
                Ok(names)
            })();
            drop(tx.send(result));
        });
    }

    pub(super) fn poll(&mut self) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(entries)) => {
                    self.entries = entries;
                    self.notice = "";
                }
                Err(mpsc::TryRecvError::Empty) => return,
                _ => self.notice = "folder-error",
            }
            self.pending = None;
        }
    }

    pub(super) fn paste(&mut self, value: &str) {
        if self.focus == 1 && !value.chars().any(char::is_control) && value.len() <= 4096 {
            value.clone_into(&mut self.path);
            self.entries.clear();
            self.catalog = false;
        }
    }

    pub(super) fn key(&mut self, key: KeyCode) -> Option<String> {
        match key {
            KeyCode::Tab => self.focus = (self.focus + 1) % 3,
            KeyCode::BackTab => self.focus = (self.focus + 2) % 3,
            KeyCode::F(3) if self.pending.is_none() => {
                if let Some(home) = std::env::home_dir() {
                    self.catalog = false;
                    self.open(home);
                }
            }
            KeyCode::F(4) if self.pending.is_none() => {
                self.catalog = self.kind == Kind::Archive;
                self.open(self.managed.clone());
            }
            KeyCode::F(5) if self.kind == Kind::Archive && self.pending.is_none() => {
                self.all = !self.all;
                self.catalog = false;
                self.open(PathBuf::from(&self.path));
            }
            KeyCode::Down if self.focus == 0 => {
                self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
            }
            KeyCode::Up if self.focus == 0 => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter if self.focus == 0 => {
                if let Some(name) = self.entries.get(self.selected) {
                    let path = PathBuf::from(&self.path).join(name);
                    if self.kind != Kind::Directory && !name.ends_with('/') {
                        return path.to_str().map(str::to_owned);
                    }
                    self.open(path);
                }
            }
            KeyCode::Backspace if self.focus == 0 && self.pending.is_none() => {
                if let Some(parent) = std::path::Path::new(&self.path).parent() {
                    self.catalog = false;
                    self.open(parent.to_owned());
                }
            }
            KeyCode::Char(c) if self.focus == 1 && !c.is_control() && self.path.len() < 4096 => {
                self.entries.clear();
                self.path.push(c);
                self.notice = "";
            }
            KeyCode::Backspace if self.focus == 1 => {
                self.entries.clear();
                self.path.pop();
                self.notice = "";
            }
            KeyCode::Enter if self.focus == 1 && self.pending.is_none() => {
                self.catalog = false;
                self.open(PathBuf::from(&self.path));
            }
            KeyCode::Enter if self.focus == 2 => {
                let path = PathBuf::from(&self.path);
                if path.is_absolute() {
                    // Folder creation is deferred to the confirmed export action.
                    return Some(self.path.clone());
                }
                self.notice = "folder-error";
            }
            _ => {}
        }
        None
    }

    pub(super) fn draw(&self, frame: &mut Frame, m: &Messages, ascii: bool) {
        let area = frame.area();
        frame.render_widget(Clear, area);
        let mut rows = vec![
            m.get(if self.kind == Kind::Directory {
                "folder-keys"
            } else {
                "restore-picker"
            }),
            format!(
                "{} {}",
                if self.focus == 1 { ">" } else { " " },
                text(&self.path)
            ),
            format!(
                "{} {}",
                if self.focus == 2 { ">" } else { " " },
                m.get(if self.kind == Kind::Directory {
                    "folder-use"
                } else {
                    "restore-file-use"
                })
            ),
        ];
        if self.kind == Kind::Archive {
            rows.push(m.get(if self.catalog {
                "restore-managed"
            } else {
                "restore-host"
            }));
            rows.push(m.get(if self.all {
                "restore-filter-all"
            } else {
                "restore-filter-age"
            }));
        }
        if !self.notice.is_empty() {
            rows.push(m.get(self.notice));
        }
        let count = usize::from(area.height.saturating_sub(8)).max(1);
        for (i, name) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.selected.saturating_sub(count - 1))
            .take(count)
        {
            rows.push(format!(
                "{} {}",
                if i == self.selected && self.focus == 0 {
                    ">"
                } else {
                    " "
                },
                text(name)
            ));
        }
        frame.render_widget(
            Paragraph::new(rows.join("\n"))
                .wrap(Wrap { trim: false })
                .block(crate::view::block(
                    m.get(if self.kind == Kind::Directory {
                        "folder-title"
                    } else {
                        "restore-file-title"
                    }),
                    ascii,
                )),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_and_loose_files_are_separate_and_extension_filter_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        let id = "a".repeat(32);
        let registered = format!("elzevir-{id}.age");
        for name in [&registered, "loose.age", "older.archive"] {
            std::fs::write(dir.path().join(name), []).unwrap();
        }
        let mut picker = Picker::files(dir.path().to_owned(), true, &[id]);
        let collect = |p: &mut Picker| {
            p.entries = p.pending.take().unwrap().recv().unwrap().unwrap();
        };
        collect(&mut picker);
        assert_eq!(picker.entries.as_slice(), std::slice::from_ref(&registered));
        assert_eq!(
            picker.key(KeyCode::Enter).map(PathBuf::from),
            Some(dir.path().join(registered))
        );
        picker.key(KeyCode::F(5));
        collect(&mut picker);
        assert!(picker.entries.iter().any(|s| s == "older.archive"));
        picker.key(KeyCode::F(5));
        collect(&mut picker);
        assert!(!picker.entries.iter().any(|s| s == "older.archive"));
        assert!(picker.entries.iter().any(|s| s == "loose.age"));
    }

    #[test]
    fn folders_and_text_have_distinct_navigation_and_backspace() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut picker = Picker::new(root.clone());
        picker.entries = picker.pending.take().unwrap().recv().unwrap().unwrap();
        picker.selected = picker
            .entries
            .iter()
            .position(|name| name == "src")
            .unwrap();
        assert!(picker.key(KeyCode::Enter).is_none());
        assert_eq!(PathBuf::from(&picker.path), root.join("src"));
        picker.entries = picker.pending.take().unwrap().recv().unwrap().unwrap();
        picker.key(KeyCode::Backspace);
        assert_eq!(PathBuf::from(&picker.path), root);
        picker.key(KeyCode::Tab);
        picker.key(KeyCode::Char('x'));
        picker.key(KeyCode::Backspace);
        assert_eq!(PathBuf::from(&picker.path), root);
        picker.key(KeyCode::Tab);
        assert_eq!(picker.key(KeyCode::Enter).map(PathBuf::from), Some(root));
    }
}
