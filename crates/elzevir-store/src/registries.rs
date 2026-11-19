// SPDX-License-Identifier: GPL-3.0-or-later
//! `catalog.change_registry`: branches, shelving places, collections and circulation
//! policies created, renamed and deleted by the library
//! (docs/specs/catalogo/bibframe-completo.md, items 23d and 23f).

use elzevir_app::catalog::{CatalogOutput, CatalogRefusal};
use elzevir_app::commands::WriteRefusal;
use elzevir_app::registries::{
    BranchContact, RegistryChange, RegistryEntry, RegistryList, RegistryRefusal, checked,
};
use elzevir_domain::catalog::{BranchId, CollectionId, PolicyId, SublocationId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::catalog::refused;
use crate::places::Coded;
use crate::{CommandFailed, StoreError, WriteTx};

type Changed = Result<CatalogOutput, CommandFailed<WriteRefusal>>;

/// The constant SQL of one registry.
struct Table {
    coded: Coded,
    exists: &'static str,
    /// Another row of the same list with the name: `?1` the name, `?2` the row (0 for a
    /// new one), `?3` the branch of a per-branch list.
    taken: &'static str,
    rename: &'static str,
    /// Counts what uses the row.
    users: &'static str,
    delete: &'static str,
}

const BRANCHES: Table = Table {
    coded: Coded::Branch,
    exists: "SELECT 1 FROM branches WHERE id = ?1",
    taken: "SELECT 1 FROM branches WHERE name = ?1 AND id <> ?2 AND ?3 IS NULL",
    rename: "UPDATE branches SET name = ?2, row_version = row_version + 1 WHERE id = ?1",
    users: "SELECT (SELECT count(*) FROM items WHERE home_branch = ?1 OR holding_branch = ?1)
                 + (SELECT count(*) FROM sublocations WHERE branch_id = ?1)
                 + (SELECT count(*) FROM collections WHERE branch_id = ?1)
                 + (SELECT count(*) FROM patrons WHERE branch_id = ?1)
                 + (SELECT count(*) FROM circulation_rules WHERE branch_id = ?1)",
    delete: "DELETE FROM branches WHERE id = ?1",
};

const SUBLOCATIONS: Table = Table {
    coded: Coded::Sublocation,
    exists: "SELECT 1 FROM sublocations WHERE id = ?1",
    taken: "SELECT 1 FROM sublocations WHERE name = ?1 AND id <> ?2
             AND branch_id = coalesce(?3, (SELECT branch_id FROM sublocations WHERE id = ?2))",
    rename: "UPDATE sublocations SET name = ?2, row_version = row_version + 1 WHERE id = ?1",
    users: "SELECT count(*) FROM items WHERE sublocation_id = ?1",
    delete: "DELETE FROM sublocations WHERE id = ?1",
};

const COLLECTIONS: Table = Table {
    coded: Coded::Collection,
    exists: "SELECT 1 FROM collections WHERE id = ?1",
    taken: "SELECT 1 FROM collections WHERE name = ?1 AND id <> ?2
             AND branch_id = coalesce(?3, (SELECT branch_id FROM collections WHERE id = ?2))",
    rename: "UPDATE collections SET name = ?2, row_version = row_version + 1 WHERE id = ?1",
    users: "SELECT count(*) FROM items WHERE collection_id = ?1",
    delete: "DELETE FROM collections WHERE id = ?1",
};

const POLICIES: Table = Table {
    coded: Coded::Policy,
    exists: "SELECT 1 FROM circulation_policies WHERE id = ?1",
    taken: "SELECT 1 FROM circulation_policies WHERE name = ?1 AND id <> ?2 AND ?3 IS NULL",
    rename: "UPDATE circulation_policies SET name = ?2, row_version = row_version + 1
             WHERE id = ?1",
    users: "SELECT (SELECT count(*) FROM items WHERE policy_id = ?1)
                 + (SELECT count(*) FROM circulation_rules WHERE policy_id = ?1)",
    delete: "DELETE FROM circulation_policies WHERE id = ?1",
};

const fn table_of(entry: RegistryEntry) -> (&'static Table, i64) {
    match entry {
        RegistryEntry::Branch(BranchId(id)) => (&BRANCHES, id),
        RegistryEntry::Sublocation(SublocationId(id)) => (&SUBLOCATIONS, id),
        RegistryEntry::Collection(CollectionId(id)) => (&COLLECTIONS, id),
        RegistryEntry::Policy(PolicyId(id)) => (&POLICIES, id),
    }
}

fn refuse(refusal: RegistryRefusal) -> CommandFailed<WriteRefusal> {
    refused(CatalogRefusal::Registry(refusal))
}

fn found(
    connection: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<bool, StoreError> {
    Ok(connection
        .query_row(sql, args, |_| Ok(()))
        .optional()?
        .is_some())
}

/// `catalog.change_registry`.
pub(crate) fn change_registry(tx: &WriteTx<'_>, change: &RegistryChange) -> Changed {
    let connection = tx.connection();
    let change = checked(change.clone()).map_err(refuse)?;
    let entry = match change {
        RegistryChange::Create { list, name } => Some(create(connection, list, &name)?),
        RegistryChange::Rename { entry, name } => {
            let (table, id) = table_of(entry);
            exists(connection, table, id)?;
            if found(connection, table.taken, params![name, id, None::<i64>])? {
                return Err(refuse(RegistryRefusal::NameTaken));
            }
            connection
                .execute(table.rename, params![id, name])
                .map_err(StoreError::from)?;
            Some(entry)
        }
        RegistryChange::Delete(entry) => {
            let (table, id) = table_of(entry);
            exists(connection, table, id)?;
            let users: i64 = connection
                .query_row(table.users, params![id], |row| row.get(0))
                .map_err(StoreError::from)?;
            if users > 0 {
                return Err(refuse(RegistryRefusal::InUse));
            }
            connection
                .execute(table.delete, params![id])
                .map_err(StoreError::from)?;
            // Its Koha code names nothing now.
            connection
                .execute(
                    "DELETE FROM koha_codes WHERE kind = ?1 AND local_id = ?2",
                    params![table.coded as i64, id],
                )
                .map_err(StoreError::from)?;
            None
        }
        RegistryChange::Contact { branch, contact } => {
            exists(connection, &BRANCHES, branch.0)?;
            set_contact(connection, branch, &contact)?;
            Some(RegistryEntry::Branch(branch))
        }
    };
    Ok(CatalogOutput::RegistryChanged(entry))
}

fn exists(
    connection: &Connection,
    table: &Table,
    id: i64,
) -> Result<(), CommandFailed<WriteRefusal>> {
    if found(connection, table.exists, params![id])? {
        Ok(())
    } else {
        Err(refuse(RegistryRefusal::Unknown))
    }
}

fn create(
    connection: &Connection,
    list: RegistryList,
    name: &str,
) -> Result<RegistryEntry, CommandFailed<WriteRefusal>> {
    let (table, branch, insert) = match list {
        RegistryList::Branches => (&BRANCHES, None, "INSERT INTO branches (name) VALUES (?1)"),
        RegistryList::Policies => (
            &POLICIES,
            None,
            "INSERT INTO circulation_policies (name) VALUES (?1)",
        ),
        RegistryList::Sublocations(branch) => (
            &SUBLOCATIONS,
            Some(branch),
            "INSERT INTO sublocations (name, branch_id) VALUES (?1, ?2)",
        ),
        RegistryList::Collections(branch) => (
            &COLLECTIONS,
            Some(branch),
            "INSERT INTO collections (name, branch_id) VALUES (?1, ?2)",
        ),
    };
    if let Some(branch) = branch {
        exists(connection, &BRANCHES, branch.0)?;
    }
    let branch = branch.map(|branch| branch.0);
    if found(connection, table.taken, params![name, 0_i64, branch])? {
        return Err(refuse(RegistryRefusal::NameTaken));
    }
    match branch {
        Some(branch) => connection.execute(insert, params![name, branch]),
        None => connection.execute(insert, params![name]),
    }
    .map_err(StoreError::from)?;
    let id = connection.last_insert_rowid();
    Ok(match list {
        RegistryList::Branches => RegistryEntry::Branch(BranchId(id)),
        RegistryList::Policies => RegistryEntry::Policy(PolicyId(id)),
        RegistryList::Sublocations(_) => RegistryEntry::Sublocation(SublocationId(id)),
        RegistryList::Collections(_) => RegistryEntry::Collection(CollectionId(id)),
    })
}

fn set_contact(
    connection: &Connection,
    branch: BranchId,
    contact: &BranchContact,
) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE branches SET address = ?2, phone = ?3, email = ?4, url = ?5,
                             row_version = row_version + 1
         WHERE id = ?1",
        params![
            branch.0,
            contact.address,
            contact.phone,
            contact.email,
            contact.url
        ],
    )?;
    Ok(())
}

/// The contact data of every branch that has some, by branch id.
pub(crate) fn contacts(c: &Connection) -> Result<Vec<(BranchId, BranchContact)>, rusqlite::Error> {
    let mut statement = c.prepare(
        "SELECT id, address, phone, email, url FROM branches
         WHERE coalesce(address, phone, email, url) IS NOT NULL ORDER BY id",
    )?;
    statement
        .query_map([], |row| {
            Ok((
                BranchId(row.get(0)?),
                BranchContact {
                    address: row.get(1)?,
                    phone: row.get(2)?,
                    email: row.get(3)?,
                    url: row.get(4)?,
                },
            ))
        })?
        .collect()
}
