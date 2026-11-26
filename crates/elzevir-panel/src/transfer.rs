// SPDX-License-Identifier: GPL-3.0-or-later
//! A user-reviewed download command for a single catalogued encrypted backup.
use crate::{
    LocalBackup, Messages, Mode, Operations, Panel, Section,
    access_actions::Actions,
    ui::{Action, Scene, card, theme},
};
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Rect},
    text::Line,
    widgets::Clear,
};
use std::path::{Path, PathBuf};

fn ssh_name(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

fn command(endpoint: &str, archive: &Path) -> Option<String> {
    let (user, host) = endpoint
        .split_once('@')
        .map_or((None, endpoint), |(u, h)| (Some(u), h));
    let ipv6 = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .is_some_and(|h| h.parse::<std::net::Ipv6Addr>().is_ok());
    if user.is_some_and(|u| !ssh_name(u)) || !(ssh_name(host) || ipv6) {
        return None;
    }
    let mut path = archive.to_str()?.to_owned();
    if cfg!(windows) && !path.starts_with('/') {
        path = format!("/{}", path.replace('\\', "/"));
    }
    if !path.starts_with('/')
        || !path
            .chars()
            .all(|c| c.is_alphanumeric() || "/. _-:".contains(c))
    {
        return None;
    }
    Some(format!("scp '{endpoint}:{path}' ."))
}

pub(crate) struct Dialog {
    folder: Option<PathBuf>,
    archive: Option<PathBuf>,
    endpoint: String,
    notice: Option<&'static str>,
}

impl Dialog {
    fn new(ops: Option<&Operations>) -> Self {
        let folder = ops
            .and_then(|o| o.backup_folder.as_ref())
            .filter(|p| p.is_absolute())
            .cloned();
        let id = ops
            .filter(|o| o.local_backup == LocalBackup::Verified)
            .and_then(|o| o.managed_ids.last())
            .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()));
        let archive = folder
            .as_ref()
            .zip(id)
            .map(|(folder, id)| folder.join(format!("elzevir-{id}.age")));
        Self {
            folder,
            archive,
            endpoint: String::new(),
            notice: None,
        }
    }

    pub(crate) fn draw(
        &self,
        frame: &mut Frame,
        m: &Messages,
        scene: &mut Scene,
        ascii: bool,
        desktop: bool,
    ) {
        scene.clear();
        let area = frame
            .area()
            .centered(Constraint::Length(96), Constraint::Length(17));
        frame.render_widget(Clear, area);
        let cmd = self
            .archive
            .as_deref()
            .and_then(|p| command(&self.endpoint, p));
        let rows = vec![
            Line::from(m.get("transfer-instructions")),
            Line::default(),
            Line::from(m.values(
                "transfer-host",
                &[(
                    "value",
                    format!("{}{}", self.endpoint, if ascii { "_" } else { "▏" }),
                )],
            ))
            .style(theme::surface().fg(theme::ACCENT)),
            Line::from(m.get("transfer-alias")),
            Line::default(),
            Line::from(cmd.as_deref().map_or_else(
                || {
                    m.get(if self.archive.is_none() {
                        "transfer-no-file"
                    } else if self.endpoint.is_empty() {
                        "transfer-host-needed"
                    } else {
                        "transfer-invalid"
                    })
                },
                str::to_owned,
            )),
            Line::default(),
            Line::from(self.notice.map(|n| m.get(n)).unwrap_or_default()),
        ];
        card(
            frame,
            area,
            &m.get("transfer-title"),
            rows,
            theme::ACCENT,
            ascii,
            0,
        );
        let actions = [
            cmd.is_some().then_some(("transfer-copy", KeyCode::F(2))),
            (desktop && self.folder.is_some()).then_some(("transfer-folder", KeyCode::F(3))),
            Some(("ui-cancel", KeyCode::Esc)),
        ];
        scene.buttons(
            frame,
            Rect::new(
                area.x + 1,
                area.bottom() - 2,
                area.width.saturating_sub(2),
                1,
            ),
            actions
                .into_iter()
                .flatten()
                .map(|(label, code)| (m.get(label), Action::Key(code), code == KeyCode::F(2))),
        );
    }
}

/// Routes keys and visible button hits to the same operator action.
pub(crate) fn handle(
    dialog: &mut Option<Dialog>,
    input: &Event,
    panel: &Panel,
    actions: &mut Actions,
) -> bool {
    let code = match input {
        Event::Key(key)
            if key.kind != KeyEventKind::Release
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(key.code)
        }
        Event::Key(key)
            if key.code == KeyCode::Char('c')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && dialog.is_some() =>
        {
            Some(KeyCode::Esc)
        }
        _ => None,
    };
    if let Some(active) = dialog {
        if let Event::Paste(text) = input
            && text.len() <= 250
            && text.chars().all(|c| c.is_ascii_graphic())
        {
            active.endpoint.clone_from(text);
            active.notice = None;
        }
        match code {
            Some(KeyCode::Esc) => *dialog = None,
            Some(KeyCode::F(2)) => {
                if let Some(cmd) = active
                    .archive
                    .as_deref()
                    .and_then(|p| command(&active.endpoint, p))
                {
                    active.notice = Some(if actions.copy(&cmd) {
                        "address-copy-requested"
                    } else {
                        "transfer-select"
                    });
                }
            }
            Some(KeyCode::F(3)) => {
                active.notice = Some(actions.open(active.folder.as_deref().map(Path::as_os_str)));
            }
            Some(KeyCode::Backspace) => {
                active.endpoint.pop();
                active.notice = None;
            }
            Some(KeyCode::Char(c)) if active.endpoint.len() < 250 && c.is_ascii_graphic() => {
                active.endpoint.push(c);
                active.notice = None;
            }
            _ => {}
        }
        return true;
    }
    if code != Some(KeyCode::Char('z'))
        || panel.section != Section::Backup
        || panel.inspection.is_some()
        || panel.form.is_some()
        || panel.palette
    {
        return false;
    }
    let Mode::Live(state) = &panel.mode else {
        return false;
    };
    *dialog = Some(Dialog::new(
        state
            .snapshot
            .as_ref()
            .filter(|_| state.age_seconds <= 5)
            .map(|s| &s.operations),
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_single_literal_remote_archive_can_be_offered_to_the_local_shell() {
        let file = Path::new("/srv/biblioteca azul/copia.age");
        assert_eq!(
            command("ana@biblioteca", file).as_deref(),
            Some("scp 'ana@biblioteca:/srv/biblioteca azul/copia.age' .")
        );
        assert!(command("usuario@[::1]", file).is_some());
        for invalid in [
            "",
            "-v",
            "user@-o",
            "user@example@test",
            "two hosts",
            "host;date",
            "$(other)",
        ] {
            assert!(command(invalid, file).is_none());
        }
        for unsafe_path in [
            "/srv/it's/copia.age",
            "/srv/*.age",
            "/srv/file\n.age",
            "/srv/$name.age",
        ] {
            assert!(command("biblioteca", Path::new(unsafe_path)).is_none());
        }
    }
}
