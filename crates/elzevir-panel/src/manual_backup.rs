// SPDX-License-Identifier: GPL-3.0-or-later
//! Operator-host destination picker for creating and exporting an encrypted backup.
use crate::{
    Messages, Mode, Operation, Panel, Section,
    access_actions::Actions,
    backup_center::{Controls, block, bytes, columns, inner, line, row},
    dashboard::text,
    ui::{self, Action, Scene, Variant, theme},
};
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind},
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Back,
    Up,
    Refresh,
    Info,
    Places,
    Place(usize),
    FolderName,
    Path,
    Go,
    Make,
    Entry(usize),
    Scroll(bool),
    Open,
    Scope,
    Profile,
    Submit,
    Cancel,
}

struct Entry {
    name: OsString,
    directory: bool,
    size: u64,
    date: Option<String>,
}
struct Listing {
    folder: PathBuf,
    entries: Vec<Entry>,
    free: Option<u64>,
    limited: bool,
}
type ReadResult = Result<Listing, &'static str>;

fn safe_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
        && matches!(parts.next(), Some(Component::Normal(_)))
        && parts.next().is_none()
}

fn read_folder(path: &Path, managed: &Path, child: Option<&str>) -> ReadResult {
    let mut folder = path.to_owned();
    if !folder.is_absolute() {
        return Err("mb-invalid");
    }
    if let Some(name) = child {
        if !safe_name(name) {
            return Err("mb-invalid-name");
        }
        folder = folder.join(name);
        std::fs::create_dir(&folder).map_err(|_| "mb-mkdir-failed")?;
    }
    let mut entries = Vec::new();
    let mut limited = false;
    match std::fs::read_dir(&folder) {
        Ok(files) => {
            for (i, item) in files.take(513).enumerate() {
                let entry = item.map_err(|_| "folder-error")?;
                if i == 512 {
                    limited = true;
                    break;
                }
                let kind = entry.file_type().map_err(|_| "folder-error")?;
                if !(kind.is_dir() || kind.is_file()) {
                    continue;
                }
                let meta = entry.metadata().map_err(|_| "folder-error")?;
                entries.push(Entry {
                    name: entry.file_name(),
                    directory: kind.is_dir(),
                    size: meta.len(),
                    date: meta
                        .modified()
                        .ok()
                        .and_then(|t| jiff::Timestamp::try_from(t).ok())
                        .map(|t| t.strftime("%F %R").to_string()),
                });
            }
        }
        Err(e)
            if folder == managed && e.kind() == std::io::ErrorKind::NotFound && child.is_none() => {
        }
        Err(_) => return Err("folder-error"),
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.cmp(&b.name))
    });
    // The managed destination can be absent until the first backup is published.
    let measure = if folder == managed && !folder.exists() {
        folder.parent().unwrap_or(&folder)
    } else {
        &folder
    };
    let free = fs4::available_space(measure).ok();
    Ok(Listing {
        folder,
        entries,
        free,
        limited,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit {
    Path,
    Name,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CompactView {
    Files,
    Places,
    Info,
}

pub(crate) struct Manual {
    managed: PathBuf,
    places: Vec<(&'static str, PathBuf)>,
    history: Vec<PathBuf>,
    listing: Option<Listing>,
    request: Option<Receiver<ReadResult>>,
    back_after_read: bool,
    path: String,
    name: String,
    edit: Option<Edit>,
    replace: bool,
    selected: Option<usize>,
    offset: usize,
    notice: Option<&'static str>,
    read_error: Option<&'static str>,
    compact: CompactView,
    remote: bool,
}
impl Manual {
    pub(crate) fn active(panel: &Panel) -> bool {
        panel.section == Section::Backup
            && panel.form.as_ref().is_some_and(|f| {
                matches!(
                    f.operation,
                    Operation::BackupNow { .. } | Operation::ExportArtifact { .. }
                )
            })
    }
    pub(crate) fn start(panel: &Panel) -> Option<Self> {
        let Mode::Live(state) = &panel.mode else {
            return None;
        };
        let managed = state.snapshot.as_ref()?.operations.backup_folder.clone()?;
        let mut places = vec![("mb-managed", managed.clone())];
        if let Some(home) = std::env::home_dir() {
            places.push(("mb-home", home.clone()));
            places.push(("mb-docs", home.join("Documents")));
            places.push(("mb-downloads", home.join("Downloads")));
        }
        if let Some(root) = managed.ancestors().last() {
            places.push(("mb-root", root.to_owned()));
        }
        #[cfg(windows)]
        for letter in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", char::from(letter)));
            if root.is_dir() && !places.iter().any(|(_, p)| *p == root) {
                places.push(("mb-drive", root));
            }
        }
        #[cfg(unix)]
        for path in ["/media", "/mnt"] {
            if Path::new(path).is_dir() {
                places.push(("mb-drive", path.into()));
            }
        }
        let mut picker = Self {
            managed: managed.clone(),
            places,
            history: Vec::new(),
            listing: None,
            request: None,
            back_after_read: false,
            path: String::new(),
            name: String::new(),
            edit: None,
            replace: false,
            selected: None,
            offset: 0,
            notice: None,
            read_error: None,
            compact: CompactView::Files,
            remote: ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"]
                .iter()
                .any(|k| std::env::var_os(k).is_some()),
        };
        picker.load(managed, None, false);
        Some(picker)
    }
    fn load(&mut self, path: PathBuf, child: Option<String>, back: bool) {
        if self.request.is_some() {
            return;
        }
        self.edit = None;
        self.notice = None;
        self.back_after_read = back;
        let managed = self.managed.clone();
        let (tx, rx) = mpsc::channel();
        self.request = Some(rx);
        std::thread::spawn(move || {
            let answer = read_folder(&path, &managed, child.as_deref());
            drop(tx.send(answer));
        });
    }
    pub(crate) fn poll(&mut self) {
        let Some(rx) = &self.request else { return };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("folder-error"),
        };
        self.request = None;
        self.read_error = result.as_ref().err().copied();
        if let Ok(next) = result {
            if self.back_after_read {
                self.history.pop();
            } else if let Some(old) = &self.listing
                && old.folder != next.folder
            {
                if self.history.len() >= 64 {
                    self.history.remove(0);
                }
                self.history.push(old.folder.clone());
            }
            self.path = next.folder.to_string_lossy().into_owned();
            self.notice = next.limited.then_some("mb-limit");
            self.listing = Some(next);
            self.selected = None;
            self.offset = 0;
            self.name.clear();
            self.compact = CompactView::Files;
        }
    }
    fn ready(&self, panel: &Panel) -> bool {
        let export = panel
            .form
            .as_ref()
            .is_some_and(|f| matches!(f.operation, Operation::ExportArtifact { .. }));
        self.request.is_none()
            && self.edit.is_none()
            && !panel.diagnostics.pending
            && self.read_error.is_none()
            && self.listing.as_ref().is_some_and(|l| {
                l.folder.to_str() == Some(self.path.as_str())
                    && (!export || l.folder != self.managed)
                    && l.free != Some(0)
            })
    }
    fn activate(&mut self, cmd: Control, panel: &mut Panel, actions: &mut Actions) {
        if cmd == Control::Cancel {
            panel.form = None;
            return;
        }
        if self.request.is_some() {
            return;
        }
        let here = self
            .listing
            .as_ref()
            .map_or_else(|| self.managed.clone(), |l| l.folder.clone());
        match cmd {
            Control::Path => {
                self.edit = Some(Edit::Path);
                self.replace = true;
            }
            Control::FolderName => {
                self.edit = Some(Edit::Name);
                self.name.clear();
                self.replace = false;
            }
            Control::Info => {
                self.compact = if self.compact == CompactView::Info {
                    CompactView::Files
                } else {
                    CompactView::Info
                }
            }
            Control::Places => {
                self.compact = if self.compact == CompactView::Places {
                    CompactView::Files
                } else {
                    CompactView::Places
                }
            }
            Control::Go => self.load(PathBuf::from(&self.path), None, false),
            Control::Make => {
                if safe_name(&self.name) {
                    self.load(here, Some(self.name.clone()), false);
                } else {
                    self.notice = Some("mb-invalid-name");
                }
            }
            Control::Up => {
                if let Some(parent) = here.parent() {
                    self.load(parent.to_owned(), None, false);
                }
            }
            Control::Back => {
                if let Some(path) = self.history.last() {
                    self.load(path.clone(), None, true);
                }
            }
            Control::Refresh => self.load(here, None, false),
            Control::Place(i) => {
                if let Some((_, p)) = self.places.get(i) {
                    self.load(p.clone(), None, false);
                }
            }
            Control::Entry(i) => {
                if let Some(entry) = self.listing.as_ref().and_then(|l| l.entries.get(i)) {
                    if entry.directory {
                        self.load(here.join(&entry.name), None, false);
                    } else {
                        self.selected = Some(i);
                    }
                }
            }
            Control::Open => self.notice = Some(actions.open(Some(here.as_os_str()))),
            Control::Scope | Control::Profile => {
                if let Some(f) = &mut panel.form {
                    let (idx, choices): (usize, &[&str]) = if cmd == Control::Scope {
                        (0, &["full", "database"])
                    } else {
                        (1, &["fast", "balanced", "compact"])
                    };
                    if let Some(value) = f.values.get_mut(idx) {
                        let i = choices.iter().position(|x| *x == value).unwrap_or(0);
                        if let Some(next) = choices.get(i + 1).or(choices.first()) {
                            (*next).clone_into(value);
                        }
                    }
                }
            }
            Control::Submit if self.ready(panel) => {
                if let Some(f) = &mut panel.form
                    && let Some(v) = f.values.last_mut()
                {
                    if here == self.managed {
                        v.clear();
                    } else {
                        self.path.clone_into(v);
                    }
                    panel.submit_form();
                }
            }
            Control::Submit | Control::Cancel | Control::Scroll(_) => {}
        }
    }
    fn edit_text(&mut self, value: &str) {
        if value.chars().any(char::is_control) {
            return;
        }
        let Some(which) = self.edit else { return };
        let field = if which == Edit::Path {
            &mut self.path
        } else {
            &mut self.name
        };
        if self.replace {
            field.clear();
            self.replace = false;
        }
        if field.chars().count() + value.chars().count() <= 512 {
            field.push_str(value);
        }
    }
    pub(crate) fn input(
        &mut self,
        event: &Event,
        scene: &mut Scene,
        panel: &mut Panel,
        actions: &mut Actions,
    ) {
        let action = match event {
            Event::Mouse(mouse) => {
                let point = (mouse.column, mouse.row).into();
                scene.pointer = Some(point);
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        scene.press(point);
                        None
                    }
                    MouseEventKind::Up(MouseButton::Left) => scene.release(point),
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => Some(Action::File(
                        Control::Scroll(mouse.kind == MouseEventKind::ScrollDown),
                    )),
                    _ => None,
                }
            }
            Event::Resize(..) | Event::FocusLost => {
                scene.reset_input();
                None
            }
            Event::Paste(text) => {
                self.edit_text(text);
                None
            }
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc => {
                    if self.edit.take().is_some() {
                        self.path = self
                            .listing
                            .as_ref()
                            .map_or_else(String::new, |l| l.folder.to_string_lossy().into_owned());
                        self.notice = None;
                        None
                    } else {
                        Some(Action::File(Control::Cancel))
                    }
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    scene.focus_next(k.code == KeyCode::Tab);
                    None
                }
                KeyCode::Enter if self.edit.is_some() && scene.focus().is_none() => {
                    Some(Action::File(if self.edit == Some(Edit::Name) {
                        Control::Make
                    } else {
                        Control::Go
                    }))
                }
                KeyCode::Enter | KeyCode::Char(' ') if scene.focus().is_some() => scene.focus(),
                KeyCode::Backspace if self.edit.is_some() => {
                    let f = if self.edit == Some(Edit::Path) {
                        &mut self.path
                    } else {
                        &mut self.name
                    };
                    if self.replace {
                        f.clear();
                        self.replace = false;
                    } else {
                        f.pop();
                    }
                    None
                }
                KeyCode::Char(c) => {
                    self.edit_text(&c.to_string());
                    None
                }
                KeyCode::Up | KeyCode::Down => {
                    Some(Action::File(Control::Scroll(k.code == KeyCode::Down)))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(Action::File(c)) = action {
            if matches!(c, Control::FolderName | Control::Path) {
                scene.reset_input();
            }
            if let Control::Scroll(down) = c {
                self.offset = self.offset.min(usize::from(scene.scroll_limit));
                self.offset = if down {
                    self.offset
                        .saturating_add(1)
                        .min(usize::from(scene.scroll_limit))
                } else {
                    self.offset.saturating_sub(1)
                };
            } else {
                self.activate(c, panel, actions);
            }
        }
    }
}

fn file(cmd: Control) -> Action {
    Action::File(cmd)
}
impl Manual {
    pub(crate) fn draw(&self, f: &mut Frame, panel: &Panel, m: &Messages, scene: &mut Scene) {
        scene.clear();
        let r = inner(f.area());
        ui::fill(f, f.area(), theme::muted());
        let wide = r.width >= 110;
        let large = wide && r.height >= 36;
        let export = panel
            .form
            .as_ref()
            .is_some_and(|form| matches!(form.operation, Operation::ExportArtifact { .. }));
        let title = if export {
            "bc-copy-selected"
        } else {
            "mb-title"
        };
        line(
            f,
            r,
            format!(
                "ELZEVIR   /   {}   /   {}",
                m.get("desk-backups"),
                m.get(title)
            ),
            theme::WARNING,
            true,
        );
        line(
            f,
            row(r, 2, 1),
            m.get(if self.remote { "mb-ssh" } else { "mb-local" }),
            theme::MUTED,
            false,
        );
        let tail = if large { 8 } else { 5 };
        let work = row(r, 4, r.height.saturating_sub(4 + tail));
        let mut ui = Controls { f, scene, m };
        if wide {
            let [places, list, context] = Layout::horizontal([
                Constraint::Percentage(17),
                Constraint::Percentage(53),
                Constraint::Fill(1),
            ])
            .spacing(1)
            .areas(work);
            self.places(&mut ui, places);
            self.directory(&mut ui, list);
            self.context(&mut ui, context, panel);
        } else {
            let [local, list] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
                .spacing(1)
                .areas(work);
            ui.buttons(
                local,
                &[
                    ("mb-places", file(Control::Places), Variant::Secondary, true),
                    ("mb-format", file(Control::Info), Variant::Secondary, true),
                ],
            );
            match self.compact {
                CompactView::Info => self.context(&mut ui, list, panel),
                CompactView::Places => self.places(&mut ui, list),
                CompactView::Files => self.directory(&mut ui, list),
            }
        }
        self.footer(&mut ui, row(r, r.height - tail, tail), panel, export, large);
    }
    fn footer(
        &self,
        ui: &mut Controls<'_, '_>,
        bottom: Rect,
        panel: &Panel,
        export: bool,
        large: bool,
    ) {
        let m = ui.m;
        let tail = bottom.height;
        let state = panel.form.as_ref();
        if !export {
            let scope = if state
                .and_then(|f| f.values.first())
                .is_some_and(|v| v == "database")
            {
                "backup-db"
            } else {
                "backup-full"
            };
            let profile = match state.and_then(|f| f.values.get(1)).map(String::as_str) {
                Some("fast") => "backup-fast",
                Some("compact") => "backup-compact",
                _ => "backup-balanced",
            };
            let slots = columns(row(bottom, 0, 1), 2);
            for ((label, value, cmd), slot) in [
                ("backup-scope", scope, Control::Scope),
                ("mb-profile", profile, Control::Profile),
            ]
            .into_iter()
            .zip(slots)
            {
                ui::Button::new(
                    format!("{} · {}", m.get(label), m.get(value)),
                    file(cmd),
                    Variant::Secondary,
                )
                .draw(ui.f, slot, ui.scene);
            }
        }
        let feedback = self
            .notice
            .or(self.read_error)
            .or(self.request.as_ref().map(|_| "folder-loading"));
        let archive_name = match state.map(|f| &f.operation) {
            Some(Operation::ExportArtifact { id, .. }) => format!("elzevir-{}.age", text(id)),
            _ => m.get("mb-file-name"),
        };
        line(
            ui.f,
            row(bottom, 1, 1),
            self.selected
                .and_then(|i| self.listing.as_ref()?.entries.get(i))
                .filter(|_| feedback.is_none())
                .map_or_else(
                    || feedback.map_or(archive_name, |key| m.get(key)),
                    |e| format!("{} · {}", text(&e.name.to_string_lossy()), bytes(e.size)),
                ),
            if self.notice.is_some() || self.read_error.is_some() {
                theme::WARNING
            } else {
                theme::MUTED
            },
            false,
        );
        let bh = if large { 3 } else { 2 };
        ui.buttons(
            row(bottom, tail - bh, bh),
            &[
                (
                    "desk-cancel",
                    file(Control::Cancel),
                    Variant::Secondary,
                    true,
                ),
                (
                    if export { "bc-copy-out" } else { "desk-create" },
                    file(Control::Submit),
                    Variant::Primary,
                    self.ready(panel),
                ),
            ],
        );
    }
    fn places(&self, ui: &mut Controls<'_, '_>, r: Rect) {
        block(ui.f, r, ui.m.get("mb-places"));
        let t = inner(r);
        for (i, ((key, path), y)) in self.places.iter().zip(1u16..r.height).enumerate() {
            let title = if *key == "mb-drive" {
                text(&path.to_string_lossy())
            } else {
                ui.m.get(key)
            };
            ui::Button::new(
                title,
                file(Control::Place(i)),
                if self.listing.as_ref().is_some_and(|l| l.folder == *path) {
                    Variant::Secondary
                } else {
                    Variant::Quiet
                },
            )
            .draw(ui.f, row(t, y, 1), ui.scene);
        }
    }
    fn context(&self, ui: &mut Controls<'_, '_>, r: Rect, panel: &Panel) {
        let m = ui.m;
        block(ui.f, r, m.get("mb-format"));
        let t = inner(r);
        let s = match &panel.mode {
            Mode::Live(st) if st.age_seconds <= 5 => st.snapshot.as_ref(),
            _ => None,
        };
        let estimate = s.and_then(|s| {
            s.storage.database_bytes.map(|db| {
                db.saturating_add(s.storage.wal_bytes.unwrap_or(0))
                    .saturating_mul(3)
            })
        });
        let free = self.listing.as_ref().and_then(|l| l.free);
        let lines = [
            m.get("mb-native"),
            m.get("mb-contents"),
            m.get("mb-index"),
            m.get("bc-key-note"),
            m.values(
                "home-space",
                &[("value", free.map_or_else(|| m.get("panel-unknown"), bytes))],
            ),
            m.values(
                "backup-estimate",
                &[(
                    "value",
                    estimate.map_or_else(|| m.get("panel-unknown"), bytes),
                )],
            ),
        ];
        let height = t.height.saturating_sub(1);
        ui.f.render_widget(
            Paragraph::new(lines.join(if height < 18 { "\n" } else { "\n\n" }))
                .style(theme::surface())
                .wrap(Wrap { trim: true }),
            row(t, 1, height),
        );
    }
    fn directory(&self, ui: &mut Controls<'_, '_>, r: Rect) {
        let m = ui.m;
        let ready = self.request.is_none();
        block(ui.f, r, m.get("mb-explorer"));
        let t = inner(r);
        ui.buttons(
            row(t, 1, 1),
            &[
                (
                    "mb-back",
                    file(Control::Back),
                    Variant::Secondary,
                    !self.history.is_empty() && ready,
                ),
                ("mb-parent", file(Control::Up), Variant::Secondary, ready),
                (
                    "mb-new",
                    file(Control::FolderName),
                    Variant::Secondary,
                    ready,
                ),
                (
                    "desk-open",
                    file(Control::Open),
                    Variant::Secondary,
                    ready && ui.scene.desktop,
                ),
            ],
        );
        let name = self.edit == Some(Edit::Name);
        let pathrow = row(t, 3, 1);
        let [input, go] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(12)])
            .spacing(1)
            .areas(pathrow);
        let field = if name {
            Control::FolderName
        } else {
            Control::Path
        };
        ui.scene.add(input, file(field));
        let shown = if name {
            format!("{} {}", m.get("mb-name"), text(&self.name))
        } else if self.path.is_empty() {
            text(&self.managed.to_string_lossy())
        } else {
            text(&self.path)
        };
        let width = ratatui::text::Line::from(shown.as_str()).width();
        ui.f.render_widget(
            Paragraph::new(shown)
                .style(Style::new().fg(theme::TEXT).bg(if self.edit.is_some() {
                    theme::STEEL
                } else {
                    theme::BACKGROUND
                }))
                .scroll((
                    0,
                    u16::try_from(width)
                        .unwrap_or(u16::MAX)
                        .saturating_sub(input.width),
                )),
            input,
        );
        ui.buttons(
            go,
            &[(
                if name { "mb-make" } else { "mb-go" },
                file(if name { Control::Make } else { Control::Go }),
                Variant::Secondary,
                ready,
            )],
        );
        self.files(ui, r);
    }
    fn files(&self, ui: &mut Controls<'_, '_>, r: Rect) {
        let t = inner(r);
        let m = ui.m;
        let ready = self.request.is_none();
        line(ui.f, row(t, 5, 1), m.get("mb-columns"), theme::MUTED, true);
        let entries = self
            .listing
            .as_ref()
            .map_or(&[][..], |l| l.entries.as_slice());
        let page = usize::from(t.height.saturating_sub(8)).max(1);
        ui.scene.scroll_limit =
            u16::try_from(entries.len().saturating_sub(page)).unwrap_or(u16::MAX);
        let start = self.offset.min(usize::from(ui.scene.scroll_limit));
        if entries.is_empty() {
            line(
                ui.f,
                row(t, 6, 1),
                m.get(if ready {
                    "mb-no-files"
                } else {
                    "folder-loading"
                }),
                theme::MUTED,
                false,
            );
        }
        for (y, (i, e)) in (6u16..).zip(entries.iter().enumerate().skip(start).take(page)) {
            let slot = row(t, y, 1);
            let cols = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Length(11),
                Constraint::Length(16),
            ])
            .areas::<3>(slot);
            let hot = ready && ui.scene.add(slot, file(Control::Entry(i)));
            if hot || ui.scene.focus() == Some(file(Control::Entry(i))) || self.selected == Some(i)
            {
                ui::fill(ui.f, slot, theme::surface().bg(theme::STEEL));
            }
            line(
                ui.f,
                cols[0],
                format!(
                    "{} {}",
                    if e.directory { ">" } else { " " },
                    text(&e.name.to_string_lossy())
                ),
                if e.directory {
                    theme::ACCENT
                } else {
                    theme::TEXT
                },
                e.directory,
            );
            line(
                ui.f,
                cols[1],
                if e.directory {
                    m.get("mb-folder")
                } else {
                    bytes(e.size)
                },
                theme::MUTED,
                false,
            );
            line(
                ui.f,
                cols[2],
                e.date.clone().unwrap_or_default(),
                theme::MUTED,
                false,
            );
        }
        let at = t.height.saturating_sub(1);
        ui.buttons(
            row(t, at, 1),
            &[
                (
                    "desk-up",
                    file(Control::Scroll(false)),
                    Variant::Quiet,
                    start > 0,
                ),
                ("mb-refresh", file(Control::Refresh), Variant::Quiet, ready),
                (
                    "desk-down",
                    file(Control::Scroll(true)),
                    Variant::Quiet,
                    start + page < entries.len(),
                ),
            ],
        );
        if page < entries.len() && t.height > 9 {
            Self::scrollbar(ui, r, page, start, entries.len());
        }
    }

    fn scrollbar(ui: &mut Controls<'_, '_>, r: Rect, page: usize, start: usize, total: usize) {
        let track = Rect::new(r.right() - 1, r.y + 6, 1, u16::try_from(page).unwrap_or(1));
        ui::fill(ui.f, track, theme::surface().bg(theme::BACKGROUND));
        let thumb = (page * page / total).max(1);
        let pos = (start * (page - thumb))
            .checked_div(total - page)
            .unwrap_or(0);
        ui::fill(
            ui.f,
            row(
                track,
                u16::try_from(pos).unwrap_or(0),
                u16::try_from(thumb).unwrap_or(1),
            ),
            Style::new().bg(theme::STEEL),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_creates_only_an_explicit_child_and_reads_existing_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("backups");
        assert!(read_folder(&managed, &managed, None).is_ok());
        assert!(!managed.exists());
        for name in [
            "..",
            ".",
            "../external",
            "one/two",
            "C:another",
            "line\nbreak",
        ] {
            assert!(read_folder(tmp.path(), &managed, Some(name)).is_err());
        }
        let folder = read_folder(tmp.path(), &managed, Some("Revisão")).unwrap();
        assert_eq!(folder.folder, tmp.path().join("Revisão"));
        assert!(read_folder(tmp.path(), &managed, Some("Revisão")).is_err());
        assert!(read_folder(&tmp.path().join("missing"), &managed, None).is_err());
    }
}
