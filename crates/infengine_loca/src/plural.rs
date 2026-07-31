//! A11.1: counting things in a language that does not count like English.
//!
//! "1 věc, 3 věci, 5 věcí" is not an edge case, it is how most of the
//! world's languages work, and the number of forms differs per language:
//! English has two, Czech four, Arabic six. Picking the form is a solved
//! problem with a published answer - the Unicode CLDR plural rules - so
//! this module does not invent one. It wraps ICU4X, the Unicode
//! consortium's own implementation, with the CLDR data compiled in.
//!
//! What matters for authoring: the *translator* decides how many forms
//! their language needs. An English file supplies `one` and `other`; the
//! Czech translator adds `few` without the English file, the room data,
//! or a line of code changing. A language that needs no extra forms
//! writes none.

use std::fmt;

use icu_locale_core::Locale;
use icu_plurals::{PluralCategory, PluralRules};
use serde::{Deserialize, Serialize};

/// A CLDR plural category.
///
/// Mirrored rather than re-exported so the file format does not move
/// when the underlying library does, and so it can derive the traits a
/// RON map key needs.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Category {
    Zero,
    One,
    Two,
    Few,
    Many,
    /// The catch-all every language has. A line with no other form uses
    /// this one.
    #[default]
    Other,
}

impl Category {
    /// All categories, in CLDR's own order.
    pub const ALL: [Category; 6] = [
        Category::Zero,
        Category::One,
        Category::Two,
        Category::Few,
        Category::Many,
        Category::Other,
    ];

    /// CLDR's name for the category, used as the key in a locale file.
    pub fn name(self) -> &'static str {
        match self {
            Category::Zero => "zero",
            Category::One => "one",
            Category::Two => "two",
            Category::Few => "few",
            Category::Many => "many",
            Category::Other => "other",
        }
    }

    fn from_icu(category: PluralCategory) -> Self {
        match category {
            PluralCategory::Zero => Category::Zero,
            PluralCategory::One => Category::One,
            PluralCategory::Two => Category::Two,
            PluralCategory::Few => Category::Few,
            PluralCategory::Many => Category::Many,
            PluralCategory::Other => Category::Other,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Parse a language tag, falling back to English.
///
/// An unknown or malformed tag must not take the game down, and English
/// rules at least produce a grammatical sentence for the many languages
/// that share its one/other split.
fn locale_of(language: &str) -> Locale {
    language
        .parse::<Locale>()
        .unwrap_or_else(|_| "en".parse().expect("en is a valid tag"))
}

/// Which form `count` takes in `language`.
pub fn category_for(language: &str, count: u64) -> Category {
    let rules = PluralRules::try_new_cardinal(locale_of(language).into())
        .unwrap_or_else(|_| {
            PluralRules::try_new_cardinal(locale_of("en").into())
                .expect("English rules are compiled in")
        });
    Category::from_icu(rules.category_for(count))
}

/// Which form an ordinal takes: "1st, 2nd, 3rd" and their equivalents.
pub fn ordinal_category_for(language: &str, count: u64) -> Category {
    let rules = PluralRules::try_new_ordinal(locale_of(language).into())
        .unwrap_or_else(|_| {
            PluralRules::try_new_ordinal(locale_of("en").into())
                .expect("English rules are compiled in")
        });
    Category::from_icu(rules.category_for(count))
}

/// The categories a language actually uses, so the editor can show a
/// translator exactly the fields their language needs and no others.
pub fn categories_used(language: &str) -> Vec<Category> {
    let mut used: Vec<Category> = Vec::new();
    // Sampling covers every rule CLDR expresses for the languages a game
    // ships: small integers separate one/two/few/many, and the larger
    // values catch the teen and hundred exceptions some languages have.
    for n in (0..=120u64).chain([200, 1000, 1001, 10_000, 100_000, 1_000_000]) {
        let category = category_for(language, n);
        if !used.contains(&category) {
            used.push(category);
        }
    }
    used.sort();
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_splits_one_from_the_rest() {
        assert_eq!(category_for("en", 1), Category::One);
        assert_eq!(category_for("en", 0), Category::Other);
        assert_eq!(category_for("en", 2), Category::Other);
        assert_eq!(category_for("en", 100), Category::Other);
    }

    #[test]
    fn czech_needs_the_form_english_does_not_have() {
        // 1 věc / 3 věci / 5 věcí - the case that started this.
        assert_eq!(category_for("cs", 1), Category::One);
        assert_eq!(category_for("cs", 2), Category::Few);
        assert_eq!(category_for("cs", 3), Category::Few);
        assert_eq!(category_for("cs", 4), Category::Few);
        assert_eq!(category_for("cs", 5), Category::Other);
        assert_eq!(category_for("cs", 0), Category::Other);
    }

    #[test]
    fn polish_differs_from_czech_despite_being_close() {
        // Polish puts most large numbers in `many`, Czech in `other`.
        // Hardcoding one Slavic rule set for both would be wrong.
        assert_eq!(category_for("pl", 2), Category::Few);
        assert_eq!(category_for("pl", 5), Category::Many);
        assert_ne!(category_for("pl", 5), category_for("cs", 5));
    }

    #[test]
    fn russian_and_czech_also_differ() {
        assert_eq!(category_for("ru", 1), Category::One);
        assert_eq!(category_for("ru", 21), Category::One, "21 behaves like 1");
        assert_eq!(category_for("cs", 21), Category::Other);
    }

    #[test]
    fn japanese_has_a_single_form() {
        for n in [0, 1, 2, 5, 100] {
            assert_eq!(category_for("ja", n), Category::Other);
        }
        assert_eq!(categories_used("ja"), vec![Category::Other]);
    }

    #[test]
    fn arabic_uses_the_categories_european_languages_never_touch() {
        let used = categories_used("ar");
        assert!(used.contains(&Category::Zero));
        assert!(used.contains(&Category::Two));
        assert!(used.len() > 4, "got {used:?}");
    }

    #[test]
    fn categories_used_reports_what_a_translator_must_fill_in() {
        assert_eq!(
            categories_used("en"),
            vec![Category::One, Category::Other]
        );
        assert_eq!(
            categories_used("cs"),
            vec![Category::One, Category::Few, Category::Other],
            "the `many` form is only for fractions, which counts never hit"
        );
    }

    #[test]
    fn an_unknown_language_falls_back_instead_of_failing() {
        // A typo in a language tag must not take the game down.
        assert_eq!(category_for("not-a-language!!", 1), Category::One);
        assert_eq!(category_for("", 5), Category::Other);
    }

    #[test]
    fn ordinals_are_ruled_separately_from_cardinals() {
        // English: 1st, 2nd, 3rd, 4th - four ordinal forms against two
        // cardinal ones.
        assert_eq!(ordinal_category_for("en", 1), Category::One);
        assert_eq!(ordinal_category_for("en", 2), Category::Two);
        assert_eq!(ordinal_category_for("en", 3), Category::Few);
        assert_eq!(ordinal_category_for("en", 4), Category::Other);
        assert_eq!(category_for("en", 2), Category::Other, "cardinal differs");
    }

    #[test]
    fn category_names_match_cldr() {
        assert_eq!(Category::Few.name(), "few");
        assert_eq!(Category::default(), Category::Other);
        assert_eq!(Category::ALL.len(), 6);
        assert_eq!(Category::Many.to_string(), "many");
    }
}
