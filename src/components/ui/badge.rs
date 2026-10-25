//! Badge: a compact indicator of a status or category.
use super::utils::{classes, merge_classes};
use dioxus::prelude::*;

/// The colors of a `Badge`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BadgeVariant {
    /// The primary color.
    #[default]
    Default,
    /// The secondary color, for less emphasis.
    Secondary,
    /// The destructive color, for errors and removals.
    Destructive,
    /// A border and no fill.
    Outline,
    /// The success color.
    Success,
    /// The warning color.
    Warning,
    /// The info color.
    Info,
}

impl BadgeVariant {
    /// The variant's border, background, and text color classes.
    pub const fn class(self) -> &'static str {
        match self {
            Self::Default => "border-transparent bg-primary text-primary-foreground",
            Self::Secondary => "border-transparent bg-secondary text-secondary-foreground",
            Self::Destructive => "border-transparent bg-destructive text-destructive-foreground",
            Self::Outline => "border-border text-foreground",
            Self::Success => "border-transparent bg-success text-success-foreground",
            Self::Warning => "border-transparent bg-warning text-warning-foreground",
            Self::Info => "border-transparent bg-info text-info-foreground",
        }
    }
}

const BADGE_BASE_CLASS: &str = "inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-semibold transition-colors";

/// Classes for the badge: base classes, the variant's, then `class` merged over them.
pub fn badge_class(variant: BadgeVariant, class: &str) -> String {
    merge_classes(classes([Some(BADGE_BASE_CLASS), Some(variant.class())]), class)
}

#[component]
pub fn Badge(
    #[props(default)] variant: BadgeVariant, #[props(default)] class: String, children: Element,
) -> Element {
    let class = badge_class(variant, &class);

    rsx! {
      span {
        class,
        {children}
      }
    }
}
