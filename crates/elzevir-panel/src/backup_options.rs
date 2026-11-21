// SPDX-License-Identifier: GPL-3.0-or-later
use serde::{Deserialize, Serialize};

/// Contents of an authenticated local backup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupScope {
    /// SQLite, institution settings and eligible secrets.
    #[default]
    Full,
    /// Only the standalone SQLite image and its manifest.
    Database,
}

/// Bounded single-threaded Zstandard compression profiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupProfile {
    /// Prefer lower CPU cost.
    Fast,
    /// Standard compression.
    #[default]
    Balanced,
    /// Spend more CPU to reduce storage.
    Compact,
}

impl BackupProfile {
    /// Zstandard compression level.
    #[must_use]
    pub const fn level(self) -> i32 {
        match self {
            Self::Fast => 1,
            Self::Balanced => 3,
            Self::Compact => 7,
        }
    }
}

/// Options submitted to the institution worker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupOptions {
    /// Included data.
    pub scope: BackupScope,
    /// Compression profile.
    pub profile: BackupProfile,
}
