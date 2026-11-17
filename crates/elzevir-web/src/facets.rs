// SPDX-License-Identifier: GPL-3.0-or-later
//! The facets strip of the staff listing (docs/specs/catalogo/facetas.md, part b): the
//! filters of a URL, the addresses that add or drop one, and each value's name.

use elzevir_app::catalog::CodeLists;
use elzevir_app::material::{CarrierKind, ContentKind};
use elzevir_app::search::{Facet, FacetCounts, Filter};
use elzevir_data::vocabulary;
use elzevir_domain::catalog::BranchId;
use elzevir_web_templates::{ActiveFilter, FacetMenu, FacetOption, FacetStrip, I18n, url_encode};

use crate::icons::{carrier_badge, content_badge};

/// The query parameter of a filter, written `facet:value`.
pub(crate) const FILTER_PARAM: &str = "f";

/// The filters among a URL's `f=facet:value` parameters (facetas.md, item 4).
pub(crate) fn filters_of(pairs: &[(String, String)]) -> Vec<Filter> {
    Filter::parse(
        pairs
            .iter()
            .filter(|(name, _)| name == FILTER_PARAM)
            .filter_map(|(_, value)| value.split_once(':')),
    )
}

/// The listing address for `query` and `filters`.
pub(crate) fn listing_url(query: &str, filters: &[Filter]) -> String {
    let mut url = format!("/equipe/acervo?q={}", url_encode(query));
    for filter in filters {
        url.push('&');
        url.push_str(FILTER_PARAM);
        url.push('=');
        url.push_str(&url_encode(&written(filter)));
    }
    url
}

/// A filter as its URL parameter value.
fn written(filter: &Filter) -> String {
    format!("{}:{}", filter.facet.name(), filter.value)
}

/// The message key of a facet's title.
const fn title_key(facet: Facet) -> &'static str {
    match facet {
        Facet::Branch => "facet-branch",
        Facet::Content => "facet-content",
        Facet::Carrier => "facet-carrier",
        Facet::Genre => "facet-genre",
        Facet::Audience => "facet-audience",
        Facet::Language => "facet-language",
        Facet::Subject => "facet-subject",
        Facet::Year => "facet-year",
    }
}

/// A value as a person reads it: a branch's name, a content's or carrier's label, a
/// language's name; the rest as indexed.
fn label(facet: Facet, value: &str, t: &I18n, codes: &CodeLists) -> String {
    let named = match facet {
        Facet::Branch => value
            .parse()
            .ok()
            .map(|id| codes.branch(BranchId(id)).to_owned()),
        Facet::Content => {
            ContentKind::from_name(value).map(|kind| t.get(content_badge(kind).label_key))
        }
        Facet::Carrier => {
            CarrierKind::from_name(value).map(|kind| t.get(carrier_badge(kind).label_key))
        }
        Facet::Language => vocabulary::scheme("languages")
            .and_then(|scheme| scheme.concept(value))
            .map(|concept| concept.label.to_owned()),
        Facet::Genre | Facet::Audience | Facet::Subject | Facet::Year => None,
    };
    named.unwrap_or_else(|| value.to_owned())
}

/// The strip for `counts` of `query` under `filters`.
pub(crate) fn strip(
    query: &str,
    filters: &[Filter],
    counts: FacetCounts,
    t: &I18n,
    codes: &CodeLists,
) -> FacetStrip {
    let active = filters
        .iter()
        .map(|filter| {
            let rest: Vec<Filter> = filters.iter().filter(|f| *f != filter).cloned().collect();
            ActiveFilter {
                facet_key: title_key(filter.facet),
                label: label(filter.facet, &filter.value, t, codes),
                remove_url: listing_url(query, &rest),
            }
        })
        .collect();
    let full = filters.len() >= elzevir_app::search::MAX_FILTERS;
    let menus = counts
        .facets
        .into_iter()
        .map(|(facet, values)| FacetMenu {
            name: facet.name(),
            title_key: title_key(facet),
            options: values
                .into_iter()
                .map(|value| {
                    let filter = Filter {
                        facet,
                        value: value.value,
                    };
                    let add_url = (!full && !filters.contains(&filter)).then(|| {
                        let mut more = filters.to_vec();
                        more.push(filter.clone());
                        listing_url(query, &more)
                    });
                    FacetOption {
                        label: label(facet, &filter.value, t, codes),
                        families: value.families,
                        add_url,
                    }
                })
                .collect(),
        })
        .collect();
    FacetStrip {
        families: counts.families,
        active,
        kept: filters.iter().map(written).collect(),
        clear_url: (!filters.is_empty()).then(|| listing_url(query, &[])),
        menus,
    }
}

#[cfg(test)]
mod tests {
    use elzevir_app::search::FacetValue;

    use super::*;

    fn pairs(raw: &[(&str, &str)]) -> Vec<(String, String)> {
        raw.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn filters_come_from_f_parameters_and_go_back_into_urls() {
        let filters = filters_of(&pairs(&[
            ("q", "dom"),
            ("f", "language:por"),
            ("f", "subject:Brasil: história"),
            ("f", "nocolon"),
            ("f", "itemtype:BK"),
        ]));
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[1].value, "Brasil: história");
        assert_eq!(
            listing_url("dom casmurro", &filters[..1]),
            "/equipe/acervo?q=dom%20casmurro&f=language%3Apor"
        );
    }

    #[test]
    fn the_strip_names_values_and_links_each_change() {
        let t = I18n::english().unwrap();
        let codes = CodeLists::default();
        let filters = filters_of(&pairs(&[("f", "language:por")]));
        let counts = FacetCounts {
            families: 3,
            facets: vec![(
                Facet::Language,
                vec![
                    FacetValue {
                        value: "por".to_owned(),
                        families: 3,
                    },
                    FacetValue {
                        value: "eng".to_owned(),
                        families: 1,
                    },
                ],
            )],
        };
        let strip = strip("", &filters, counts, &t, &codes);
        assert_eq!(strip.kept, ["language:por"]);
        assert_eq!(strip.active[0].label, "Portuguese");
        assert_eq!(strip.active[0].remove_url, "/equipe/acervo?q=");
        assert_eq!(strip.clear_url.as_deref(), Some("/equipe/acervo?q="));
        let options = &strip.menus[0].options;
        assert_eq!(options[0].add_url, None);
        assert_eq!(
            options[1].add_url.as_deref(),
            Some("/equipe/acervo?q=&f=language%3Apor&f=language%3Aeng")
        );
        assert_eq!(options[1].label, "English");
    }
}
