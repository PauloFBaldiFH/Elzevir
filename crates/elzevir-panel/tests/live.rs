// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure live-dashboard rendering and failure transitions.
#![expect(clippy::unwrap_used, reason = "rendering helpers fail tests on errors")]

use elzevir_panel::{
    Component, Details, Host, Institution, Messages, Mode, Panel, ServerStatus, Snapshot, Storage,
    draw,
};
use ratatui::{Terminal, backend::TestBackend};

fn screen(panel: &Panel, language: &str) -> String {
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let messages = Messages::new(language).unwrap();
    terminal
        .draw(|frame| draw(frame, panel, &messages, true))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(140)
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn connected() -> Panel {
    let mut panel = Panel::live(Institution {
        name: Some("Biblioteca de teste".to_owned()),
        configured_listen: Some("127.0.0.1:0".parse().unwrap()),
        ..Institution::default()
    });
    let details = Details {
        version: "0.0.0".to_owned(),
        pid: 42,
        uptime_seconds: 123,
        database: Component::Ok,
        indexer: Component::Ok,
        listen: Some("127.0.0.1:8080".parse().unwrap()),
    };
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot = Some(Snapshot {
            status: ServerStatus::Running(details),
            storage: Storage {
                database_bytes: Some(4 * 1024 * 1024),
                wal_bytes: Some(0),
                available_bytes: Some(100 * 1024 * 1024),
            },
            host: Host {
                cpu_percent: Some(12.5),
                memory_bytes: Some((1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024)),
            },
            addresses: elzevir_panel::AddressBook {
                observed_mode: Some(elzevir_panel::StaffAccess::SameAddress),
                catalog: Some("http://127.0.0.1:8080/".to_owned()),
                staff: Some("http://127.0.0.1:8080/equipe".to_owned()),
                ..elzevir_panel::AddressBook::default()
            },
            ..Snapshot::default()
        });
    }
    panel
}

#[test]
fn live_dashboard_is_translated_and_separates_configured_and_observed() {
    let panel = connected();
    insta::assert_snapshot!(screen(&panel, "en"));
    insta::assert_snapshot!(screen(&panel, "pt-BR"));
    for language in ["en", "pt-BR"] {
        let text = screen(&panel, language);
        assert!(!text.contains("DEMO"));
        assert!(!text.contains("panel-"));
        assert!(text.contains("127.0.0.1:0"));
        assert!(text.contains("127.0.0.1:8080"));
    }
}

#[test]
fn failures_and_old_observations_never_show_old_health_as_current() {
    let mut panel = connected();
    if let Mode::Live(state) = &mut panel.mode {
        state.age_seconds = 6;
    }
    let stale = screen(&panel, "en");
    assert!(stale.contains("No recent observation"));
    assert!(!stale.contains("PID: 42"));
    for (status, expected) in [
        (ServerStatus::Stopped, "Stopped"),
        (ServerStatus::Unavailable, "Unavailable"),
    ] {
        if let Mode::Live(state) = &mut panel.mode {
            state.age_seconds = 0;
            state.snapshot = Some(Snapshot {
                status,
                ..Snapshot::default()
            });
        }
        let text = screen(&panel, "en");
        assert!(text.contains(expected));
        assert!(!text.contains("PID: 42"));
    }
}

#[test]
fn untrusted_strings_cannot_inject_terminal_controls() {
    let mut panel = connected();
    if let Mode::Live(state) = &mut panel.mode {
        state.institution.name = Some("library\x1b[31m\u{202e}name".to_owned());
        if let Some(Snapshot {
            status: ServerStatus::Running(details),
            ..
        }) = &mut state.snapshot
        {
            details.version = "0\x1b[2J".to_owned();
        }
    }
    let text = screen(&panel, "en");
    assert!(!text.contains('\x1b') && !text.contains('\u{202e}'));
}

#[test]
fn live_states_and_sections_have_both_translations() {
    for language in ["en", "pt-BR"] {
        let mut panel = connected();
        for section in elzevir_panel::Section::ALL {
            panel.section = section;
            assert!(!screen(&panel, language).contains("panel-"));
        }
        panel.section = elzevir_panel::Section::Dashboard;
        if let Mode::Live(state) = &mut panel.mode
            && let Some(snapshot) = &mut state.snapshot
            && let ServerStatus::Running(details) = &snapshot.status
        {
            let mut details = details.clone();
            details.indexer = Component::Failed;
            snapshot.status = ServerStatus::Degraded(details);
        }
        assert!(!screen(&panel, language).contains("panel-"));
        if let Mode::Live(state) = &mut panel.mode {
            state.snapshot = None;
        }
        assert!(!screen(&panel, language).contains("panel-"));
    }
}

#[test]
fn wildcard_binding_is_rendered_as_a_local_address_not_a_publication_claim() {
    for (bound, local) in [
        ("0.0.0.0:8080", "http://127.0.0.1:8080/"),
        ("[::]:8080", "http://[::1]:8080/"),
    ] {
        let mut panel = connected();
        if let Mode::Live(state) = &mut panel.mode
            && let Some(Snapshot {
                status: ServerStatus::Running(details),
                ..
            }) = &mut state.snapshot
        {
            details.listen = Some(bound.parse().unwrap());
            if let Some(snapshot) = &mut state.snapshot {
                snapshot.addresses.catalog = Some(local.to_owned());
                snapshot.addresses.staff = Some(format!("{local}equipe"));
            }
        }
        let text = screen(&panel, "en");
        assert!(text.contains(local));
        assert!(text.contains("HTTP not tested"));
        assert!(text.contains("Internet access: not configured"));
    }
}

#[test]
fn refresh_and_scroll_are_ui_actions_without_backend_io() {
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)),
        140,
        30,
    );
    assert!(panel.refresh_requested);
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
        140,
        30,
    );
    assert!(panel.scroll > 0);
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        140,
        30,
    );
    assert_eq!(panel.scroll, 0);
}

#[test]
fn diagnostics_show_translated_checks_events_and_explicit_actions() {
    use elzevir_panel::{CheckResult, Diagnostics, LogEntry, LogEvent, LogFeed, LogState};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Diagnostics;
    if let Mode::Live(state) = &mut panel.mode
        && let Some(snapshot) = &mut state.snapshot
    {
        snapshot.diagnostics = Diagnostics {
            quick_check: CheckResult::Ok,
            catalog_http: CheckResult::Failed,
            verified_at: Some("2026-11-16T12:00:00Z".to_owned()),
            logs: LogFeed {
                state: LogState::Ready,
                entries: vec![LogEntry {
                    timestamp: "2026-11-16T12:00:00Z".to_owned(),
                    event: LogEvent::ServerStarted,
                }],
                ..LogFeed::default()
            },
            ..Diagnostics::default()
        };
    }
    for (lang, event) in [("en", "Server started"), ("pt-BR", "Servidor iniciado")] {
        let text = screen(&panel, lang);
        assert!(text.contains("quick_check: OK"));
        assert!(text.contains(event));
        assert!(!text.contains("panel-"));
        insta::assert_snapshot!(format!("diagnostics_{lang}"), text);
    }
    for key in ['v', 'e'] {
        panel.handle(
            &Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)),
            140,
            30,
        );
    }
    assert!(panel.diagnostics.pending);
    assert_eq!(
        panel.diagnostics.requested,
        Some(elzevir_panel::ObservationRequest::Export)
    );
}

#[test]
fn diagnostic_states_events_and_exports_are_localized_without_terminal_payloads() {
    use elzevir_panel::{CheckResult, ExportState, LogEntry, LogEvent, LogState};
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Diagnostics;
    for language in ["en", "pt-BR"] {
        for event in [
            LogEvent::ServerStarted,
            LogEvent::ServerStopped,
            LogEvent::ServerFailed,
            LogEvent::IndexerCommitted,
            LogEvent::IndexerFailed,
            LogEvent::IndexerPanicked,
            LogEvent::AdminHandlerFailed,
            LogEvent::RuntimeCleanupFailed,
        ] {
            if let Mode::Live(state) = &mut panel.mode
                && let Some(snapshot) = &mut state.snapshot
            {
                let values = &mut snapshot.diagnostics;
                values.logs.state = LogState::Ready;
                values.logs.entries = vec![LogEntry {
                    timestamp: "2026-11-16T12:00:00Z".to_owned(),
                    event,
                }];
                values.quick_check = CheckResult::Unknown;
                values.export = ExportState::Saved("test\x1b[2J\u{202e}.zip".to_owned());
            }
            let text = screen(&panel, language);
            assert!(!text.contains("panel-"));
            assert!(!text.contains('\x1b') && !text.contains('\u{202e}'));
        }
        for log_state in [LogState::Missing, LogState::Unavailable, LogState::Ready] {
            if let Mode::Live(state) = &mut panel.mode
                && let Some(snapshot) = &mut state.snapshot
            {
                snapshot.diagnostics.logs.state = log_state;
                snapshot.diagnostics.logs.entries.clear();
                snapshot.diagnostics.logs.limited = true;
                snapshot.diagnostics.logs.omitted = 3;
                snapshot.diagnostics.export = ExportState::Failed;
            }
            assert!(!screen(&panel, language).contains("panel-"));
        }
    }
}

#[test]
fn addresses_separate_loaded_configuration_observation_and_local_tests() {
    use elzevir_panel::{AddressBook, CheckResult, StaffAccess};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Addresses;
    if let Mode::Live(state) = &mut panel.mode {
        state.institution.configured_access = Some(StaffAccess::SameAddress);
        if let Some(snapshot) = &mut state.snapshot {
            snapshot.addresses = AddressBook {
                observed_mode: Some(StaffAccess::Hostname {
                    catalog_host: "catalog.example".to_owned(),
                    staff_host: "staff.example".to_owned(),
                }),
                catalog: Some("http://catalog.example:8080/".to_owned()),
                staff: Some("http://staff.example:8080/equipe".to_owned()),
                catalog_test: CheckResult::Ok,
                staff_test: CheckResult::Failed,
                tested_at: Some("2026-11-16T12:00:00Z".to_owned()),
                ..AddressBook::default()
            };
        }
    }
    for lang in ["en", "pt-BR"] {
        let text = screen(&panel, lang);
        assert!(text.contains("http://staff.example:8080/equipe"));
        assert!(text.contains("2026-11-16T12:00:00Z"));
        assert!(!text.contains("panel-"));
        insta::assert_snapshot!(format!("addresses_{lang}"), text);
    }
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)),
        140,
        30,
    );
    assert_eq!(
        panel.diagnostics.requested,
        Some(elzevir_panel::ObservationRequest::TestAddresses)
    );
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        140,
        30,
    );
    assert_eq!(
        panel.diagnostics.requested,
        Some(elzevir_panel::ObservationRequest::TestAddresses)
    );
}
