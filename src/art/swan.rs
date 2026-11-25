//! PLACEHOLDER: the original low-resolution art, scaled up.

use super::legacy;
use crate::gfx::*;

/// Paint the Admiral with expression `expr` on a transparent
/// SPR_W x SPR_H (336x456) canvas.
pub fn paint(c: &mut Canvas, expr: &str) {
    legacy::upscaled_sprite(c, "a", expr);
}

/// The region of the sprite that changes between expressions: (x, y, w, h).
pub fn face_rect() -> (i32, i32, i32, i32) {
    (48, 100, 90, 78)
}
