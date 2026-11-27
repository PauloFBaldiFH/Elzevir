// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure live-dashboard rendering and failure transitions.
#![expect(clippy::unwrap_used, reason = "rendering helpers fail tests on errors")]

use elzevir_panel::{
    Component, Details, Host, Institution, Messages, Mode, Panel, ServerStatus, Snapshot, Storage,
    draw,
};
use ratatui::{Terminal, backend::TestBackend};

fn screen(panel: &Panel, language: &str) -> String {
    sized_screen(panel, language, 140, 30)
}

fn sized_screen(panel: &Panel, language: &str, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let messages = Messages::new(language).unwrap();
    terminal
        .draw(|frame| draw(frame, panel, &messages, true))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
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
                capacity_bytes: Some(1 << 30),
            },
            host: Host {
                cpu_percent: Some(12.5),
                memory_bytes: Some((1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024)),
                ..Host::default()
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
        assert!(text.contains("Public HTTPS test: Not read"));
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

#[test]
fn interactive_cloudflare_sign_in_displays_only_the_projected_browser_link() {
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Addresses;
    if let Mode::Live(state) = &mut panel.mode {
        let sample = state.snapshot.as_mut().unwrap();
        sample
            .operations
            .authorization_uris
            .push("https://dash.cloudflare.com/argotunnel?callback=fictional".to_owned());
    }
    for language in ["en", "pt-BR"] {
        let rendered = screen(&panel, language);
        assert!(rendered.contains("dash.cloudflare.com/argotunnel?callback=fictional"));
        assert!(!rendered.contains("op-authorize-uri"));
        assert!(!rendered.contains("PRIVATE KEY"));
    }
}

#[test]
fn public_tunnel_test_and_log_tabs_never_conflate_a_gate_with_a_failure() {
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Addresses;
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.tunnel =
            elzevir_panel::TunnelState::Configured(elzevir_panel::Tunnel {
                catalog: "https://catalog.example.org/".to_owned(),
                staff: "https://staff.example.org/equipe".to_owned(),
                staff_tested_at: None,
                catalog_check: elzevir_panel::PublicCheck::Ok,
                staff_check: elzevir_panel::PublicCheck::Protected,
                tested_at: Some("2026-11-17T00:00:00Z".to_owned()),
                logs: elzevir_panel::TunnelLog::Ready {
                    events: vec!["tunnel-event-connected"],
                    omitted: 1,
                },
            });
    }
    for (language, gate, event) in [
        ("en", "Protected by", "Connector registered"),
        ("pt-BR", "Protegido pela", "Conector registrou"),
    ] {
        panel.tunnel_tab = elzevir_panel::TunnelTab::Test;
        let tested = screen(&panel, language);
        assert!(tested.contains(gate));
        assert!(tested.contains("catalog.example.org"));
        panel.tunnel_tab = elzevir_panel::TunnelTab::Log;
        let logged = screen(&panel, language);
        assert!(logged.contains(event));
        assert!(!logged.contains("tunnel-event-connected"));
    }
}

#[test]
fn remote_staff_password_is_masked_while_editing_and_confirming_in_both_languages() {
    use elzevir_panel::{AddressOperation, Form, Operation, Section};
    let mut panel = connected();
    panel.section = Section::Addresses;
    let mut form = Form::new(Operation::Address(AddressOperation::Staff {
        username: "operator".to_owned(),
        password: "fictional-private-password".to_owned(),
        cidrs: String::new(),
    }));
    form.field = 1;
    panel.form = Some(form);
    for language in ["en", "pt-BR"] {
        let editing = screen(&panel, language);
        assert!(!editing.contains("fictional-private-password"));
        assert!(editing.contains("********"));
    }
    panel.form.as_mut().unwrap().field = 3;
    for language in ["en", "pt-BR"] {
        let confirmed = screen(&panel, language);
        assert!(!confirmed.contains("fictional-private-password"));
        assert!(confirmed.contains("********"));
    }
    assert!(!format!("{panel:?}").contains("fictional-private-password"));
}

#[test]
fn cloud_provider_answers_are_masked_and_bound_to_the_current_question_revision() {
    use elzevir_panel::{
        CloudOperation, CloudQuestion, CloudView, ObservationRequest, Operation, Section,
    };
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = Section::Backup;
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.cloud = Some(CloudView {
            question: Some(CloudQuestion {
                id: "a".repeat(32),
                revision: "b".repeat(32),
                name: "config_token".to_owned(),
                help: String::new(),
                examples: vec![],
                default: String::new(),
                secret: true,
                refused: false,
            }),
            ..CloudView::default()
        });
    }
    for code in std::iter::once(KeyCode::Char('a'))
        .chain("fictional-private-token".chars().map(KeyCode::Char))
    {
        panel.handle(
            &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            120,
            32,
        );
    }
    for language in ["en", "pt-BR"] {
        assert!(!screen(&panel, language).contains("fictional-private-token"));
        assert!(screen(&panel, language).contains("********"));
    }
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        120,
        32,
    );
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
        120,
        32,
    );
    assert!(
        matches!(&panel.diagnostics.requested,Some(ObservationRequest::Operate(Operation::Cloud(CloudOperation::Answer {revision,..}))) if *revision=="b".repeat(32))
    );
}

#[test]
fn domain_flow_distinguishes_registration_connector_and_public_test_results() {
    use elzevir_panel::{ConnectorState, DomainPhase, DomainStatus, Section};
    let mut panel = connected();
    panel.section = Section::Addresses;
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.domain = Some(DomainStatus {
            broker_url: "https://broker.elzevir.app".to_owned(),
            domain: "elzevir.app".to_owned(),
            phase: DomainPhase::Provisioning,
            catalog: Some("fictional.elzevir.app".to_owned()),
            staff: Some("fictional-admin.elzevir.app".to_owned()),
            failure: None,
            registration_at: None,
            connector_at: "2026-11-17T00:00:00Z".into(),
            poll_seconds: None,
            connector: ConnectorState::Stopped,
            staff_allow_cidrs: vec!["192.0.2.0/24".to_owned()],
            remote_status: None,
        });
    }
    for (language, phase, connector) in [
        ("en", "Provisioning DNS and tunnel", "Stopped"),
        ("pt-BR", "Provisionando DNS e túnel", "Parado"),
    ] {
        let rendered = screen(&panel, language);
        assert!(rendered.contains(phase));
        assert!(rendered.contains(connector));
        assert!(rendered.contains("https://fictional.elzevir.app"));
        assert!(rendered.contains("fictional-admin.elzevir.app"));
        assert!(rendered.contains("192.0.2.0/24"));
        assert!(!rendered.contains("domain-"));
        assert!(!rendered.contains("device_code"));
    }
}

#[test]
fn registration_form_explains_the_default_broker_in_both_languages() {
    use elzevir_panel::{AddressOperation, Form, Operation, Section};
    let mut panel = connected();
    panel.section = Section::Addresses;
    panel.form = Some(Form::new(Operation::Address(AddressOperation::Enroll {
        base: String::new(),
        institution: "Fictional library".to_owned(),
        email: "fictional@example.org".to_owned(),
        name: "fictional".to_owned(),
        service: "elzevir".to_owned(),
    })));
    for language in ["en", "pt-BR"] {
        let rendered = screen(&panel, language);
        assert!(rendered.contains("broker.elzevir.app"), "{rendered}");
        assert!(!rendered.contains("broker-url"));
    }
}

#[test]
fn the_service_section_starts_and_stops_a_personal_server_and_restarts_a_managed_one() {
    use elzevir_panel::{Operation, Section, ServerAction};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let press = |panel: &mut Panel, key| {
        panel.handle(
            &Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)),
            140,
            30,
        );
    };
    let chosen = |key| {
        let mut panel = connected();
        panel.section = Section::Restart;
        press(&mut panel, key);
        panel.form.take().and_then(elzevir_panel::Form::finish)
    };
    assert_eq!(chosen('i'), Some(Operation::Server(ServerAction::Start)));
    assert_eq!(chosen('p'), Some(Operation::Server(ServerAction::Stop)));
    assert_eq!(chosen('t'), Some(Operation::Server(ServerAction::Restart)));
    for language in ["en", "pt-BR"] {
        let mut panel = connected();
        panel.section = Section::Restart;
        let messages = Messages::new(language).unwrap();
        for key in [
            "op-server-start",
            "op-server-stop",
            "op-server-restart",
            "op-server-done",
            "service-uptime",
            "service-stopped-hint",
            "ui-settings",
            "ui-reboot",
        ] {
            assert_ne!(messages.get(key), key);
        }
        let shown = screen(&panel, language);
        for button in [
            "ui-start",
            "ui-stop",
            "ui-restart",
            "ui-settings",
            "ui-reboot",
        ] {
            let text = messages.get(button);
            let text = text.trim_start_matches(|c: char| !c.is_alphabetic());
            assert!(shown.contains(text), "{button}\n{shown}");
        }
        insta::assert_snapshot!(format!("service_{language}"), shown);
    }

    let mut managed = Panel::live(Institution {
        managed: true,
        ..Institution::default()
    });
    managed.section = Section::Restart;
    press(&mut managed, 'p');
    assert!(
        managed.form.is_none(),
        "the service manager stops a managed server"
    );
    press(&mut managed, 't');
    assert!(matches!(
        managed.form.take().map(|form| form.operation),
        Some(Operation::Restart { .. })
    ));
}

#[test]
fn the_settings_form_shows_every_field_and_picks_the_language_from_buttons() {
    use elzevir_panel::{Operation, Section};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let press = |panel: &mut Panel, code| {
        panel.handle(
            &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            140,
            30,
        );
    };
    let mut panel = Panel::live(Institution {
        name: Some("Biblioteca de teste".to_owned()),
        configured_listen: Some("127.0.0.1:7390".parse().unwrap()),
        language: Some("pt-BR".to_owned()),
        ..Institution::default()
    });
    panel.section = Section::Restart;
    press(&mut panel, KeyCode::Char('c'));
    let shown = screen(&panel, "pt-BR");
    for text in [
        "Nome da biblioteca",
        "Biblioteca de teste",
        "127.0.0.1:7390",
        "English",
        "Português",
        "Confirmar",
    ] {
        assert!(shown.contains(text), "{text}\n{shown}");
    }
    insta::assert_snapshot!("settings_form_pt-BR", shown);
    // Typing never edits a choice; the arrows change it.
    press(&mut panel, KeyCode::Tab);
    press(&mut panel, KeyCode::Tab);
    press(&mut panel, KeyCode::Char('x'));
    press(&mut panel, KeyCode::Right);
    panel.submit_form();
    assert!(panel.form.is_none());
    assert!(matches!(
        &panel.diagnostics.requested,
        Some(elzevir_panel::ObservationRequest::Operate(Operation::Settings { name, listen, language }))
            if name == "Biblioteca de teste" && listen == "127.0.0.1:7390" && language == "en"
    ));
}

#[test]
fn local_backup_form_shows_scope_codec_and_destination() {
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Backup;
    panel.form = Some(elzevir_panel::Form::new(
        elzevir_panel::Operation::BackupNow {
            options: elzevir_panel::BackupOptions::default(),
            folder: String::new(),
        },
    ));
    for language in ["en", "pt-BR"] {
        insta::assert_snapshot!(format!("local_backup_{language}"), screen(&panel, language));
    }
}

#[test]
fn assisted_restore_review_defaults_to_back_and_requires_a_choice() {
    use elzevir_panel::{BackupScope, Form, ObservationRequest, Operation, RestoreReview};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Backup;
    let review = RestoreReview {
        token: "local-review".into(),
        id: "00112233445566778899aabbccddeeff".into(),
        file: "biblioteca.age".into(),
        created: "2026-11-20T22:30:00Z".into(),
        scope: BackupScope::Database,
        schema: 1,
        managed: false,
    };
    let event = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    panel.form = Some(Form::new(Operation::ApplyRestore(review.clone())));
    for lang in ["pt-BR", "en"] {
        insta::assert_snapshot!(format!("restore_review_{lang}"), screen(&panel, lang));
    }
    panel.handle(&Event::Paste("y\n".into()), 140, 30);
    panel.handle(&event(KeyCode::Enter), 140, 30);
    assert!(panel.diagnostics.requested.is_none());
    panel.form = Some(Form::new(Operation::ApplyRestore(review.clone())));
    panel.handle(&event(KeyCode::Right), 140, 30);
    for (width, height) in [(79, 24), (80, 17)] {
        panel.handle(&event(KeyCode::Enter), width, height);
        panel.handle(&event(KeyCode::Char('y')), width, height);
        assert!(panel.form.is_some());
        assert!(panel.diagnostics.requested.is_none());
    }
    for height in [18, 24] {
        for (lang, impact, choice) in [
            ("en", "Database only", "Back"),
            ("pt-BR", "Somente banco", "Voltar"),
        ] {
            let shown = sized_screen(&panel, lang, 80, height);
            for label in ["biblioteca.age", impact, choice] {
                assert!(shown.contains(label), "{shown}");
            }
        }
    }
    panel.handle(&event(KeyCode::Enter), 80, 18);
    assert_eq!(
        panel.diagnostics.requested,
        Some(ObservationRequest::Operate(Operation::ApplyRestore(review)))
    );
}

#[test]
fn pasted_restore_fields_cannot_trigger_navigation_or_submission() {
    use ratatui::crossterm::event::Event;
    let mut panel = connected();
    panel.form = Some(elzevir_panel::Form::new(
        elzevir_panel::Operation::Restore {
            input: String::new(),
            key: String::new(),
        },
    ));
    panel.handle(&Event::Paste("'/media/a b.age'".into()), 140, 30);
    assert_eq!(
        panel.form.as_ref().unwrap().values.first().unwrap(),
        "'/media/a b.age'"
    );
    panel.handle(&Event::Paste("y\nq".into()), 140, 30);
    assert!(
        panel
            .form
            .as_ref()
            .unwrap()
            .values
            .first()
            .unwrap()
            .is_empty()
    );
    assert!(panel.diagnostics.requested.is_none());
    assert!(!panel.quit);
}

fn displayed_files(panel: &mut Panel) -> Option<&mut Vec<(String, u64)>> {
    let Mode::Live(state) = &mut panel.mode else {
        return None;
    };
    Some(
        &mut state
            .snapshot
            .as_mut()?
            .operations
            .cloud
            .as_mut()?
            .remote
            .as_mut()?
            .files,
    )
}

#[test]
fn cloud_tabs_show_destination_quota_pending_status_and_bound_actions() {
    use elzevir_panel::{
        CloudConnection, CloudOperation, CloudPlan, CloudTarget, CloudView, Operation,
        RemoteCommand, RemotePage,
    };
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Backup;
    panel.backups.page = elzevir_panel::BackupPage::Connections;
    let target = CloudTarget {
        plan: "a".repeat(32),
        connection: "b".repeat(32),
        folder: "Acervo/noite".into(),
    };
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.cloud = Some(CloudView {
            plans: vec![CloudPlan {
                id: target.plan.clone(),
                name: "Noite".into(),
                connection: target.connection.clone(),
                folder: target.folder.clone(),
                keep: 7,
                daily: None,
                retention_pending: false,
            }],
            connections: vec![CloudConnection {
                id: target.connection.clone(),
                name: "Biblioteca".into(),
                provider: "s3".into(),
                ready: true,
                test_failed: false,
                tested_at: None,
            }],
            transfers: vec![(target.clone(), "c".repeat(32), "cloud-unconfirmed".into())],
            remote: Some(RemotePage {
                target: target.clone(),
                page: 1,
                pages: 1,
                files: vec![("c".repeat(32), 128), ("d".repeat(32), 256)],
                free: None,
            }),
            ..Default::default()
        });
    }
    let key = |panel: &mut Panel, c| {
        panel.handle(
            &Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
            140,
            30,
        );
    };
    for tab in ["connections", "plans", "files", "history"] {
        insta::assert_snapshot!(format!("cloud_{tab}_pt_BR"), screen(&panel, "pt-BR"));
        key(&mut panel, ']');
    }
    key(&mut panel, '.');
    displayed_files(&mut panel).unwrap().reverse();
    key(&mut panel, 'D');
    assert_eq!(
        panel.form.take().unwrap().operation,
        Operation::Cloud(CloudOperation::Remote(
            target.clone(),
            RemoteCommand::Download("d".repeat(32))
        ))
    );
    key(&mut panel, 'D');
    displayed_files(&mut panel)
        .unwrap()
        .retain(|(id, _)| *id != "d".repeat(32));
    panel.submit_form();
    assert!(panel.diagnostics.requested.is_none());
    assert_eq!(panel.cloud_tab, 2);
    key(&mut panel, 'D');
    assert!(panel.form.is_none());
    assert!(screen(&panel, "pt-BR").contains("saiu da lista"));
    key(&mut panel, '.');
    key(&mut panel, 'D');
    assert!(
        matches!(panel.form.take().unwrap().operation,Operation::Cloud(CloudOperation::Remote(_,RemoteCommand::Download(id))) if id == "c".repeat(32))
    );
    key(&mut panel, '+');
    key(&mut panel, 'M');
    assert!(
        matches!(panel.form.take().unwrap().operation,Operation::Cloud(CloudOperation::SendGroup(targets,_)) if targets==[target])
    );
}

#[test]
fn narrow_and_ascii_sharing_show_only_the_catalog_and_escape_returns_to_addresses() {
    use elzevir_panel::{PublicCheck, Section, Tunnel, TunnelState, TunnelTab};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = Section::Addresses;
    panel.tunnel_tab = TunnelTab::Share;
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.tunnel = TunnelState::Configured(Tunnel {
            catalog: "https://public.elzevir.app/".into(),
            staff: "https://staff.elzevir.app/equipe".into(),
            catalog_check: PublicCheck::NotRun,
            staff_check: PublicCheck::NotRun,
            tested_at: None,
            staff_tested_at: None,
            logs: elzevir_panel::TunnelLog::default(),
        });
    }
    for (width, ascii) in [(52, false), (120, true)] {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| draw(frame, &panel, &Messages::new("pt-BR").unwrap(), ascii))
            .unwrap();
        let buffer = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(buffer.contains("https://public.elzevir.app/"));
        assert!(!buffer.contains("staff.elzevir.app"));
        assert!(!buffer.contains(['▀', '▄', '█']));
    }
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        52,
        24,
    );
    assert_eq!(panel.tunnel_tab, TunnelTab::Addresses);
}

#[test]
fn managing_addresses_exposes_saved_enrollment_and_reports_disabled_staff() {
    use elzevir_panel::{AddressOperation, Operation, RemoteAddress, Section, TunnelTab};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = Section::Addresses;
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.remote_address = Some(RemoteAddress {
            at: "2026-11-20T12:00:00Z".into(),
            listed: false,
            staff_remote: false,
        });
    }
    let press = |panel: &mut Panel, code| {
        panel.handle(
            &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            140,
            30,
        );
    };
    press(&mut panel, KeyCode::Char(']'));
    assert_eq!(panel.tunnel_tab, TunnelTab::Manage);
    let rendered = screen(&panel, "en");
    assert!(rendered.contains("Remote staff access: disabled"));
    insta::assert_snapshot!("manage_addresses", rendered);
    press(&mut panel, KeyCode::Char('S'));
    assert!(panel.form.is_none());
    press(&mut panel, KeyCode::Char('G'));
    assert!(matches!(
        panel.form.as_ref().unwrap().operation,
        Operation::Address(AddressOperation::Resume { .. })
    ));
    press(&mut panel, KeyCode::Esc);
    press(&mut panel, KeyCode::Char('g'));
    assert!(!panel.form.as_ref().unwrap().labels.contains(&"broker-url"));
    press(&mut panel, KeyCode::F(2));
    assert_eq!(
        panel.form.as_ref().unwrap().labels.last(),
        Some(&"broker-url")
    );
}

#[test]
fn advanced_broker_field_stays_visible_at_compact_sizes() {
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Addresses;
    for code in [KeyCode::Char('g'), KeyCode::F(2), KeyCode::Char('Z')] {
        panel.handle(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)), 80, 18);
    }
    for height in [18, 24] {
        for lang in ["en", "pt-BR"] {
            let shown = sized_screen(&panel, lang, 80, height);
            assert!(shown.contains("Broker HTTPS"), "{shown}");
            assert!(shown.contains("Z▏"), "{shown}");
        }
    }
}

#[test]
fn dashboard_reports_a_published_backup_even_after_an_export_failure() {
    let mut panel = connected();
    if let Mode::Live(state) = &mut panel.mode {
        let ops = &mut state.snapshot.as_mut().unwrap().operations;
        ops.local_backup = elzevir_panel::LocalBackup::Verified;
        ops.latest_export_failed = Some(true);
        ops.jobs = vec![elzevir_panel::Job {
            id: String::new(),
            at: "10:00 UTC".to_owned(),
            action: "op-backup-now",
            state: "op-failed",
            phase: None,
        }];
        ops.cloud = Some(elzevir_panel::CloudView {
            transfers: vec![(
                elzevir_panel::CloudTarget {
                    plan: String::new(),
                    connection: String::new(),
                    folder: String::new(),
                },
                String::new(),
                "op-queued".to_owned(),
            )],
            ..Default::default()
        });
    }
    let rendered = screen(&panel, "en");
    assert!(rendered.contains("Verified at creation"));
    assert!(rendered.contains("Export failed"));
    assert!(rendered.contains("Last request: Failed"));
    assert!(rendered.contains("Cloud transfer: Queued"));
    if let Mode::Live(state) = &mut panel.mode {
        state.age_seconds = 6;
    }
    assert!(!screen(&panel, "en").contains("Verified at creation"));
}

#[test]
fn local_schedule_cannot_be_replaced_from_an_unknown_or_stale_policy() {
    use elzevir_panel::Section;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = Section::Backup;
    let edit = Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    panel.handle(&edit, 80, 24);
    assert!(panel.form.is_none());
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.keep = Some(12);
        state.age_seconds = 6;
    }
    panel.handle(&edit, 80, 24);
    assert!(panel.form.is_none());
    if let Mode::Live(state) = &mut panel.mode {
        state.age_seconds = 0;
    }
    panel.handle(&edit, 80, 24);
    assert_eq!(panel.form.as_ref().unwrap().values[0], "12");
}

#[test]
fn cloud_workspace_exposes_mouse_actions_in_a_compact_terminal() {
    let mut panel = connected();
    panel.section = elzevir_panel::Section::Backup;
    panel.backups.page = elzevir_panel::BackupPage::Connections;
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.cloud =
            Some(elzevir_panel::CloudView::default());
    }
    let compact = sized_screen(&panel, "pt-BR", 80, 24);
    for label in [
        "Consultar provedores",
        "Nova conexão",
        "Agendamentos",
        "Voltar à central",
    ] {
        assert!(compact.contains(label), "{compact}");
    }
}

#[test]
fn provider_selection_filters_the_verified_catalogue_without_requesting_a_job() {
    use elzevir_panel::{BackupPage, CloudView, Section};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let mut panel = connected();
    panel.section = Section::Backup;
    panel.backups.page = BackupPage::Providers;
    if let Mode::Live(state) = &mut panel.mode {
        state.snapshot.as_mut().unwrap().operations.cloud = Some(CloudView {
            providers: vec![
                ("drive".into(), "Google Drive".into()),
                ("sftp".into(), "SFTP".into()),
            ],
            ..CloudView::default()
        });
    }
    for c in "GOOGLE".chars() {
        panel.handle(
            &Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
            80,
            24,
        );
    }
    let content = sized_screen(&panel, "pt-BR", 80, 24);
    assert!(content.contains("Google Drive"));
    assert!(!content.contains("SFTP"));
    assert!(panel.diagnostics.requested.is_none());
    panel.handle(
        &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        80,
        24,
    );
    assert_eq!(panel.backups.page, BackupPage::Connections);
}
