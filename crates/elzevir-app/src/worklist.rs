// SPDX-License-Identifier: GPL-3.0-or-later
//! Work lists of the staff listing (docs/specs/catalogo/facetas.md, part c): views of
//! the families a person touched or a file brought today, read from the `audit_log`.

use std::num::NonZeroUsize;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::catalog::CatalogReadError;
use crate::listing::{CatalogListing, FamilyHead, FamilyRow, MAX_FROM, PAGE_SIZE, row};
use crate::staff::StaffId;

/// Most recent qualifying commands a list reads (facetas.md, items 15 and 16).
pub const LIST_COMMANDS: usize = 2_000;

/// Longest name of a saved list, in characters (docs/specs/catalogo/lote.md, item 17).
pub const MAX_LIST_NAME: usize = 100;

/// Most families a saved list holds (lote.md, item 17).
pub const MAX_LIST_FAMILIES: usize = 5_000;

/// A saved list of families (lote.md, part c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SavedListId(pub i64);

/// A work list, as the listing offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkList {
    /// The families the signed-in person touched.
    Recent,
    /// The families of the records a file brought today.
    ImportedToday,
    /// A list the team saved (lote.md, item 19).
    Saved(SavedListId),
}

impl WorkList {
    /// The lists every listing offers, in the order of the strip.
    pub const ALL: [Self; 2] = [Self::Recent, Self::ImportedToday];

    /// The stable name the URL carries: `s` and the number for a saved list.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Recent => "recent".to_owned(),
            Self::ImportedToday => "imported-today".to_owned(),
            Self::Saved(list) => format!("s{}", list.0),
        }
    }

    /// The list of a URL name; `None` for an unknown or malformed one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(digits) = name.strip_prefix('s')
            && !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return digits.parse().ok().map(|id| Self::Saved(SavedListId(id)));
        }
        Self::ALL.into_iter().find(|list| list.name() == name)
    }
}

/// A saved list as the strip names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedList {
    /// The list.
    pub id: SavedListId,
    /// Its name.
    pub name: String,
    /// Families it holds, gone ones included.
    pub families: usize,
}

/// A change of the saved lists (lote.md, item 21).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedListChange {
    /// A new list holding these families.
    Create {
        /// Its name.
        name: String,
        /// Its families, in the order they were selected.
        heads: Vec<FamilyHead>,
    },
    /// Families added to a list; those already there stay where they are.
    Add {
        /// The list.
        list: SavedListId,
        /// The families.
        heads: Vec<FamilyHead>,
    },
    /// Families taken out of a list.
    Remove {
        /// The list.
        list: SavedListId,
        /// The families.
        heads: Vec<FamilyHead>,
    },
    /// The list gone; its families stay in the catalog.
    Delete {
        /// The list.
        list: SavedListId,
    },
}

/// Why a change of the saved lists was refused (lote.md, item 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedListRefusal {
    /// The name is empty or too long.
    BadName,
    /// Another list has this name, case aside.
    NameTaken,
    /// The list would hold more than [`MAX_LIST_FAMILIES`].
    TooManyFamilies,
    /// No list has this id.
    Unknown,
}

impl SavedListRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::BadName => "saved-list-error-name",
            Self::NameTaken => "saved-list-error-taken",
            Self::TooManyFamilies => "saved-list-error-full",
            Self::Unknown => "saved-list-error-unknown",
        }
    }
}

/// `change` with its name trimmed, or why the name is refused.
///
/// # Errors
///
/// [`SavedListRefusal::BadName`] for an empty or too long name.
pub fn checked(change: SavedListChange) -> Result<SavedListChange, SavedListRefusal> {
    match change {
        SavedListChange::Create { name, heads } => {
            let name = name.trim();
            if name.is_empty() || name.chars().count() > MAX_LIST_NAME {
                return Err(SavedListRefusal::BadName);
            }
            Ok(SavedListChange::Create {
                name: name.to_owned(),
                heads,
            })
        }
        other => Ok(other),
    }
}

/// What a list asks the store, once the edge knows who asks and when today began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkListQuery {
    /// What this staff account's commands touched.
    TouchedBy(StaffId),
    /// What imports from a file brought since this instant.
    ImportedSince(Timestamp),
    /// What a saved list holds, the last added first.
    Saved(SavedListId),
}

/// Port: the family heads of a work list. The store implements it.
pub trait WorkListReader: Send + Sync {
    /// Up to `limit` heads of `query`, the most recently touched first, skipping
    /// `offset`; each family once, within the last [`LIST_COMMANDS`] qualifying commands.
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn heads(
        &self,
        query: WorkListQuery,
        offset: usize,
        limit: NonZeroUsize,
    ) -> Result<Vec<FamilyHead>, CatalogReadError>;

    /// Every saved list, in name order (lote.md, item 18).
    ///
    /// # Errors
    ///
    /// When storage fails.
    fn saved_lists(&self) -> Result<Vec<SavedList>, CatalogReadError>;
}

/// One page of a work list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPage {
    /// The rows, most recent first.
    pub rows: Vec<FamilyRow>,
    /// The offset the next page starts from, when there is one.
    pub next: Option<usize>,
}

/// The page of `query` starting at family `from` (cut to [`MAX_FROM`]). Families gone in
/// between are left out, so a page can be short.
///
/// # Errors
///
/// When the catalog fails.
pub fn page<R, L>(
    reader: &R,
    listing: &L,
    query: WorkListQuery,
    from: usize,
) -> Result<ListPage, CatalogReadError>
where
    R: WorkListReader + ?Sized,
    L: CatalogListing + ?Sized,
{
    let from = from.min(MAX_FROM);
    // One more than a page tells whether a next page exists without counting.
    let mut heads = reader.heads(query, from, PAGE_SIZE.saturating_add(1))?;
    let more = heads.len() > PAGE_SIZE.get();
    heads.truncate(PAGE_SIZE.get());
    let next = more.then(|| from.saturating_add(PAGE_SIZE.get()));
    let rows = listing
        .families(&heads)?
        .into_iter()
        .map(|family| row(family, &[]))
        .collect();
    Ok(ListPage { rows, next })
}

#[cfg(test)]
mod tests {
    use elzevir_domain::catalog::WorkId;

    use super::*;
    use crate::listing::{Family, FamilyCursor, ScanCode, Scanned};

    /// Works 1 to 65 were touched, the highest last; Work 7 is gone.
    struct Fake;

    impl WorkListReader for Fake {
        fn heads(
            &self,
            _: WorkListQuery,
            offset: usize,
            limit: NonZeroUsize,
        ) -> Result<Vec<FamilyHead>, CatalogReadError> {
            Ok((1..=65)
                .rev()
                .skip(offset)
                .take(limit.get())
                .map(|n| FamilyHead::Work(WorkId(n)))
                .collect())
        }

        fn saved_lists(&self) -> Result<Vec<SavedList>, CatalogReadError> {
            Ok(Vec::new())
        }
    }

    impl CatalogListing for Fake {
        fn families_after(
            &self,
            _: Option<&FamilyCursor>,
            _: NonZeroUsize,
        ) -> Result<Vec<Family>, CatalogReadError> {
            Ok(Vec::new())
        }

        fn families(&self, heads: &[FamilyHead]) -> Result<Vec<Family>, CatalogReadError> {
            Ok(heads
                .iter()
                .filter(|head| **head != FamilyHead::Work(WorkId(7)))
                .map(|head| Family {
                    cursor: FamilyCursor {
                        sort_key: String::new(),
                        head: *head,
                    },
                    hub_title: None,
                    works: Vec::new(),
                })
                .collect())
        }

        fn scanned(&self, _: ScanCode<'_>) -> Result<Option<Scanned>, CatalogReadError> {
            Ok(None)
        }
    }

    #[test]
    fn list_pages_follow_each_other_and_skip_gone_families() {
        let query = WorkListQuery::TouchedBy(StaffId(1));
        let first = page(&Fake, &Fake, query, 0).unwrap();
        assert_eq!(first.rows.len(), 30);
        assert_eq!(first.rows[0].cursor.head, FamilyHead::Work(WorkId(65)));
        assert_eq!(first.next, Some(30));
        let last = page(&Fake, &Fake, query, 60).unwrap();
        assert_eq!(last.rows.len(), 5);
        assert_eq!(last.next, None);
        let second = page(&Fake, &Fake, query, 30).unwrap();
        assert_eq!(second.rows.len(), 29, "Works 35 to 6, Work 7 gone");
        assert_eq!(second.next, Some(60));
        let third_from_end = page(&Fake, &Fake, query, 55).unwrap();
        assert_eq!(third_from_end.rows.len(), 9, "10 heads, Work 7 gone");
        assert_eq!(
            WorkList::from_name("imported-today"),
            Some(WorkList::ImportedToday)
        );
        assert_eq!(WorkList::from_name("quarantine"), None);
        assert_eq!(
            WorkList::from_name("s12"),
            Some(WorkList::Saved(SavedListId(12)))
        );
        assert_eq!(WorkList::Saved(SavedListId(12)).name(), "s12");
        for bad in ["s", "s-1", "s1x", "s+2"] {
            assert_eq!(WorkList::from_name(bad), None, "{bad}");
        }
        assert_eq!(
            checked(SavedListChange::Create {
                name: "  Doações  ".to_owned(),
                heads: Vec::new()
            }),
            Ok(SavedListChange::Create {
                name: "Doações".to_owned(),
                heads: Vec::new()
            })
        );
        assert_eq!(
            checked(SavedListChange::Create {
                name: " ".to_owned(),
                heads: Vec::new()
            }),
            Err(SavedListRefusal::BadName)
        );
    }
}
