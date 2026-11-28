// SPDX-License-Identifier: GPL-3.0-or-later
//! Public access workflows use the host's existing observations and reviewed operations.
use crate::{
    DomainPhase, Messages, Mode, Panel, Snapshot, TunnelTab,
    backup_center::{Controls, block, inner, key, line, row, sample},
    dashboard::{row as value, safe_text},
    overview::Endpoint,
    ui::{self, Action, Scene, Variant, theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Paragraph, Wrap},
};

pub(crate) const PAGES: [TunnelTab; 5] = [
    TunnelTab::Addresses,
    TunnelTab::Manage,
    TunnelTab::OwnDomain,
    TunnelTab::Test,
    TunnelTab::Log,
];

fn page_title(tab: TunnelTab) -> &'static str {
    match tab {
        TunnelTab::Addresses => "ac-overview",
        TunnelTab::Manage => "ac-free",
        TunnelTab::OwnDomain => "ac-own",
        TunnelTab::Test => "ac-tests",
        TunnelTab::Log => "ac-events",
        TunnelTab::Share => "broker-qr",
    }
}

pub(crate) fn draw(f: &mut Frame, panel: &Panel, m: &Messages, scene: &mut Scene, ascii: bool) {
    let area = f.area();
    let observed = sample(panel);
    ui::header(f, row(area, 0, 1), panel, m, ascii);
    crate::view::draw_tabs(f, row(area, 1, 1), panel, m, scene);
    let area = inner(area);
    let tab = panel.tunnel_tab;
    line(f, row(area, 3, 1), m.get("ac-title"), theme::ACCENT, true);
    let h = if area.height >= 28 { 3 } else { 2 };
    scene.buttons(
        f,
        row(area, 4, h),
        PAGES[..3].iter().map(|&page| {
            (
                m.get(page_title(page)),
                Action::AccessPage(page),
                page == tab,
            )
        }),
    );
    let start = 5 + h;
    let body = row(area, start, area.height.saturating_sub(start + 3));
    let mut ui = Controls { f, scene, m };
    let details = if tab == TunnelTab::Addresses {
        overview(&mut ui, body, observed, ascii)
    } else {
        toolbar(&mut ui, body, panel, observed)
    };
    details_view(&mut ui, details, panel, observed);
    let public = observed
        .and_then(|s| Endpoint::PublicCatalog.url(s))
        .is_some();
    let footer = row(area, area.height - 3, 2);
    ui.buttons(
        footer,
        &[
            (
                "ac-tests",
                Action::AccessPage(TunnelTab::Test),
                section_style(tab == TunnelTab::Test),
                true,
            ),
            (
                "ac-events",
                Action::AccessPage(TunnelTab::Log),
                section_style(tab == TunnelTab::Log),
                true,
            ),
            ("ac-share", key('Q'), Variant::Secondary, public),
            (
                "bc-prev",
                Action::Scroll(false),
                Variant::Quiet,
                panel.scroll.min(ui.scene.scroll_limit) > 0,
            ),
            (
                "bc-next",
                Action::Scroll(true),
                Variant::Quiet,
                panel.scroll < ui.scene.scroll_limit,
            ),
        ],
    );
    line(
        ui.f,
        row(area, area.height - 1, 1),
        m.get("ac-keys"),
        theme::MUTED,
        false,
    );
}

fn overview(
    ui: &mut Controls<'_, '_>,
    body: Rect,
    observed: Option<&Snapshot>,
    ascii: bool,
) -> Rect {
    if body.width < 140 || body.height < 18 {
        crate::overview::address_table(ui.f, ui.scene, ui.m, row(body, 0, 6), observed, ascii);
        return row(body, 6, body.height.saturating_sub(6));
    }
    let [addresses, status] = Layout::horizontal([Constraint::Percentage(62), Constraint::Fill(1)])
        .spacing(1)
        .areas(body);
    let table = row(addresses, 0, 8);
    block(ui.f, table, ui.m.get("ac-observed"));
    crate::overview::address_table(
        ui.f,
        ui.scene,
        ui.m,
        row(inner(table), 1, 6),
        observed,
        ascii,
    );
    let help = row(addresses, 9, 7);
    block(ui.f, help, ui.m.get("ac-choose-route"));
    ui.f.render_widget(
        Paragraph::new(vec![
            Line::from(ui.m.get("ac-free-guide")),
            Line::default(),
            Line::from(ui.m.get("ac-own-guide")),
        ])
        .wrap(Wrap { trim: false }),
        inner(row(help, 1, 6)),
    );
    status
}

fn toolbar(
    ui: &mut Controls<'_, '_>,
    body: Rect,
    panel: &Panel,
    observed: Option<&Snapshot>,
) -> Rect {
    let groups = tool_rows(panel, observed);
    let wide = body.width >= 118 && body.height >= 15;
    let per_row = if wide || (body.width < 98 && body.height >= 12) {
        2
    } else {
        4
    };
    let (tools, details) = if wide {
        let [tools, details] = Layout::horizontal([Constraint::Length(54), Constraint::Fill(1)])
            .spacing(1)
            .areas(body);
        (tools, details)
    } else {
        let count: usize = groups
            .iter()
            .map(|group| group.len().div_ceil(per_row))
            .sum();
        let height = u16::try_from(count).unwrap_or(u16::MAX) + 1;
        (
            row(body, 0, height),
            row(body, height, body.height.saturating_sub(height)),
        )
    };
    block(ui.f, tools, ui.m.get(page_title(panel.tunnel_tab)));
    let height = if wide && body.height >= 25 { 3 } else { 1 };
    let mut y = 1;
    for group in groups {
        for buttons in group.chunks(per_row) {
            ui.buttons(row(inner(tools), y, height), buttons);
            y += height;
        }
    }
    details
}

const fn section_style(active: bool) -> Variant {
    if active {
        Variant::Primary
    } else {
        Variant::Secondary
    }
}

type Control = (&'static str, Action, Variant, bool);

fn tool_rows(panel: &Panel, observed: Option<&Snapshot>) -> Vec<Vec<Control>> {
    let ready = !panel.diagnostics.pending;
    let button = |label, c, style, enabled| (label, key(c), style, ready && enabled);
    let primary = |label, c| button(label, c, Variant::Primary, true);
    let secondary = |label, c| button(label, c, Variant::Secondary, true);
    match panel.tunnel_tab {
        TunnelTab::Manage => {
            let domain = observed.and_then(|s| s.operations.domain.as_ref());
            let mut groups = vec![vec![
                primary("ac-request", 'g'),
                secondary("ac-resume", 'G'),
                secondary("ac-recover", 'o'),
                secondary("ac-refresh", 'c'),
            ]];
            let registered = domain.is_some_and(|d| d.phase != DomainPhase::NotRegistered)
                || observed.is_some_and(|s| s.operations.remote_address.is_some());
            if registered {
                groups.extend([
                    vec![
                        secondary("ac-list", 'j'),
                        secondary("ac-unlist", 'J'),
                        secondary("ac-staff-on", 'w'),
                        secondary("ac-staff-off", 'W'),
                    ],
                    vec![
                        secondary("ac-export", 'k'),
                        secondary("ac-renew", 'n'),
                        button("ac-withdraw", 'z', Variant::Danger, true),
                    ],
                ]);
            }
            if panel.connector_action_available('P') || panel.connector_action_available('S') {
                groups.push(vec![
                    button(
                        "ac-pause",
                        'P',
                        Variant::Secondary,
                        panel.connector_action_available('P'),
                    ),
                    button(
                        "ac-start",
                        'S',
                        Variant::Secondary,
                        panel.connector_action_available('S'),
                    ),
                ]);
            }
            groups
        }
        TunnelTab::OwnDomain => vec![vec![
            primary("ac-authorize", 'l'),
            secondary("ac-configure", 's'),
        ]],
        TunnelTab::Test => vec![vec![
            button("desk-test-local", 'v', Variant::Primary, observed.is_some()),
            button(
                "ac-test-catalog",
                't',
                Variant::Secondary,
                observed
                    .and_then(|s| Endpoint::PublicCatalog.url(s))
                    .is_some(),
            ),
            button(
                "ac-test-staff",
                'T',
                Variant::Secondary,
                observed
                    .and_then(|s| Endpoint::PublicStaff.url(s))
                    .is_some(),
            ),
        ]],
        TunnelTab::Log => vec![vec![secondary("ac-reobserve", 'f')]],
        _ => Vec::new(),
    }
}

fn details_view(ui: &mut Controls<'_, '_>, area: Rect, panel: &Panel, observed: Option<&Snapshot>) {
    let m = ui.m;
    let Mode::Live(state) = &panel.mode else {
        return;
    };
    let tab = panel.tunnel_tab;
    let title = match tab {
        TunnelTab::Manage => "ac-free-details",
        TunnelTab::OwnDomain => "ac-own-details",
        TunnelTab::Test => "ac-test-details",
        TunnelTab::Log => "ac-log-details",
        _ => "ac-overview-details",
    };
    block(ui.f, area, m.get(title));
    let mut rows = Vec::new();
    if let Some(key) = panel.access_notice {
        rows.push(Line::from(m.get(key)).style(theme::surface().fg(theme::ACCENT)));
    }
    if observed.is_none() {
        rows.push(Line::from(m.get(if state.age_seconds > 5 {
            "panel-state-stale"
        } else {
            "panel-state-loading"
        })));
    }
    let ops = observed.map(|s| &s.operations);
    if let Some(ops) = ops {
        let cloudflare = ops
            .authorization_uris
            .iter()
            .filter(|_| matches!(tab, TunnelTab::Addresses | TunnelTab::OwnDomain));
        let broker = ops
            .broker_authorization_uris
            .iter()
            .filter(|_| matches!(tab, TunnelTab::Addresses | TunnelTab::Manage));
        for uri in cloudflare.chain(broker) {
            rows.push(value(m, "op-authorize-uri", Some(safe_text(uri, 2048))));
        }
    }
    match tab {
        TunnelTab::Manage => {
            rows.push(m.get("ac-free-guide").into());
            if ops
                .and_then(|o| o.domain.as_ref())
                .is_none_or(|d| d.remote_status.is_none())
            {
                rows.push(m.get("ac-broker-unconfirmed").into());
            }
        }
        TunnelTab::OwnDomain => {
            rows.extend(["ac-own-guide", "ac-own-effect"].map(|k| Line::from(m.get(k))));
            if ops.and_then(|o| o.domain.as_ref()).is_some_and(|d| {
                d.registration_at.is_some() || d.phase != DomainPhase::NotRegistered
            }) {
                rows.push(
                    Line::from(m.get("ac-existing-free"))
                        .style(theme::surface().fg(theme::WARNING)),
                );
            }
        }
        TunnelTab::Test => rows.push(m.get("ac-test-guide").into()),
        _ => {}
    }
    if matches!(tab, TunnelTab::Manage | TunnelTab::OwnDomain) {
        rows.push(m.get("ac-host-operator").into());
    }
    rows.extend(crate::tunnel_view::rows(ops, tab, m));
    if tab == TunnelTab::Test {
        rows.extend(crate::address_view::rows(
            &state.institution,
            observed.map(|s| &s.addresses),
            panel.diagnostics.pending,
            m,
        ));
    }
    let content = inner(row(area, 1, area.height.saturating_sub(1)));
    let paragraph = Paragraph::new(rows).wrap(Wrap { trim: false });
    ui.scene.access_page_height = content.height;
    ui.scene.scroll_limit = u16::try_from(paragraph.line_count(content.width))
        .unwrap_or(u16::MAX)
        .saturating_sub(content.height);
    ui.f.render_widget(
        paragraph.scroll((panel.scroll.min(ui.scene.scroll_limit), 0)),
        content,
    );
}
