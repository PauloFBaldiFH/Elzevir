// SPDX-License-Identifier: GPL-3.0-or-later
//! What kind of resource a record is, read from its description: the content of the Work
//! and the carrier of the Instance, each as one of the categories that have an icon
//! (docs/specs/catalogo/bibframe-completo.md, items 14 and 15).

use elzevir_domain::statement::{Description, Value};

/// The kind of content, after the Work's classes and `bf:content`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentKind {
    /// `bf:Text`, `txt`.
    Text,
    /// `bf:NotatedMusic`, `ntm`.
    NotatedMusic,
    /// `bf:Audio`, spoken word, performed music, sounds.
    Audio,
    /// `bf:MovingImage`.
    MovingImage,
    /// `bf:StillImage`.
    StillImage,
    /// `bf:Serial`.
    Serial,
    /// Tactile content (`tct` and the other tactile types) or a `bf:Tactile` Instance.
    Tactile,
    /// `bf:Cartography`.
    Cartography,
    /// `bf:Object`, three-dimensional form.
    Object,
    /// `bf:Multimedia` and `bf:Kit`, computer program.
    Multimedia,
    /// `bf:MixedMaterial`.
    Mixed,
    /// `bf:Dataset`, computer dataset.
    Dataset,
}

/// The kind of carrier, after the Instance's classes and `bf:carrier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierKind {
    /// `bf:Print`, volume.
    Print,
    /// `bf:Electronic`, online resource.
    Online,
    /// Audio disc, videodisc.
    Disc,
}

/// The content and carrier of a record; either may be unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Material {
    /// The Work's content.
    pub content: Option<ContentKind>,
    /// The Instance's carrier.
    pub carrier: Option<CarrierKind>,
}

const CONTENT_KINDS: [ContentKind; 12] = [
    ContentKind::Text,
    ContentKind::NotatedMusic,
    ContentKind::Audio,
    ContentKind::MovingImage,
    ContentKind::StillImage,
    ContentKind::Serial,
    ContentKind::Tactile,
    ContentKind::Cartography,
    ContentKind::Object,
    ContentKind::Multimedia,
    ContentKind::Mixed,
    ContentKind::Dataset,
];

const CARRIER_KINDS: [CarrierKind; 3] =
    [CarrierKind::Print, CarrierKind::Online, CarrierKind::Disc];

impl ContentKind {
    /// Every kind, in a fixed order.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &CONTENT_KINDS
    }

    /// A stable name, for the search index.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::NotatedMusic => "notated-music",
            Self::Audio => "audio",
            Self::MovingImage => "moving-image",
            Self::StillImage => "still-image",
            Self::Serial => "serial",
            Self::Tactile => "tactile",
            Self::Cartography => "cartography",
            Self::Object => "object",
            Self::Multimedia => "multimedia",
            Self::Mixed => "mixed",
            Self::Dataset => "dataset",
        }
    }

    /// The kind with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        CONTENT_KINDS.into_iter().find(|kind| kind.name() == name)
    }

    /// The kind a subclass of `bf:Work` stands for, if any.
    #[must_use]
    pub fn of_class(class: &str) -> Option<Self> {
        Some(match class {
            "Serial" => Self::Serial,
            "Text" => Self::Text,
            "NotatedMusic" => Self::NotatedMusic,
            "Audio" | "MusicAudio" | "NonMusicAudio" => Self::Audio,
            "MovingImage" => Self::MovingImage,
            "StillImage" => Self::StillImage,
            "Cartography" => Self::Cartography,
            "Object" => Self::Object,
            "Multimedia" | "Kit" => Self::Multimedia,
            "MixedMaterial" => Self::Mixed,
            "Dataset" => Self::Dataset,
            _ => return None,
        })
    }

    /// The kind a `contentTypes` code stands for, if any.
    #[must_use]
    pub fn of_content(code: &str) -> Option<Self> {
        Some(match code {
            "tct" | "tci" | "tcm" | "tcn" | "tcf" => Self::Tactile,
            "txt" => Self::Text,
            "ntm" => Self::NotatedMusic,
            "spw" | "prm" | "snd" => Self::Audio,
            "tdi" | "tdm" => Self::MovingImage,
            "sti" => Self::StillImage,
            "cri" | "crd" | "crf" | "crm" => Self::Cartography,
            "tdf" => Self::Object,
            "cop" => Self::Multimedia,
            "cod" => Self::Dataset,
            _ => return None,
        })
    }
}

impl CarrierKind {
    /// Every kind, in a fixed order.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &CARRIER_KINDS
    }

    /// A stable name, for the search index.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::Online => "online",
            Self::Disc => "disc",
        }
    }

    /// The kind with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        CARRIER_KINDS.into_iter().find(|kind| kind.name() == name)
    }

    /// The kind a subclass of `bf:Instance` stands for, if any.
    #[must_use]
    pub fn of_class(class: &str) -> Option<Self> {
        match class {
            "Print" => Some(Self::Print),
            "Electronic" => Some(Self::Online),
            _ => None,
        }
    }

    /// The kind a `carriers` code stands for, if any.
    #[must_use]
    pub fn of_carrier(code: &str) -> Option<Self> {
        match code {
            "nc" => Some(Self::Print),
            "cr" => Some(Self::Online),
            "sd" | "vd" => Some(Self::Disc),
            _ => None,
        }
    }
}

/// The content and carrier that `work` and `instance` describe. Tactile content and
/// serials win, since their icon says more than the class beside them; otherwise the first
/// class with a kind, then the first `bf:content` with one.
#[must_use]
pub fn material_of(work: &Description, instance: &Description) -> Material {
    Material {
        content: content_of(work, instance),
        carrier: carrier_of(instance),
    }
}

fn content_of(work: &Description, instance: &Description) -> Option<ContentKind> {
    let contents = || concepts(work, "content", "contentTypes");
    let has_class = |description: &Description, class: &str| {
        description.classes.iter().any(|name| name == class)
    };
    if has_class(instance, "Tactile") || contents().any(|code| code.starts_with("tc")) {
        return Some(ContentKind::Tactile);
    }
    if has_class(work, "Serial") {
        return Some(ContentKind::Serial);
    }
    work.classes
        .iter()
        .find_map(|class| ContentKind::of_class(class))
        .or_else(|| contents().find_map(ContentKind::of_content))
}

fn carrier_of(instance: &Description) -> Option<CarrierKind> {
    let by_class = instance
        .classes
        .iter()
        .find_map(|class| match class.as_str() {
            "Print" => Some(CarrierKind::Print),
            "Electronic" => Some(CarrierKind::Online),
            _ => None,
        });
    by_class.or_else(|| {
        concepts(instance, "carrier", "carriers").find_map(|code| match code {
            "nc" => Some(CarrierKind::Print),
            "cr" => Some(CarrierKind::Online),
            "sd" | "vd" => Some(CarrierKind::Disc),
            _ => None,
        })
    })
}

/// The codes of the concepts of `scheme` that `description` gives for `property`.
fn concepts<'a>(
    description: &'a Description,
    property: &'a str,
    scheme: &'a str,
) -> impl Iterator<Item = &'a str> {
    description
        .statements
        .iter()
        .filter(move |statement| statement.property == property)
        .filter_map(move |statement| match &statement.value {
            Value::Concept { scheme: s, code } if s == scheme => Some(code.as_str()),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::description::concept;

    fn described(classes: &[&str], statements: &[(&str, &str, &str)]) -> Description {
        Description {
            classes: classes.iter().map(|class| (*class).to_owned()).collect(),
            statements: statements
                .iter()
                .map(|(property, scheme, code)| concept(property, scheme, code))
                .collect(),
        }
    }

    #[test]
    fn the_content_comes_from_the_classes_then_from_the_content_types() {
        let none = Description::default();
        let of = |work: &Description, instance: &Description| material_of(work, instance);
        assert_eq!(
            of(&described(&["Monograph", "Text"], &[]), &none).content,
            Some(ContentKind::Text)
        );
        assert_eq!(
            of(&described(&["Text", "Serial"], &[]), &none).content,
            Some(ContentKind::Serial)
        );
        assert_eq!(
            of(
                &described(&[], &[("content", "contentTypes", "spw")]),
                &none
            )
            .content,
            Some(ContentKind::Audio)
        );
        assert_eq!(
            of(
                &described(&["Text"], &[("content", "contentTypes", "tct")]),
                &none
            )
            .content,
            Some(ContentKind::Tactile)
        );
        assert_eq!(
            of(&described(&["Text"], &[]), &described(&["Tactile"], &[])).content,
            Some(ContentKind::Tactile)
        );
        assert_eq!(of(&none, &none), Material::default());
    }

    #[test]
    fn the_carrier_comes_from_the_class_then_from_the_carrier_types() {
        let none = Description::default();
        let carrier = |instance: &Description| material_of(&none, instance).carrier;
        assert_eq!(
            carrier(&described(&["Print"], &[])),
            Some(CarrierKind::Print)
        );
        assert_eq!(
            carrier(&described(&[], &[("carrier", "carriers", "cr")])),
            Some(CarrierKind::Online)
        );
        assert_eq!(
            carrier(&described(&[], &[("carrier", "carriers", "sd")])),
            Some(CarrierKind::Disc)
        );
        assert_eq!(
            carrier(&described(&[], &[("carrier", "carriers", "nb")])),
            None
        );
    }

    #[test]
    fn every_kind_has_a_name_that_comes_back() {
        for kind in ContentKind::all() {
            assert_eq!(ContentKind::from_name(kind.name()), Some(*kind));
        }
        for kind in CarrierKind::all() {
            assert_eq!(CarrierKind::from_name(kind.name()), Some(*kind));
        }
    }
}
