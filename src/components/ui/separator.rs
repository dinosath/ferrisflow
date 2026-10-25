//! Separator: a thin line that divides content, exposed as a separator or hidden
//! as decoration.
use super::utils::{classes, merge_classes};
use dioxus::prelude::*;

/// Which way the line runs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SeparatorOrientation {
    /// A full-width line between stacked content.
    #[default]
    Horizontal,
    /// A full-height line between side-by-side content.
    Vertical,
}

impl SeparatorOrientation {
    /// The line's size: one pixel thick, full length.
    pub const fn class(self) -> &'static str {
        match self {
            Self::Horizontal => "h-px w-full",
            Self::Vertical => "h-full w-px",
        }
    }

    /// The `aria-orientation` value: `horizontal` or `vertical`.
    pub const fn aria_orientation(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

const SEPARATOR_BASE_CLASS: &str = "shrink-0 bg-border";

/// Classes for the line: border color, the orientation's size, then `class` merged
/// over them.
pub fn separator_class(orientation: SeparatorOrientation, class: &str) -> String {
    merge_classes(classes([Some(SEPARATOR_BASE_CLASS), Some(orientation.class())]), class)
}

#[component]
pub fn Separator(
    #[props(default)] orientation: SeparatorOrientation, #[props(default)] class: String,
    #[props(default)] decorative: bool,
) -> Element {
    let class = separator_class(orientation, &class);
    let role = if decorative { "presentation" } else { "separator" };
    let aria_hidden = decorative.to_string();

    rsx! {
      div {
        class,
        role,
        "aria-hidden": aria_hidden,
        "aria-orientation": orientation.aria_orientation(),
      }
    }
}
