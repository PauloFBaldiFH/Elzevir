// SPDX-License-Identifier: GPL-3.0-or-later
//! A patron signing in to the OPAC and reading their own account
//! (docs/specs/opac/conta-do-leitor.md).

use elzevir_domain::patron::PatronId;
use jiff::civil::Date;

use crate::patrons::ReadingHistory;
use crate::staff::{DECOY_HASH, hash_password, verify_password};

/// Whether `hash` is a bcrypt hash, as Koha keeps them (`$2a$08$…`).
#[must_use]
pub fn is_koha_hash(hash: &str) -> bool {
    hash.starts_with("$2")
}

/// Whether `password` matches Koha's bcrypt `hash`. Koha hashes the UTF-8 bytes of the
/// password; a malformed hash never matches.
///
/// Koha: `C4::Auth::checkpw_hash`, `Koha::AuthUtils::hash_password`.
fn verify_koha_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password.as_bytes(), hash).unwrap_or(false)
}

/// Whether `password` is the patron's OPAC password, with the Elzevir's Argon2id `hash`
/// or Koha's bcrypt `hash` the migration brought. Nothing is upgraded: a check that only
/// reads, as the SIP2 server makes, leaves the upgrade to the OPAC sign-in.
///
/// Koha: `C4::Auth::checkpw_hash`.
#[must_use]
pub fn password_matches(password: &str, hash: &str) -> bool {
    if is_koha_hash(hash) {
        verify_koha_password(password, hash)
    } else {
        verify_password(password, hash)
    }
}

/// What signing in compares: the patron and their password hash, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatronCredentials {
    /// The patron.
    pub id: PatronId,
    /// The first name, or the surname without one; the only name the OPAC home shows.
    pub greeting: String,
    /// The Argon2id hash, or Koha's bcrypt hash brought by the migration
    /// (docs/specs/migracao/leitores-do-koha.md, item 12); `None` when there is no password.
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
    /// Whether the returned loans are kept, as the patron agreed.
    pub reading_history: ReadingHistory,
    /// The additional fields shown on the OPAC, read only
    /// (docs/specs/circulacao/patron-attributes.md, item 13).
    pub extra_lines: Vec<crate::patron_fields::ExtraLine>,
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

    /// Records the patron's own choice about the reading history; no staff member acts.
    ///
    /// # Errors
    /// [`PatronAccountError`] when storage fails or the patron is gone.
    fn set_reading_history(
        &self,
        id: PatronId,
        choice: ReadingHistory,
    ) -> Result<(), PatronAccountError>;

    /// Replaces a password hash brought from Koha by the Argon2id hash of the same password,
    /// once the patron signed in with it; no staff member acts.
    ///
    /// # Errors
    /// [`PatronAccountError`] when storage fails or the patron is gone.
    fn upgrade_password(
        &self,
        id: PatronId,
        password_hash: String,
    ) -> Result<(), PatronAccountError>;
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
    if is_koha_hash(hash) {
        if !verify_koha_password(password, hash) {
            return Err(PatronLoginRefused::InvalidCredentials);
        }
        if let Ok(argon) = hash_password(password) {
            // The patron is in either way; a failed upgrade is tried again at the next sign-in.
            let _retried_next_time = accounts.upgrade_password(credentials.id, argon);
        }
    } else if !verify_password(password, hash) {
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

    struct One(PatronCredentials, std::sync::Mutex<Vec<String>>);

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

        fn set_reading_history(
            &self,
            _: PatronId,
            _: ReadingHistory,
        ) -> Result<(), PatronAccountError> {
            Ok(())
        }

        fn upgrade_password(&self, _: PatronId, hash: String) -> Result<(), PatronAccountError> {
            self.1.lock().unwrap().push(hash);
            Ok(())
        }
    }

    fn patron(password_hash: Option<String>) -> One {
        One(
            PatronCredentials {
                id: PatronId(7),
                greeting: "Ana".to_owned(),
                password_hash,
            },
            std::sync::Mutex::default(),
        )
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

    /// Koha 26.05 on the oracle: `bcrypt('Senha fictícia 1', '$2a$08$' . en_base64('0123456789abcdef'))`.
    const KOHA_HASH: &str = "$2a$08$KBCwKxOzLha2MUDgW0PjXeL8PLapJrHH7Qyn8x/oWtvIYu16Gx/s2";

    #[test]
    fn a_koha_password_signs_in_once_and_becomes_argon2() {
        let ana = patron(Some(KOHA_HASH.to_owned()));
        assert_eq!(
            log_in(&ana, "0042", "Senha fictícia 2"),
            Err(PatronLoginRefused::InvalidCredentials)
        );
        assert!(ana.1.lock().unwrap().is_empty());
        let identity = log_in(&ana, "0042", "Senha fictícia 1").unwrap();
        assert_eq!(identity.id, PatronId(7));
        let upgraded = ana.1.lock().unwrap().clone();
        assert_eq!(upgraded.len(), 1);
        assert!(verify_password("Senha fictícia 1", &upgraded[0]));
        // A hash that only looks like bcrypt never matches.
        assert!(!verify_koha_password("x", "$2a$08$broken"));
    }

    #[test]
    fn a_password_is_checked_against_either_hash_without_upgrading() {
        assert!(password_matches("Senha fictícia 1", KOHA_HASH));
        assert!(!password_matches("Senha fictícia 2", KOHA_HASH));
        let argon = hash_password("Senha fictícia 1").unwrap();
        assert!(password_matches("Senha fictícia 1", &argon));
        assert!(!password_matches("Senha fictícia 2", &argon));
        assert!(!password_matches("Senha fictícia 1", "not a hash"));
    }
}
