// SPDX-License-Identifier: GPL-3.0-or-later
//! Embedded panel translations, independent of the web templates.

use crate::PanelError;
use fluent_bundle::{FluentBundle, FluentResource};
use unic_langid::langid;

/// Fluent messages for the selected panel language.
pub struct Messages(FluentBundle<FluentResource>);

impl Messages {
    /// Loads an embedded English or Brazilian Portuguese translation.
    ///
    /// # Errors
    /// Returns an error for unsupported languages or invalid Fluent resources.
    pub fn new(language: &str) -> Result<Self, PanelError> {
        let (id, source) = match language {
            "en" => (langid!("en"), include_str!("../locales/en.ftl")),
            "pt-BR" => (langid!("pt-BR"), include_str!("../locales/pt-BR.ftl")),
            _ => {
                return Err(PanelError::Messages(format!(
                    "unsupported language: {language}"
                )));
            }
        };
        let resource = FluentResource::try_new(source.to_owned())
            .map_err(|(_, errors)| PanelError::Messages(format!("{errors:?}")))?;
        let mut bundle = FluentBundle::new(vec![id]);
        bundle.set_use_isolating(false);
        bundle
            .add_resource(resource)
            .map_err(|errors| PanelError::Messages(format!("{errors:?}")))?;
        Ok(Self(bundle))
    }

    /// Resolves an interface message from the selected language.
    #[must_use]
    pub fn get(&self, key: &str) -> String {
        let Some(pattern) = self.0.get_message(key).and_then(|message| message.value()) else {
            return key.to_owned();
        };
        self.0
            .format_pattern(pattern, None, &mut Vec::new())
            .into_owned()
    }

    pub(crate) fn values(&self, key: &str, values: &[(&str, String)]) -> String {
        let Some(pattern) = self.0.get_message(key).and_then(|message| message.value()) else {
            return key.to_owned();
        };
        let mut args = fluent_bundle::FluentArgs::new();
        for (name, value) in values {
            args.set(*name, value.as_str());
        }
        self.0
            .format_pattern(pattern, Some(&args), &mut Vec::new())
            .into_owned()
    }
}
