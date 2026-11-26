// SPDX-License-Identifier: GPL-3.0-or-later
//! The records of the OAI-PMH repository (docs/specs/opac/oai-pmh.md, items 7, 8, 13
//! and 15).

use elzevir_app::oai::{Datestamp, OaiCatalog, OaiHeader, OaiUnavailable};
use elzevir_domain::catalog::InstanceId;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::ElzevirStore;
use crate::catalog::ReadFailure;

const HEADER: &str = "SELECT i.id, d.changed_at, i.opac_hidden
     FROM instances AS i JOIN instance_datestamps AS d ON d.instance_id = i.id";

impl OaiCatalog for ElzevirStore {
    fn earliest(&self) -> Result<Option<Datestamp>, OaiUnavailable> {
        self.readers()
            .read(|c| {
                let earliest: Option<String> = c.query_row(
                    "SELECT min(changed_at) FROM instance_datestamps",
                    [],
                    |row| row.get(0),
                )?;
                Ok::<_, ReadFailure>(earliest.and_then(|text| Datestamp::parse(&text)))
            })
            .map_err(unavailable)
    }

    fn header(&self, instance: InstanceId) -> Result<Option<OaiHeader>, OaiUnavailable> {
        self.readers()
            .read(|c| {
                Ok::<_, ReadFailure>(
                    c.prepare_cached(&format!("{HEADER} WHERE i.id = ?1"))?
                        .query_row(params![instance.0], header)
                        .optional()?
                        .flatten(),
                )
            })
            .map_err(unavailable)
    }

    fn headers(
        &self,
        from_id: i64,
        from: Option<&Datestamp>,
        until: Option<&Datestamp>,
        limit: usize,
    ) -> Result<Vec<OaiHeader>, OaiUnavailable> {
        let from = from.map(Datestamp::as_str);
        let until = until.map(Datestamp::as_str);
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.readers()
            .read(|c| headers(c, from_id, from, until, limit))
            .map_err(unavailable)
    }
}

fn headers(
    c: &Connection,
    from_id: i64,
    from: Option<&str>,
    until: Option<&str>,
    limit: i64,
) -> Result<Vec<OaiHeader>, ReadFailure> {
    let mut statement = c.prepare_cached(&format!(
        "{HEADER} WHERE i.id >= ?1
             AND (?2 IS NULL OR d.changed_at >= ?2)
             AND (?3 IS NULL OR d.changed_at <= ?3)
         ORDER BY i.id LIMIT ?4"
    ))?;
    let rows = statement.query_map(params![from_id, from, until, limit], header)?;
    let mut found = Vec::new();
    for row in rows {
        found.extend(row?);
    }
    Ok(found)
}

/// A header; `None` when the stored time is not one (never written by the triggers).
fn header(row: &Row<'_>) -> rusqlite::Result<Option<OaiHeader>> {
    let instance = InstanceId(row.get(0)?);
    let changed_at: String = row.get(1)?;
    let hidden: bool = row.get(2)?;
    Ok(Datestamp::parse(&changed_at).map(|datestamp| OaiHeader {
        instance,
        datestamp,
        hidden,
    }))
}

fn unavailable(error: impl std::fmt::Display) -> OaiUnavailable {
    OaiUnavailable(error.to_string())
}
