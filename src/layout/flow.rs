//! Distributes blocks over columns.
//!
//! Blocks are grouped into *units* that must stay together: a section title
//! always travels with its first dish, and with `keep_together` a whole
//! section is one unit when it fits in a column.

use std::ops::Range;

use super::blocks::Block;

const EPSILON: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub column: usize,
    /// Top of the block relative to the top of the column.
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct Flowed {
    pub placements: Vec<Placement>,
    /// Height of the tallest column.
    pub used_height: f32,
    pub columns_used: usize,
}

#[derive(Debug, Clone)]
struct Unit {
    blocks: Range<usize>,
    height: f32,
    space_before: f32,
}

fn unit_of(blocks: &[Block], range: Range<usize>) -> Unit {
    let mut height = 0.0;
    for (i, block) in blocks[range.clone()].iter().enumerate() {
        if i > 0 {
            height += block.space_before;
        }
        height += block.height;
    }
    Unit {
        space_before: blocks[range.start].space_before,
        blocks: range,
        height,
    }
}

fn build_units(
    blocks: &[Block],
    sections: &[Range<usize>],
    column_height: f32,
    keep_together: bool,
) -> Vec<Unit> {
    let mut units = Vec::new();
    for section in sections {
        if section.is_empty() {
            continue;
        }
        let whole = unit_of(blocks, section.clone());
        if keep_together && whole.height <= column_height + EPSILON {
            units.push(whole);
            continue;
        }
        // Title + first dish stay together; every other dish is free.
        let head_end = (section.start + 2).min(section.end);
        units.push(unit_of(blocks, section.start..head_end));
        for i in head_end..section.end {
            units.push(unit_of(blocks, i..i + 1));
        }
    }
    units
}

/// Places blocks into at most `columns` columns of `column_height`.
/// Returns `None` when they do not fit, unless `force` is set, in which case
/// everything that does not fit is stacked in the last column.
pub fn flow(
    blocks: &[Block],
    sections: &[Range<usize>],
    columns: usize,
    column_height: f32,
    keep_together: bool,
    force: bool,
) -> Option<Flowed> {
    let columns = columns.max(1);
    let units = build_units(blocks, sections, column_height, keep_together);
    let mut placements = vec![Placement { column: 0, y: 0.0 }; blocks.len()];
    let mut column = 0;
    let mut y: f32 = 0.0;
    let mut used_height: f32 = 0.0;

    for unit in &units {
        let mut top = if y > 0.0 { y + unit.space_before } else { 0.0 };
        if top + unit.height > column_height + EPSILON && y > 0.0 && column + 1 < columns {
            column += 1;
            top = 0.0;
        }
        if top + unit.height > column_height + EPSILON && !force {
            return None;
        }
        // Lay out the unit's blocks.
        let mut cursor = top;
        for (i, index) in unit.blocks.clone().enumerate() {
            if i > 0 {
                cursor += blocks[index].space_before;
            }
            placements[index] = Placement { column, y: cursor };
            cursor += blocks[index].height;
        }
        y = cursor;
        used_height = used_height.max(y);
    }

    Some(Flowed {
        placements,
        used_height,
        columns_used: if units.is_empty() { 0 } else { column + 1 },
    })
}

/// Like [`flow`], but finds the shortest column height that still fits so
/// the columns come out balanced.
pub fn flow_balanced(
    blocks: &[Block],
    sections: &[Range<usize>],
    columns: usize,
    max_height: f32,
    keep_together: bool,
) -> Option<Flowed> {
    let best = flow(blocks, sections, columns, max_height, keep_together, false)?;
    if columns <= 1 {
        return Some(best);
    }
    let mut lo = 0.0_f32;
    let mut hi = max_height;
    let mut best = best;
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        match flow(blocks, sections, columns, mid, keep_together, false) {
            Some(result) => {
                hi = mid;
                best = result;
            }
            None => lo = mid,
        }
        if hi - lo < 0.5 {
            break;
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(h: f32, sb: f32) -> Block {
        Block {
            ops: vec![],
            height: h,
            space_before: sb,
        }
    }

    #[test]
    fn single_column_stacks_and_drops_leading_space() {
        let blocks = vec![block(10.0, 5.0), block(20.0, 4.0), block(20.0, 4.0)];
        let sections = std::iter::once(0..3).collect::<Vec<_>>();
        let r = flow(&blocks, &sections, 1, 100.0, true, false).unwrap();
        assert_eq!(r.placements[0].y, 0.0);
        assert_eq!(r.placements[1].y, 14.0);
        assert_eq!(r.used_height, 58.0);
    }

    #[test]
    fn too_tall_fails_unless_forced() {
        let blocks = vec![block(10.0, 0.0), block(200.0, 0.0)];
        let sections = std::iter::once(0..2).collect::<Vec<_>>();
        assert!(flow(&blocks, &sections, 2, 100.0, true, false).is_none());
        assert!(flow(&blocks, &sections, 2, 100.0, true, true).is_some());
    }

    #[test]
    fn balanced_two_columns() {
        // Two identical sections should end up one per column.
        let blocks = vec![
            block(10.0, 8.0),
            block(30.0, 2.0),
            block(10.0, 8.0),
            block(30.0, 2.0),
        ];
        let sections = vec![0..2, 2..4];
        let r = flow_balanced(&blocks, &sections, 2, 500.0, true).unwrap();
        assert_eq!(r.placements[2].column, 1);
        assert!((r.used_height - 42.0).abs() < 1.0);
    }

    #[test]
    fn title_never_orphaned() {
        // Column fits title + 1 dish only; next section title must move with its dish.
        let blocks = vec![
            block(10.0, 0.0),
            block(30.0, 0.0),
            block(10.0, 0.0),
            block(30.0, 0.0),
        ];
        let sections = vec![0..2, 2..4];
        let r = flow(&blocks, &sections, 2, 50.0, false, false).unwrap();
        assert_eq!(r.placements[2].column, r.placements[3].column);
        assert_eq!(r.placements[2].column, 1);
    }
}
