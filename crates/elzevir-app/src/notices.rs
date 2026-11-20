// SPDX-License-Identifier: GPL-3.0-or-later
//! The queue of notices as the sender reads it (docs/specs/circulacao/avisos.md).

use elzevir_domain::notice::NoticeLevel;
use jiff::Zoned;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// `notices.id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoticeId(pub i64);

/// What became of a notice the sender tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeOutcome {
    /// The mail server took it.
    Sent,
    /// The mail server refused it for good.
    Failed,
}

impl NoticeOutcome {
    /// The status stored with the notice.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }
}

/// One Item a notice lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeItem {
    /// The title of the Item's Instance.
    pub title: String,
    /// The Item's barcode.
    pub barcode: String,
    /// When it was due, in an overdue notice.
    pub due: Option<Zoned>,
}

/// What a notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAbout {
    /// Items overdue, at this level.
    Overdue(NoticeLevel),
    /// A hold waiting to be picked up until this day.
    HoldWaiting(Date),
}

/// A notice waiting to go out by e-mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEmail {
    /// The notice.
    pub id: NoticeId,
    /// Where it goes.
    pub to: String,
    /// The patron's name, first name first.
    pub name: String,
    /// The branch it comes from.
    pub library: String,
    /// The branch's e-mail address, for replies.
    pub reply_to: Option<String>,
    /// What it is about.
    pub about: NoticeAbout,
    /// The Items, the first due first.
    pub items: Vec<NoticeItem>,
}

/// Reading the queue failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("notice queue unavailable: {0}")]
pub struct NoticeQueueError(pub String);

/// The queue of notices, for the sender.
pub trait NoticeQueue: Send + Sync {
    /// The oldest e-mail notices still pending, at most `limit`.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn pending_emails(&self, limit: usize) -> Result<Vec<PendingEmail>, NoticeQueueError>;
}
