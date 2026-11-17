//! Output of the layout engine: a flat list of positioned drawing operations.
//!
//! Coordinates are PDF points with the origin at the top-left corner of the
//! page and y growing downwards. Renderers only need to translate these
//! primitives; they make no layout decisions of their own.

use crate::domain::{Rgb, TextRole};

#[derive(Debug, Clone, PartialEq)]
pub enum DrawOp {
    Text {
        x: f32,
        /// Baseline position.
        y: f32,
        text: String,
        role: TextRole,
        size: f32,
        /// Extra space between glyphs, in ems.
        tracking: f32,
        color: Rgb,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: Rgb,
        /// Round dots instead of a solid line (price leaders).
        dotted: bool,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        color: Rgb,
    },
    Logo {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

impl DrawOp {
    pub fn translated(mut self, dx: f32, dy: f32) -> Self {
        match &mut self {
            DrawOp::Text { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            DrawOp::Line { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
            DrawOp::Circle { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            DrawOp::Logo { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct PageLayout {
    pub width: f32,
    pub height: f32,
    pub ops: Vec<DrawOp>,
    /// Scale applied to every font size and gap (1.0 = sizes as configured).
    pub scale: f32,
    /// `false` when the menu could not fit even at the minimum scale.
    pub fits: bool,
}
