// SPDX-License-Identifier: GPL-3.0-or-later
//! The structured form of a prepared import's recipe (UI spec §21.1, steps 4 and 5):
//! MARC edit rules, record exclusions and matching choices, without writing JSON.

use elzevir_app::import::edits::{Change, Edit, FieldValue, Recipe, TextSubfield};
use elzevir_app::matching::{ItemAction, Matching, OnMatch, OnNoMatch};
use elzevir_domain::catalog::MatchRule;
use elzevir_web_templates::{ImportMatching, ImportRule};
use serde::Deserialize;

/// Longest value accepted in one text box of the form; the recipe has its own limits.
const MAX_FORM_TEXT: usize = 9_998;

/// One submission of the structured recipe form.
#[derive(Deserialize)]
pub(crate) struct RuleForm {
    pub(crate) csrf: String,
    pub(crate) revision: u64,
    /// `add`, `remove`, `exclude`, `include` or `matching`.
    action: String,
    #[serde(default)]
    record: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    op: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    to: String,
    #[serde(default)]
    indicators: String,
    #[serde(default)]
    subfields: String,
    index: Option<usize>,
    #[serde(default)]
    rule: String,
    #[serde(default)]
    on_match: String,
    #[serde(default)]
    on_no_match: String,
    #[serde(default)]
    items: String,
}

/// Applies the submission to the saved recipe. The error is the Fluent key that tells
/// the cataloguer which box to correct; the server still validates the whole recipe.
pub(crate) fn apply(recipe: &mut Recipe, form: &RuleForm) -> Result<(), &'static str> {
    match form.action.as_str() {
        "add" => {
            let edit = edit(form)?;
            recipe.edits.push(edit);
        }
        "remove" => {
            let index = form.index.ok_or("import-rule-missing")?;
            if index >= recipe.edits.len() {
                return Err("import-rule-missing");
            }
            recipe.edits.remove(index);
        }
        "exclude" => {
            recipe
                .exclude
                .insert(record(&form.record)?.ok_or("import-rule-record")?);
        }
        "include" => {
            recipe
                .exclude
                .remove(&record(&form.record)?.ok_or("import-rule-record")?);
        }
        "matching" => recipe.matching = matching(form)?,
        _ => return Err("import-rule-missing"),
    }
    Ok(())
}

fn edit(form: &RuleForm) -> Result<Edit, &'static str> {
    let tag = form.tag.trim();
    if tag.len() != 3 || !tag.bytes().all(|byte| byte.is_ascii_digit()) || tag == "000" {
        return Err("import-rule-tag");
    }
    let control = tag < "010";
    let change = match form.op.as_str() {
        "add_field" => Change::AddField {
            field: field_value(form, control)?,
        },
        "replace_field" => Change::ReplaceField {
            field: field_value(form, control)?,
        },
        "remove_field" => Change::RemoveField,
        "add_subfield" => Change::AddSubfield {
            code: code(&form.code, control)?,
            value: text(&form.value)?,
        },
        "replace_subfield" => Change::ReplaceSubfield {
            code: code(&form.code, control)?,
            value: text(&form.value)?,
        },
        "remove_subfield" => Change::RemoveSubfield {
            code: code(&form.code, control)?,
        },
        "replace_text" => {
            if form.from.is_empty() {
                return Err("import-rule-from");
            }
            Change::ReplaceText {
                from: text(&form.from)?,
                to: text(&form.to)?,
            }
        }
        _ => return Err("import-rule-op"),
    };
    Ok(Edit {
        record: record(&form.record)?,
        tag: tag.to_owned(),
        change,
    })
}

/// An empty box means the whole batch.
fn record(value: &str) -> Result<Option<u64>, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)
        .map(Some)
        .ok_or("import-rule-record")
}

fn code(value: &str, control: bool) -> Result<char, &'static str> {
    if control {
        return Err("import-rule-control");
    }
    let mut chars = value.trim().chars();
    match (chars.next(), chars.next()) {
        (Some(code), None) if code.is_ascii_alphanumeric() => Ok(code),
        _ => Err("import-rule-code"),
    }
}

fn text(value: &str) -> Result<String, &'static str> {
    if value.len() > MAX_FORM_TEXT || value.chars().any(char::is_control) {
        return Err("import-rule-value");
    }
    Ok(value.to_owned())
}

/// A new field as a cataloguer types it in mnemonic MARC: indicators with `\` or `#` for
/// blank, and subfields as `$aText$bMore`, with `{dollar}` for a literal dollar sign.
fn field_value(form: &RuleForm, control: bool) -> Result<FieldValue, &'static str> {
    if control {
        return Ok(FieldValue::Control {
            value: text(&form.value)?,
        });
    }
    let mut indicators = [' ', ' '];
    let typed: Vec<char> = form.indicators.chars().collect();
    if typed.len() > 2 {
        return Err("import-rule-indicators");
    }
    for (slot, typed) in indicators.iter_mut().zip(typed) {
        *slot = match typed {
            '\\' | '#' | ' ' => ' ',
            other if other.is_ascii_alphanumeric() => other,
            _ => return Err("import-rule-indicators"),
        };
    }
    Ok(FieldValue::Data {
        indicators,
        subfields: subfields(&form.subfields)?,
    })
}

fn subfields(value: &str) -> Result<Vec<TextSubfield>, &'static str> {
    let value = text(value.trim())?;
    let Some(rest) = value.strip_prefix('$') else {
        return Err("import-rule-subfields");
    };
    rest.split('$')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(code) if code.is_ascii_alphanumeric() => Ok(TextSubfield {
                    code,
                    value: chars.as_str().replace("{dollar}", "$"),
                }),
                _ => Err("import-rule-subfields"),
            }
        })
        .collect()
}

fn matching(form: &RuleForm) -> Result<Option<Matching>, &'static str> {
    let rule = match form.rule.as_str() {
        "none" => return Ok(None),
        "isbn" => MatchRule::Isbn,
        "issn" => MatchRule::Issn,
        "koha" => MatchRule::KohaBiblionumber,
        _ => return Err("import-rule-matching"),
    };
    let on_match = match form.on_match.as_str() {
        "ignore" => OnMatch::Ignore,
        "create" => OnMatch::CreateNew,
        "replace" => OnMatch::Replace,
        _ => return Err("import-rule-matching"),
    };
    let on_no_match = match form.on_no_match.as_str() {
        "create" => OnNoMatch::CreateNew,
        "ignore" => OnNoMatch::Ignore,
        _ => return Err("import-rule-matching"),
    };
    let items = match form.items.as_str() {
        "always" => ItemAction::AlwaysAdd,
        "matches" => ItemAction::AddOnlyForMatches,
        "new" => ItemAction::AddOnlyForNew,
        "replace" => ItemAction::Replace,
        "ignore" => ItemAction::Ignore,
        _ => return Err("import-rule-matching"),
    };
    Ok(Some(Matching {
        rule,
        on_match,
        on_no_match,
        items,
    }))
}

/// The saved rules, in application order, as the form lists them.
pub(crate) fn rules(recipe: &Recipe) -> Vec<ImportRule> {
    recipe
        .edits
        .iter()
        .enumerate()
        .map(|(index, edit)| {
            let (op_key, detail) = match &edit.change {
                Change::AddField { field } => ("import-op-add-field", field_text(field)),
                Change::ReplaceField { field } => ("import-op-replace-field", field_text(field)),
                Change::RemoveField => ("import-op-remove-field", String::new()),
                Change::AddSubfield { code, value } => {
                    ("import-op-add-subfield", format!("${code}{value}"))
                }
                Change::ReplaceSubfield { code, value } => {
                    ("import-op-replace-subfield", format!("${code}{value}"))
                }
                Change::RemoveSubfield { code } => {
                    ("import-op-remove-subfield", format!("${code}"))
                }
                Change::ReplaceText { from, to } => {
                    ("import-op-replace-text", format!("{from} → {to}"))
                }
            };
            ImportRule {
                index,
                record: edit.record,
                tag: edit.tag.clone(),
                op_key,
                detail,
            }
        })
        .collect()
}

fn field_text(field: &FieldValue) -> String {
    match field {
        FieldValue::Control { value } => value.clone(),
        FieldValue::Data {
            indicators,
            subfields,
        } => {
            let mut text: String = indicators
                .iter()
                .map(|indicator| if *indicator == ' ' { '#' } else { *indicator })
                .collect();
            for subfield in subfields {
                text.push('$');
                text.push(subfield.code);
                text.push_str(&subfield.value.replace('$', "{dollar}"));
            }
            text
        }
    }
}

/// The saved matching choices, as the values of the form's selects.
pub(crate) fn matching_view(matching: Option<Matching>) -> ImportMatching {
    let Some(matching) = matching else {
        return ImportMatching {
            rule: "none",
            on_match: "ignore",
            on_no_match: "create",
            items: "always",
        };
    };
    ImportMatching {
        rule: match matching.rule {
            MatchRule::Isbn => "isbn",
            MatchRule::Issn => "issn",
            MatchRule::KohaBiblionumber => "koha",
        },
        on_match: match matching.on_match {
            OnMatch::Ignore => "ignore",
            OnMatch::CreateNew => "create",
            OnMatch::Replace => "replace",
        },
        on_no_match: match matching.on_no_match {
            OnNoMatch::CreateNew => "create",
            OnNoMatch::Ignore => "ignore",
        },
        items: match matching.items {
            ItemAction::AlwaysAdd => "always",
            ItemAction::AddOnlyForMatches => "matches",
            ItemAction::AddOnlyForNew => "new",
            ItemAction::Replace => "replace",
            ItemAction::Ignore => "ignore",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(pairs: &[(&str, &str)]) -> RuleForm {
        let mut form = RuleForm {
            csrf: String::new(),
            revision: 1,
            action: String::new(),
            record: String::new(),
            tag: String::new(),
            op: String::new(),
            code: String::new(),
            value: String::new(),
            from: String::new(),
            to: String::new(),
            indicators: String::new(),
            subfields: String::new(),
            index: None,
            rule: String::new(),
            on_match: String::new(),
            on_no_match: String::new(),
            items: String::new(),
        };
        for (name, value) in pairs {
            let value = (*value).to_owned();
            match *name {
                "action" => form.action = value,
                "record" => form.record = value,
                "tag" => form.tag = value,
                "op" => form.op = value,
                "code" => form.code = value,
                "value" => form.value = value,
                "from" => form.from = value,
                "to" => form.to = value,
                "indicators" => form.indicators = value,
                "subfields" => form.subfields = value,
                "index" => form.index = value.parse().ok(),
                "rule" => form.rule = value,
                "on_match" => form.on_match = value,
                "on_no_match" => form.on_no_match = value,
                "items" => form.items = value,
                other => panic!("unknown box {other}"),
            }
        }
        form
    }

    #[test]
    fn a_marcedit_line_becomes_a_new_field_rule_for_the_batch() {
        let mut recipe = Recipe::default();
        apply(
            &mut recipe,
            &form(&[
                ("action", "add"),
                ("tag", "590"),
                ("op", "add_field"),
                ("indicators", "\\\\"),
                ("subfields", "$aDoação de 2026$bR{dollar}10"),
            ]),
        )
        .unwrap();
        let [edit] = recipe.edits.as_slice() else {
            panic!("one rule");
        };
        assert_eq!(edit.record, None);
        assert_eq!(edit.tag, "590");
        let Change::AddField {
            field:
                FieldValue::Data {
                    indicators,
                    subfields,
                },
        } = &edit.change
        else {
            panic!("a data field");
        };
        assert_eq!(*indicators, [' ', ' ']);
        let codes: Vec<_> = subfields
            .iter()
            .map(|subfield| (subfield.code, subfield.value.as_str()))
            .collect();
        assert_eq!(codes, [('a', "Doação de 2026"), ('b', "R$10")]);
        assert_eq!(rules(&recipe)[0].detail, "##$aDoação de 2026$bR{dollar}10");
    }

    #[test]
    fn a_rule_for_one_record_keeps_its_number() {
        let mut recipe = Recipe::default();
        apply(
            &mut recipe,
            &form(&[
                ("action", "add"),
                ("record", "3"),
                ("tag", "245"),
                ("op", "replace_subfield"),
                ("code", "a"),
                ("value", "Título revisto"),
            ]),
        )
        .unwrap();
        assert_eq!(recipe.edits[0].record, Some(3));
    }

    #[test]
    fn wrong_boxes_name_the_box_to_fix() {
        let mut recipe = Recipe::default();
        let cases = [
            (
                vec![("tag", "24"), ("op", "remove_field")],
                "import-rule-tag",
            ),
            (
                vec![("tag", "245"), ("op", "add_subfield"), ("code", "ab")],
                "import-rule-code",
            ),
            (
                vec![("tag", "008"), ("op", "remove_subfield"), ("code", "a")],
                "import-rule-control",
            ),
            (
                vec![("tag", "245"), ("op", "add_field"), ("subfields", "aTexto")],
                "import-rule-subfields",
            ),
            (
                vec![("tag", "245"), ("op", "replace_text"), ("from", "")],
                "import-rule-from",
            ),
            (
                vec![("tag", "245"), ("op", "remove_field"), ("record", "0")],
                "import-rule-record",
            ),
        ];
        for (pairs, key) in cases {
            let mut pairs = pairs;
            pairs.push(("action", "add"));
            assert_eq!(apply(&mut recipe, &form(&pairs)), Err(key), "{pairs:?}");
        }
        assert!(recipe.edits.is_empty());
    }

    #[test]
    fn exclusions_rules_and_matching_round_trip_through_the_form() {
        let mut recipe = Recipe::default();
        apply(
            &mut recipe,
            &form(&[("action", "exclude"), ("record", "2")]),
        )
        .unwrap();
        assert!(recipe.exclude.contains(&2));
        apply(
            &mut recipe,
            &form(&[("action", "include"), ("record", "2")]),
        )
        .unwrap();
        assert!(recipe.exclude.is_empty());
        apply(
            &mut recipe,
            &form(&[("action", "add"), ("tag", "952"), ("op", "remove_field")]),
        )
        .unwrap();
        apply(&mut recipe, &form(&[("action", "remove"), ("index", "0")])).unwrap();
        assert!(recipe.edits.is_empty());
        assert_eq!(
            apply(&mut recipe, &form(&[("action", "remove"), ("index", "0")])),
            Err("import-rule-missing")
        );
        apply(
            &mut recipe,
            &form(&[
                ("action", "matching"),
                ("rule", "isbn"),
                ("on_match", "replace"),
                ("on_no_match", "create"),
                ("items", "replace"),
            ]),
        )
        .unwrap();
        let view = matching_view(recipe.matching);
        assert_eq!(
            (view.rule, view.on_match, view.on_no_match, view.items),
            ("isbn", "replace", "create", "replace")
        );
        apply(
            &mut recipe,
            &form(&[("action", "matching"), ("rule", "none")]),
        )
        .unwrap();
        assert!(recipe.matching.is_none());
    }
}
