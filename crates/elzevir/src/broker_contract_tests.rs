// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact wire fixtures for the deployed Elzevir broker contract; no network or services.
use super::*;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};
struct Clock {
    now: Cell<i64>,
    waits: RefCell<Vec<u32>>,
}
impl Clock {
    fn new() -> Self {
        Self {
            now: Cell::new(42),
            waits: RefCell::new(vec![]),
        }
    }
}
impl FlowClock for Clock {
    fn now(&self) -> i64 {
        self.now.get()
    }
    fn wait(&self, seconds: u32) {
        self.waits.borrow_mut().push(seconds);
        self.now.set(self.now.get() + i64::from(seconds));
    }
}
struct Wire {
    replies: RefCell<VecDeque<(&'static str, &'static str, u16, Value)>>,
    public: RefCell<Option<Vec<u8>>>,
}
impl Wire {
    fn new(approved: bool) -> Self {
        let start = json!({"device_code":"private-device-code", "suggested_slug":"fictional", "domain":"elzevir.app", "expires_in":600, "interval":7, "status": if approved { "approved" } else { "pending" }, "hostnames":if approved { json!({"opac":"fictional.elzevir.app","staff":"fictional-admin.elzevir.app"}) } else { Value::Null }});
        let mut replies = VecDeque::from([
            ("POST", "/v1/device/start", 200, start),
            (
                "POST",
                "/v1/enroll",
                202,
                json!({"library_id":"fictional-library","job_id":"fictional-job","domain":"elzevir.app","hostnames":{"opac":"fictional.elzevir.app","staff":"fictional-admin.elzevir.app"}}),
            ),
            (
                "GET",
                "/v1/jobs/fictional-job",
                200,
                json!({"state":"done"}),
            ),
            ("GET", "/v1/library", 200, Self::library()),
            (
                "GET",
                "/v1/tunnel-token",
                200,
                json!({"tunnel_token":"fictional-token-012345678901234567890123456789"}),
            ),
            ("GET", "/v1/library", 200, Self::library()),
        ]);
        if !approved {
            replies.insert(
                1,
                ("POST", "/v1/device/poll", 200, json!({"status":"approved"})),
            );
        }
        Self {
            replies: RefCell::new(replies),
            public: RefCell::new(None),
        }
    }
    fn library() -> Value {
        json!({"status":"active", "hostnames":{"opac":"fictional.elzevir.app","staff":"fictional-admin.elzevir.app"}, "staff_remote_access":true, "staff_allow_cidrs":["192.0.2.0/24"]})
    }
}
impl Transport for Wire {
    fn request(
        &self,
        _: &str,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<Response, String> {
        let (expected_method, expected_path, status, value) = self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected broker request");
        assert_eq!((method, path), (expected_method, expected_path));
        if path == "/v1/device/start" {
            let sent: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(sent["domain"], "elzevir.app");
            assert_eq!(
                sent,
                json!({"institution_name":"Fictional library", "contact_email":"fictional@example.org", "requested_name":"fictional", "domain":"elzevir.app"})
            );
        } else if path == "/v1/enroll" {
            let sent: Value = serde_json::from_slice(body).unwrap();
            let public = base64::engine::general_purpose::STANDARD
                .decode(sent["public_key"].as_str().unwrap())
                .unwrap();
            assert_eq!(public.len(), 32);
            *self.public.borrow_mut() = Some(public);
        } else if path != "/v1/device/poll" {
            let get = |key: &str| {
                headers
                    .iter()
                    .find(|(name, _)| *name == key)
                    .unwrap()
                    .1
                    .as_str()
            };
            let canonical = format!(
                "KEI-SIG-v1\n{method}\n{path}\n{}\n{}\n{:x}",
                get("X-KEI-Timestamp"),
                get("X-KEI-Nonce"),
                <sha2::Sha256 as sha2::Digest>::digest(body)
            );
            let signature = base64::engine::general_purpose::STANDARD
                .decode(get("X-KEI-Signature"))
                .unwrap();
            ring::signature::UnparsedPublicKey::new(
                &ring::signature::ED25519,
                self.public.borrow().as_ref().unwrap(),
            )
            .verify(canonical.as_bytes(), &signature)
            .unwrap();
            if matches!(method, "GET" | "DELETE") {
                assert!(body.is_empty());
            }
        }
        Ok(Response {
            status,
            body: zeroize::Zeroizing::new(if status == 204 {
                vec![]
            } else {
                serde_json::to_vec(&value).unwrap()
            }),
        })
    }
}
struct ConnectorFake {
    connects: Cell<usize>,
    stops: Cell<usize>,
}
impl ConnectorFake {
    fn new() -> Self {
        Self {
            connects: Cell::new(0),
            stops: Cell::new(0),
        }
    }
}
impl Connector for ConnectorFake {
    fn connect(&self, _: &Path, _: &Setup, hosts: &Hosts, token: &str) -> Result<(), String> {
        assert_eq!(hosts.staff, "fictional-admin.elzevir.app");
        assert!(token.starts_with("fictional-token"));
        self.connects.set(self.connects.get() + 1);
        Ok(())
    }
    fn stop(&self, _: &Path) -> Result<(), String> {
        self.stops.set(self.stops.get() + 1);
        Ok(())
    }
}
#[test]
fn already_approved_registration_skips_capabilities_and_poll_and_persists_private_config() {
    let directory = tempfile::tempdir().unwrap();
    let setup = tests::fixture(directory.path());
    let wire = Wire::new(true);
    let clock = Clock::new();
    let connector = ConnectorFake::new();
    execute_with_clock(directory.path(), &setup, &wire, &connector, None, &clock).unwrap();
    assert!(clock.waits.borrow().is_empty());
    assert!(wire.replies.borrow().is_empty());
    assert_eq!(connector.connects.get(), 1);
    let stored: Value =
        serde_json::from_slice(&read(&location(directory.path())).unwrap()).unwrap();
    assert_eq!(stored["library_id"], "fictional-library");
    assert!(stored["private_key"].as_str().is_some());
    assert_eq!(stored["domain"], "elzevir.app");
    assert_eq!(stored["hostnames"]["opac"], "fictional.elzevir.app");
    assert!(!stored.to_string().contains("fictional-token"));
}
#[test]
fn pending_registration_respects_advertised_intervals_and_resumes_provisioning() {
    let directory = tempfile::tempdir().unwrap();
    let setup = tests::fixture(directory.path());
    let wire = Wire::new(false);
    let clock = Clock::new();
    let connector = ConnectorFake::new();
    wire.replies.borrow_mut().insert(
        1,
        (
            "POST",
            "/v1/device/poll",
            200,
            json!({"status":"pending","interval":3}),
        ),
    );
    wire.replies.borrow_mut().insert(
        4,
        (
            "GET",
            "/v1/jobs/fictional-job",
            200,
            json!({"state":"running"}),
        ),
    );
    execute_with_clock(directory.path(), &setup, &wire, &connector, None, &clock).unwrap();
    assert_eq!(*clock.waits.borrow(), [7, 3, 5]);
    assert_eq!(connector.connects.get(), 1);
}
#[test]
fn denied_expired_or_wrong_domain_never_enroll_or_start_a_connector() {
    for status in ["denied", "expired", "wrong_domain"] {
        let directory = tempfile::tempdir().unwrap();
        let setup = tests::fixture(directory.path());
        let wire = Wire::new(false);
        let connector = ConnectorFake::new();
        if status == "wrong_domain" {
            wire.replies.borrow_mut().get_mut(0).unwrap().3["domain"] = json!("koha.nexus");
        } else {
            wire.replies.borrow_mut().get_mut(1).unwrap().3 = json!({"status":status});
        }
        assert!(
            execute_with_clock(
                directory.path(),
                &setup,
                &wire,
                &connector,
                None,
                &Clock::new()
            )
            .is_err()
        );
        assert_eq!(connector.connects.get(), 0);
        assert!(load(directory.path()).unwrap().library.is_none());
    }
}
#[test]
fn failed_provisioning_keeps_the_identity_and_exposes_only_classified_errors() {
    let directory = tempfile::tempdir().unwrap();
    let setup = tests::fixture(directory.path());
    let wire = Wire::new(true);
    let connector = ConnectorFake::new();
    wire.replies.borrow_mut().get_mut(2).unwrap().3 =
        json!({"state":"failed","error":"secret-token-must-never-reach-the-view"});
    assert!(
        execute_with_clock(
            directory.path(),
            &setup,
            &wire,
            &connector,
            None,
            &Clock::new()
        )
        .is_err()
    );
    let identity = load(directory.path()).unwrap();
    assert_eq!(identity.library.as_deref(), Some("fictional-library"));
    assert_eq!(identity.phase, DomainPhase::Failed);
    assert!(
        !serde_json::to_string(&identity)
            .unwrap()
            .contains("secret-token")
    );
    assert_eq!(connector.connects.get(), 0);
}
#[test]
fn signed_mutations_accept_empty_success_and_withdraw_allows_fresh_enrollment() {
    let directory = tempfile::tempdir().unwrap();
    let mut setup = tests::fixture(directory.path());
    let wire = Wire::new(true);
    let connector = ConnectorFake::new();
    let clock = Clock::new();
    execute_with_clock(directory.path(), &setup, &wire, &connector, None, &clock).unwrap();
    let enrollment = setup.action.clone();
    let old_key = wire.public.borrow().clone();
    for (action, method, path) in [
        (Action::DisableStaff, "DELETE", "/v1/staff-credentials"),
        (
            Action::Staff {
                username: "remote".into(),
                password: "fictional-password".into(),
                cidrs: vec!["192.0.2.0/24".into()],
            },
            "PUT",
            "/v1/staff-credentials",
        ),
        (Action::Withdraw, "DELETE", "/v1/library"),
    ] {
        setup.action = action;
        wire.replies
            .borrow_mut()
            .push_back((method, path, 204, Value::Null));
        if path != "/v1/library" {
            wire.replies
                .borrow_mut()
                .push_back(("GET", "/v1/library", 200, Wire::library()));
        }
        execute_with_clock(directory.path(), &setup, &wire, &connector, None, &clock).unwrap();
    }
    assert!(!location(directory.path()).exists());
    assert!(wire.replies.borrow().is_empty());
    setup.action = enrollment;
    let next = Wire::new(true);
    execute_with_clock(directory.path(), &setup, &next, &connector, None, &clock).unwrap();
    assert_ne!(*next.public.borrow(), old_key);
}

#[test]
fn rotation_and_heartbeat_are_signed_and_accept_no_content() {
    let directory = tempfile::tempdir().unwrap();
    let mut setup = tests::fixture(directory.path());
    let wire = Wire::new(true);
    let connector = ConnectorFake::new();
    let clock = Clock::new();
    execute_with_clock(directory.path(), &setup, &wire, &connector, None, &clock).unwrap();
    wire.replies.borrow_mut().extend([
        ("POST", "/v1/heartbeat", 204, Value::Null),
        ("POST", "/v1/rotate", 204, Value::Null),
        ("GET", "/v1/library", 200, Wire::library()),
        (
            "GET",
            "/v1/tunnel-token",
            200,
            json!({"tunnel_token":"fictional-token-new-012345678901234567890123456789"}),
        ),
        ("GET", "/v1/library", 200, Wire::library()),
    ]);
    heartbeat_with(directory.path(), &wire).unwrap();
    setup.action = Action::Renew;
    execute_with_clock(directory.path(), &setup, &wire, &connector, None, &clock).unwrap();
    assert_eq!(connector.connects.get(), 2);
    assert!(wire.replies.borrow().is_empty());
}

#[test]
fn public_projection_preserves_network_restrictions_without_private_material() {
    let directory = tempfile::tempdir().unwrap();
    let setup = tests::fixture(directory.path());
    execute_with_clock(
        directory.path(),
        &setup,
        &Wire::new(true),
        &ConnectorFake::new(),
        None,
        &Clock::new(),
    )
    .unwrap();
    let view = domain::projection(directory.path(), ConnectorState::Stopped).unwrap();
    assert_eq!(view.phase, DomainPhase::Ready);
    assert_eq!(view.connector, ConnectorState::Stopped);
    assert_eq!(view.staff_allow_cidrs, ["192.0.2.0/24"]);
    let public = serde_json::to_string(&view).unwrap();
    for secret in [
        "private_key",
        "device_code",
        "fictional-token",
        "private-device-code",
    ] {
        assert!(!public.contains(secret));
    }
    assert!(public.contains("fictional-admin.elzevir.app"));
}

#[test]
fn legacy_private_configuration_is_read_without_replacing_its_key_or_hosts() {
    let directory = tempfile::tempdir().unwrap();
    let setup = tests::fixture(directory.path());
    execute_with_clock(
        directory.path(),
        &setup,
        &Wire::new(true),
        &ConnectorFake::new(),
        None,
        &Clock::new(),
    )
    .unwrap();
    let identity = load(directory.path()).unwrap();
    let public = identity.key().unwrap().public();
    let mut legacy = serde_json::to_value(&identity).unwrap();
    let object = legacy.as_object_mut().unwrap();
    for (current, old) in [
        ("private_key", "seed"),
        ("library_id", "library"),
        ("job_id", "job"),
        ("hostnames", "hosts"),
    ] {
        let value = object.remove(current).unwrap();
        object.insert(old.to_owned(), value);
    }
    for field in ["domain", "approved", "interval", "phase", "failure"] {
        object.remove(field);
    }
    object["hosts"] = json!({"opac":"catalog.example.org","staff":"staff.example.org"});
    publish(
        directory.path(),
        &location(directory.path()),
        &serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let loaded = load(directory.path()).unwrap();
    assert_eq!(loaded.key().unwrap().public(), public);
    assert_eq!(loaded.hosts.as_ref().unwrap().opac, "catalog.example.org");
    assert!(loaded.domain.is_none());
}

#[test]
fn job_polling_is_bounded_and_an_inconsistent_staff_hostname_is_never_connected() {
    let directory = tempfile::tempdir().unwrap();
    let setup = tests::fixture(directory.path());
    let wire = Wire::new(true);
    wire.replies.borrow_mut().truncate(2);
    for _ in 0..120 {
        wire.replies.borrow_mut().push_back((
            "GET",
            "/v1/jobs/fictional-job",
            200,
            json!({"state":"running"}),
        ));
    }
    let clock = Clock::new();
    let connector = ConnectorFake::new();
    assert!(execute_with_clock(directory.path(), &setup, &wire, &connector, None, &clock).is_err());
    assert_eq!(clock.now.get(), 642);
    assert_eq!(
        load(directory.path()).unwrap().phase,
        DomainPhase::Provisioning
    );
    assert_eq!(connector.connects.get(), 0);
    let mut hosts = Hosts {
        opac: "fictional.elzevir.app".to_owned(),
        staff: "other-admin.elzevir.app".to_owned(),
    };
    assert!(hosts.validate_for(Some("elzevir.app")).is_err());
}
