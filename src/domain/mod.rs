//! Pure data model of a menu. No I/O, no UI, no rendering.

pub mod allergen;
pub mod language;
pub mod menu;
pub mod price;
pub mod style;

pub use allergen::Allergen;
pub use language::Language;
pub use menu::{Direction, Header, LogoSource, MenuDocument, Product, Section};
pub use style::*;
