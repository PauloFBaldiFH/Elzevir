// SPDX-License-Identifier: GPL-3.0-or-later
//! Listener/Host boundaries and source-IP restrictions, independent of authentication.

use axum::{
    Router,
    extract::{ConnectInfo, Request, State},
    http::{StatusCode, header, uri::Authority},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Redirect, Response},
};
use std::net::{IpAddr, SocketAddr};

/// Validated request policy for one HTTP listener.
#[derive(Clone, Default)]
pub struct AccessPolicy(Policy, Option<(SocketAddr, SocketAddr)>);

#[derive(Clone, Default)]
enum Policy {
    #[default]
    Shared,
    Hosts {
        catalog: String,
        staff: String,
    },
    Catalog,
    Staff(Vec<Network>),
}

/// Invalid access configuration, without echoing values.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid staff access configuration")]
pub struct AccessError;

impl AccessPolicy {
    /// Restricts catalog and staff routes to distinct DNS hostnames.
    ///
    /// # Errors
    /// Refuses invalid, duplicate or non-ASCII DNS names.
    pub fn hostnames(catalog: &str, staff: &str) -> Result<Self, AccessError> {
        let catalog = normalize_hostname(catalog)?;
        let staff = normalize_hostname(staff)?;
        if catalog == staff {
            return Err(AccessError);
        }
        Ok(Self(Policy::Hosts { catalog, staff }, None))
    }

    /// Catalog listener with no staff handlers reachable.
    #[must_use]
    pub const fn catalog_only() -> Self {
        Self(Policy::Catalog, None)
    }

    /// Staff listener, optionally restricted to TCP peer IPs/CIDRs.
    ///
    /// # Errors
    /// Refuses invalid networks or more than thirty-two entries.
    pub fn staff_only(networks: &[String]) -> Result<Self, AccessError> {
        if networks.len() > 32 {
            return Err(AccessError);
        }
        let networks = networks
            .iter()
            .map(|s| Network::parse(s))
            .collect::<Result<_, _>>()?;
        Ok(Self(Policy::Staff(networks), None))
    }

    /// Enables navigation redirects after both actual listener addresses are known.
    #[must_use]
    pub fn with_listener_addresses(mut self, catalog: SocketAddr, staff: SocketAddr) -> Self {
        self.1 = if catalog.port() != 0 && staff.port() != 0 {
            Some((catalog, staff))
        } else {
            None
        };
        self
    }
}

/// Normalizes a DNS name without accepting URL components or wildcards.
///
/// # Errors
/// Refuses empty, oversized or invalid ASCII DNS labels.
pub fn normalize_hostname(value: &str) -> Result<String, AccessError> {
    let name = value.strip_suffix('.').unwrap_or(value);
    if name.is_empty() || name.len() > 253 || !name.is_ascii() || name.parse::<IpAddr>().is_ok() {
        return Err(AccessError);
    }
    for label in name.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(AccessError);
        }
    }
    Ok(name.to_ascii_lowercase())
}

#[derive(Clone)]
struct Network {
    address: IpAddr,
    prefix: u8,
}

impl Network {
    fn parse(value: &str) -> Result<Self, AccessError> {
        let (address, prefix) = value
            .split_once('/')
            .map_or((value, None), |(a, p)| (a, Some(p)));
        let address: IpAddr = address.parse().map_err(|_| AccessError)?;
        let bits = if address.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.map_or(Ok(bits), |p| p.parse::<u8>().map_err(|_| AccessError))?;
        if prefix > bits {
            return Err(AccessError);
        }
        Ok(Self { address, prefix })
    }

    fn contains(&self, address: IpAddr) -> bool {
        match (self.address, address) {
            (IpAddr::V4(network), address) => {
                let address = match address {
                    IpAddr::V4(a) => Some(a),
                    IpAddr::V6(a) => a.to_ipv4_mapped(),
                };
                address.is_some_and(|a| {
                    self.prefix == 0
                        || (u32::from(network) ^ u32::from(a)) >> (32 - self.prefix) == 0
                })
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                self.prefix == 0
                    || (u128::from(network) ^ u128::from(address)) >> (128 - self.prefix) == 0
            }
            _ => false,
        }
    }
}

fn authority(value: &str) -> Result<(String, Option<u16>), AccessError> {
    if value.len() > 260 || value.contains('@') || value.ends_with(':') {
        return Err(AccessError);
    }
    let authority: Authority = value.parse().map_err(|_| AccessError)?;
    if value.contains(':') && authority.port_u16().is_none_or(|port| port == 0) {
        return Err(AccessError);
    }
    Ok((normalize_hostname(authority.host())?, authority.port_u16()))
}

fn request_host(request: &Request) -> Result<String, AccessError> {
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let host = hosts
        .next()
        .and_then(|h| h.to_str().ok())
        .ok_or(AccessError)?;
    if hosts.next().is_some() {
        return Err(AccessError);
    }
    let (host, port) = authority(host)?;
    if let Some(uri_authority) = request.uri().authority() {
        let (uri_host, uri_port) = authority(uri_authority.as_str())?;
        if request.uri().scheme_str() != Some("http")
            || uri_host != host
            || uri_port.unwrap_or(80) != port.unwrap_or(80)
        {
            return Err(AccessError);
        }
    }
    Ok(host)
}

async fn enforce(State(policy): State<AccessPolicy>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let staff = path == "/equipe" || path.starts_with("/equipe/");
    let asset = path.starts_with("/static/");
    match &policy.0 {
        Policy::Hosts {
            catalog,
            staff: staff_host,
        } => {
            let Ok(host) = request_host(&request) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            let allowed = if host == *catalog {
                !staff
            } else if host == *staff_host {
                staff || asset
            } else {
                false
            };
            if !allowed {
                if let Some((catalog_address, staff_address)) = policy.1
                    && request.method() == axum::http::Method::GET
                {
                    if host == *catalog && path == "/equipe" {
                        return redirect(&request, staff_host, staff_address.port());
                    }
                    if host == *staff_host && !staff && !asset {
                        return redirect(&request, catalog, catalog_address.port());
                    }
                }
                return StatusCode::NOT_FOUND.into_response();
            }
        }
        Policy::Catalog if staff => {
            if path == "/equipe"
                && request.method() == axum::http::Method::GET
                && let Some((_, address)) = policy.1
            {
                return same_host_redirect(&request, address);
            }
            return StatusCode::NOT_FOUND.into_response();
        }
        Policy::Staff(networks) => {
            if !staff && !asset {
                if request.method() == axum::http::Method::GET
                    && let Some((address, _)) = policy.1
                {
                    return same_host_redirect(&request, address);
                }
                return StatusCode::NOT_FOUND.into_response();
            }
            if !networks.is_empty() {
                let peer = request.extensions().get::<ConnectInfo<SocketAddr>>();
                if !peer.is_some_and(|peer| {
                    networks.iter().any(|network| network.contains(peer.0.ip()))
                }) {
                    return StatusCode::FORBIDDEN.into_response();
                }
            }
        }
        Policy::Shared | Policy::Catalog => {}
    }
    next.run(request).await
}

fn redirect(request: &Request, host: &str, port: u16) -> Response {
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", axum::http::uri::PathAndQuery::as_str);
    Redirect::temporary(&format!("//{host}:{port}{path}")).into_response()
}

fn same_host_redirect(request: &Request, address: SocketAddr) -> Response {
    if !address.ip().is_unspecified() {
        let host = match address.ip() {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        return redirect(request, &host, address.port());
    }
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let authority = hosts
        .next()
        .and_then(|h| h.to_str().ok())
        .filter(|s| s.len() <= 260 && !s.contains('@'))
        .and_then(|s| s.parse::<Authority>().ok());
    let Some(authority) = authority.filter(|_| hosts.next().is_none()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let host = authority.host();
    if authority.as_str().strip_prefix(host).is_none_or(|suffix| {
        !suffix.is_empty() && authority.port_u16().is_none_or(|port| port == 0)
    }) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let ip = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host)
        .parse::<IpAddr>();
    if ip.is_err() && normalize_hostname(host).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    redirect(request, host, address.port())
}

/// Applies listener boundaries and security headers before route handlers.
pub fn restrict_access(router: Router, policy: AccessPolicy) -> Router {
    router
        .layer(from_fn_with_state(policy, enforce))
        .layer(axum::middleware::map_response(crate::security::headers))
}
