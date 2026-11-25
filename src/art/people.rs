//! PLACEHOLDER: the original low-resolution art, scaled up.

use super::legacy;
use crate::gfx::*;

/// Paint character `who` with expression `expr` on a transparent
/// SPR_W x SPR_H (336x456) canvas.
pub fn paint(c: &mut Canvas, who: &str, expr: &str) {
    legacy::upscaled_sprite(c, who, expr);
}

/// The region of the sprite that changes between expressions (brows, eyes,
/// mouth): (x, y, w, h) in sprite pixels.
pub fn face_rect(_who: &str) -> (i32, i32, i32, i32) {
    (114, 138, 108, 108)
}
