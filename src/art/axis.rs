//! PLACEHOLDER: the original low-resolution art, scaled up.

use super::legacy;
use crate::gfx::*;

/// Paint the static scene on a full-frame (1152x648) canvas.
pub fn paint(c: &mut Canvas) {
    legacy::upscaled_bg(c, "axis", 0.5);
}

/// Cheap per-frame animated overlay, drawn over a copy of `paint`'s output.
pub fn animate(_c: &mut Canvas, _t: f32) {}
