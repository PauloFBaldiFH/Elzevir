// SPDX-License-Identifier: GPL-3.0-or-later
//! The patron attributes of `migrate-koha` and `switch-from-koha`: Koha's types become
//! additional fields and their values come to the migrated patrons
//! (docs/specs/circulacao/patron-attributes.md, items 16 to 18).

use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::koha_migration::{KohaAttributesMigrated, KohaPlan, ValueLeftover};
use elzevir_app::patrons::{PatronCommand, PatronOutput};
use elzevir_store::ElzevirStore;

/// The types and values, in batches, after the patrons.
pub(crate) fn migrate(
    store: &ElzevirStore,
    plan: &KohaPlan,
) -> Result<KohaAttributesMigrated, String> {
    let mut done = KohaAttributesMigrated::default();
    for batch in plan.attribute_batches() {
        let answer = store
            .submit(WriteCommand::Patrons(PatronCommand::MigrateKohaAttributes(
                Box::new(batch),
            )))
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
        let WriteOutput::Patrons(PatronOutput::KohaAttributesMigrated(write)) = answer else {
            return Err("unexpected answer from the store".to_owned());
        };
        done.add(write);
    }
    Ok(done)
}

pub(crate) const fn leftover(why: ValueLeftover) -> &'static str {
    match why {
        ValueLeftover::Patron => "patron not migrated",
        ValueLeftover::Type => "type not migrated",
        ValueLeftover::Invalid => "not in the list, not a date, or too long",
        ValueLeftover::Repeated => "more values than the field takes",
        ValueLeftover::Taken => "unique value another patron holds",
    }
}

/// The report: counts and field names, never a value (item 18).
pub(crate) fn report(plan: &KohaPlan, done: &KohaAttributesMigrated) -> String {
    let read = &plan.attributes_read;
    let left: usize = done.left.values().sum();
    let mut lines = vec![
        format!(
            "patron attributes: {} fields created ({} options), {} already migrated, {} types left out for a taken name",
            done.fields_created,
            done.options,
            done.fields_existing,
            done.names_taken.len()
        ),
        format!(
            "attribute values: {} migrated, {} already migrated, {left} left out",
            done.values, done.already
        ),
    ];
    for code in &done.names_taken {
        lines.push(format!(
            "  type {code}: its name and code are taken by other fields"
        ));
    }
    for ((field, why), count) in &done.left {
        lines.push(format!("  {field}: {count} {}", leftover(*why)));
    }
    if read.list_as_text > 0 {
        lines.push(format!(
            "  {} list types without values came as text",
            read.list_as_text
        ));
    }
    if read.options_cut > 0 {
        lines.push(format!(
            "  {} list types had more options than a field takes",
            read.options_cut
        ));
    }
    if !read.settings_not_kept.is_empty() {
        lines.push("attribute type settings not kept (types using each):".to_owned());
        for (setting, count) in &read.settings_not_kept {
            lines.push(format!("  {setting}: {count}"));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The `[sip2]` block the switch adds to the configuration it writes, commented because
/// the SIP2 server only starts with an address the library chooses (item 20).
pub(crate) fn sip2_block(plan: &KohaPlan) -> Option<String> {
    let currency = plan.sip2_currency.as_ref()?;
    Some(format!(
        "\n# Koha's active currency, which the SIP2 server sends in BH. Uncomment the block\n\
         # with the address the self-check machines use to turn the SIP2 server on.\n\
         # [sip2]\n\
         # listen = \"0.0.0.0:6001\"\n\
         # currency = {}\n",
        toml::Value::String(currency.clone())
    ))
}

/// The report line of the SIP2 currency (item 20).
pub(crate) fn sip2_report(plan: &KohaPlan, file: &str) -> String {
    match &plan.sip2_currency {
        Some(currency) => {
            format!("SIP2 currency: {currency}, in a commented [sip2] block of {file}\n")
        }
        None => {
            "SIP2 currency: Koha has no active currency; [sip2] currency left as it is\n".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_currency_comes_commented_and_quoted() {
        let mut plan = KohaPlan::default();
        assert_eq!(sip2_block(&plan), None);
        assert!(sip2_report(&plan, "elzevir.toml").contains("no active currency"));
        plan.sip2_currency = Some("RE\"".to_owned());
        let block = sip2_block(&plan).unwrap_or_default();
        assert!(
            block
                .lines()
                .all(|line| line.is_empty() || line.starts_with('#'))
        );
        // Uncommented, the block is a valid `[sip2]` section.
        let uncommented = block
            .lines()
            .filter_map(|line| line.strip_prefix("# "))
            .filter(|line| line.starts_with('[') || line.contains(" = "))
            .collect::<Vec<_>>()
            .join("\n");
        let value: toml::Table = toml::from_str(&uncommented).unwrap_or_default();
        let currency = value
            .get("sip2")
            .and_then(|sip2| sip2.get("currency"))
            .and_then(toml::Value::as_str);
        assert_eq!(currency, Some("RE\""));
    }
}
