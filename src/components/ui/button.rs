//! Button: the main control for actions and form submission.
use super::density::use_density;
use super::utils::{UiDensity, classes, merge_classes};
use dioxus::prelude::*;

/// The look of a `Button`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ButtonVariant {
    /// The primary fill, for the main action.
    #[default]
    Primary,
    /// The secondary fill, for other actions.
    Secondary,
    /// The destructive fill, for actions that delete or cannot be undone.
    Destructive,
    /// A border on the background.
    Outline,
    /// No fill until hovered.
    Ghost,
    /// Looks like a link, underlined on hover.
    Link,
}

impl ButtonVariant {
    /// The variant's color and hover classes.
    pub const fn class(self) -> &'static str {
        match self {
            Self::Primary => "bg-primary text-primary-foreground hover:bg-primary/90",
            Self::Secondary => "bg-secondary text-secondary-foreground hover:bg-secondary/80",
            Self::Destructive => {
                "bg-destructive text-destructive-foreground hover:bg-destructive/90"
            }
            Self::Outline => "border border-input bg-background hover:bg-accent",
            Self::Ghost => "bg-transparent hover:bg-accent",
            Self::Link => {
                "bg-transparent text-foreground decoration-primary underline-offset-4 hover:underline"
            }
        }
    }
}

/// The height and padding of a `Button`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ButtonSize {
    /// Small: 32 pixels high.
    Sm,
    /// Medium: 40 pixels high.
    #[default]
    Md,
    /// Large: 48 pixels high.
    Lg,
    /// A 40 pixel square, for a button that holds only an icon.
    Icon,
}

impl ButtonSize {
    /// The size's height, padding, and text size classes.
    pub const fn class(self) -> &'static str {
        match self {
            Self::Sm => "h-8 px-3 text-sm",
            Self::Md => "h-10 px-4 text-sm",
            Self::Lg => "h-12 px-6 text-base",
            Self::Icon => "h-10 w-10",
        }
    }
}

const BUTTON_BASE_CLASS: &str = "inline-flex items-center justify-center rounded-md font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-50";

/// Classes for the button: base classes, the variant's, the size's, a minimum
/// height for `density` (and a minimum width at Touch), then `class` merged over them.
/// At the default `Comfortable` the size alone sets the height, so `Sm` stays
/// 32 pixels high.
pub fn button_class(
    variant: ButtonVariant, size: ButtonSize, density: UiDensity, class: &str,
) -> String {
    let density_class = match density {
        UiDensity::Compact => Some("min-h-8"),
        UiDensity::Comfortable => None,
        UiDensity::Touch => Some("min-h-12 min-w-11"),
    };

    merge_classes(
        classes([
            Some(BUTTON_BASE_CLASS),
            Some(variant.class()),
            Some(size.class()),
            density_class,
        ]),
        class,
    )
}

/// Takes its density from the nearest `DensityProvider` (RFC 0078). Calls
/// `onclick` on a click, Enter, or Space. Other attributes, such as
/// `type`, `name`, and `aria-label`, are passed to the button, which keeps the
/// native `submit` type inside a form unless `r#type` says otherwise.
#[component]
pub fn Button(
    #[props(default)] variant: ButtonVariant, #[props(default)] size: ButtonSize,
    #[props(default)] class: String, #[props(default)] disabled: bool,
    #[props(default)] onclick: Option<EventHandler<MouseEvent>>,
    #[props(extends = GlobalAttributes, extends = button)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let class = button_class(variant, size, use_density(), &class);

    rsx! {
      button {
        class,
        disabled,
        onclick: move |event| {
          if let Some(handler) = onclick {
            handler.call(event);
          }
        },
        ..attributes,
        {children}
      }
    }
}
