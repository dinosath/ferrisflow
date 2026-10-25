//! Alert: a callout for an important status or validation message, with
//! title and description parts.
use super::utils::{classes, merge_classes};
use dioxus::prelude::*;

/// The tone of an alert, which sets its border, surface, and text colors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AlertVariant {
    /// A neutral card for general notices.
    #[default]
    Default,
    /// An error or a failed action, in the destructive color.
    Destructive,
    /// A completed action, on a success tint.
    Success,
    /// Something that needs attention, on a warning tint.
    Warning,
    /// Neutral information, on an info tint.
    Info,
}

impl AlertVariant {
    /// The border, surface, and text colors for this tone.
    pub const fn class(self) -> &'static str {
        match self {
            Self::Default => "border-border bg-card text-foreground",
            Self::Destructive => "border-destructive bg-card text-destructive",
            // The status colors are too light to be text (RFC 0058), so they tint
            // the surface and border instead.
            Self::Success => "border-success/50 bg-success/10 text-foreground",
            Self::Warning => "border-warning/50 bg-warning/10 text-foreground",
            Self::Info => "border-info/50 bg-info/10 text-foreground",
        }
    }
}

const ALERT_BASE_CLASS: &str = "relative w-full rounded-md border p-4";
const ALERT_TITLE_BASE_CLASS: &str = "mb-1 font-medium leading-none tracking-normal";
const ALERT_DESCRIPTION_BASE_CLASS: &str = "text-sm";

/// Classes for the alert box: base classes, the variant's colors, then `class` merged
/// over them.
pub fn alert_class(variant: AlertVariant, class: &str) -> String {
    merge_classes(classes([Some(ALERT_BASE_CLASS), Some(variant.class())]), class)
}

/// Classes for the title: base classes with `class` merged over them.
pub fn alert_title_class(class: &str) -> String {
    merge_classes(classes([Some(ALERT_TITLE_BASE_CLASS)]), class)
}

/// Classes for the description: base classes, a text color that suits the variant,
/// then `class` merged over them.
pub fn alert_description_class(variant: AlertVariant, class: &str) -> String {
    let variant_class = match variant {
        AlertVariant::Default => "text-muted-foreground",
        AlertVariant::Destructive => "text-destructive",
        // Muted text is checked only on plain surfaces, not on the status tints.
        AlertVariant::Success | AlertVariant::Warning | AlertVariant::Info => "text-foreground",
    };

    merge_classes(classes([Some(ALERT_DESCRIPTION_BASE_CLASS), Some(variant_class)]), class)
}

#[component]
pub fn Alert(
    #[props(default)] variant: AlertVariant, #[props(default)] class: String, children: Element,
) -> Element {
    let class = alert_class(variant, &class);

    rsx! {
      div {
        role: "alert",
        class,
        {children}
      }
    }
}

#[component]
pub fn AlertTitle(#[props(default)] class: String, children: Element) -> Element {
    let class = alert_title_class(&class);

    rsx! {
      div {
        class,
        {children}
      }
    }
}

#[component]
pub fn AlertDescription(
    #[props(default)] variant: AlertVariant, #[props(default)] class: String, children: Element,
) -> Element {
    let class = alert_description_class(variant, &class);

    rsx! {
      div {
        class,
        {children}
      }
    }
}
