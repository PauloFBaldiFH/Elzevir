// SPDX-License-Identifier: GPL-3.0-or-later
//! The stored records as the export reads them (docs/specs/catalogo/linha-de-comando.md).

use std::num::NonZeroUsize;

use elzevir_app::catalog::{
    AuthorityLink, CatalogReadError, ItemData, NewContribution, NewIdentifier, NewInstance,
    NewNote, NewRecord, NewSubject, NewWorkRelation,
};
use elzevir_app::export::{RecordSource, StoredRecord};
use elzevir_domain::catalog::{
    AgentKind, AuthorityLinkKind, HubId, IdentifierKind, InstanceId, Money, NoteKind, SubjectKind,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::ElzevirStore;
use crate::catalog::{
    ReadFailure, name_parts_of_column, status_at, subdivisions_of_column, work_relations,
};
use crate::statements::read_description;
use elzevir_domain::statement::{EntityKind, EntityRef};

impl RecordSource for ElzevirStore {
    fn records_after(
        &self,
        after: Option<InstanceId>,
        limit: NonZeroUsize,
    ) -> Result<Vec<StoredRecord>, CatalogReadError> {
        self.readers()
            .read(|c| records_after(c, after, limit))
            .map_err(|e| CatalogReadError(e.to_string()))
    }
}

/// The columns of one Instance and its Work.
struct Head {
    instance: i64,
    work: i64,
    title: String,
    edition: Option<String>,
    place: Option<String>,
    publisher: Option<String>,
    date: Option<String>,
    extent: Option<String>,
    series: Option<String>,
    work_title: String,
    hub: Option<i64>,
    classification: Option<String>,
    classification_scheme: Option<String>,
    summary: Option<String>,
    audience: Option<String>,
}

impl Head {
    fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            instance: row.get(0)?,
            work: row.get(1)?,
            title: row.get(2)?,
            edition: row.get(3)?,
            place: row.get(4)?,
            publisher: row.get(5)?,
            date: row.get(6)?,
            extent: row.get(7)?,
            series: row.get(8)?,
            work_title: row.get(9)?,
            hub: row.get(10)?,
            classification: row.get(11)?,
            classification_scheme: row.get(12)?,
            summary: row.get(13)?,
            audience: row.get(14)?,
        })
    }
}

fn records_after(
    c: &Connection,
    after: Option<InstanceId>,
    limit: NonZeroUsize,
) -> Result<Vec<StoredRecord>, ReadFailure> {
    let mut statement = c.prepare_cached(
        "SELECT instances.id, works.id, instances.title, edition, place, publisher, date,
                extent, series, works.title, works.hub_id, works.classification, terms.code, works.summary, works.audience
         FROM instances
         JOIN works ON works.id = instances.work_id
         LEFT JOIN terms ON terms.id = works.classification_scheme
         WHERE instances.id > ?1
         ORDER BY instances.id
         LIMIT ?2",
    )?;
    let limit = i64::try_from(limit.get()).unwrap_or(i64::MAX);
    let heads = statement
        .query_map(params![after.map_or(0, |id| id.0), limit], Head::from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    heads.into_iter().map(|head| stored(c, head)).collect()
}

const fn subject(entity: EntityKind, id: i64) -> EntityRef {
    EntityRef { entity, id }
}

fn stored(c: &Connection, head: Head) -> Result<StoredRecord, ReadFailure> {
    let relations = work_relations(c, head.work)?
        .into_iter()
        .map(|(kind, work)| NewWorkRelation { kind, work })
        .collect();
    let record = NewRecord {
        work_title: head.work_title,
        work_description: read_description(c, subject(EntityKind::Work, head.work))?,
        hub: head.hub.map(HubId),
        relations,
        classification: head.classification,
        classification_scheme: head.classification_scheme,
        summary: head.summary,
        audience: head.audience,
        languages: languages(c, head.work)?,
        contributions: contributions(c, head.work)?,
        subjects: subjects(c, head.work)?,
        instance: NewInstance {
            title: head.title,
            description: read_description(c, subject(EntityKind::Instance, head.instance))?,
            edition: head.edition,
            place: head.place,
            publisher: head.publisher,
            date: head.date,
            extent: head.extent,
            series: head.series,
            identifiers: identifiers(c, head.instance)?,
            notes: notes(c, head.instance)?,
        },
        items: items(c, head.instance)?,
        source: c
            .query_row(
                "SELECT record FROM source_records WHERE instance_id = ?1",
                params![head.instance],
                |row| row.get(0),
            )
            .optional()?,
    };
    Ok(StoredRecord {
        instance: InstanceId(head.instance),
        record,
    })
}

fn required<T>(
    code: u8,
    what: &'static str,
    from_code: fn(u8) -> Option<T>,
) -> Result<T, ReadFailure> {
    from_code(code).ok_or(ReadFailure::UnknownCode { what, code })
}

fn languages(c: &Connection, work: i64) -> Result<Vec<String>, ReadFailure> {
    let mut statement = c.prepare_cached(
        "SELECT terms.code FROM work_languages JOIN terms ON terms.id = work_languages.language
         WHERE work_id = ?1 ORDER BY position",
    )?;
    Ok(statement
        .query_map(params![work], |row| row.get(0))?
        .collect::<Result<_, _>>()?)
}

pub(crate) fn contributions(
    c: &Connection,
    work: i64,
) -> Result<Vec<NewContribution>, ReadFailure> {
    let mut statement = c.prepare_cached(
        "SELECT agents.kind, agents.heading, terms.code, is_primary, agents.name_parts,
                role_term, position
         FROM contributions
         JOIN agents ON agents.id = contributions.agent_id
         LEFT JOIN terms ON terms.id = contributions.role
         WHERE work_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map(params![work], |row| {
            Ok((
                (row.get::<_, u8>(0)?, row.get(1)?, row.get(2)?, row.get(3)?),
                (row.get::<_, String>(4)?, row.get(5)?, row.get::<_, i64>(6)?),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |((kind, heading, role, primary), (name_parts, role_term, position))| {
                Ok(NewContribution {
                    kind: required(kind, "agent kind", AgentKind::from_code)?,
                    heading,
                    role,
                    role_term,
                    primary,
                    name_parts: name_parts_of_column(&name_parts)?,
                    identifiers: links(
                        c,
                        "SELECT kind, value FROM contribution_identifiers
                         WHERE work_id = ?1 AND contribution = ?2 ORDER BY position",
                        work,
                        position,
                    )?,
                })
            },
        )
        .collect()
}

/// The links of the heading at `position` of `work`, read by `select` (a fixed statement).
fn links(
    c: &Connection,
    select: &'static str,
    work: i64,
    position: i64,
) -> Result<Vec<AuthorityLink>, ReadFailure> {
    let mut statement = c.prepare_cached(select)?;
    let rows = statement
        .query_map(params![work, position], |row| {
            Ok((row.get::<_, u8>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(kind, value)| {
            Ok(AuthorityLink {
                kind: required(kind, "authority link kind", AuthorityLinkKind::from_code)?,
                value,
            })
        })
        .collect()
}

pub(crate) fn subjects(c: &Connection, work: i64) -> Result<Vec<NewSubject>, ReadFailure> {
    let mut statement = c.prepare_cached(
        "SELECT subjects.kind, subjects.heading, terms.code, subjects.subdivisions,
                subjects.name_parts, position
         FROM work_subjects
         JOIN subjects ON subjects.id = work_subjects.subject_id
         LEFT JOIN terms ON terms.id = subjects.thesaurus
         WHERE work_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map(params![work], |row| {
            Ok((
                (row.get::<_, u8>(0)?, row.get(1)?, row.get(2)?),
                (
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |((kind, heading, thesaurus), (subdivisions, name_parts, position))| {
                Ok(NewSubject {
                    kind: required(kind, "subject kind", SubjectKind::from_code)?,
                    heading,
                    thesaurus,
                    subdivisions: subdivisions_of_column(&subdivisions)?,
                    name_parts: name_parts_of_column(&name_parts)?,
                    identifiers: links(
                        c,
                        "SELECT kind, value FROM subject_identifiers
                         WHERE work_id = ?1 AND subject = ?2 ORDER BY position",
                        work,
                        position,
                    )?,
                })
            },
        )
        .collect()
}

fn notes(c: &Connection, instance: i64) -> Result<Vec<NewNote>, ReadFailure> {
    let mut statement = c.prepare_cached(
        "SELECT kind, text FROM instance_notes WHERE instance_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map(params![instance], |row| {
            Ok((row.get::<_, u8>(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(kind, text)| {
            Ok(NewNote {
                kind: required(kind, "note kind", NoteKind::from_code)?,
                text,
            })
        })
        .collect()
}

fn identifiers(c: &Connection, instance: i64) -> Result<Vec<NewIdentifier>, ReadFailure> {
    let mut statement = c.prepare_cached(
        "SELECT kind, value, qualifier FROM instance_identifiers
         WHERE instance_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map(params![instance], |row| {
            Ok((row.get::<_, u8>(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(kind, value, qualifier)| {
            Ok(NewIdentifier {
                kind: required(kind, "identifier kind", IdentifierKind::from_code)?,
                value,
                qualifier,
            })
        })
        .collect()
}

/// The Items of an Instance as the MARC edge writes them: each registry row by its Koha
/// code (item 23b), or by its name when it has none.
fn items(c: &Connection, instance: i64) -> Result<Vec<ItemData>, ReadFailure> {
    let mut statement = c.prepare_cached(
        "SELECT barcode, inventory_number,
                COALESCE(home_code.code, home.name), COALESCE(holding_code.code, holding.name),
                COALESCE(policy_code.code, policy.name), call_number,
                COALESCE(location_code.code, location.name),
                COALESCE(collection_code.code, collection.name), price_cents,
                replacement_price_cents, copy_number, acquired_on,
                not_for_loan, damaged, lost, withdrawn, restricted
         FROM items
         JOIN branches AS home ON home.id = items.home_branch
         JOIN branches AS holding ON holding.id = items.holding_branch
         JOIN circulation_policies AS policy ON policy.id = items.policy_id
         LEFT JOIN sublocations AS location ON location.id = items.sublocation_id
         LEFT JOIN collections AS collection ON collection.id = items.collection_id
         LEFT JOIN koha_codes AS home_code
              ON home_code.kind = 1 AND home_code.local_id = home.id
         LEFT JOIN koha_codes AS holding_code
              ON holding_code.kind = 1 AND holding_code.local_id = holding.id
         LEFT JOIN koha_codes AS policy_code
              ON policy_code.kind = 4 AND policy_code.local_id = policy.id
         LEFT JOIN koha_codes AS location_code
              ON location_code.kind = 2 AND location_code.local_id = location.id
         LEFT JOIN koha_codes AS collection_code
              ON collection_code.kind = 3 AND collection_code.local_id = collection.id
         WHERE instance_id = ?1 ORDER BY items.id",
    )?;
    Ok(statement
        .query_map(params![instance], |row| {
            Ok(ItemData {
                barcode: row.get(0)?,
                inventory_number: row.get(1)?,
                home_branch: row.get(2)?,
                holding_branch: row.get(3)?,
                item_type: row.get(4)?,
                call_number: row.get(5)?,
                location: row.get(6)?,
                collection: row.get(7)?,
                price: row.get::<_, Option<i64>>(8)?.map(Money),
                replacement_price: row.get::<_, Option<i64>>(9)?.map(Money),
                copy_number: row.get(10)?,
                acquired_on: row.get(11)?,
                status: status_at(row, 12)?,
            })
        })?
        .collect::<Result<_, _>>()?)
}
