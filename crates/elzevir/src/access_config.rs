// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed staff topology shared by configuration and local discovery metadata.

use elzevir_web::{AccessError, AccessPolicy};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StaffMode {
    SameAddress {},
    Hostname {
        catalog_host: String,
        staff_host: String,
    },
    SeparateListener {
        listen: SocketAddr,
        #[serde(default)]
        allow_ips: Vec<String>,
    },
}

impl Default for StaffMode {
    fn default() -> Self {
        Self::SameAddress {}
    }
}

impl StaffMode {
    pub(crate) fn validate(&mut self, catalog: Option<SocketAddr>) -> Result<(), AccessError> {
        match self {
            Self::SameAddress {} => Ok(()),
            Self::Hostname {
                catalog_host,
                staff_host,
            } => {
                *catalog_host = elzevir_web::normalize_hostname(catalog_host)?;
                *staff_host = elzevir_web::normalize_hostname(staff_host)?;
                AccessPolicy::hostnames(catalog_host, staff_host).map(drop)
            }
            Self::SeparateListener { listen, allow_ips } => {
                if listen.port() != 0 && catalog == Some(*listen) {
                    return Err(AccessError);
                }
                AccessPolicy::staff_only(allow_ips).map(drop)
            }
        }
    }

    pub(crate) fn catalog_policy(&self) -> Result<AccessPolicy, AccessError> {
        match self {
            Self::SameAddress {} => Ok(AccessPolicy::default()),
            Self::Hostname {
                catalog_host,
                staff_host,
            } => AccessPolicy::hostnames(catalog_host, staff_host),
            Self::SeparateListener { .. } => Ok(AccessPolicy::catalog_only()),
        }
    }

    pub(crate) fn staff_policy(&self) -> Result<Option<(SocketAddr, AccessPolicy)>, AccessError> {
        match self {
            Self::SeparateListener { listen, allow_ips } => {
                Ok(Some((*listen, AccessPolicy::staff_only(allow_ips)?)))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn observed(&self, address: Option<SocketAddr>) -> Self {
        match (self, address) {
            (Self::SeparateListener { allow_ips, .. }, Some(listen)) => Self::SeparateListener {
                listen,
                allow_ips: allow_ips.clone(),
            },
            _ => self.clone(),
        }
    }
}
