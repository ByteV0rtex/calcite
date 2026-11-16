//! Quick, quiet motion: things arrive with a short fade and lift, then sit
//! still. Nothing animates while idle.

use std::time::Duration;

use gpui_kit::{
    Animation, AnimationElement, AnimationExt as _, ElementId, IntoElement, Styled, ease_out_quint,
    px,
};

/// How long an arrival takes.
pub const ARRIVE_MS: u64 = 180;

/// Ease-out that waits `delay` of the total before starting, so several
/// elements can arrive one after another.
fn after(delay_ms: u64, run_ms: u64) -> impl Fn(f32) -> f32 {
    let start = delay_ms as f32 / (delay_ms + run_ms) as f32;
    let ease = ease_out_quint();
    move |t| {
        if t <= start {
            0.0
        } else {
            ease(((t - start) / (1.0 - start)).min(1.0))
        }
    }
}

fn animation(delay_ms: u64, run_ms: u64) -> Animation {
    Animation::new(Duration::from_millis(delay_ms + run_ms)).with_easing(after(delay_ms, run_ms))
}

/// Fade in.
pub fn fade<E: Styled + IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    delay_ms: u64,
) -> AnimationElement<E> {
    element.with_animation(id, animation(delay_ms, ARRIVE_MS), |el, d| el.opacity(d))
}

/// Fade in while moving `distance` px into place (positive: from below).
/// For elements in normal flow; it sets `relative` positioning.
pub fn rise<E: Styled + IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    delay_ms: u64,
    distance: f32,
) -> AnimationElement<E> {
    element.with_animation(id, animation(delay_ms, ARRIVE_MS), move |el, d| {
        el.opacity(d).relative().top(px(distance * (1.0 - d)))
    })
}

/// Like [`rise`] but slower, for big first-impression moments.
pub fn rise_slow<E: Styled + IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    delay_ms: u64,
    distance: f32,
) -> AnimationElement<E> {
    element.with_animation(id, animation(delay_ms, 520), move |el, d| {
        el.opacity(d).relative().top(px(distance * (1.0 - d)))
    })
}

#[cfg(test)]
mod tests {
    use super::after;

    #[test]
    fn waits_then_eases_to_one() {
        let f = after(100, 100);
        assert_eq!(f(0.0), 0.0);
        assert_eq!(f(0.5), 0.0);
        assert!(f(0.75) > 0.5);
        assert!((f(1.0) - 1.0).abs() < 1e-4);
    }
}
