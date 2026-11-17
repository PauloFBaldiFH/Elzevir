// SPDX-License-Identifier: GPL-3.0-or-later
//! Operational HTTPS trusts the host certificate store and never disables validation.
use std::{sync::Arc, time::Duration};
pub(crate) fn secure_agent(redirects: u32, timeout: Duration) -> Result<ureq::Agent, String> {
    use ureq::tls::{Certificate, RootCerts, TlsConfig};
    let found = rustls_native_certs::load_native_certs();
    let roots: Vec<Certificate<'static>> = found
        .certs
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()).to_owned())
        .collect();
    if roots.is_empty() {
        return Err("no system TLS certificates available".to_owned());
    }
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::new_with_certs(&roots))
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    Ok(ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .http_status_as_error(false)
        .max_redirects(redirects)
        .timeout_global(Some(timeout))
        .build()
        .into())
}
