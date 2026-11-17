// SPDX-License-Identifier: GPL-3.0-or-later
//! Local address candidates and bounded HTTP tests, without DNS or network scanning.

use crate::access_config::StaffMode;
use elzevir_panel::{AddressBook, CheckResult, ObservationRequest, StaffAccess};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

#[derive(clap::Args)]
pub(crate) struct AddressArgs {
    /// Institution configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Data directory, overriding the configuration file.
    #[arg(long, required_unless_present = "config")]
    data_dir: Option<PathBuf>,
    /// Test local catalog and login-form HTTP responses, without DNS.
    #[arg(long)]
    test: bool,
}

pub(crate) fn run(args: AddressArgs) -> Result<(), String> {
    let configured = args.config.is_some();
    let settings = crate::server_config::ServeArgs::for_status(args.config, args.data_dir)
        .resolve()
        .map_err(|error| error.to_string())?;
    let status = crate::status::inspect(&settings.data_dir);
    let book = Context::default().observe(
        &status,
        if args.test {
            &ObservationRequest::TestAddresses
        } else {
            &ObservationRequest::Poll
        },
    );
    let observed_mode = status.staff.as_ref().map(|mode| match mode {
        StaffMode::SameAddress {} => "same_address",
        StaffMode::Hostname { .. } => "hostname",
        StaffMode::SeparateListener { .. } => "separate_listener",
    });
    println!(
        "{}",
        serde_json::json!({
            "schema":1, "state":status.state,
            "configured": if configured {Some(serde_json::json!({"listen":settings.listen,"staff":settings.staff}))} else {None},
            "observed_mode":observed_mode, "observed_staff":status.staff, "observed_listen":status.listen,
            "catalog":book.catalog,"staff":book.staff,"catalog_lan":book.catalog_lan,"staff_lan":book.staff_lan,
            "catalog_test":book.catalog_test.code(),"staff_test":book.staff_test.code(),"tested_at":book.tested_at,
            "test_scope":"this_computer_only", "dns":"not_tested", "internet":"not_tested"
        })
    );
    Ok(())
}

type Identity = (String, Option<SocketAddr>, Option<StaffMode>);

#[derive(Default)]
pub(crate) struct Context {
    identity: Option<Identity>,
    book: AddressBook,
}

impl Context {
    pub(crate) fn observe(
        &mut self,
        status: &crate::status::Status,
        request: &ObservationRequest,
    ) -> AddressBook {
        let identity = status
            .health
            .as_ref()
            .map(|health| (health.instance.clone(), status.listen, status.staff.clone()));
        let ips = local_ips();
        let mut book = if identity.is_some() {
            build(status.listen, status.staff.as_ref(), &ips)
        } else {
            AddressBook::default()
        };
        if identity == self.identity {
            book.catalog_test = self.book.catalog_test;
            book.staff_test = self.book.staff_test;
            book.tested_at.clone_from(&self.book.tested_at);
        }
        if request != &ObservationRequest::Poll {
            book.tested_at = Some(jiff::Timestamp::now().to_string());
            let (catalog, staff, catalog_host, staff_host) =
                test_targets(status.listen, status.staff.as_ref());
            book.catalog_test = probe(catalog, catalog_host, "/", &ips);
            book.staff_test = probe(staff, staff_host, "/equipe/login", &ips);
        }
        self.identity = identity;
        self.book = book.clone();
        book
    }

    pub(crate) fn book(&self) -> AddressBook {
        self.book.clone()
    }
}

pub(crate) fn presentation(mode: &StaffMode) -> StaffAccess {
    match mode {
        StaffMode::SameAddress {} => StaffAccess::SameAddress,
        StaffMode::Hostname {
            catalog_host,
            staff_host,
        } => StaffAccess::Hostname {
            catalog_host: catalog_host.clone(),
            staff_host: staff_host.clone(),
        },
        StaffMode::SeparateListener { listen, allow_ips } => StaffAccess::SeparateListener {
            listen: *listen,
            allow_ips: allow_ips.clone(),
        },
    }
}

fn local_ips() -> Vec<IpAddr> {
    // Interface enumeration is optional evidence, not proof that other computers can connect.
    sysinfo::Networks::new_with_refreshed_list().map_or_else(
        |_| Vec::new(),
        |networks| {
            networks
                .values()
                .flat_map(sysinfo::NetworkData::ip_networks)
                .take(256)
                .map(|network| network.addr)
                .collect()
        },
    )
}

fn local(mut address: SocketAddr) -> SocketAddr {
    if address.ip().is_unspecified() {
        address.set_ip(if address.is_ipv4() {
            std::net::Ipv4Addr::LOCALHOST.into()
        } else {
            std::net::Ipv6Addr::LOCALHOST.into()
        });
    }
    address
}

fn url(address: SocketAddr, host: Option<&str>, path: &str) -> String {
    host.map_or_else(
        || format!("http://{}{path}", local(address)),
        |host| format!("http://{host}:{}{path}", address.port()),
    )
}

fn eligible(ip: IpAddr) -> bool {
    !ip.is_unspecified()
        && !ip.is_loopback()
        && !ip.is_multicast()
        && match ip {
            IpAddr::V4(ip) => !ip.is_link_local() && !ip.is_broadcast(),
            IpAddr::V6(ip) => !ip.is_unicast_link_local() && ip.to_ipv4_mapped().is_none(),
        }
}

fn lan(address: SocketAddr, host: Option<&str>, path: &str, ips: &[IpAddr]) -> Vec<String> {
    if address.ip().is_loopback() || host.is_some() {
        return Vec::new();
    }
    let mut ips = ips
        .iter()
        .copied()
        .filter(|ip| {
            eligible(*ip)
                && ip.is_ipv4() == address.is_ipv4()
                && (address.ip().is_unspecified() || address.ip() == *ip)
        })
        .collect::<Vec<_>>();
    ips.sort();
    ips.dedup();
    ips.into_iter()
        .take(8)
        .map(|ip| url(SocketAddr::new(ip, address.port()), None, path))
        .collect()
}

fn test_targets(
    catalog: Option<SocketAddr>,
    mode: Option<&StaffMode>,
) -> (
    Option<SocketAddr>,
    Option<SocketAddr>,
    Option<&str>,
    Option<&str>,
) {
    match mode {
        Some(StaffMode::SameAddress {}) => (catalog, catalog, None, None),
        Some(StaffMode::Hostname {
            catalog_host,
            staff_host,
        }) => (catalog, catalog, Some(catalog_host), Some(staff_host)),
        Some(StaffMode::SeparateListener { listen, .. }) => (catalog, Some(*listen), None, None),
        None => (None, None, None, None),
    }
}

fn build(catalog: Option<SocketAddr>, mode: Option<&StaffMode>, ips: &[IpAddr]) -> AddressBook {
    let (_, staff, catalog_host, staff_host) = test_targets(catalog, mode);
    AddressBook {
        observed_mode: mode.map(presentation),
        catalog: catalog.map(|a| url(a, catalog_host, "/")),
        staff: staff.map(|a| url(a, staff_host, "/equipe")),
        catalog_lan: catalog.map_or_else(Vec::new, |a| lan(a, catalog_host, "/", ips)),
        staff_lan: staff.map_or_else(Vec::new, |a| lan(a, staff_host, "/equipe", ips)),
        ..AddressBook::default()
    }
}

fn probe(
    address: Option<SocketAddr>,
    host: Option<&str>,
    path: &str,
    ips: &[IpAddr],
) -> CheckResult {
    let Some(address) = address.map(local) else {
        return CheckResult::Unknown;
    };
    if !address.ip().is_loopback() && !ips.contains(&address.ip()) {
        return CheckResult::Unknown;
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .proxy(None)
            .http_status_as_error(false)
            .max_redirects(0)
            .max_response_header_size(4096)
            .timeout_global(Some(Duration::from_millis(500)))
            .build(),
    );
    let mut request = agent.get(format!("http://{address}{path}"));
    if let Some(host) = host {
        request = request.header("Host", format!("{host}:{}", address.port()));
    }
    if request
        .call()
        .is_ok_and(|response| response.status() == 200)
    {
        CheckResult::Ok
    } else {
        CheckResult::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evidence_is_invalidated_by_binding_policy_or_identity_changes() {
        let root = tempfile::tempdir().unwrap();
        let mut status = crate::status::inspect(root.path());
        status.listen = Some("127.0.0.1:8080".parse().unwrap());
        status.staff = Some(StaffMode::SameAddress {});
        status.health = Some(crate::health::Report {
            schema: 1,
            instance: "0".repeat(32),
            pid: 1,
            version: "test".to_owned(),
            uptime_seconds: 0,
            database: crate::health::Check::Ok,
            indexer: crate::health::Check::Ok,
        });
        let mut context = Context::default();
        context.observe(&status, &ObservationRequest::Poll);
        context.book.catalog_test = CheckResult::Ok;
        context.book.tested_at = Some("2026-11-16T12:00:00Z".to_owned());
        assert_eq!(
            context
                .observe(&status, &ObservationRequest::Poll)
                .catalog_test,
            CheckResult::Ok
        );
        status.listen = Some("127.0.0.1:8081".parse().unwrap());
        assert!(
            context
                .observe(&status, &ObservationRequest::Poll)
                .tested_at
                .is_none()
        );
        context.book.catalog_test = CheckResult::Ok;
        status.staff = Some(StaffMode::SeparateListener {
            listen: "127.0.0.1:8082".parse().unwrap(),
            allow_ips: vec![],
        });
        assert_eq!(
            context
                .observe(&status, &ObservationRequest::Poll)
                .catalog_test,
            CheckResult::NotRun
        );
        context.book.catalog_test = CheckResult::Ok;
        status.health = None;
        let unavailable = context.observe(&status, &ObservationRequest::Poll);
        assert!(unavailable.catalog.is_none() && unavailable.staff.is_none());
        assert_eq!(unavailable.catalog_test, CheckResult::NotRun);
    }

    #[test]
    fn wildcard_and_specific_bindings_only_offer_eligible_local_interfaces() {
        let ips = [
            "127.0.0.1",
            "192.0.2.10",
            "192.0.2.11",
            "224.0.0.1",
            "169.254.1.1",
            "2001:db8::1",
        ]
        .map(|s| s.parse().unwrap());
        let book = build(
            Some("0.0.0.0:8080".parse().unwrap()),
            Some(&StaffMode::SameAddress {}),
            &ips,
        );
        assert_eq!(book.catalog.as_deref(), Some("http://127.0.0.1:8080/"));
        assert_eq!(
            book.catalog_lan,
            ["http://192.0.2.10:8080/", "http://192.0.2.11:8080/"]
        );
        assert!(
            build(
                Some("127.0.0.1:8080".parse().unwrap()),
                Some(&StaffMode::SameAddress {}),
                &ips
            )
            .catalog_lan
            .is_empty()
        );
        let specific = build(
            Some("192.0.2.11:8080".parse().unwrap()),
            Some(&StaffMode::SameAddress {}),
            &ips,
        );
        assert_eq!(specific.catalog_lan, ["http://192.0.2.11:8080/"]);
        let v6 = build(
            Some("[::]:8080".parse().unwrap()),
            Some(&StaffMode::SameAddress {}),
            &ips,
        );
        assert_eq!(v6.catalog.as_deref(), Some("http://[::1]:8080/"));
        assert_eq!(v6.catalog_lan, ["http://[2001:db8::1]:8080/"]);
    }

    #[test]
    fn hostname_candidates_preserve_host_routing_and_legacy_topology_stays_unknown() {
        let mode = StaffMode::Hostname {
            catalog_host: "catalog.example".to_owned(),
            staff_host: "staff.example".to_owned(),
        };
        let book = build(Some("127.0.0.1:8080".parse().unwrap()), Some(&mode), &[]);
        assert_eq!(
            book.catalog.as_deref(),
            Some("http://catalog.example:8080/")
        );
        assert_eq!(
            book.staff.as_deref(),
            Some("http://staff.example:8080/equipe")
        );
        assert!(book.catalog_lan.is_empty());
        assert!(
            build(Some("127.0.0.1:8080".parse().unwrap()), None, &[])
                .staff
                .is_none()
        );
    }
}
