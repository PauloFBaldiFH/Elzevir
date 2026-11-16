// SPDX-License-Identifier: GPL-3.0-or-later
//! The icon and label of each categorical value (docs/specs/catalogo/bibframe-completo.md,
//! item 15): symbols of `assets/elzevir-icons.svg`, message keys of the Fluent files.

use elzevir_app::material::{CarrierKind, ContentKind, Material};
use elzevir_web_templates::Badge;

/// The icon of a family or Instance whose description names no content.
pub(crate) const NO_CONTENT: &str = "content-unknown";

/// The badge of a kind of content.
pub(crate) const fn content_badge(kind: ContentKind) -> Badge {
    let (icon, label_key) = match kind {
        ContentKind::Text => ("content-text", "content-text"),
        ContentKind::NotatedMusic => ("content-notated-music", "content-notated-music"),
        ContentKind::Audio => ("content-audio", "content-audio"),
        ContentKind::MovingImage => ("content-moving-image", "content-moving-image"),
        ContentKind::StillImage => ("content-still-image", "content-still-image"),
        ContentKind::Serial => ("content-serial", "content-serial"),
        ContentKind::Tactile => ("content-tactile", "content-tactile"),
        ContentKind::Cartography => ("content-cartography", "content-cartography"),
        ContentKind::Object => ("content-object", "content-object"),
        // Both are made of several parts: the same layers, with their own label.
        ContentKind::Multimedia => ("content-multimedia", "content-multimedia"),
        ContentKind::Mixed => ("content-multimedia", "content-mixed"),
        ContentKind::Dataset => ("content-dataset", "content-dataset"),
    };
    Badge { icon, label_key }
}

/// The badge of a kind of carrier.
pub(crate) const fn carrier_badge(kind: CarrierKind) -> Badge {
    let (icon, label_key) = match kind {
        CarrierKind::Print => ("carrier-print", "carrier-print"),
        CarrierKind::Online => ("carrier-online", "carrier-online"),
        CarrierKind::Disc => ("carrier-disc", "carrier-disc"),
    };
    Badge { icon, label_key }
}

/// The icon of a subclass of `bf:Work` or `bf:Instance` in a select, if it has one.
pub(crate) fn class_icon(class: &str) -> Option<&'static str> {
    if class == "Tactile" {
        return Some(content_badge(ContentKind::Tactile).icon);
    }
    ContentKind::of_class(class)
        .map(|kind| content_badge(kind).icon)
        .or_else(|| CarrierKind::of_class(class).map(|kind| carrier_badge(kind).icon))
}

/// The icon of a concept in a select, written `scheme/code`, if it has one.
pub(crate) fn concept_icon(value: &str) -> Option<&'static str> {
    match value.split_once('/')? {
        ("contentTypes", code) => {
            ContentKind::of_content(code).map(|kind| content_badge(kind).icon)
        }
        ("carriers", code) => CarrierKind::of_carrier(code).map(|kind| carrier_badge(kind).icon),
        _ => None,
    }
}

/// The badges of a record's content and carrier, in that order.
pub(crate) fn material_badges(material: Material) -> Vec<Badge> {
    material
        .content
        .map(content_badge)
        .into_iter()
        .chain(material.carrier.map(carrier_badge))
        .collect()
}

/// The icon an Item of this record shows: its carrier's, else its content's.
pub(crate) fn item_icon(material: Material) -> &'static str {
    material
        .carrier
        .map(|kind| carrier_badge(kind).icon)
        .or_else(|| material.content.map(|kind| content_badge(kind).icon))
        .unwrap_or(NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_and_concepts_in_selects_have_the_icon_of_their_kind() {
        assert_eq!(class_icon("NotatedMusic"), Some("content-notated-music"));
        assert_eq!(class_icon("Electronic"), Some("carrier-online"));
        assert_eq!(class_icon("Tactile"), Some("content-tactile"));
        assert_eq!(class_icon("Monograph"), None);
        assert_eq!(concept_icon("contentTypes/spw"), Some("content-audio"));
        assert_eq!(concept_icon("carriers/vd"), Some("carrier-disc"));
        assert_eq!(concept_icon("mediaTypes/n"), None);
    }

    const ICONS: &str = include_str!("../assets/elzevir-icons.svg");

    #[test]
    fn every_badge_has_its_symbol_and_its_label() {
        let en = include_str!("../../elzevir-web-templates/locales/en/main.ftl");
        let badges = ContentKind::all()
            .iter()
            .map(|kind| content_badge(*kind))
            .chain(CarrierKind::all().iter().map(|kind| carrier_badge(*kind)));
        for badge in badges {
            assert!(
                ICONS.contains(&format!("id=\"{}\"", badge.icon)),
                "{}",
                badge.icon
            );
            assert!(
                en.contains(&format!("\n{} = ", badge.label_key)),
                "{}",
                badge.label_key
            );
        }
        assert!(ICONS.contains(&format!("id=\"{NO_CONTENT}\"")));
    }
}
