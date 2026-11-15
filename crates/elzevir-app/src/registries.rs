// SPDX-License-Identifier: GPL-3.0-or-later
//! The registries a library keeps for its Items: branches with their contact data,
//! the shelving places and collections of each branch, and circulation policies
//! (docs/specs/catalogo/bibframe-completo.md, items 23a to 23d and 23f).

use elzevir_domain::catalog::{BranchId, CollectionId, PolicyId, SublocationId};
use serde::{Deserialize, Serialize};

/// Longest name of a registry row.
pub const MAX_NAME: usize = 256;
/// Longest contact field of a branch.
pub const MAX_CONTACT: usize = 512;

/// A list a new row joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryList {
    /// Branches.
    Branches,
    /// The shelving places of a branch.
    Sublocations(BranchId),
    /// The collections of a branch.
    Collections(BranchId),
    /// Circulation policies.
    Policies,
}

/// One stored row of a registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryEntry {
    /// A branch.
    Branch(BranchId),
    /// A shelving place.
    Sublocation(SublocationId),
    /// A collection.
    Collection(CollectionId),
    /// A circulation policy.
    Policy(PolicyId),
}

/// How people reach a branch (`bf:Organization`); every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchContact {
    /// Postal address.
    pub address: Option<String>,
    /// Phone.
    pub phone: Option<String>,
    /// E-mail.
    pub email: Option<String>,
    /// Web page.
    pub url: Option<String>,
}

/// One change to the registries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryChange {
    /// Adds a row named `name` to `list`.
    Create {
        /// The list.
        list: RegistryList,
        /// Its name.
        name: String,
    },
    /// Renames a row.
    Rename {
        /// The row.
        entry: RegistryEntry,
        /// Its new name.
        name: String,
    },
    /// Deletes a row no Item, shelving place or collection uses.
    Delete(RegistryEntry),
    /// Replaces the contact data of a branch.
    Contact {
        /// The branch.
        branch: BranchId,
        /// Its contact data.
        contact: BranchContact,
    },
}

/// Why a registry change was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryRefusal {
    /// The name is empty or too long, or a contact field is too long.
    BadName,
    /// Another row of the same list has this name.
    NameTaken,
    /// The row is in use, so it stays.
    InUse,
    /// No row has this id.
    Unknown,
}

impl RegistryRefusal {
    /// Fluent key of the message.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::BadName => "registries-error-name",
            Self::NameTaken => "registries-error-taken",
            Self::InUse => "registries-error-in-use",
            Self::Unknown => "registries-error-unknown",
        }
    }
}

/// The change trimmed as it is stored: names trimmed and not empty, contact fields
/// trimmed and empty ones dropped.
///
/// # Errors
///
/// [`RegistryRefusal::BadName`] when a name is empty or a text is too long.
pub fn checked(change: RegistryChange) -> Result<RegistryChange, RegistryRefusal> {
    let name = |name: String| {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME {
            Err(RegistryRefusal::BadName)
        } else {
            Ok(trimmed.to_owned())
        }
    };
    let field = |text: Option<String>| -> Result<Option<String>, RegistryRefusal> {
        match text.as_deref().map(str::trim) {
            None | Some("") => Ok(None),
            Some(text) if text.chars().count() > MAX_CONTACT => Err(RegistryRefusal::BadName),
            Some(text) => Ok(Some(text.to_owned())),
        }
    };
    Ok(match change {
        RegistryChange::Create { list, name: typed } => RegistryChange::Create {
            list,
            name: name(typed)?,
        },
        RegistryChange::Rename { entry, name: typed } => RegistryChange::Rename {
            entry,
            name: name(typed)?,
        },
        RegistryChange::Delete(entry) => RegistryChange::Delete(entry),
        RegistryChange::Contact { branch, contact } => RegistryChange::Contact {
            branch,
            contact: BranchContact {
                address: field(contact.address)?,
                phone: field(contact.phone)?,
                email: field(contact.email)?,
                url: field(contact.url)?,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_empty_ones_refused() {
        assert_eq!(
            checked(RegistryChange::Create {
                list: RegistryList::Policies,
                name: "  Empréstimo normal ".to_owned(),
            }),
            Ok(RegistryChange::Create {
                list: RegistryList::Policies,
                name: "Empréstimo normal".to_owned(),
            })
        );
        assert_eq!(
            checked(RegistryChange::Rename {
                entry: RegistryEntry::Branch(BranchId(1)),
                name: "   ".to_owned(),
            }),
            Err(RegistryRefusal::BadName)
        );
    }

    #[test]
    fn empty_contact_fields_are_dropped() {
        let Ok(RegistryChange::Contact { contact, .. }) = checked(RegistryChange::Contact {
            branch: BranchId(1),
            contact: BranchContact {
                address: Some(" Rua A, 1 ".to_owned()),
                phone: Some(" ".to_owned()),
                email: None,
                url: Some("https://biblioteca.example".to_owned()),
            },
        }) else {
            panic!("a valid contact")
        };
        assert_eq!(contact.address.as_deref(), Some("Rua A, 1"));
        assert_eq!(contact.phone, None);
        assert_eq!(contact.url.as_deref(), Some("https://biblioteca.example"));
    }
}
