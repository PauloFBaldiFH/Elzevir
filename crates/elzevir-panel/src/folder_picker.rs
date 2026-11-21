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

pub(super) struct Picker {
    path: String,
    managed: PathBuf,
    entries: Vec<String>,
    pending: Option<Receiver<std::io::Result<Vec<String>>>>,
    selected: usize,
    focus: usize,
    notice: &'static str,
}

impl Picker {
    pub(super) fn new(managed: PathBuf) -> Self {
        let mut picker = Self {
            path: String::new(),
            managed: managed.clone(),
            entries: Vec::new(),
            pending: None,
            selected: 0,
            focus: 0,
            notice: "",
        };
        picker.open(managed);
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
        std::thread::spawn(move || {
            let result = (|| {
                let mut names = Vec::new();
                for entry in std::fs::read_dir(path)?.take(4096) {
                    let entry = entry?;
                    if entry.file_type()?.is_dir()
                        && let Ok(name) = entry.file_name().into_string()
                    {
                        names.push(name);
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

    pub(super) fn key(&mut self, key: KeyCode) -> Option<String> {
        match key {
            KeyCode::Tab => self.focus = (self.focus + 1) % 3,
            KeyCode::BackTab => self.focus = (self.focus + 2) % 3,
            KeyCode::F(3) if self.pending.is_none() => {
                if let Some(home) = std::env::home_dir() {
                    self.open(home);
                }
            }
            KeyCode::F(4) if self.pending.is_none() => self.open(self.managed.clone()),
            KeyCode::Down if self.focus == 0 => {
                self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
            }
            KeyCode::Up if self.focus == 0 => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter if self.focus == 0 => {
                if let Some(name) = self.entries.get(self.selected) {
                    self.open(PathBuf::from(&self.path).join(name));
                }
            }
            KeyCode::Backspace if self.focus == 0 && self.pending.is_none() => {
                if let Some(parent) = std::path::Path::new(&self.path).parent() {
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
            m.get("folder-keys"),
            format!(
                "{} {}",
                if self.focus == 1 { ">" } else { " " },
                text(&self.path)
            ),
            format!(
                "{} {}",
                if self.focus == 2 { ">" } else { " " },
                m.get("folder-use")
            ),
        ];
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
                .block(crate::view::block(m.get("folder-title"), ascii)),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
