// SPDX-License-Identifier: GPL-3.0-or-later
//! The operator's overview of observed health, retained backups and listeners.
use crate::{
    Inspection, LiveState, LocalBackup, Messages, Mode, Operations, Panel, Section, ServerStatus,
    Snapshot, TunnelState,
    dashboard::{row, text},
    ui::{self, Action, Availability, Button, Intent, Scene, Variant, theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
};

fn bytes(n: u64) -> String {
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    if n >= GIB {
        format!("{}.{:01} GiB", n / GIB, (n % GIB) * 10 / GIB)
    } else if n >= MIB {
        format!("{}.{:01} MiB", n / MIB, (n % MIB) * 10 / MIB)
    } else if n >= 1024 {
        format!("{} KiB", n / 1024)
    } else {
        format!("{n} B")
    }
}

fn emphasis(s: String, color: Color) -> Line<'static> {
    Line::from(s).style(Style::new().fg(color).add_modifier(Modifier::BOLD))
}

fn protection(ops: Option<&Operations>, m: &Messages) -> Vec<Line<'static>> {
    let (key, color) = match ops.map(|o| o.local_backup).unwrap_or_default() {
        LocalBackup::Verified => ("home-verified", theme::SUCCESS),
        LocalBackup::Empty => ("home-no-backup", theme::WARNING),
        LocalBackup::Unknown => ("home-unknown-backup", theme::MUTED),
    };
    let mut rows = vec![emphasis(m.get(key), color)];
    if let Some(ops) = ops {
        if let Some(job) = ops
            .jobs
            .iter()
            .find(|job| matches!(job.action, "op-backup-now" | "cloud-backup"))
        {
            rows.push(row(
                m,
                "home-attempt",
                Some(format!("{} · {}", m.get(job.state), text(&job.at))),
            ));
        }
        if ops.latest_export_failed == Some(true) {
            rows.push(emphasis(m.get("home-export-failed"), theme::WARNING));
        }
        let remote = ops.cloud.as_ref().map(|cloud| {
            cloud
                .transfers
                .first()
                .map_or_else(|| m.get("home-no-transfer"), |(_, _, status)| m.get(status))
        });
        rows.push(row(m, "home-cloud", remote));
        if let Some(cloud) = &ops.cloud {
            let pending = cloud
                .transfers
                .iter()
                .skip(1)
                .filter(|(_, _, s)| matches!(s.as_str(), "op-queued" | "cloud-unconfirmed"))
                .count();
            if pending > 0 {
                rows.push(row(m, "home-older-pending", Some(pending.to_string())));
            }
        }
        if let Some(daily) = &ops.daily {
            rows.push(row(m, "home-daily", Some(text(daily))));
        }
    }
    rows
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Endpoint {
    Catalog,
    Staff,
    LanCatalog,
    LanStaff,
    PublicCatalog,
    PublicStaff,
}

impl Endpoint {
    const ALL: [Self; 6] = [
        Self::Catalog,
        Self::Staff,
        Self::LanCatalog,
        Self::LanStaff,
        Self::PublicCatalog,
        Self::PublicStaff,
    ];
    fn key(self) -> &'static str {
        match self {
            Self::Catalog => "desk-local-catalog",
            Self::Staff => "desk-local-staff",
            Self::LanCatalog => "desk-lan-catalog",
            Self::LanStaff => "desk-lan-staff",
            Self::PublicCatalog => "desk-internet-catalog",
            Self::PublicStaff => "desk-internet-staff",
        }
    }
    fn staff_disabled(self, sample: &Snapshot) -> bool {
        self == Self::PublicStaff
            && sample
                .operations
                .remote_address
                .as_ref()
                .is_some_and(|r| !r.staff_remote)
    }
    pub(crate) fn url(self, sample: &Snapshot) -> Option<&str> {
        if self.staff_disabled(sample) {
            return None;
        }
        let book = &sample.addresses;
        let url = match self {
            Self::Catalog => book.catalog.as_deref(),
            Self::Staff => book.staff.as_deref(),
            Self::LanCatalog => book.catalog_lan.first().map(String::as_str),
            Self::LanStaff => book.staff_lan.first().map(String::as_str),
            Self::PublicCatalog | Self::PublicStaff => {
                let TunnelState::Configured(t) = &sample.operations.tunnel else {
                    return None;
                };
                return if self == Self::PublicCatalog {
                    crate::sharing::public_url(&t.catalog)
                } else {
                    crate::sharing::public_route(&t.staff, "/equipe")
                };
            }
        }?;
        (url.starts_with("http://") && url.len() <= 2048 && !url.chars().any(char::is_control))
            .then_some(url)
    }
}

fn slice(area: Rect, y: u16, height: u16) -> Rect {
    Rect::new(area.x, area.y + y, area.width, height)
}
fn inset(area: Rect, x: u16) -> Rect {
    Rect::new(
        area.x + x,
        area.y,
        area.width.saturating_sub(2 * x),
        area.height,
    )
}
fn columns(area: Rect, count: u16) -> Vec<Rect> {
    Layout::horizontal((0..count).map(|_| Constraint::Fill(1)))
        .spacing(1)
        .split(area)
        .to_vec()
}
fn strong(color: Color) -> Style {
    theme::surface().fg(color).add_modifier(Modifier::BOLD)
}

// All child geometry is in content coordinates. The same viewport clips drawing and pointer targets.
struct Page<'a, 'f> {
    frame: &'a mut Frame<'f>,
    scene: &'a mut Scene,
    m: &'a Messages,
    viewport: Rect,
    offset: u16,
    ascii: bool,
}
impl Page<'_, '_> {
    fn visible(&self, area: Rect) -> Rect {
        let clipped = area.intersection(Rect::new(
            self.viewport.x,
            self.offset,
            self.viewport.width,
            self.viewport.height,
        ));
        if clipped.is_empty() {
            return Rect::default();
        }
        Rect::new(
            clipped.x,
            self.viewport.y + clipped.y - self.offset,
            clipped.width,
            clipped.height,
        )
    }
    fn paint(&mut self, r: Rect, style: Style) {
        let v = self.visible(r);
        ui::fill(self.frame, v, style);
    }
    fn line(&mut self, r: Rect, line: impl Into<Line<'static>>) {
        let v = self.visible(slice(r, 0, 1));
        if !v.is_empty() {
            self.frame.render_widget(Paragraph::new(line.into()), v);
        }
    }
    fn label(&mut self, r: Rect, key: &str, style: Style) {
        self.line(r, Line::from(self.m.get(key)).style(style));
    }
    fn block(&mut self, r: Rect, key: &str) {
        self.paint(r, theme::surface());
        let band = theme::surface()
            .bg(theme::STEEL)
            .add_modifier(Modifier::BOLD);
        self.paint(slice(r, 0, 1), band);
        self.label(inset(r, 1), key, band);
    }
    fn button(
        &mut self,
        r: Rect,
        key: &str,
        action: Action,
        variant: Variant,
        state: Availability,
    ) {
        let v = self.visible(r);
        Button::new(self.m.get(key), action, variant)
            .available(state)
            .draw(self.frame, v, self.scene);
    }
}

fn enabled(value: bool) -> Availability {
    if value {
        Availability::Enabled
    } else {
        Availability::Disabled
    }
}
fn ready(value: bool, pending: bool) -> Availability {
    if pending && value {
        Availability::Busy
    } else {
        enabled(value)
    }
}

fn service(state: &LiveState) -> (&'static str, Color) {
    if state.age_seconds > 5 {
        return ("panel-state-stale", theme::MUTED);
    }
    match state.snapshot.as_ref().map(|s| &s.status) {
        Some(ServerStatus::Running(_)) => ("panel-state-running", theme::SUCCESS),
        Some(ServerStatus::Degraded(_)) => ("panel-state-degraded", theme::WARNING),
        Some(ServerStatus::Stopped) => ("panel-state-stopped", theme::WARNING),
        Some(ServerStatus::Unavailable) => ("panel-state-unavailable", theme::MUTED),
        None => ("panel-state-loading", theme::MUTED),
    }
}

pub(crate) fn draw(frame: &mut Frame, panel: &Panel, m: &Messages, ascii: bool, scene: &mut Scene) {
    let Mode::Live(state) = &panel.mode else {
        return;
    };
    let sample = state.snapshot.as_ref().filter(|_| state.age_seconds <= 5);
    let area = frame.area();
    let wide = area.width >= 112;
    let desktop = wide && area.height >= 40;
    let roomy = u16::from(desktop);
    let outer = inset(area, if wide { 2 } else { 1 });
    let heading = masthead(
        frame,
        scene,
        state,
        m,
        outer,
        roomy,
        (ascii, panel.inspection.is_some()),
    );
    let bar_h = if area.height >= 28 { 3 } else { 2 };
    let quick = Rect::new(outer.x, area.bottom() - bar_h - 2, outer.width, bar_h);
    let view = Rect::new(
        outer.x,
        heading.bottom() + 1,
        outer.width,
        quick.y.saturating_sub(heading.bottom() + 2),
    );
    let tile_h = 3 + 2 * roomy;
    let status_h = if wide { tile_h } else { 2 * tile_h + 1 };
    let mid_h = 9 + 3 * roomy;
    let mid_y = status_h + 1;
    let access_y = mid_y + if wide { mid_h + 1 } else { 2 * (mid_h + 1) };
    let access_h = if wide { 10 + roomy } else { 12 };
    let inspection = panel.inspection.as_ref();
    let total = if inspection.is_some() {
        13
    } else {
        access_y + access_h
    };
    let limit = total.saturating_sub(view.height);
    scene.scroll_limit = limit;
    let offset = panel.scroll.min(limit);
    let scroll_w = if limit > 0 { 2 } else { 0 };
    let content = Rect::new(view.x, 0, view.width - scroll_w, total);
    let mut page = Page {
        frame,
        scene,
        m,
        viewport: view,
        offset,
        ascii,
    };
    if let Some(issue) = inspection {
        welcome(&mut page, content, issue);
    } else {
        let slots = if wide {
            columns(slice(content, 0, tile_h), 4)
        } else {
            [0, tile_h + 1]
                .into_iter()
                .flat_map(|y| columns(slice(content, y, tile_h), 2))
                .collect()
        };
        tiles(&mut page, &slots, state, sample);
        let middle = if wide {
            columns(slice(content, mid_y, mid_h), 2)
        } else {
            vec![
                slice(content, mid_y, mid_h),
                slice(content, mid_y + mid_h + 1, mid_h),
            ]
        };
        if let [resources, ops] = middle.as_slice() {
            meters(&mut page, *resources, sample);
            operations(&mut page, *ops, panel, state, sample);
        }
        addresses(&mut page, slice(content, access_y, access_h), sample, panel);
    }
    if limit > 0 {
        let track = Rect::new(view.right() - 1, view.y, 1, view.height);
        ui::fill(frame, track, theme::surface());
        let length = (view.height * view.height / total).max(1);
        let pos = (offset * (view.height - length))
            .checked_div(limit)
            .unwrap_or(0);
        ui::fill(
            frame,
            Rect::new(track.x, track.y + pos, 1, length),
            Style::new().bg(theme::STEEL),
        );
    }
    footer(frame, scene, panel, m, quick, sample, offset);
}

fn masthead(
    frame: &mut Frame,
    scene: &mut Scene,
    state: &LiveState,
    m: &Messages,
    outer: Rect,
    roomy: u16,
    flags: (bool, bool),
) -> Rect {
    let (ascii, inspection) = flags;
    let header = slice(outer, roomy, 1);
    let brand = if ascii { "ELZEVIR" } else { "▣ ELZEVIR" };
    frame.render_widget(
        Paragraph::new(brand).style(Style::new().fg(theme::WARNING).add_modifier(Modifier::BOLD)),
        header,
    );
    frame.render_widget(
        Paragraph::new(m.get("desk-admin")).style(Style::new().fg(theme::TEXT)),
        Rect::new(header.x + 13, header.y, 30, 1),
    );
    let name = state
        .institution
        .name
        .as_deref()
        .map_or_else(|| m.get("inspect-installation"), text);
    let (key, tint) = if inspection {
        ("inspect-mode", theme::ACCENT)
    } else {
        service(state)
    };
    let badge = m.get(key);
    let bw = if outer.width >= 112 {
        u16::try_from(badge.chars().count() + 4).unwrap_or(20)
    } else {
        0
    };
    let right = Rect::new(
        header.x + 40,
        header.y,
        header.width.saturating_sub(41 + bw),
        1,
    );
    frame.render_widget(
        Paragraph::new(name)
            .right_aligned()
            .style(Style::new().fg(theme::MUTED)),
        right,
    );
    if bw > 0 {
        frame.render_widget(
            Paragraph::new(format!("{} {badge}", if ascii { "*" } else { "●" }))
                .centered()
                .style(strong(tint)),
            Rect::new(header.right() - bw, header.y, bw, 1),
        );
    }
    let nav = slice(outer, 1 + roomy * 2, 1 + roomy * 2);
    let sections = [
        (
            Section::Dashboard,
            if outer.width < 90 {
                "tab-overview"
            } else {
                "desk-overview"
            },
        ),
        (Section::Backup, "tab-backup"),
        (Section::Addresses, "tab-access"),
        (Section::Restart, "tab-restart"),
        (Section::Updates, "tab-updates"),
        (Section::Diagnostics, "tab-health"),
        (Section::Settings, "tab-settings"),
    ];
    for ((section, key), r) in sections.into_iter().zip(columns(nav, 7)) {
        Button::new(
            m.get(key),
            Action::Section(section),
            if section == Section::Dashboard {
                Variant::Primary
            } else {
                Variant::Secondary
            },
        )
        .draw(frame, r, scene);
    }
    let heading = slice(outer, nav.bottom() + roomy, 1);
    frame.render_widget(
        Paragraph::new(m.get("desk-title"))
            .style(Style::new().fg(theme::TEXT).add_modifier(Modifier::BOLD)),
        heading,
    );
    heading
}

fn footer(
    frame: &mut Frame,
    scene: &mut Scene,
    panel: &Panel,
    m: &Messages,
    quick: Rect,
    sample: Option<&Snapshot>,
    offset: u16,
) {
    let limit = scene.scroll_limit;
    let outer = quick;
    let area = frame.area();
    let links = if panel.inspection.is_some() {
        vec![
            ("tab-settings", Action::Section(Section::Settings)),
            ("panel-diagnostics", Action::Section(Section::Diagnostics)),
        ]
    } else {
        vec![
            ("desk-backups", Action::Section(Section::Backup)),
            ("desk-restore", Action::Launch(Intent::Restore)),
            ("desk-addresses", Action::Section(Section::Addresses)),
            ("tab-settings", Action::Section(Section::Settings)),
        ]
    };
    for ((key, action), r) in links.into_iter().zip(columns(
        quick,
        if panel.inspection.is_some() { 2 } else { 4 },
    )) {
        let state = if matches!(action, Action::Launch(..)) {
            ready(sample.is_some(), panel.diagnostics.pending)
        } else {
            Availability::Enabled
        };
        Button::new(m.get(key), action, Variant::Secondary)
            .available(state)
            .draw(frame, r, scene);
    }
    let foot = Rect::new(outer.x, area.bottom() - 1, outer.width, 1);
    let message = panel
        .access_notice
        .or_else(|| sample.and_then(|s| s.operations.result))
        .unwrap_or("desk-help");
    let available = foot.width.saturating_sub(if limit > 0 { 25 } else { 0 });
    let feedback = Line::from(m.get(message));
    let feedback_height = if feedback.width() > usize::from(available) {
        2
    } else {
        1
    };
    frame.render_widget(
        Paragraph::new(feedback)
            .style(theme::muted())
            .wrap(ratatui::widgets::Wrap { trim: true }),
        Rect::new(
            foot.x,
            area.bottom() - feedback_height,
            available,
            feedback_height,
        ),
    );
    if limit > 0 {
        for ((key, down, allowed), r) in [
            ("desk-up", false, offset > 0),
            ("desk-down", true, offset < limit),
        ]
        .into_iter()
        .zip(columns(Rect::new(foot.right() - 24, foot.y, 24, 1), 2))
        {
            Button::new(m.get(key), Action::Scroll(down), Variant::Secondary)
                .available(enabled(allowed))
                .draw(frame, r, scene);
        }
    }
}

fn tiles(page: &mut Page<'_, '_>, slots: &[Rect], state: &LiveState, sample: Option<&Snapshot>) {
    let ops = sample.map(|s| &s.operations);
    let backup = match ops.map(|o| o.local_backup).unwrap_or_default() {
        LocalBackup::Verified => ("desk-verified", theme::SUCCESS),
        LocalBackup::Empty => ("desk-empty", theme::WARNING),
        LocalBackup::Unknown => ("panel-unknown", theme::MUTED),
    };
    let internet = match ops.map(|o| &o.tunnel) {
        Some(TunnelState::Configured(t)) if t.catalog_check == crate::PublicCheck::Ok => {
            ("desk-tested", theme::SUCCESS)
        }
        Some(TunnelState::Configured(t)) if t.catalog_check == crate::PublicCheck::Failed => {
            ("desk-test-failed", theme::WARNING)
        }
        Some(TunnelState::Configured(_)) => ("desk-configured", theme::ACCENT),
        _ if ops
            .and_then(|o| o.domain.as_ref())
            .is_some_and(|d| d.catalog.is_some()) =>
        {
            ("desk-assigned", theme::ACCENT)
        }
        Some(TunnelState::NotConfigured) => ("desk-unpublished", theme::MUTED),
        _ => ("panel-unknown", theme::MUTED),
    };
    let release = if ops.is_some_and(|o| o.update_available) {
        ("ui-update-available", theme::WARNING)
    } else if ops.and_then(|o| o.latest.as_ref()).is_some() {
        ("desk-current", theme::SUCCESS)
    } else {
        ("desk-unchecked", theme::MUTED)
    };
    let cards = [
        (
            "home-server",
            service(state),
            if state.institution.managed {
                "home-managed"
            } else {
                "home-personal"
            },
        ),
        ("home-protection", backup, "desk-local-copy"),
        ("desk-internet", internet, "desk-public-endpoint"),
        ("panel-updates", release, "desk-release"),
    ];
    for ((key, (status, color), hint), r) in cards.into_iter().zip(slots.iter().copied()) {
        page.block(r, key);
        let marker = if page.ascii { "*" } else { "●" };
        page.line(
            slice(inset(r, 1), if r.height >= 5 { 2 } else { 1 }, 1),
            Line::from(format!("{marker} {}", page.m.get(status))).style(strong(color)),
        );
        if r.height >= 5 {
            page.label(
                slice(inset(r, 1), 3, 1),
                hint,
                theme::surface().fg(theme::MUTED),
            );
        }
    }
}

#[expect(
    clippy::float_arithmetic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "CPU usage is a sampled float; clamp it to the bounded terminal cell count before conversion"
)]
fn cpu_cells(percent: f32, width: u16) -> u16 {
    (percent.clamp(0.0, 100.0) * f32::from(width) / 100.0).round() as u16
}

fn meters(page: &mut Page<'_, '_>, r: Rect, sample: Option<&Snapshot>) {
    page.block(r, "desk-machine");
    let inner = inset(r, 1);
    let host = sample.map(|s| &s.host);
    let cpu = host.and_then(|h| h.cpu_percent).filter(|v| v.is_finite());
    let ram = host.and_then(|h| h.memory_bytes);
    let disk = sample.and_then(|s| s.storage.available_bytes.zip(s.storage.capacity_bytes));
    let ratio = |(used, total): (u64, u64)| {
        (u128::from(used.min(total)) * u128::from(inner.width))
            .checked_div(u128::from(total))
            .and_then(|n| u16::try_from(n).ok())
    };
    let unknown = || page.m.get("panel-unknown");
    let rows = [
        (
            "CPU",
            cpu.map_or_else(unknown, |v| format!("{v:.1}%")),
            cpu.map(|v| cpu_cells(v, inner.width)),
        ),
        (
            "RAM",
            ram.map_or_else(unknown, |(u, t)| format!("{} / {}", bytes(u), bytes(t))),
            ram.and_then(ratio),
        ),
        (
            "desk-disk",
            disk.map_or_else(unknown, |(free, total)| {
                format!("{} / {}", bytes(total.saturating_sub(free)), bytes(total))
            }),
            disk.and_then(|(free, total)| ratio((total.saturating_sub(free), total))),
        ),
    ];
    let start = if r.height >= 12 { 2 } else { 1 };
    for ((label, value, ratio), i) in rows.into_iter().zip(0u16..) {
        let y = start + 2 * i;
        let name = if label == "desk-disk" {
            page.m.get(label)
        } else {
            label.to_owned()
        };
        let line = slice(inner, y, 1);
        page.line(line, Line::from(name).style(strong(theme::TEXT)));
        let v = page.visible(line);
        if !v.is_empty() {
            page.frame.render_widget(
                Paragraph::new(value)
                    .right_aligned()
                    .style(theme::surface().fg(theme::MUTED)),
                v,
            );
        }
        let bar = slice(inner, y + 1, 1);
        page.paint(bar, Style::new().bg(theme::BACKGROUND));
        if let Some(fill) = ratio {
            page.paint(
                Rect { width: fill, ..bar },
                Style::new().bg(if i == 2 {
                    theme::SUCCESS
                } else {
                    theme::ACCENT
                }),
            );
        }
    }
    let rss = host.and_then(|h| h.resident).map(bytes);
    let proc_cpu = host.and_then(|h| h.process_cpu).map(|n| format!("{n:.1}%"));
    page.line(
        slice(inner, r.height - 2, 1),
        Line::from(page.m.values(
            "desk-process",
            &[
                ("rss", rss.unwrap_or_else(|| page.m.get("panel-unknown"))),
                (
                    "cpu",
                    proc_cpu.unwrap_or_else(|| page.m.get("panel-unknown")),
                ),
            ],
        )),
    );
    page.line(
        slice(inner, r.height - 1, 1),
        row(
            page.m,
            "home-db",
            sample.and_then(|s| s.storage.database_bytes).map(bytes),
        ),
    );
}

fn operations(
    page: &mut Page<'_, '_>,
    r: Rect,
    panel: &Panel,
    state: &LiveState,
    sample: Option<&Snapshot>,
) {
    page.block(r, "desk-operations");
    let inner = inset(r, 1);
    let ops = sample.map(|s| &s.operations);
    let button_h = if r.height > 9 { 3 } else { 2 };
    let mut rows = protection(ops, page.m);
    if let Some(ops) = ops
        && ops.daily.is_none()
        && ops.keep.is_some()
    {
        rows.push(Line::from(page.m.get("home-unscheduled")));
    }
    for (line, i) in rows.into_iter().zip(1..r.height - button_h - 1) {
        page.line(slice(inner, i, 1), line);
    }
    let online = sample.is_some_and(|s| {
        matches!(
            s.status,
            ServerStatus::Running(_) | ServerStatus::Degraded(_)
        )
    });
    let stopped = sample.is_some_and(|s| matches!(s.status, ServerStatus::Stopped));
    let mut actions = vec![("desk-create", Intent::Create, Variant::Primary, online)];
    if state.institution.managed || !stopped {
        actions.push((
            "desk-restart",
            Intent::Restart,
            Variant::Secondary,
            state.institution.managed || online,
        ));
    }
    if !state.institution.managed {
        actions.push(if stopped {
            ("desk-start", Intent::Start, Variant::Secondary, true)
        } else {
            ("desk-stop", Intent::Stop, Variant::Danger, online)
        });
    }
    let slots = columns(
        slice(inner, r.height - button_h - 1, button_h),
        u16::try_from(actions.len()).unwrap_or(3),
    );
    for ((key, intent, variant, allowed), rect) in actions.into_iter().zip(slots) {
        page.button(
            rect,
            key,
            Action::Launch(intent),
            variant,
            ready(
                allowed,
                panel.diagnostics.pending
                    || (intent == Intent::Create
                        && ops.is_some_and(|o| {
                            o.jobs.iter().any(|j| {
                                matches!(j.action, "op-backup-now" | "cloud-backup")
                                    && matches!(j.state, "op-queued" | "op-running")
                            })
                        })),
            ),
        );
    }
}

fn addresses(page: &mut Page<'_, '_>, r: Rect, sample: Option<&Snapshot>, panel: &Panel) {
    page.block(r, "desk-addresses");
    let m = page.m;
    let inner = inset(r, 1);
    let tunnel = sample.and_then(|s| match &s.operations.tunnel {
        TunnelState::Configured(t) => Some(t),
        _ => None,
    });
    let local = sample
        .filter(|s| s.addresses.tested_at.is_some())
        .map_or_else(
            || m.get("desk-http-untested"),
            |s| {
                m.values(
                    "desk-http-status",
                    &[
                        (
                            "catalog",
                            crate::address_view::check(m, s.addresses.catalog_test),
                        ),
                        (
                            "staff",
                            crate::address_view::check(m, s.addresses.staff_test),
                        ),
                    ],
                )
            },
        );
    let public = row(
        m,
        "home-https",
        tunnel.map(|t| crate::tunnel_view::check(m, t.catalog_check)),
    )
    .to_string();
    let compact = r.width < 95;
    let detail = theme::surface().fg(theme::MUTED);
    page.line(
        slice(inner, 1, 1),
        Line::from(if compact {
            local
        } else {
            format!("{local}   |   {public}")
        })
        .style(detail),
    );
    if compact {
        page.line(slice(inner, 2, 1), Line::from(public).style(detail));
    }
    let bound = sample.and_then(|s| match &s.status {
        ServerStatus::Running(d) | ServerStatus::Degraded(d) => d.listen,
        _ => None,
    });
    if let Mode::Live(state) = &panel.mode {
        let configured = row(
            m,
            "home-bind",
            state.institution.configured_listen.map(|s| s.to_string()),
        );
        let observed = row(m, "home-observed", bound.map(|s| s.to_string()));
        if compact {
            page.line(slice(inner, 3, 1), configured.style(detail));
            page.line(slice(inner, 4, 1), observed.style(detail));
        } else {
            page.line(
                slice(inner, 2, 1),
                Line::from(format!("{configured}   |   {observed}")).style(detail),
            );
        }
    }
    address_rows(page, inner, sample, if compact { 5 } else { 3 });
    address_tests(
        page,
        inner,
        sample,
        tunnel.is_some(),
        panel.diagnostics.pending,
    );
}

fn address_tests(
    page: &mut Page<'_, '_>,
    inner: Rect,
    sample: Option<&Snapshot>,
    configured: bool,
    pending: bool,
) {
    let m = page.m;
    let y = inner.height - 1;
    let local = slice(Rect { width: 21, ..inner }, y, 1);
    page.button(
        local,
        "desk-test-local",
        Action::Launch(Intent::TestLocal),
        Variant::Secondary,
        ready(sample.is_some(), pending),
    );
    let external = Rect::new(local.right() + 1, local.y, 19, 1);
    page.button(
        external,
        "desk-test-public",
        Action::Launch(Intent::TestPublic),
        Variant::Secondary,
        ready(configured, pending),
    );
    page.line(
        Rect::new(
            external.right() + 1,
            external.y,
            inner.right().saturating_sub(external.right() + 1),
            1,
        ),
        Line::from(m.get(if page.scene.desktop {
            "desk-local-scope"
        } else {
            "desk-no-desktop"
        }))
        .style(theme::surface().fg(theme::MUTED)),
    );
}

pub(crate) fn address_table(
    frame: &mut Frame,
    scene: &mut Scene,
    m: &Messages,
    viewport: Rect,
    sample: Option<&Snapshot>,
    ascii: bool,
) {
    let mut page = Page {
        frame,
        scene,
        m,
        viewport,
        offset: 0,
        ascii,
    };
    address_rows(&mut page, Rect { y: 0, ..viewport }, sample, 0);
}

fn address_rows(page: &mut Page<'_, '_>, inner: Rect, sample: Option<&Snapshot>, start: u16) {
    let m = page.m;
    for (i, endpoint) in (0u16..).zip(Endpoint::ALL) {
        let y = start + i;
        let line = slice(inner, y, 1);
        let label_w = if inner.width >= 100 { 22 } else { 21 };
        let action_w = if inner.width >= 100 { 12 } else { 8 };
        let url = sample.and_then(|s| endpoint.url(s));
        let address_area = Rect::new(
            line.x + label_w,
            line.y,
            line.width - label_w - 2 * action_w - 2,
            1,
        );
        page.label(
            Rect {
                width: label_w,
                ..line
            },
            endpoint.key(),
            theme::surface().fg(theme::TEXT),
        );
        let empty = match (endpoint, sample.map(|s| &s.operations.tunnel)) {
            _ if sample.is_some_and(|s| endpoint.staff_disabled(s)) => "desk-staff-disabled",
            (Endpoint::PublicCatalog | Endpoint::PublicStaff, Some(TunnelState::NotConfigured)) => {
                "desk-unpublished"
            }
            (Endpoint::LanCatalog | Endpoint::LanStaff, _) => "desk-no-lan",
            _ => "panel-unknown",
        };
        page.line(
            address_area,
            Line::from(url.map_or_else(
                || {
                    let assignment =
                        sample
                            .and_then(|s| s.operations.domain.as_ref())
                            .and_then(|d| match endpoint {
                                Endpoint::PublicCatalog => d.catalog.as_deref(),
                                Endpoint::PublicStaff
                                    if !sample.is_some_and(|s| endpoint.staff_disabled(s)) =>
                                {
                                    d.staff.as_deref()
                                }
                                _ => None,
                            });
                    assignment.map_or_else(
                        || m.get(empty),
                        |host| m.values("desk-assigned-host", &[("value", text(host))]),
                    )
                },
                text,
            ))
            .style(theme::surface().fg(if url.is_some() {
                theme::ACCENT
            } else {
                theme::MUTED
            })),
        );
        for (j, open, key) in [(0, false, "desk-copy"), (1, true, "desk-open")] {
            page.button(
                Rect::new(
                    address_area.right() + 1 + j * (action_w + 1),
                    line.y,
                    action_w,
                    1,
                ),
                key,
                Action::Address(endpoint, open),
                Variant::Secondary,
                enabled(url.is_some() && (!open || page.scene.desktop)),
            );
        }
    }
}

fn welcome(page: &mut Page<'_, '_>, r: Rect, issue: &Inspection) {
    use crate::InstallationIssue::{
        InvalidConfig, Legacy, MissingAdministrator, MissingDatabase, New, Unavailable,
    };
    let r = Rect { height: 12, ..r };
    let inner = inset(r, 2);
    page.block(r, "inspect-installation");
    page.label(
        slice(inner, 2, 1),
        if issue.issue == New {
            "inspect-empty"
        } else {
            "inspect-review"
        },
        strong(theme::WARNING),
    );
    let reason = match issue.issue {
        New => "inspect-new",
        MissingAdministrator => "inspect-no-admin",
        Legacy => "inspect-legacy",
        InvalidConfig => "inspect-config",
        MissingDatabase => "inspect-no-db",
        Unavailable => "inspect-unknown",
    };
    page.label(slice(inner, 3, 1), reason, theme::surface());
    page.label(slice(inner, 5, 1), "desk-data", strong(theme::MUTED));
    page.line(slice(inner, 6, 1), Line::from(text(&issue.location)));
    let (key, action) = if issue.issue == New {
        ("inspect-create", Action::Launch(Intent::Setup))
    } else if issue.running {
        ("inspect-connect", Action::Launch(Intent::Setup))
    } else {
        ("section-settings", Action::Section(Section::Settings))
    };
    page.button(
        slice(
            Rect {
                width: inner.width.min(36),
                ..inner
            },
            8,
            3,
        ),
        key,
        action,
        Variant::Primary,
        Availability::Enabled,
    );
}

pub(crate) fn gallery(frame: &mut Frame, m: &Messages, scene: &mut Scene) {
    let rect = frame
        .area()
        .centered(Constraint::Max(96), Constraint::Length(17));
    ui::fill(frame, rect, theme::surface());
    let inner = inset(rect, 2);
    frame.render_widget(
        Paragraph::new(m.get("desk-gallery")).style(strong(theme::WARNING)),
        slice(inner, 1, 1),
    );
    frame.render_widget(
        Paragraph::new(m.get("desk-gallery-help")).style(theme::surface()),
        slice(inner, 3, 1),
    );
    let mut slots = columns(slice(inner, 5, 3), 3);
    slots.extend(columns(slice(inner, 9, 3), 3));
    let examples = [
        (
            "desk-primary",
            true,
            Variant::Primary,
            Availability::Enabled,
        ),
        (
            "desk-secondary",
            true,
            Variant::Secondary,
            Availability::Enabled,
        ),
        ("desk-danger", false, Variant::Danger, Availability::Enabled),
        (
            "desk-disabled",
            true,
            Variant::Secondary,
            Availability::Disabled,
        ),
        ("desk-busy", true, Variant::Secondary, Availability::Busy),
        (
            "desk-success",
            true,
            Variant::Success,
            Availability::Enabled,
        ),
    ];
    for ((key, value, variant, available), r) in examples.into_iter().zip(slots) {
        Button::new(m.get(key), Action::Sample(value), variant)
            .available(available)
            .draw(frame, r, scene);
    }
    if let Some(ok) = scene.sample {
        frame.render_widget(
            Paragraph::new(m.get(if ok {
                "desk-example-ok"
            } else {
                "desk-example-error"
            }))
            .style(strong(if ok { theme::SUCCESS } else { theme::WARNING })),
            slice(inner, 13, 1),
        );
    }
    Button::new(m.get("desk-close"), Action::Gallery, Variant::Quiet).draw(
        frame,
        slice(inner, 15, 1),
        scene,
    );
}

#[cfg(test)]
mod tests {
    use super::{Endpoint, Snapshot, TunnelState};
    use crate::{PublicCheck, RemoteAddress, Tunnel, TunnelLog};

    #[test]
    fn public_staff_uses_the_advertised_staff_path_and_respects_remote_opt_out() {
        let mut sample = Snapshot::default();
        sample.operations.tunnel = TunnelState::Configured(Tunnel {
            catalog: "https://catalog.example/".into(),
            staff: "https://staff.example/equipe".into(),
            catalog_check: PublicCheck::NotRun,
            staff_check: PublicCheck::NotRun,
            tested_at: None,
            staff_tested_at: None,
            logs: TunnelLog::Unavailable,
        });
        assert_eq!(
            Endpoint::PublicStaff.url(&sample),
            Some("https://staff.example/equipe")
        );
        sample.operations.remote_address = Some(RemoteAddress {
            at: String::new(),
            listed: false,
            staff_remote: false,
        });
        assert_eq!(Endpoint::PublicStaff.url(&sample), None);
        assert_eq!(
            Endpoint::PublicCatalog.url(&sample),
            Some("https://catalog.example/")
        );
    }
}
