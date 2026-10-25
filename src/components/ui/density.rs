//! The density a page's interactive controls take from `DensityProvider`
//! (RFC 0078).

use super::utils::{UiDensity, merge_classes};
use dioxus::prelude::*;

/// Gives every interactive component inside it `density`. Without one they
/// render at `UiDensity::Comfortable`, the shadcn/ui sizes. It renders no
/// element of its own.
#[component]
pub fn DensityProvider(density: UiDensity, children: Element) -> Element {
    let mut shared = use_context_provider(|| Signal::new(density));
    if *shared.peek() != density {
        shared.set(density);
    }

    rsx! { {children} }
}

/// The density of the nearest `DensityProvider`, or `Comfortable`.
pub fn use_density() -> UiDensity {
    try_use_context::<Signal<UiDensity>>().map(|density| density()).unwrap_or_default()
}

/// The Touch floor for a control that holds text: 44 CSS pixels high, and as
/// wide, so an icon button stays square.
pub fn density_control_class(density: UiDensity) -> &'static str {
    match density {
        UiDensity::Touch => "min-h-11 min-w-11",
        UiDensity::Compact | UiDensity::Comfortable => "",
    }
}

/// The Touch hit area for a control drawn smaller than a target: a 44 by 44
/// CSS pixel `after:` pseudo-element centered on it, which takes the press
/// without changing the look. `relative` positions it; an inline `position`,
/// such as a slider thumb's, still wins.
pub fn density_hit_area_class(density: UiDensity) -> &'static str {
    match density {
        UiDensity::Touch => {
            "relative after:absolute after:left-1/2 after:top-1/2 after:size-11 after:-translate-x-1/2 after:-translate-y-1/2"
        }
        UiDensity::Compact | UiDensity::Comfortable => "",
    }
}

/// Merges the app's `class` over `density_class`, so the app's classes win
/// where they conflict, as they do over a component's own classes.
#[allow(dead_code)] // Features with no interactive control leave it unused.
pub(crate) fn with_density(density_class: &str, class: &str) -> String {
    if density_class.is_empty() {
        class.to_string()
    } else {
        merge_classes(density_class.to_string(), class)
    }
}
