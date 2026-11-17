//! Menu content: header, sections and dishes, plus the editing operations
//! the UI performs on them. Kept free of any UI or rendering concerns.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::allergen::Allergen;
use super::style::MenuStyle;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "path")]
pub enum LogoSource {
    /// The default logo. In a menu: the logo set in the app settings.
    /// In the app settings: the Casa Peix logo bundled with the application.
    #[default]
    #[serde(alias = "builtin")]
    Default,
    /// A PNG / JPEG / WebP / GIF picked by the user.
    File(PathBuf),
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Header {
    pub title: String,
    pub subtitle: String,
    pub logo: LogoSource,
}

impl Default for Header {
    fn default() -> Self {
        Self { title: "Carta del Jardín".into(), subtitle: String::new(), logo: LogoSource::Default }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Product {
    pub name: String,
    pub description: String,
    /// Free text; numbers are formatted as prices (see `domain::price`).
    pub price: String,
    pub allergens: BTreeSet<Allergen>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Section {
    pub name: String,
    /// Optional line under the title, e.g. "Para compartir".
    pub note: String,
    pub products: Vec<Product>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuDocument {
    pub header: Header,
    pub sections: Vec<Section>,
    pub footer: String,
    pub style: MenuStyle,
}

impl Default for MenuDocument {
    fn default() -> Self {
        Self {
            header: Header::default(),
            sections: Vec::new(),
            footer: "IVA incluido. Si tiene alguna alergia o intolerancia, consulte a nuestro personal."
                .into(),
            style: MenuStyle::default(),
        }
    }
}

/// Which way to move an item in a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

fn move_item<T>(items: &mut [T], index: usize, dir: Direction) -> bool {
    match dir {
        Direction::Up if index > 0 && index < items.len() => {
            items.swap(index, index - 1);
            true
        }
        Direction::Down if index + 1 < items.len() => {
            items.swap(index, index + 1);
            true
        }
        _ => false,
    }
}

impl MenuDocument {
    /// Empty menu with a single section, used by "New menu".
    pub fn blank(section_name: &str) -> Self {
        let mut doc = Self::default();
        doc.add_section(section_name);
        doc
    }

    pub fn add_section(&mut self, name: &str) -> usize {
        self.sections.push(Section { name: name.into(), ..Default::default() });
        self.sections.len() - 1
    }

    pub fn remove_section(&mut self, index: usize) -> bool {
        if index < self.sections.len() {
            self.sections.remove(index);
            true
        } else {
            false
        }
    }

    pub fn move_section(&mut self, index: usize, dir: Direction) -> bool {
        move_item(&mut self.sections, index, dir)
    }

    pub fn add_product(&mut self, section: usize, name: &str) -> Option<usize> {
        let s = self.sections.get_mut(section)?;
        s.products.push(Product { name: name.into(), ..Default::default() });
        Some(s.products.len() - 1)
    }

    pub fn remove_product(&mut self, section: usize, product: usize) -> bool {
        match self.sections.get_mut(section) {
            Some(s) if product < s.products.len() => {
                s.products.remove(product);
                true
            }
            _ => false,
        }
    }

    pub fn move_product(&mut self, section: usize, product: usize, dir: Direction) -> bool {
        self.sections
            .get_mut(section)
            .is_some_and(|s| move_item(&mut s.products, product, dir))
    }

    pub fn product_mut(&mut self, section: usize, product: usize) -> Option<&mut Product> {
        self.sections.get_mut(section)?.products.get_mut(product)
    }

    pub fn toggle_allergen(&mut self, section: usize, product: usize, allergen: Allergen) -> bool {
        let Some(p) = self.product_mut(section, product) else { return false };
        if !p.allergens.remove(&allergen) {
            p.allergens.insert(allergen);
        }
        true
    }

    /// Allergens present on at least one dish, in official order.
    pub fn used_allergens(&self) -> BTreeSet<Allergen> {
        self.sections
            .iter()
            .flat_map(|s| s.products.iter())
            .flat_map(|p| p.allergens.iter().copied())
            .collect()
    }

    /// A complete example menu so the app never starts empty.
    pub fn sample() -> Self {
        use Allergen::*;
        fn p(name: &str, description: &str, price: &str, allergens: &[Allergen]) -> Product {
            Product {
                name: name.into(),
                description: description.into(),
                price: price.into(),
                allergens: allergens.iter().copied().collect(),
            }
        }
        let sections = vec![
            Section {
                name: "Para picar".into(),
                note: "Ideales para compartir".into(),
                products: vec![
                    p("Croquetas de jamón ibérico", "Bechamel cremosa y rebozado crujiente (6 uds.)", "9,50", &[Gluten, Eggs, Dairy]),
                    p("Patatas bravas", "Salsa brava de la casa y alioli suave", "6,50", &[Eggs]),
                    p("Pan de cristal con tomate", "Tomate rallado, aceite de oliva virgen extra y sal en escamas", "4", &[Gluten]),
                    p("Mejillones al vapor", "Con limón y laurel", "11", &[Molluscs]),
                ],
            },
            Section {
                name: "Ensaladas".into(),
                note: String::new(),
                products: vec![
                    p("Ensalada del Jardín", "Brotes tiernos, tomate cherry, aguacate y vinagreta de cítricos", "11,50", &[Mustard]),
                    p("Burrata con tomate rosa", "Pesto de albahaca y piñones tostados", "13,90", &[Dairy, TreeNuts]),
                ],
            },
            Section {
                name: "Del mar".into(),
                note: String::new(),
                products: vec![
                    p("Arroz negro", "Sepia, alioli y un toque de pimentón (mín. 2 personas, precio por persona)", "17,50", &[Molluscs, Crustaceans, Eggs]),
                    p("Lubina a la brasa", "Verduras de temporada y aceite de hierbas", "21", &[Fish]),
                    p("Pulpo a la gallega", "Patata cachelo, pimentón de la Vera", "19,50", &[Molluscs]),
                ],
            },
            Section {
                name: "De la tierra".into(),
                note: String::new(),
                products: vec![
                    p("Entrecot de vaca madurada", "300 g, con patatas y pimientos de Padrón", "24", &[]),
                    p("Hamburguesa Casa Peix", "Ternera, queso curado, cebolla caramelizada y pan brioche", "15,50", &[Gluten, Dairy, Eggs, Sesame]),
                ],
            },
            Section {
                name: "Postres".into(),
                note: String::new(),
                products: vec![
                    p("Crema catalana", "Receta de la abuela", "6", &[Eggs, Dairy]),
                    p("Tarta de queso", "Al horno, con frutos rojos", "6,50", &[Gluten, Eggs, Dairy]),
                    p("Sorbete de limón", "Con cava", "5,50", &[Sulphites]),
                ],
            },
        ];
        Self { sections, ..Self::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_operations() {
        let mut doc = MenuDocument::blank("A");
        let s = doc.add_section("B");
        assert_eq!(s, 1);
        let p = doc.add_product(s, "Dish").unwrap();
        doc.toggle_allergen(s, p, Allergen::Fish);
        assert!(doc.used_allergens().contains(&Allergen::Fish));
        doc.toggle_allergen(s, p, Allergen::Fish);
        assert!(doc.used_allergens().is_empty());
        assert!(doc.move_section(1, Direction::Up));
        assert!(!doc.move_section(0, Direction::Up));
        assert!(doc.remove_section(0));
        assert_eq!(doc.sections.len(), 1);
    }

    #[test]
    fn json_roundtrip_and_forward_compat() {
        let doc = MenuDocument::sample();
        let json = serde_json::to_string(&doc).unwrap();
        let back: MenuDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        // Unknown/missing fields must not break loading.
        let partial: MenuDocument = serde_json::from_str(r#"{"header":{"title":"X"}}"#).unwrap();
        assert_eq!(partial.header.title, "X");
        assert!(partial.style.show_allergen_legend);
        // Documents saved before the rename still load.
        let old: MenuDocument = serde_json::from_str(r#"{"header":{"logo":{"kind":"builtin"}}}"#).unwrap();
        assert_eq!(old.header.logo, LogoSource::Default);
    }
}
