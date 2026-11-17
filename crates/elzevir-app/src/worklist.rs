// SPDX-License-Identifier: GPL-3.0-or-later
//! Work lists of the staff listing (docs/specs/catalogo/facetas.md, part c): views of
//! the families a person touched or a file brought today, read from the `audit_log`.

use std::num::NonZeroUsize;

use jiff::Timestamp;

use crate::catalog::CatalogReadError;
use crate::listing::{CatalogListing, FamilyHead, FamilyRow, MAX_FROM, PAGE_SIZE, row};
use crate::staff::StaffId;

/// Most recent qualifying commands a list reads (facetas.md, items 15 and 16).
pub const LIST_COMMANDS: usize = 2_000;

/// A work list, as the listing offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkList {
    /// The families the signed-in person touched.
    Recent,
    /// The families of the records a file brought today.
    ImportedToday,
}

impl WorkList {
    /// Every list, in the order of the strip.
    pub const ALL: [Self; 2] = [Self::Recent, Self::ImportedToday];

    /// The stable name the URL carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::ImportedToday => "imported-today",
        }
    }

    /// The list of a URL name; `None` for an unknown one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|list| list.name() == name)
    }
}

/// What a list asks the store, once the edge knows who asks and when today began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkListQuery {
    /// What this staff account's commands touched.
    TouchedBy(StaffId),
    /// What imports from a file brought since this instant.
    ImportedSince(Timestamp),
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
    }
}
