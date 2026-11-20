// SPDX-License-Identifier: GPL-3.0-or-later
//! A patron signing in to the OPAC and reading their own account
//! (docs/specs/opac/conta-do-leitor.md).

use elzevir_domain::patron::PatronId;
use jiff::civil::Date;

use crate::staff::{DECOY_HASH, verify_password};

/// What signing in compares: the patron and their password hash, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronCredentials {
    /// The patron.
    pub id: PatronId,
    /// The first name, or the surname without one; the only name the OPAC home shows.
    pub greeting: String,
    /// The Argon2id hash; `None` when the staff set no password.
    pub password_hash: Option<String>,
}

/// A patron signed in to the OPAC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronIdentity {
    /// The patron.
    pub id: PatronId,
    /// The name the OPAC home greets them with.
    pub greeting: String,
}

/// What the patron sees of their own account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronAccount {
    /// Full name, first name first.
    pub name: String,
    /// Library card number.
    pub cardnumber: String,
    /// Name of the home branch.
    pub branch_name: String,
    /// Name of the category.
    pub category_name: String,
    /// Last day of the enrolment.
    pub expires: Date,
}

/// Reading patron accounts failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("patron accounts unavailable: {0}")]
pub struct PatronAccountError(pub String);

/// The patron accounts as the OPAC reads them. Never lists patrons.
pub trait PatronAccounts: Send + Sync {
    /// The patron with card number `cardnumber`.
    ///
    /// # Errors
    /// [`PatronAccountError`] when storage fails.
    fn credentials(
        &self,
        cardnumber: &str,
    ) -> Result<Option<PatronCredentials>, PatronAccountError>;

    /// The patron's own account.
    ///
    /// # Errors
    /// [`PatronAccountError`] when storage fails.
    fn account(&self, id: PatronId) -> Result<Option<PatronAccount>, PatronAccountError>;
}

/// Why signing in was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatronLoginRefused {
    /// Unknown card, no password or the wrong one; deliberately one answer (spec, item 6).
    #[error("invalid card number or password")]
    InvalidCredentials,
    /// Storage failed.
    #[error(transparent)]
    Unavailable(#[from] PatronAccountError),
}

/// Checks `cardnumber` and `password` (spec, items 5 and 6). An unknown card or a patron
/// without a password costs one hash check all the same, so time tells nothing.
///
/// Koha: `C4::Auth::checkpw`.
///
/// # Errors
/// See [`PatronLoginRefused`].
pub fn log_in(
    accounts: &dyn PatronAccounts,
    cardnumber: &str,
    password: &str,
) -> Result<PatronIdentity, PatronLoginRefused> {
    let found = accounts.credentials(cardnumber.trim())?;
    let hash = found
        .as_ref()
        .and_then(|credentials| credentials.password_hash.as_deref());
    let Some((credentials, hash)) = found.as_ref().zip(hash) else {
        if let Some(decoy) = DECOY_HASH.as_deref() {
            // Spend the same effort as a real check; the result is irrelevant.
            let _same_cost = verify_password(password, decoy);
        }
        return Err(PatronLoginRefused::InvalidCredentials);
    };
    if !verify_password(password, hash) {
        return Err(PatronLoginRefused::InvalidCredentials);
    }
    Ok(PatronIdentity {
        id: credentials.id,
        greeting: credentials.greeting.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staff::hash_password;

    struct One(PatronCredentials);

    impl PatronAccounts for One {
        fn credentials(
            &self,
            cardnumber: &str,
        ) -> Result<Option<PatronCredentials>, PatronAccountError> {
            Ok((cardnumber == "0042").then(|| self.0.clone()))
        }

        fn account(&self, _: PatronId) -> Result<Option<PatronAccount>, PatronAccountError> {
            Ok(None)
        }
    }

    fn patron(password_hash: Option<String>) -> One {
        One(PatronCredentials {
            id: PatronId(7),
            greeting: "Ana".to_owned(),
            password_hash,
        })
    }

    #[test]
    fn the_right_card_and_password_sign_in_and_nothing_else_does() {
        let ana = patron(Some(hash_password("estante-azul").unwrap()));
        let identity = log_in(&ana, " 0042 ", "estante-azul").unwrap();
        assert_eq!(
            (identity.id, identity.greeting.as_str()),
            (PatronId(7), "Ana")
        );
        for (card, password) in [("0042", "errada"), ("9999", "estante-azul")] {
            assert_eq!(
                log_in(&ana, card, password),
                Err(PatronLoginRefused::InvalidCredentials)
            );
        }
        // Without a password, the patron has no OPAC account.
        assert_eq!(
            log_in(&patron(None), "0042", ""),
            Err(PatronLoginRefused::InvalidCredentials)
        );
    }
}
