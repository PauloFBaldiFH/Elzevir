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
}
