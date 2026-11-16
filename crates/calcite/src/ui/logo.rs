//! The Calcite mark, the gem-cut C, drawn facet by facet so it can
//! assemble itself on the welcome screen.

use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt as _, IntoElement, ParentElement as _, PathBuilder, Styled as _,
    canvas, div, point, px, rgb,
};

/// The fourteen facets on a 64-unit grid, in the order they appear: around
/// the C from its upper tip to its lower one. Lit from the top left.
const FACETS: [([f32; 6], u32); 14] = [
    ([42.72, 6.13, 57.87, 21.28, 45.86, 26.25], 0xE6A23C),
    ([42.72, 6.13, 45.86, 26.25, 37.75, 18.14], 0xD48F2C),
    ([21.28, 6.13, 42.72, 6.13, 37.75, 18.14], 0xF4CD86),
    ([21.28, 6.13, 37.75, 18.14, 26.25, 18.14], 0xEDB65A),
    ([6.13, 21.28, 21.28, 6.13, 26.25, 18.14], 0xFFE7B3),
    ([6.13, 21.28, 26.25, 18.14, 18.14, 26.25], 0xF4CD86),
    ([6.13, 42.72, 6.13, 21.28, 18.14, 26.25], 0xEDB65A),
    ([6.13, 42.72, 18.14, 26.25, 18.14, 37.75], 0xE6A23C),
    ([21.28, 57.87, 6.13, 42.72, 18.14, 37.75], 0xD48F2C),
    ([21.28, 57.87, 18.14, 37.75, 26.25, 45.86], 0xC27A20),
    ([42.72, 57.87, 21.28, 57.87, 26.25, 45.86], 0xC27A20),
    ([42.72, 57.87, 26.25, 45.86, 37.75, 45.86], 0xA9661A),
    ([57.87, 42.72, 42.72, 57.87, 37.75, 45.86], 0xA9661A),
    ([57.87, 42.72, 37.75, 45.86, 45.86, 37.75], 0x8A5214),
];

/// When each facet starts, and how long it takes, as parts of the whole.
const STAGGER: f32 = 0.04;
const RUN: f32 = 0.44;

/// How far along facet `ix` is when the whole mark is at `progress` (0..1).
fn facet_progress(ix: usize, progress: f32) -> f32 {
    let t = ((progress - ix as f32 * STAGGER) / RUN).clamp(0.0, 1.0);
    // Ease out cubic.
    1.0 - (1.0 - t).powi(3)
}

/// The mark at a given point in its assembly (1.0 = finished).
pub fn gem(size: f32, progress: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let unit = size / 64.0;
            for (ix, (pts, color)) in FACETS.iter().enumerate() {
                let e = facet_progress(ix, progress);
                if e <= 0.0 {
                    continue;
                }
                let cx = (pts[0] + pts[2] + pts[4]) / 3.0;
                let cy = (pts[1] + pts[3] + pts[5]) / 3.0;
                // Grow from the facet's centre; a little short of full size
                // leaves the thin seams between facets.
                let scale = (0.55 + 0.45 * e) * 0.965;
                let corner = |x: f32, y: f32| {
                    point(
                        bounds.origin.x + px((cx + (x - cx) * scale) * unit),
                        bounds.origin.y + px((cy + (y - cy) * scale) * unit),
                    )
                };
                let mut path = PathBuilder::fill();
                path.add_polygon(
                    &[
                        corner(pts[0], pts[1]),
                        corner(pts[2], pts[3]),
                        corner(pts[4], pts[5]),
                    ],
                    true,
                );
                if let Ok(path) = path.build() {
                    let fill: gpui_kit::Hsla = rgb(*color).into();
                    window.paint_path(path, fill.opacity(e));
                }
            }
        },
    )
    .size(px(size))
}

/// The mark assembling itself over about a second, then holding still.
pub fn gem_assembling(size: f32, id: &'static str) -> impl IntoElement {
    div().size(px(size)).with_animation(
        id,
        Animation::new(Duration::from_millis(1100)),
        move |el, progress| el.child(gem(size, progress)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facets_arrive_in_order_and_all_finish() {
        assert_eq!(facet_progress(0, 0.0), 0.0);
        assert!(facet_progress(0, 0.2) > facet_progress(5, 0.2));
        for ix in 0..FACETS.len() {
            assert!((facet_progress(ix, 1.0) - 1.0).abs() < 1e-6, "facet {ix}");
        }
    }
}
