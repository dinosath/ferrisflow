//! Avatar: a user's or entity's round image, with a fallback for initials when
//! there is no image.
use super::utils::{classes, merge_classes};
use dioxus::prelude::*;

const AVATAR_BASE_CLASS: &str = "relative flex h-10 w-10 shrink-0 overflow-hidden rounded-full";
const AVATAR_IMAGE_BASE_CLASS: &str = "aspect-square h-full w-full object-cover";
const AVATAR_FALLBACK_BASE_CLASS: &str = "flex h-full w-full items-center justify-center rounded-full bg-muted text-sm font-medium text-foreground";

/// Classes for the round, clipped frame, with `class` merged over them.
pub fn avatar_class(class: &str) -> String {
    merge_classes(classes([Some(AVATAR_BASE_CLASS)]), class)
}

/// Classes for the image, which fills and covers the frame, with `class` merged over them.
pub fn avatar_image_class(class: &str) -> String {
    merge_classes(classes([Some(AVATAR_IMAGE_BASE_CLASS)]), class)
}

/// Classes for the fallback: centered text on a muted fill, with `class` merged over them.
pub fn avatar_fallback_class(class: &str) -> String {
    merge_classes(classes([Some(AVATAR_FALLBACK_BASE_CLASS)]), class)
}

#[component]
pub fn Avatar(#[props(default)] class: String, children: Element) -> Element {
    let class = avatar_class(&class);

    rsx! {
      span {
        class,
        {children}
      }
    }
}

#[component]
pub fn AvatarImage(
    src: String, #[props(default)] alt: String, #[props(default)] class: String,
) -> Element {
    let class = avatar_image_class(&class);

    rsx! {
      img {
        class,
        src,
        alt,
      }
    }
}

#[component]
pub fn AvatarFallback(#[props(default)] class: String, children: Element) -> Element {
    let class = avatar_fallback_class(&class);

    rsx! {
      span {
        class,
        {children}
      }
    }
}
