// SPDX-License-Identifier: GPL-3.0-or-later
//! Confirmed writes remain distinct from navigation and polling.
use elzevir_panel::{Institution, ObservationRequest, Operation, Panel, Section};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
fn key(panel: &mut Panel, code: KeyCode) {
    panel.handle(
        &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        120,
        32,
    );
}
#[test]
fn backup_requires_confirmation_and_key_repeat_does_not_duplicate_it() {
    let mut panel = Panel::live(Institution::default());
    panel.section = Section::Backup;
    key(&mut panel, KeyCode::Char('n'));
    assert!(panel.diagnostics.requested.is_none());
    for _ in 0..3 {
        key(&mut panel, KeyCode::Tab);
    }
    assert!(panel.form.as_ref().unwrap().confirming());
    key(&mut panel, KeyCode::Char('y'));
    assert_eq!(
        panel.diagnostics.requested,
        Some(ObservationRequest::Operate(Operation::BackupNow {
            options: elzevir_panel::BackupOptions::default(),
            folder: String::new()
        }))
    );
    key(&mut panel, KeyCode::Char('n'));
    assert!(panel.form.is_none());
    assert!(panel.diagnostics.pending);
}
#[test]
fn restore_fields_are_bounded_and_escape_cancels_without_submission() {
    let mut panel = Panel::live(Institution::default());
    panel.section = Section::Backup;
    key(&mut panel, KeyCode::Char('o'));
    for _ in 0..1000 {
        key(&mut panel, KeyCode::Char('a'));
    }
    assert_eq!(
        panel.form.as_ref().unwrap().values.first().unwrap().len(),
        512
    );
    key(&mut panel, KeyCode::Enter);
    key(&mut panel, KeyCode::Char('k'));
    key(&mut panel, KeyCode::Enter);
    assert!(panel.form.as_ref().unwrap().confirming());
    key(&mut panel, KeyCode::Esc);
    assert!(panel.form.is_none());
    assert!(panel.diagnostics.requested.is_none());
    assert!(!panel.diagnostics.pending);
}

#[test]
fn interrupted_update_recovery_requires_a_target_and_explicit_confirmation() {
    let mut panel = Panel::live(Institution::default());
    panel.section = Section::Updates;
    key(&mut panel, KeyCode::Char('r'));
    assert!(panel.form.is_some());
    assert!(panel.diagnostics.requested.is_none());
    key(&mut panel, KeyCode::Enter);
    assert!(panel.form.as_ref().unwrap().confirming());
    key(&mut panel, KeyCode::Char('y'));
    assert_eq!(
        panel.diagnostics.requested,
        Some(ObservationRequest::Operate(Operation::RecoverUpdate {
            executable: String::new()
        }))
    );
}

#[test]
fn editing_a_selected_plan_keeps_the_destination_out_of_editable_fields() {
    use elzevir_panel::{CloudOperation, Form};
    let policy = CloudOperation::Plan {
        id: "a".repeat(32),
        name: "Daily".into(),
        connection: "b".repeat(32),
        folder: "copies".into(),
        keep: "12".into(),
        time: "04:30".into(),
        zone: "UTC".into(),
    };
    let mut form = Form::new(Operation::Cloud(policy.clone()));
    assert_eq!(
        form.labels,
        ["cloud-plan-name", "op-keep", "op-time", "op-zone"]
    );
    form.field = form.labels.len();
    assert_eq!(form.finish(), Some(Operation::Cloud(policy)));
}
