// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure typed intentions for the compatible free-address service.
/// One separately confirmed address operation.
#[derive(Clone, PartialEq, Eq)]
pub enum AddressOperation {
    /// Begin or resume device authorization.
    Enroll {
        /// HTTPS broker origin.
        base: String,
        /// Institution name.
        institution: String,
        /// Contact email.
        email: String,
        /// Requested name.
        name: String,
        /// Local service name.
        service: String,
    },
    /// Refresh observed remote settings.
    Refresh,
    /// Choose public directory visibility.
    Listing {
        /// Desired visibility.
        public: bool,
    },
    /// Set edge credentials independently of the local staff account.
    Staff {
        /// Edge user.
        username: String,
        /// Private password, masked in forms.
        password: String,
        /// Comma-separated network ranges.
        cidrs: String,
    },
    /// Disable remote staff access.
    DisableStaff,
    /// Rotate the connector token.
    Renew {
        /// Local service name.
        service: String,
    },
    /// Request deletion and stop the local connector.
    Withdraw,
    /// Save the private address identity to a new file.
    Export {
        /// New private destination.
        output: String,
    },
    /// Import identity, validate it and rotate the token.
    Recover {
        /// Private recovery file.
        input: String,
        /// Local service name.
        service: String,
    },
}
impl std::fmt::Debug for AddressOperation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AddressOperation { .. }")
    }
}
impl AddressOperation {
    pub(crate) const fn key(&self) -> &'static str {
        match self {
            Self::Enroll { .. } => "broker-enroll",
            Self::Refresh => "broker-refresh",
            Self::Listing { public: true } => "broker-list",
            Self::Listing { public: false } => "broker-unlist",
            Self::Staff { .. } => "broker-staff",
            Self::DisableStaff => "broker-disable-staff",
            Self::Renew { .. } => "broker-renew",
            Self::Withdraw => "broker-withdraw",
            Self::Export { .. } => "broker-export",
            Self::Recover { .. } => "broker-recover",
        }
    }
    pub(crate) fn fields(&self) -> (Vec<&'static str>, Vec<String>) {
        match self {
            Self::Enroll {
                base,
                institution,
                email,
                name,
                service,
            } => (
                vec![
                    "broker-url",
                    "broker-institution",
                    "broker-email",
                    "broker-name",
                    "op-service",
                ],
                vec![
                    base.clone(),
                    institution.clone(),
                    email.clone(),
                    name.clone(),
                    service.clone(),
                ],
            ),
            Self::Staff {
                username,
                password,
                cidrs,
            } => (
                vec!["broker-username", "broker-password", "broker-cidrs"],
                vec![username.clone(), password.clone(), cidrs.clone()],
            ),
            Self::Renew { service } => (vec!["op-service"], vec![service.clone()]),
            Self::Export { output } => (vec!["op-output"], vec![output.clone()]),
            Self::Recover { input, service } => (
                vec!["op-input", "op-service"],
                vec![input.clone(), service.clone()],
            ),
            _ => (vec![], vec![]),
        }
    }
    pub(crate) fn finish(self, values: Vec<String>) -> Option<Self> {
        let mut values = values.into_iter();
        Some(match self {
            Self::Enroll { .. } => Self::Enroll {
                base: values.next()?,
                institution: values.next()?,
                email: values.next()?,
                name: values.next()?,
                service: values.next()?,
            },
            Self::Staff { .. } => Self::Staff {
                username: values.next()?,
                password: values.next()?,
                cidrs: values.next()?,
            },
            Self::Renew { .. } => Self::Renew {
                service: values.next()?,
            },
            Self::Export { .. } => Self::Export {
                output: values.next()?,
            },
            Self::Recover { .. } => Self::Recover {
                input: values.next()?,
                service: values.next()?,
            },
            other => other,
        })
    }
}
pub(crate) fn shortcut(key: char) -> Option<AddressOperation> {
    Some(match key {
        'g' => AddressOperation::Enroll {
            base: String::new(),
            institution: String::new(),
            email: String::new(),
            name: String::new(),
            service: "elzevir".to_owned(),
        },
        'c' => AddressOperation::Refresh,
        'j' => AddressOperation::Listing { public: true },
        'J' => AddressOperation::Listing { public: false },
        'w' => AddressOperation::Staff {
            username: String::new(),
            password: String::new(),
            cidrs: String::new(),
        },
        'W' => AddressOperation::DisableStaff,
        'n' => AddressOperation::Renew {
            service: "elzevir".to_owned(),
        },
        'z' => AddressOperation::Withdraw,
        'k' => AddressOperation::Export {
            output: String::new(),
        },
        'o' => AddressOperation::Recover {
            input: String::new(),
            service: "elzevir".to_owned(),
        },
        _ => return None,
    })
}
