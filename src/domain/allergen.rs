//! The 14 allergens that must be declared in the EU (Regulation 1169/2011, Annex II).

use serde::{Deserialize, Serialize};

use super::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Allergen {
    Gluten,
    Crustaceans,
    Eggs,
    Fish,
    Peanuts,
    Soy,
    Dairy,
    TreeNuts,
    Celery,
    Mustard,
    Sesame,
    Sulphites,
    Lupin,
    Molluscs,
}

impl Allergen {
    /// All allergens in their official numbering order.
    pub const ALL: [Allergen; 14] = [
        Allergen::Gluten,
        Allergen::Crustaceans,
        Allergen::Eggs,
        Allergen::Fish,
        Allergen::Peanuts,
        Allergen::Soy,
        Allergen::Dairy,
        Allergen::TreeNuts,
        Allergen::Celery,
        Allergen::Mustard,
        Allergen::Sesame,
        Allergen::Sulphites,
        Allergen::Lupin,
        Allergen::Molluscs,
    ];

    /// 1-based number used on the printed menu.
    pub fn number(self) -> u8 {
        Self::ALL.iter().position(|a| *a == self).unwrap_or(0) as u8 + 1
    }

    pub fn from_index(index: usize) -> Option<Allergen> {
        Self::ALL.get(index).copied()
    }

    pub fn index(self) -> usize {
        self.number() as usize - 1
    }

    /// Name printed in the menu legend.
    pub fn label(self, language: Language) -> &'static str {
        match language {
            Language::Es => self.label_es(),
            Language::En => self.label_en(),
        }
    }

    fn label_es(self) -> &'static str {
        match self {
            Allergen::Gluten => "Gluten",
            Allergen::Crustaceans => "Crustáceos",
            Allergen::Eggs => "Huevos",
            Allergen::Fish => "Pescado",
            Allergen::Peanuts => "Cacahuetes",
            Allergen::Soy => "Soja",
            Allergen::Dairy => "Lácteos",
            Allergen::TreeNuts => "Frutos de cáscara",
            Allergen::Celery => "Apio",
            Allergen::Mustard => "Mostaza",
            Allergen::Sesame => "Sésamo",
            Allergen::Sulphites => "Sulfitos",
            Allergen::Lupin => "Altramuces",
            Allergen::Molluscs => "Moluscos",
        }
    }

    fn label_en(self) -> &'static str {
        match self {
            Allergen::Gluten => "Gluten",
            Allergen::Crustaceans => "Crustaceans",
            Allergen::Eggs => "Eggs",
            Allergen::Fish => "Fish",
            Allergen::Peanuts => "Peanuts",
            Allergen::Soy => "Soy",
            Allergen::Dairy => "Milk",
            Allergen::TreeNuts => "Tree nuts",
            Allergen::Celery => "Celery",
            Allergen::Mustard => "Mustard",
            Allergen::Sesame => "Sesame",
            Allergen::Sulphites => "Sulphites",
            Allergen::Lupin => "Lupin",
            Allergen::Molluscs => "Molluscs",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_is_one_based_and_stable() {
        assert_eq!(Allergen::Gluten.number(), 1);
        assert_eq!(Allergen::Molluscs.number(), 14);
        for (i, a) in Allergen::ALL.iter().enumerate() {
            assert_eq!(Allergen::from_index(i), Some(*a));
            assert_eq!(a.index(), i);
            assert!(!a.label(Language::Es).is_empty() && !a.label(Language::En).is_empty());
        }
        assert_eq!(Allergen::Dairy.label(Language::Es), "Lácteos");
        assert_eq!(Allergen::Dairy.label(Language::En), "Milk");
    }
}
