// SPDX-License-Identifier: GPL-3.0-or-later
//! Operator-side directory browsing. Directory enumeration runs outside the input loop.
use crate::{Messages, dashboard::text};
use ratatui::{Frame, crossterm::event::KeyCode, widgets::Clear};
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
            self.pending = None;
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
                self.pending = None;
                self.catalog = false;
                self.path.push(c);
                self.notice = "";
            }
            KeyCode::Backspace if self.focus == 1 => {
                self.entries.clear();
                self.pending = None;
                self.catalog = false;
                self.path.pop();
                self.notice = "";
            }
            KeyCode::Enter | KeyCode::Right if self.focus == 1 && self.pending.is_none() => {
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

    pub(super) fn activate(&mut self, action: crate::ui::Action) -> Option<KeyCode> {
        use crate::ui::Action;
        match action {
            Action::PickerEntry(i) if self.entries.get(i).is_some() => {
                self.selected = i;
                self.focus = 0;
                Some(KeyCode::Enter)
            }
            Action::Field(1) => {
                self.focus = 1;
                None
            }
            Action::Field(2) => {
                self.focus = 2;
                Some(KeyCode::Enter)
            }
            Action::Key(k) => {
                if matches!(k, KeyCode::Backspace | KeyCode::Up | KeyCode::Down) {
                    self.focus = 0;
                }
                if k == KeyCode::Right {
                    self.focus = 1;
                }
                Some(k)
            }
            _ => None,
        }
    }

    fn places(&self, ui: &mut crate::backup_center::Controls<'_, '_>, area: ratatui::layout::Rect) {
        use crate::ui::{Action, Variant};
        let key = |k| Action::Key(k);
        let mut places = vec![
            (
                "restore-up",
                key(KeyCode::Backspace),
                Variant::Secondary,
                true,
            ),
            ("restore-home", key(KeyCode::F(3)), Variant::Secondary, true),
            (
                "restore-library",
                key(KeyCode::F(4)),
                Variant::Secondary,
                true,
            ),
        ];
        if self.kind == Kind::Archive {
            places.push((
                "restore-cloud",
                key(KeyCode::F(6)),
                Variant::Secondary,
                true,
            ));
        }
        ui.buttons(area, &places);
    }
    fn path_bar(
        &self,
        ui: &mut crate::backup_center::Controls<'_, '_>,
        area: ratatui::layout::Rect,
    ) {
        use crate::ui::{Action, Variant, theme};
        let [path, go] = ratatui::layout::Layout::horizontal([
            ratatui::layout::Constraint::Fill(1),
            ratatui::layout::Constraint::Length(12),
        ])
        .spacing(1)
        .areas(area);
        ui.buttons(
            go,
            &[(
                "mb-go",
                Action::Key(KeyCode::Right),
                Variant::Secondary,
                self.pending.is_none(),
            )],
        );
        ui.scene.add(path, Action::Field(1));
        let shown = format!(
            "{} {}",
            if self.focus == 1 { ">" } else { "" },
            text(&self.path)
        );
        let width = ratatui::text::Line::from(shown.as_str()).width();
        let offset = if self.focus == 1 {
            width.saturating_sub(usize::from(path.width))
        } else {
            0
        };
        ui.f.render_widget(
            ratatui::widgets::Paragraph::new(shown)
                .style(ratatui::style::Style::new().fg(theme::TEXT))
                .scroll((0, u16::try_from(offset).unwrap_or(u16::MAX))),
            path,
        );
    }
    fn details(
        &self,
        ui: &mut crate::backup_center::Controls<'_, '_>,
        area: ratatui::layout::Rect,
    ) {
        use crate::backup_center::{block, inner, row};
        use ratatui::{
            text::Line,
            widgets::{Paragraph, Wrap},
        };
        block(ui.f, area, ui.m.get("restore-details"));
        let mut lines = vec![
            Line::from(
                self.entries
                    .get(self.selected)
                    .map_or_else(|| ui.m.get("restore-select"), |s| text(s)),
            ),
            Line::default(),
        ];
        let help = if self.kind == Kind::Key {
            "restore-key-detail"
        } else if self.kind == Kind::Directory {
            "folder-use"
        } else {
            "restore-archive-detail"
        };
        lines.push(Line::from(ui.m.get(help)));
        ui.f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            row(inner(area), 1, area.height.saturating_sub(1)),
        );
    }

    fn rows(&self, ui: &mut crate::backup_center::Controls<'_, '_>, area: ratatui::layout::Rect) {
        use crate::backup_center::row;
        use crate::ui::{Action, Button, Variant};
        let r = if area.width >= 108 {
            let [files, details] = ratatui::layout::Layout::horizontal([
                ratatui::layout::Constraint::Percentage(63),
                ratatui::layout::Constraint::Fill(1),
            ])
            .spacing(2)
            .areas(area);
            self.details(ui, details);
            files
        } else {
            area
        };
        let capacity = usize::from(r.height).max(1);
        let first = self.selected.saturating_sub(capacity - 1);
        for ((i, name), y) in self
            .entries
            .iter()
            .enumerate()
            .skip(first)
            .take(capacity)
            .zip(0u16..)
        {
            Button::new(
                format!(
                    "{}{}",
                    if i == self.selected && self.focus == 0 {
                        "> "
                    } else {
                        ""
                    },
                    text(name)
                ),
                Action::PickerEntry(i),
                if i == self.selected && self.focus == 0 {
                    Variant::Primary
                } else {
                    Variant::Secondary
                },
            )
            .draw(ui.f, row(r, y, 1), ui.scene);
        }
    }
    pub(super) fn draw(
        &self,
        frame: &mut Frame,
        m: &Messages,
        ascii: bool,
        scene: &mut crate::ui::Scene,
    ) {
        use crate::backup_center::{Controls, inner, line, row};
        use crate::ui::{Action, Variant, theme};
        let area = frame.area();
        frame.render_widget(Clear, area);
        let title = if self.kind == Kind::Directory {
            "folder-title"
        } else {
            "restore-file-title"
        };
        let border = crate::view::block(m.get(title), ascii)
            .title_style(ratatui::style::Style::new().fg(theme::TEXT));
        let r = inner(border.inner(area));
        frame.render_widget(border, area);
        let mut ui = Controls { f: frame, scene, m };
        self.places(&mut ui, row(r, 0, 1));
        self.path_bar(&mut ui, row(r, 2, 1));
        let status = if !self.notice.is_empty() {
            self.notice
        } else if self.catalog {
            "restore-managed"
        } else if self.kind == Kind::Archive && !self.all {
            "restore-filter-age"
        } else {
            "restore-host"
        };
        line(ui.f, row(r, 3, 1), m.get(status), theme::MUTED, false);
        self.rows(&mut ui, row(r, 4, r.height.saturating_sub(6)));
        line(
            ui.f,
            row(r, r.height.saturating_sub(2), 1),
            m.get("restore-navigation"),
            theme::MUTED,
            false,
        );
        let mut actions = vec![
            (
                "restore-location",
                Action::Field(1),
                Variant::Secondary,
                true,
            ),
            (
                if self.kind == Kind::Directory {
                    "folder-use"
                } else {
                    "restore-file-use"
                },
                Action::Field(2),
                if self.focus == 2 {
                    Variant::Primary
                } else {
                    Variant::Secondary
                },
                true,
            ),
        ];
        if self.kind == Kind::Archive {
            actions.push((
                "restore-filter",
                Action::Key(KeyCode::F(5)),
                Variant::Secondary,
                true,
            ));
        }
        actions.push((
            "desk-cancel",
            Action::Key(KeyCode::Esc),
            Variant::Quiet,
            true,
        ));
        ui.buttons(row(r, r.height.saturating_sub(1), 1), &actions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_a_path_discards_the_previous_directory_listing() {
        let root = tempfile::tempdir().unwrap();
        let mut picker = Picker::files(root.path().to_owned(), false, &[]);
        picker.key(KeyCode::Tab);
        picker.paste("/edited");
        assert!(picker.pending.is_none());
        picker.poll();
        assert!(picker.entries.is_empty());
        assert_eq!(picker.path, "/edited");
    }

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
