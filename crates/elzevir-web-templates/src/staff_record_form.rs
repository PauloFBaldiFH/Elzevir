// SPDX-License-Identifier: GPL-3.0-or-later
//! The focused block as a form, every field in its section (docs/specs/catalogo/edicao.md,
//! items 25 to 32; bibframe-completo.md, items 24 to 26).

use askama::Template;

use crate::I18n;

/// The form of one Work or one Instance.
#[derive(Template)]
#[template(path = "staff_record_form.html")]
pub struct RecordForm<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Where the edit is sent.
    pub endpoint: String,
    /// The version the form was read at.
    pub version: i64,
    /// Every field, by section, filled or not.
    pub sections: Vec<FormSection>,
    /// Where the block's history is read, for a Work or an Instance
    /// (docs/specs/catalogo/historico.md).
    pub history: Option<String>,
    /// For an authority's block, what saving changes and where else it is identified
    /// (docs/specs/catalogo/autoridades.md).
    pub authority: Option<AuthorityNote>,
}

/// What an authority's block says before its field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityNote {
    /// How many Works and records saving changes, in the screen's language.
    pub reach: String,
    /// Its identifiers in other datasets.
    pub identifiers: Vec<IdentifierChip>,
}

/// An identifier of an authority in another dataset, shown as a chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierChip {
    /// The dataset, such as `VIAF`, or the identifier itself when no dataset is known.
    pub label: String,
    /// The identifier as stored.
    pub value: String,
    /// Where it opens, for an identifier that is a web address.
    pub href: Option<String>,
}

/// The history of a Work or an Instance (docs/specs/catalogo/historico.md).
#[derive(Template)]
#[template(path = "staff_history.html")]
pub struct History<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Its lines, newest first.
    pub lines: Vec<HistoryLine>,
}

/// One line of a history, its texts already in the screen's language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryLine {
    /// When, as stored (`YYYY-MM-DDTHH:MM:SSZ`); empty when not kept.
    pub at: String,
    /// When, as shown.
    pub when: String,
    /// Who.
    pub who: String,
    /// What.
    pub what: String,
}

/// One fixed section of a form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormSection {
    /// Message key of its heading; `None` for a form of one section.
    pub heading_key: Option<&'static str>,
    /// Its fields, in order.
    pub fields: Vec<FormField>,
}

/// One field of the form: one change of the edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    /// The change's name in the edit (`Title`, `Identifiers`).
    pub change: &'static str,
    /// The field name refusals use (`title`, `identifiers`).
    pub error: &'static str,
    /// Message key of the label.
    pub label_key: &'static str,
    /// The input.
    pub input: FormInput,
    /// The BIBFRAME property whose statements the field holds; empty for a typed part.
    pub property: &'static str,
    /// The property's label in the ontology, shown in place of `label_key`'s message.
    pub ontology_label: &'static str,
    /// Whether the value came from the previous record and is not confirmed yet
    /// (docs/specs/catalogo/criacao.md, item 17).
    pub inherited: bool,
}

impl FormField {
    /// The label shown: the ontology's for a property, the interface's for a typed part.
    #[must_use]
    pub fn label(&self, t: &I18n) -> String {
        if self.ontology_label.is_empty() {
            t.get(self.label_key)
        } else {
            self.ontology_label.to_owned()
        }
    }
}

/// A form that creates: a new family (Work and Instance) or a new Instance of a Work
/// (docs/specs/catalogo/criacao.md, items 13 to 18).
#[derive(Template)]
#[template(path = "staff_new_record.html")]
pub struct NewRecordForm<'a> {
    /// Interface texts.
    pub t: &'a I18n,
    /// Where the new record is sent.
    pub endpoint: String,
    /// `family` or `instance`: how the script shapes the body.
    pub create: &'static str,
    /// Message key of the form's heading.
    pub heading_key: &'static str,
    /// Message key of the button that sends it.
    pub submit_key: &'static str,
    /// The blocks being created, in order.
    pub parts: Vec<FormPart>,
}

/// The fields of one block of a new record: every field of the entity, by section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPart {
    /// `work` or `instance`: the key of the block in the body.
    pub name: &'static str,
    /// Message key of the block's heading.
    pub heading_key: &'static str,
    /// The fields, by section.
    pub sections: Vec<FormSection>,
}

/// How a field is entered, and how the script turns it into JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormInput {
    /// One line; `required` sends a string, otherwise an empty line clears the field.
    Text {
        /// The stored value.
        value: String,
        /// Whether it cannot be cleared.
        required: bool,
    },
    /// Several lines; empty clears the field.
    LongText {
        /// The stored value.
        value: String,
    },
    /// One choice.
    Select {
        /// The stored choice, empty when none.
        value: String,
        /// The choices.
        options: Vec<FormOption>,
    },
    /// A list of rows, each sent as one object.
    Rows {
        /// The stored rows.
        rows: Vec<Vec<FormCell>>,
        /// A new row.
        blank: Vec<FormCell>,
    },
    /// A list of single values, sent as an array of strings.
    Values {
        /// The stored values.
        values: Vec<String>,
    },
    /// A yes or no, sent as `true` or `false`.
    Check {
        /// The stored value.
        checked: bool,
    },
    /// Several values sent together as one object.
    Group {
        /// Its parts, with their stored values.
        cells: Vec<FormCell>,
    },
}

/// One column of a row or part of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormColumn {
    /// Its name in the JSON object.
    pub name: &'static str,
    /// Message key of its label.
    pub label_key: &'static str,
    /// How it is entered.
    pub kind: ColumnKind,
    /// Whether an empty value is sent as `null`.
    pub optional: bool,
}

/// One column of one row, with its value (`true` or `false` for a check).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormCell {
    /// The column.
    pub column: FormColumn,
    /// Its value in this row.
    pub value: String,
}

impl FormCell {
    /// Whether its column is a select whose options have icons.
    #[must_use]
    pub fn has_icons(&self) -> bool {
        matches!(&self.column.kind, ColumnKind::Select(options)
            if options.iter().any(|option| option.icon.is_some()))
    }

    /// The icon of the chosen option, empty when it has none.
    #[must_use]
    pub fn icon(&self) -> &'static str {
        match &self.column.kind {
            ColumnKind::Select(options) => options
                .iter()
                .find(|option| option.value == self.value)
                .and_then(|option| option.icon)
                .unwrap_or_default(),
            _ => "",
        }
    }
}

/// How a column is entered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnKind {
    /// Free text.
    Text,
    /// One of `options`.
    Select(Vec<FormOption>),
    /// Yes or no.
    Check,
    /// A whole number, sent as a number (`null` when empty).
    Number,
    /// Several lines: the statements of a nested resource.
    Details,
    /// Not shown: what the row was drawn from, sent back as it is.
    Hidden,
}

/// One choice of a select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormOption {
    /// The value sent.
    pub value: String,
    /// The label shown, in the interface language.
    pub label: String,
    /// The symbol of its icon in `/static/elzevir-icons.svg`, shown beside the select
    /// while it is chosen (docs/specs/catalogo/bibframe-completo.md, item 14).
    pub icon: Option<&'static str>,
}
