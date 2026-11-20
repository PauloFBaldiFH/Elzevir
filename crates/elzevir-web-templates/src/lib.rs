// SPDX-License-Identifier: GPL-3.0-or-later
//! Askama templates for the OPAC and the staff interface, kept in their own crate to bound rebuild time.
//!
//! Templates never contain interface text: every string comes from Fluent through [`I18n`]
//! (English first, then Brazilian Portuguese; docs/arquitetura/04-decisoes.md, §1 item 7).

mod board;
mod opac_account;
mod staff_bar;
mod staff_calendar;
mod staff_catalog;
mod staff_circulation_rules;
mod staff_import;
mod staff_item_grid;
mod staff_notice_rules;
mod staff_patrons;
mod staff_record_fields;
mod staff_record_form;
mod staff_registries;

pub use board::{BoardCard, KindChoice, StaffBoardNote, note_color};
pub use opac_account::{OpacAccount, OpacLoan, OpacLogin, OpacNotice, PatronPanel};
pub use staff_bar::{BarLink, BarScope, ScopeKind, StaffBar};
pub use staff_calendar::{DayCell, EntryLine, StaffCalendar};
pub use staff_catalog::{
    ActiveFilter, AuthorityLine, BatchChoices, Cord, FacetMenu, FacetOption, FacetStrip,
    FamilyLine, HubLine, InstanceLine, PillInstance, PillLine, SavedListLink, ScanLine,
    StaffCatalog, StaffItemPill, WorkLine, WorkListLink,
};
pub use staff_circulation_rules::{
    Choice, RuleGroupPart, RuleRow, ScopeLine, StaffCirculationRules,
};
pub use staff_import::{
    ImportBatchLink, ImportChoice, ImportColumn, ImportColumns, ImportDetail, ImportDraft,
    ImportMatching, ImportPage, ImportPreview, ImportReceipt, ImportRule, ImportSample,
    ImportUpload, ImportView, RecordBlocks,
};
pub use staff_item_grid::{
    GridCell, GridColumn, GridList, GridOption, GridRow, ItemGrid, SuggestedCallNumber,
};
pub use staff_notice_rules::{
    HoldNoticeRow, NoticeLevelRow, NoticeRuleLine, NoticeTextForm, PrintedNotice, StaffNoticeRules,
    StaffPrintNotices,
};
pub use staff_patrons::{
    AccountRow, AsideForm, CategoryLine, DeskOption, HoldRow, LoanLine, Notice, PasswordSection,
    PatronFormValues, PatronLine, RestrictionRow, SlipLine, SlipSection, StaffCheckin,
    StaffCheckoutSlip, StaffPatron, StaffPatronCategories, StaffPatronForm, StaffPatrons,
};
pub use staff_record_fields::{FieldEntry, FieldGroup, FieldValue, ItemFields, RecordFields};
pub use staff_record_form::{
    AuthorityNote, ColumnKind, FormCell, FormColumn, FormField, FormInput, FormOption, FormPart,
    FormSection, History, HistoryLine, IdentifierChip, NewRecordForm, RecordForm,
};
pub use staff_registries::{BranchPart, RegistryRow, StaffRegistries};

use askama::Template;
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentResource, FluentValue};
use unic_langid::{LanguageIdentifier, langid};

/// English messages, compiled into the binary.
const EN_MESSAGES: &str = include_str!("../locales/en/main.ftl");
const PT_BR_MAIN: &str = include_str!("../locales/pt-BR/main.ftl");

/// Interface texts for one language.
pub struct I18n {
    lang: LanguageIdentifier,
    bundle: FluentBundle<FluentResource>,
}

/// The message files could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Fluent messages for {lang}: {detail}")]
pub struct I18nError {
    lang: String,
    detail: String,
}

impl I18n {
    /// English, the primary language.
    ///
    /// # Errors
    ///
    /// Fails only if the embedded `.ftl` file is invalid, which the tests rule out.
    pub fn english() -> Result<Self, I18nError> {
        Self::load(langid!("en"), EN_MESSAGES)
    }

    /// Brazilian Portuguese language bundle for the import workspace.
    ///
    /// # Errors
    /// Fails if the embedded Fluent resource is invalid.
    pub fn import_portuguese() -> Result<Self, I18nError> {
        Self::load(langid!("pt-BR"), PT_BR_MAIN)
    }

    /// Brazilian Portuguese, with English for keys awaiting translation.
    ///
    /// # Errors
    /// Reports invalid embedded Fluent resources.
    pub fn portuguese() -> Result<Self, I18nError> {
        let mut localized = Self::load(langid!("pt-BR"), EN_MESSAGES)?;
        let resource =
            FluentResource::try_new(PT_BR_MAIN.to_owned()).map_err(|(_, errors)| I18nError {
                lang: "pt-BR".into(),
                detail: format!("{errors:?}"),
            })?;
        localized.bundle.add_resource_overriding(resource);
        Ok(localized)
    }

    fn load(lang: LanguageIdentifier, source: &str) -> Result<Self, I18nError> {
        let error = |detail: String| I18nError {
            lang: lang.to_string(),
            detail,
        };
        let resource = FluentResource::try_new(source.to_owned())
            .map_err(|(_, errors)| error(format!("{errors:?}")))?;
        let mut bundle = FluentBundle::new_concurrent(vec![lang.clone()]);
        // Unicode isolation marks around placeables would end up inside HTML attributes.
        bundle.set_use_isolating(false);
        bundle
            .add_resource(resource)
            .map_err(|errors| error(format!("{errors:?}")))?;
        Ok(Self { lang, bundle })
    }

    /// The language tag, for `<html lang>`.
    #[must_use]
    pub fn lang(&self) -> String {
        self.lang.to_string()
    }

    /// The message for `key`. A missing key renders as the key itself, so a gap shows up
    /// on screen instead of failing the page; the tests make sure none are missing.
    #[must_use]
    pub fn get(&self, key: &str) -> String {
        self.format(key, None)
    }

    fn format(&self, key: &str, args: Option<&fluent_bundle::FluentArgs<'_>>) -> String {
        let Some(pattern) = self.bundle.get_message(key).and_then(|m| m.value()) else {
            return key.to_owned();
        };
        let mut errors = Vec::new();
        self.bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned()
    }

    /// Whether `key` exists.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.bundle.has_message(key)
    }

    /// Formats `key` with one named argument.
    #[must_use]
    pub fn get_with(&self, key: &str, name: &str, value: &str) -> String {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set(name, FluentValue::from(value));
        self.format(key, Some(&args))
    }

    /// Formats `key` with several named text arguments.
    #[must_use]
    pub fn get_texts(&self, key: &str, texts: &[(&str, &str)]) -> String {
        let mut args = fluent_bundle::FluentArgs::new();
        for (name, value) in texts {
            args.set(*name, FluentValue::from(*value));
        }
        self.format(key, Some(&args))
    }

    /// Formats `key` with named numbers, so that plural forms are chosen by the language.
    #[must_use]
    pub fn get_numbers(&self, key: &str, numbers: &[(&str, u64)]) -> String {
        let mut args = fluent_bundle::FluentArgs::new();
        for (name, value) in numbers {
            args.set(*name, FluentValue::from(*value));
        }
        self.format(key, Some(&args))
    }
}

/// OPAC home page.
#[derive(Template)]
#[template(path = "opac_home.html")]
pub struct OpacHome<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// The Instances catalogued last, newest first.
    pub recent: Vec<RecentCard>,
    /// The public notes of the board, newest first; empty shows the welcome note.
    pub notes: Vec<BoardCard>,
    /// The "Patron account" panel.
    pub panel: PatronPanel,
}

/// One Instance of the home page's "last catalogued" strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCard {
    /// The Instance, for the link to its record.
    pub id: i64,
    /// Its title.
    pub title: String,
    /// Its first contributor.
    pub contributor: Option<String>,
    /// Its date of publication.
    pub date: Option<String>,
    /// Its call number.
    pub call_number: Option<String>,
}

/// A categorical value with its icon (docs/specs/catalogo/bibframe-completo.md, items 15
/// and 16): the icon never stands alone, the label goes with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    /// Symbol id in `/static/elzevir-icons.svg`.
    pub icon: &'static str,
    /// Message key of its label.
    pub label_key: &'static str,
}

/// A text with the icon of what it is, such as a contributor with its kind of agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconText<'a> {
    /// Symbol id in `/static/elzevir-icons.svg`.
    pub icon: &'static str,
    /// The text.
    pub text: &'a str,
}

/// One search result as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultLine {
    /// Instance id, for the link to the record.
    pub instance: i64,
    /// Title.
    pub title: String,
    /// Main contributor.
    pub contributor: Option<String>,
    /// Date of publication.
    pub date: Option<String>,
    /// Its content and carrier, each with its icon.
    pub material: Vec<Badge>,
    /// Call number of the first Item.
    pub call_number: Option<String>,
    /// Items of the Instance.
    pub items: u32,
    /// Items available.
    pub available: u32,
}

impl ResultLine {
    /// CSS class of the availability line.
    #[must_use]
    pub const fn availability_class(&self) -> &'static str {
        if self.available > 0 {
            "available"
        } else {
            "unavailable"
        }
    }

    /// Availability, as a sentence.
    #[must_use]
    pub fn availability_text(&self, t: &I18n) -> String {
        if self.items == 0 {
            t.get("opac-availability-no-items")
        } else if self.available == 0 {
            t.get("opac-availability-none")
        } else {
            t.get_numbers(
                "opac-availability-available",
                &[
                    ("available", u64::from(self.available)),
                    ("items", u64::from(self.items)),
                ],
            )
        }
    }
}

/// The list of search results with its page links: a whole page, or only the fragment
/// that HTMX swaps in.
#[derive(Template)]
#[template(path = "opac_results.html", blocks = ["results"])]
pub struct OpacResults<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// The query as used.
    pub query: &'a str,
    /// Matching records in all.
    pub total: usize,
    /// This page, from 1.
    pub page: usize,
    /// Whether a next page exists.
    pub has_next: bool,
    /// The results of this page.
    pub rows: Vec<ResultLine>,
}

impl OpacResults<'_> {
    /// How many records matched, as a sentence.
    #[must_use]
    pub fn count_text(&self) -> String {
        self.t.get_numbers(
            "opac-results-count",
            &[("count", u64::try_from(self.total).unwrap_or(u64::MAX))],
        )
    }

    /// The query, encoded for a URL.
    #[must_use]
    pub fn query_url(&self) -> String {
        url_encode(self.query)
    }
}

/// One Item as the record shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLine {
    /// Home branch.
    pub branch: String,
    /// Shelving location.
    pub location: Option<String>,
    /// Call number.
    pub call_number: Option<String>,
    /// Whether it is available.
    pub available: bool,
    /// Its status, as shown.
    pub status: String,
    /// Symbol of its status's icon in `/static/elzevir-icons.svg`.
    pub icon: &'static str,
}

/// The body of a record page. It depends only on the record, so it is rendered once per
/// record version and kept (docs/specs/opac/busca-e-registro.md).
#[derive(Template)]
#[template(path = "opac_record_body.html")]
pub struct OpacRecordBody<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Title.
    pub title: &'a str,
    /// The Work's title, when it differs from the Instance's.
    pub work_title: Option<&'a str>,
    /// Variant, parallel and abbreviated titles.
    pub other_titles: Vec<&'a str>,
    /// Contributor headings, each with the icon of its kind of agent.
    pub contributors: Vec<IconText<'a>>,
    /// The Work's genres and forms.
    pub genres: Vec<&'a str>,
    /// Its content and carrier, each with its icon.
    pub material: Vec<Badge>,
    /// Publisher and date.
    pub publication: Option<String>,
    /// Classification number.
    pub classification: Option<&'a str>,
    /// Subject headings.
    pub subjects: Vec<&'a str>,
    /// Identifiers.
    pub identifiers: Vec<&'a str>,
    /// Language codes.
    pub languages: Vec<&'a str>,
    /// Summary.
    pub summary: Option<&'a str>,
    /// Target audience.
    pub audience: Option<&'a str>,
    /// General and local notes.
    pub notes: Vec<&'a str>,
    /// Contents notes.
    pub contents: Vec<&'a str>,
}

/// A record page around an already rendered body.
#[derive(Template)]
#[template(path = "opac_record.html")]
pub struct OpacRecord<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Title, for `<title>`.
    pub title: &'a str,
    /// The rendered [`OpacRecordBody`].
    pub body: &'a str,
    /// Items, read for each request: loans, transfers and holds change without the record.
    pub items: Vec<ItemLine>,
}

/// The page for a record that does not exist.
#[derive(Template)]
#[template(path = "opac_not_found.html")]
pub struct OpacNotFound<'a> {
    /// Interface texts.
    pub t: &'a I18n,
}

/// Percent-encodes `text` for a query string: unreserved characters stay, everything else
/// becomes `%XX` per UTF-8 byte.
#[must_use]
pub fn url_encode(text: &str) -> String {
    let hex = |nibble: u8| char::from_digit(u32::from(nibble), 16).map(|c| c.to_ascii_uppercase());
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            // Both nibbles are below 16, so `from_digit` always gives a character.
            encoded.extend(hex(byte >> 4));
            encoded.extend(hex(byte & 0x0f));
        }
    }
    encoded
}

/// Staff sign-in page.
#[derive(Template)]
#[template(path = "staff_login.html")]
pub struct StaffLogin<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Anti-forgery token echoed in the form.
    pub csrf: &'a str,
    /// Login typed before a failed attempt, to refill the field.
    pub login: &'a str,
    /// Message key of the error to show, if any.
    pub error: Option<&'static str>,
}

/// Staff home page: the notice board and the main modules (interface §22 item 11).
#[derive(Template)]
#[template(path = "staff_home.html")]
pub struct StaffHome<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// The modules this operator may see, in the draft's order; placeholders have no link.
    pub modules: Vec<BarLink>,
    /// The notes of the board, newest first; empty shows the welcome note.
    pub notes: Vec<BoardCard>,
    /// Whether the board is connected, so notes can be written.
    pub board: bool,
    /// Pending work shown on the module cards; only counts above zero.
    pub pending: Vec<PendingChip>,
}

/// One count of pending work on a module card (interface spec, §22 item 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChip {
    /// The Fluent key of the module's name, which says on which card it goes.
    pub module: &'static str,
    /// The count in words.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `t.get("...")` key in every template exists in the English messages.
    #[test]
    fn every_template_key_exists() {
        let t = I18n::english().unwrap();
        let templates = [
            include_str!("../templates/base.html"),
            include_str!("../templates/opac_home.html"),
            include_str!("../templates/opac_results.html"),
            include_str!("../templates/opac_results_list.html"),
            include_str!("../templates/opac_record.html"),
            include_str!("../templates/opac_record_body.html"),
            include_str!("../templates/opac_not_found.html"),
            include_str!("../templates/staff_login.html"),
            include_str!("../templates/staff_home.html"),
            include_str!("../templates/staff_bar.html"),
            include_str!("../templates/staff_base.html"),
            include_str!("../templates/staff_catalog.html"),
            include_str!("../templates/staff_record_fields.html"),
            include_str!("../templates/staff_record_form.html"),
            include_str!("../templates/staff_new_record.html"),
            include_str!("../templates/staff_item_grid.html"),
            include_str!("../templates/staff_form_field.html"),
            include_str!("../templates/staff_registries.html"),
            include_str!("../templates/staff_patrons.html"),
            include_str!("../templates/staff_patron.html"),
            include_str!("../templates/staff_checkout_slip.html"),
            include_str!("../templates/staff_patron_categories.html"),
            include_str!("../templates/staff_patron_form.html"),
            include_str!("../templates/staff_circulation_rules.html"),
            include_str!("../templates/staff_notice_rules.html"),
            include_str!("../templates/staff_print_notices.html"),
            include_str!("../templates/staff_calendar.html"),
            include_str!("../templates/staff_board_note.html"),
            include_str!("../templates/board_notes.html"),
            include_str!("../templates/patron_panel.html"),
            include_str!("../templates/opac_login.html"),
            include_str!("../templates/opac_account.html"),
        ];
        let mut missing = Vec::new();
        for source in templates {
            for chunk in source.split("t.get(\"").skip(1) {
                let key = chunk.split('"').next().unwrap();
                if !t.has(key) {
                    missing.push(key.to_owned());
                }
            }
        }
        assert!(missing.is_empty(), "missing Fluent keys: {missing:?}");
    }

    #[test]
    fn error_keys_used_by_the_web_layer_exist() {
        let t = I18n::english().unwrap();
        for key in [
            "staff-login-invalid",
            "staff-login-not-staff",
            "staff-login-unavailable",
        ] {
            assert!(t.has(key), "{key}");
        }
    }

    #[test]
    fn pages_render_escaped_html() {
        let t = I18n::english().unwrap();
        let page = StaffLogin {
            t: &t,
            csrf: "token",
            login: "<script>",
            error: Some("staff-login-invalid"),
        }
        .render()
        .unwrap();
        assert!(page.contains(r#"<html lang="en">"#));
        assert!(page.contains("Invalid username or password."));
        assert!(page.contains("&#60;script&#62;") || page.contains("&lt;script&gt;"));
        assert!(!page.contains("<script>\""));
    }

    #[test]
    fn queries_are_encoded_for_urls() {
        assert_eq!(url_encode("café & pão"), "caf%C3%A9%20%26%20p%C3%A3o");
        assert_eq!(url_encode("310.62 P94"), "310.62%20P94");
        assert_eq!(url_encode("a-b_c~d"), "a-b_c~d");
    }

    #[test]
    fn plurals_follow_the_number() {
        let t = I18n::english().unwrap();
        assert_eq!(
            t.get_numbers("opac-results-count", &[("count", 1)]),
            "1 result"
        );
        assert_eq!(
            t.get_numbers("opac-results-count", &[("count", 2)]),
            "2 results"
        );
    }

    #[test]
    fn missing_keys_render_as_the_key() {
        let t = I18n::english().unwrap();
        assert_eq!(t.get("no-such-key"), "no-such-key");
    }
}
