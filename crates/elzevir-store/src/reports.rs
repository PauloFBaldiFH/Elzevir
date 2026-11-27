// SPDX-License-Identifier: GPL-3.0-or-later
//! The fixed reports, read from the pool (docs/specs/operacao/relatorios.md).

use std::collections::{BTreeMap, HashMap};

use elzevir_app::material::ContentKind;
use elzevir_app::reports::{
    CirculationLine, CollectionLine, PatronsLine, Period, ReportError, ReportReader,
};
use elzevir_domain::labels::dewey_main_class;
use jiff::civil::Date;
use rusqlite::{Connection, params};

use crate::{ElzevirStore, StoreError};

/// Items 2 to 4: every loan issued or returned in the period, by branch and by the category
/// the loan was made under (the patron's for an open loan, the one kept for an ended one).
/// A moment is stored with the library's offset, so its first ten characters are its day.
const CIRCULATION: &str = "WITH counted (branch, category, issued, returned) AS (
         SELECT l.branch_id, p.category_id, 1, 0
         FROM loans l JOIN patrons p ON p.id = l.patron_id
         WHERE substr(l.issued_at, 1, 10) BETWEEN ?1 AND ?2
         UNION ALL
         SELECT branch_id, category_id, 1, 0 FROM returned_loans
         WHERE substr(issued_at, 1, 10) BETWEEN ?1 AND ?2
         UNION ALL
         SELECT return_branch_id, category_id, 0, 1 FROM returned_loans
         WHERE substr(returned_at, 1, 10) BETWEEN ?1 AND ?2)
     SELECT b.name, c.name, sum(issued), sum(returned)
     FROM counted
     JOIN branches b ON b.id = counted.branch
     LEFT JOIN patron_categories c ON c.id = counted.category
     GROUP BY counted.branch, counted.category
     ORDER BY b.name, b.id, c.name IS NULL, c.name, c.id";

/// Items 8 and 9: the patrons, and the ones whose latest checkout is on the day or after.
const PATRONS: &str =
    "SELECT b.name, c.name, count(*), count(*) FILTER (WHERE p.last_issued_on >= ?1)
     FROM patrons p
     JOIN branches b ON b.id = p.branch_id
     JOIN patron_categories c ON c.id = p.category_id
     WHERE p.anonymized_at IS NULL
     GROUP BY p.branch_id, p.category_id
     ORDER BY b.name, b.id, c.name, c.id";

/// Items 5 to 7: every Item not withdrawn, with its Instance and Work.
const ITEMS: &str = "SELECT i.instance_id, n.work_id, i.call_number
     FROM items i JOIN instances n ON n.id = i.instance_id
     WHERE i.withdrawn = 0";

fn failed(error: impl std::fmt::Display) -> ReportError {
    ReportError(error.to_string())
}

fn count(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

impl ReportReader for ElzevirStore {
    fn circulation(&self, period: Period) -> Result<Vec<CirculationLine>, ReportError> {
        let (from, to) = (period.from.to_string(), period.to.to_string());
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(CIRCULATION)?;
                let rows = statement.query_map(params![from, to], |row| {
                    Ok(CirculationLine {
                        branch: row.get(0)?,
                        category: row.get(1)?,
                        checkouts: count(row.get(2)?),
                        returns: count(row.get(3)?),
                    })
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(failed)
    }

    fn collection(&self) -> Result<Vec<CollectionLine>, ReportError> {
        self.readers().read(collection).map_err(failed)
    }

    fn patrons(&self, since: Date) -> Result<Vec<PatronsLine>, ReportError> {
        let since = since.to_string();
        self.readers()
            .read(|c| {
                let mut statement = c.prepare_cached(PATRONS)?;
                let rows = statement.query_map([since], |row| {
                    Ok(PatronsLine {
                        branch: row.get(0)?,
                        category: row.get(1)?,
                        patrons: count(row.get(2)?),
                        active: count(row.get(3)?),
                    })
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(failed)
    }
}

/// Items 5 to 7: the Items by main class and content; the content is read once per
/// Instance.
fn collection(c: &Connection) -> Result<Vec<CollectionLine>, StoreError> {
    let mut statement = c.prepare_cached(ITEMS)?;
    let items = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut contents: HashMap<i64, Option<ContentKind>> = HashMap::new();
    // Classes in order, none last; contents in the order of their list, none last.
    let rank = |content: Option<ContentKind>| {
        content.map_or(usize::MAX, |kind| {
            ContentKind::all()
                .iter()
                .position(|known| *known == kind)
                .unwrap_or(usize::MAX)
        })
    };
    let mut counts: BTreeMap<(u16, usize), (Option<ContentKind>, u64)> = BTreeMap::new();
    for (instance, work, call_number) in items {
        let content = if let Some(content) = contents.get(&instance) {
            *content
        } else {
            let content = crate::catalog::material(c, work, instance)?.content;
            contents.insert(instance, content);
            content
        };
        let class = call_number.as_deref().and_then(dewey_main_class);
        let key = (class.unwrap_or(u16::MAX), rank(content));
        counts.entry(key).or_insert((content, 0)).1 += 1;
    }
    Ok(counts
        .into_iter()
        .map(|((class, _), (content, items))| CollectionLine {
            class: (class != u16::MAX).then_some(class),
            content,
            items,
        })
        .collect())
}
