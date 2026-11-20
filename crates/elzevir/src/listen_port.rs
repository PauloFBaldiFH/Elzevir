// SPDX-License-Identifier: GPL-3.0-or-later
//! Busy listen ports are resolved in the panel instead of aborting the start.

use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::path::Path;

/// How many ports after the busy one are probed for a suggestion.
const SUGGESTION_RANGE: u16 = 100;

fn free(address: SocketAddr) -> bool {
    TcpListener::bind(address).is_ok()
}

/// The first free port after `address`'s, so Enter in the dialog just works.
fn suggest(address: SocketAddr, is_free: impl Fn(SocketAddr) -> bool) -> Option<u16> {
    let start = address.port().max(elzevir_panel::MIN_PORT - 1);
    (1..=SUGGESTION_RANGE)
        .filter_map(|step| start.checked_add(step))
        .find(|port| is_free(SocketAddr::new(address.ip(), *port)))
}

/// Returns a free listen address, asking the operator for another port when
/// `address` is taken. A port chosen in the dialog is saved in `config`.
pub(crate) fn ensure(
    address: SocketAddr,
    config: Option<&Path>,
    language: &str,
) -> Result<SocketAddr, String> {
    if free(address) {
        return Ok(address);
    }
    if !crate::first_use::interactive() {
        return Err(format!(
            "listen address {address} is in use; choose another with --listen"
        ));
    }
    let chosen =
        elzevir_panel::choose_port(language, address.port(), suggest(address, free), |port| {
            free(SocketAddr::new(address.ip(), port))
        })
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("listen address {address} is in use; start cancelled"))?;
    let chosen = SocketAddr::new(address.ip(), chosen);
    if let Some(config) = config {
        save(config, chosen.port())?;
    }
    Ok(chosen)
}

/// Replaces the configured port atomically, keeping every other setting.
fn save(config: &Path, port: u16) -> Result<(), String> {
    let original =
        std::fs::read_to_string(config).map_err(|_| "cannot read server configuration")?;
    let text = crate::server_config::with_port(&original, port)?;
    let mut file = tempfile::NamedTempFile::new_in(
        config
            .parent()
            .ok_or("server configuration parent required")?,
    )
    .map_err(|_| "cannot stage server configuration")?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "cannot save server configuration")?;
    crate::file_owner::configuration(file.path(), config)?;
    file.persist(config)
        .map_err(|_| "cannot publish server configuration")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suggestion_skips_ports_that_are_taken() {
        let address: SocketAddr = "127.0.0.1:7390".parse().unwrap();
        let taken = |candidate: SocketAddr| candidate.port() > 7392;
        assert_eq!(suggest(address, taken), Some(7393));
        assert_eq!(suggest(address, |_| false), None);
        let top: SocketAddr = "127.0.0.1:65535".parse().unwrap();
        assert_eq!(suggest(top, |_| true), None);
        let privileged: SocketAddr = "127.0.0.1:80".parse().unwrap();
        assert_eq!(suggest(privileged, |_| true), Some(1024));
    }

    #[test]
    fn a_busy_port_is_detected_and_a_free_one_is_kept() {
        let holder = TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = holder.local_addr().unwrap();
        assert!(!free(busy));
        drop(holder);
        let open = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = open.local_addr().unwrap();
        drop(open);
        assert_eq!(ensure(address, None, "en").unwrap(), address);
    }

    #[test]
    fn saving_a_port_keeps_the_rest_of_the_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("server.toml");
        std::fs::write(
            &config,
            "[institution]\nname = \"Fictional library\"\n\n[server]\nlisten = \"127.0.0.1:7390\"\n",
        )
        .unwrap();
        save(&config, 7391).unwrap();
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("127.0.0.1:7391"));
        assert!(text.contains("Fictional library"));
    }
}
