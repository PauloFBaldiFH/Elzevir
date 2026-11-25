// SPDX-License-Identifier: GPL-3.0-or-later
use crate::I18n;

/// A cataloguer's repository workspace.
#[derive(askama::Template)]
#[template(path = "staff_repository.html")]
pub struct StaffDeposit<'a> {
    /// Localized messages.
    pub t: &'a I18n,
    /// Token for this authenticated session.
    pub csrf: String,
    /// Instance selected for lookup or editing.
    pub instance: Option<i64>,
    /// Selected catalogue title.
    pub title: String,
    /// Whether the linked instance is concealed in the OPAC.
    pub opac_hidden: bool,
    /// Canonical identifier of the open deposit.
    pub id: Option<String>,
    /// Localized state label.
    pub state_key: &'static str,
    /// Logical files with their version controls.
    pub files: Vec<StaffDepositFile>,
    /// Collection identifier, name and membership.
    pub collections: Vec<(String, String, bool)>,
    /// Current collection name filter.
    pub filter: String,
}

/// One logical file shown to the staff member.
pub struct StaffDepositFile {
    /// Canonical file identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Localization key for its role.
    pub role: &'static str,
    /// Staff-only access.
    pub restricted: bool,
    /// Current version or zero when there is none.
    pub current: u64,
    /// Last sequence issued, including retired versions.
    pub last: u64,
    /// Retry key for the upload form.
    pub operation: String,
}
