//! The document being edited plus view-only state (which sections and
//! dishes are expanded). Every structural operation keeps both in sync.

use menu_core::domain::{Allergen, Direction, MenuDocument};

pub struct EditorState {
    pub doc: MenuDocument,
    section_open: Vec<bool>,
    product_open: Vec<Vec<bool>>,
}

fn direction(delta: i32) -> Direction {
    if delta < 0 {
        Direction::Up
    } else {
        Direction::Down
    }
}

fn swap_for<T>(items: &mut [T], index: usize, dir: Direction) {
    match dir {
        Direction::Up if index > 0 && index < items.len() => items.swap(index, index - 1),
        Direction::Down if index + 1 < items.len() => items.swap(index, index + 1),
        _ => {}
    }
}

impl EditorState {
    pub fn new(doc: MenuDocument) -> Self {
        let mut state = Self {
            doc,
            section_open: Vec::new(),
            product_open: Vec::new(),
        };
        state.reset_view();
        state
    }

    /// Sections expanded, dishes collapsed.
    pub fn reset_view(&mut self) {
        self.section_open = vec![true; self.doc.sections.len()];
        self.product_open = self
            .doc
            .sections
            .iter()
            .map(|s| vec![false; s.products.len()])
            .collect();
    }

    pub fn replace(&mut self, doc: MenuDocument) {
        self.doc = doc;
        self.reset_view();
    }

    pub fn section_open(&self, si: usize) -> bool {
        self.section_open.get(si).copied().unwrap_or(true)
    }

    pub fn product_open(&self, si: usize, pi: usize) -> bool {
        self.product_open
            .get(si)
            .and_then(|s| s.get(pi))
            .copied()
            .unwrap_or(false)
    }

    pub fn add_section(&mut self, name: &str) {
        self.doc.add_section(name);
        self.section_open.push(true);
        self.product_open.push(Vec::new());
    }

    pub fn remove_section(&mut self, si: usize) -> bool {
        if !self.doc.remove_section(si) {
            return false;
        }
        self.section_open.remove(si);
        self.product_open.remove(si);
        true
    }

    pub fn move_section(&mut self, si: usize, delta: i32) -> bool {
        let dir = direction(delta);
        if !self.doc.move_section(si, dir) {
            return false;
        }
        swap_for(&mut self.section_open, si, dir);
        swap_for(&mut self.product_open, si, dir);
        true
    }

    pub fn toggle_section(&mut self, si: usize) {
        if let Some(open) = self.section_open.get_mut(si) {
            *open = !*open;
        }
    }

    pub fn add_product(&mut self, si: usize, name: &str) -> bool {
        if self.doc.add_product(si, name).is_none() {
            return false;
        }
        if let Some(open) = self.product_open.get_mut(si) {
            open.push(true);
        }
        if let Some(open) = self.section_open.get_mut(si) {
            *open = true;
        }
        true
    }

    pub fn remove_product(&mut self, si: usize, pi: usize) -> bool {
        if !self.doc.remove_product(si, pi) {
            return false;
        }
        if let Some(open) = self.product_open.get_mut(si) {
            open.remove(pi);
        }
        true
    }

    pub fn move_product(&mut self, si: usize, pi: usize, delta: i32) -> bool {
        let dir = direction(delta);
        if !self.doc.move_product(si, pi, dir) {
            return false;
        }
        if let Some(open) = self.product_open.get_mut(si) {
            swap_for(open, pi, dir);
        }
        true
    }

    pub fn toggle_product(&mut self, si: usize, pi: usize) {
        if let Some(open) = self.product_open.get_mut(si).and_then(|s| s.get_mut(pi)) {
            *open = !*open;
        }
    }

    pub fn toggle_allergen(&mut self, si: usize, pi: usize, index: usize) -> bool {
        Allergen::from_index(index).is_some_and(|a| self.doc.toggle_allergen(si, pi, a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_state_follows_moves() {
        let mut ed = EditorState::new(MenuDocument::sample());
        ed.toggle_section(0); // close first
        assert!(!ed.section_open(0));
        assert!(ed.move_section(0, 1));
        assert!(ed.section_open(0));
        assert!(!ed.section_open(1));

        ed.toggle_product(1, 0);
        assert!(ed.product_open(1, 0));
        assert!(ed.move_product(1, 0, 1));
        assert!(ed.product_open(1, 1));
        assert!(ed.remove_product(1, 1));
        assert!(!ed.product_open(1, 0));

        let n = ed.doc.sections.len();
        ed.add_section("S");
        assert!(ed.add_product(n, "D"));
        assert!(ed.product_open(n, 0));
        assert!(ed.remove_section(n));
        assert_eq!(ed.doc.sections.len(), n);
    }
}
