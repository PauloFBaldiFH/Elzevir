// SPDX-License-Identifier: GPL-3.0-or-later
//! The operator's overview of observed health, retained backups and listeners.
use crate::{
    LiveState, LocalBackup, Messages, Operations, ServerStatus, Snapshot, TunnelState,
    dashboard::{component, row, text},
    ui::{card, theme},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
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

fn access(sample: Option<&Snapshot>, m: &Messages) -> Vec<Line<'static>> {
    let addr = sample.map(|s| &s.addresses);
    let mut rows = vec![
        row(
            m,
            "panel-local-catalog",
            addr.and_then(|a| a.catalog.as_deref()).map(text),
        ),
        row(
            m,
            "panel-local-staff",
            addr.and_then(|a| a.staff.as_deref()).map(text),
        ),
        Line::from(m.get(if addr.is_some_and(|a| a.tested_at.is_some()) {
            "panel-access-tests-reference"
        } else {
            "panel-binding-only"
        })),
    ];
    rows.push(
        if let Some(url) = addr.and_then(|a| a.catalog_lan.first()) {
            row(m, "home-lan", Some(text(url)))
        } else {
            Line::from(m.get("panel-lan-unknown"))
        },
    );
    rows.push(match sample.map(|s| &s.operations.tunnel) {
        Some(TunnelState::Configured(t)) => row(
            m,
            "home-https",
            Some(crate::tunnel_view::check(m, t.catalog_check)),
        ),
        Some(TunnelState::NotConfigured) => Line::from(m.get("panel-internet-unknown")),
        _ => row(m, "home-https", None),
    });
    rows
}

fn resources(sample: Option<&Snapshot>, m: &Messages) -> Vec<Line<'static>> {
    let host = sample.map(|s| &s.host);
    let storage = sample.map(|s| &s.storage);
    vec![
        row(m, "home-rss", host.and_then(|h| h.resident).map(bytes)),
        row(
            m,
            "home-process-cpu",
            host.and_then(|h| h.process_cpu).map(|n| format!("{n:.1}%")),
        ),
        row(
            m,
            "panel-host-cpu",
            host.and_then(|h| h.cpu_percent).map(|n| format!("{n:.1}")),
        ),
        row(
            m,
            "home-ram",
            host.and_then(|h| h.memory_bytes)
                .map(|(used, total)| format!("{} / {}", bytes(used), bytes(total))),
        ),
        row(
            m,
            "home-space",
            storage.and_then(|s| s.available_bytes).map(bytes),
        ),
        row(
            m,
            "home-db",
            storage.and_then(|s| s.database_bytes).map(bytes),
        ),
        row(m, "home-wal", storage.and_then(|s| s.wal_bytes).map(bytes)),
    ]
}

pub(crate) fn draw(
    frame: &mut Frame,
    area: Rect,
    state: &LiveState,
    m: &Messages,
    status: (&str, Color),
    scroll: u16,
    ascii: bool,
) {
    let sample = state.snapshot.as_ref().filter(|_| state.age_seconds <= 5);
    let details = sample.and_then(|s| match &s.status {
        ServerStatus::Running(d) | ServerStatus::Degraded(d) => Some(d),
        _ => None,
    });
    let health = vec![
        emphasis(m.get(status.0), status.1),
        row(
            m,
            "panel-database-state",
            details.map(|d| component(d.database, m)),
        ),
        row(
            m,
            "panel-indexer-state",
            details.map(|d| component(d.indexer, m)),
        ),
        row(m, "panel-version", details.map(|d| text(&d.version))),
        row(
            m,
            "home-bind",
            state.institution.configured_listen.map(|a| a.to_string()),
        ),
        row(
            m,
            "panel-age",
            sample.map(|_| state.age_seconds.to_string()),
        ),
    ];
    let mut cards = vec![
        (
            state
                .institution
                .name
                .as_deref()
                .map_or_else(|| m.get("home-server"), text),
            health,
        ),
        (
            m.get("home-protection"),
            protection(sample.map(|s| &s.operations), m),
        ),
        (m.get("home-access"), access(sample, m)),
        (m.get("home-resources"), resources(sample, m)),
    ];
    if area.width >= 148 && area.height >= 35 {
        expanded_details(&mut cards, sample, details, state.institution.managed, m);
    }
    draw_cards(frame, area, cards, scroll, ascii);
}

fn expanded_details(
    cards: &mut Vec<(String, Vec<Line<'static>>)>,
    sample: Option<&Snapshot>,
    details: Option<&crate::Details>,
    managed: bool,
    m: &Messages,
) {
    let ops = sample.map(|s| &s.operations);
    cards.push((
        m.get("home-service-detail"),
        vec![
            Line::from(m.get(if managed {
                "home-managed"
            } else {
                "home-personal"
            })),
            row(m, "panel-pid", details.map(|d| d.pid.to_string())),
            row(
                m,
                "panel-uptime",
                details.map(|d| d.uptime_seconds.to_string()),
            ),
            row(
                m,
                "home-observed",
                details.and_then(|d| d.listen).map(|a| a.to_string()),
            ),
            row(
                m,
                "op-latest",
                ops.and_then(|o| o.latest.as_deref()).map(text),
            ),
        ],
    ));
    cards.push((
        m.get("home-storage-policy"),
        vec![
            row(
                m,
                "home-backup-path",
                ops.and_then(|o| o.backup_folder.as_ref())
                    .map(|p| text(&p.to_string_lossy())),
            ),
            row(
                m,
                "home-retain",
                ops.and_then(|o| o.keep).map(|n| n.to_string()),
            ),
            row(
                m,
                "home-daily",
                ops.and_then(|o| o.daily.as_deref()).map(text).or_else(|| {
                    ops.filter(|o| o.keep.is_some())
                        .map(|_| m.get("home-unscheduled"))
                }),
            ),
        ],
    ));
}

fn draw_cards(
    frame: &mut Frame,
    area: Rect,
    cards: Vec<(String, Vec<Line<'static>>)>,
    scroll: u16,
    ascii: bool,
) {
    let cols: usize = if area.width >= 108 { 2 } else { 1 };
    let width = area.width / u16::try_from(cols).unwrap_or(1);
    let mut bands = Vec::new();
    for group in cards.chunks(cols) {
        let height = group
            .iter()
            .map(|(_, rows)| {
                rows.iter()
                    .map(|line| {
                        let columns = usize::from(width.saturating_sub(5).max(1));
                        // Leave room for the last word to wrap as a unit.
                        1 + line.width() / columns
                    })
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(1);
        bands.push(
            u16::try_from(height + 3)
                .unwrap_or(u16::MAX)
                .max(if cols == 2 {
                    area.height / u16::try_from(cards.len().div_ceil(cols)).unwrap_or(1)
                } else {
                    0
                }),
        );
    }
    let offset = scroll.min(
        bands
            .iter()
            .copied()
            .fold(0u16, u16::saturating_add)
            .saturating_sub(area.height),
    );
    let mut y = 0_u16;
    let mut iter = cards.into_iter();
    for height in bands {
        for column in 0..cols {
            let Some((title, rows)) = iter.next() else {
                break;
            };
            let skip = offset.saturating_sub(y);
            let top = y.saturating_sub(offset);
            let visible = height
                .saturating_sub(skip)
                .min(area.height.saturating_sub(top));
            if visible > 1 {
                let x = if column == 0 { 0 } else { width };
                let rect = Rect::new(area.x + x, area.y + top, width.saturating_sub(1), visible);
                card(frame, rect, &title, rows, theme::ACCENT, ascii, skip);
            }
        }
        y = y.saturating_add(height);
    }
}
