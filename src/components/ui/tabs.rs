//! Tabs: a list of triggers that each select one content panel.
use super::density::{density_control_class, use_density, with_density};
use super::element_id::next_element_id;
use super::root_state::{Controllable, use_controllable, use_root_context};
use super::roving_group::{group_part_id, use_roving_group};
use super::utils::{classes, merge_classes};
use dioxus::prelude::*;

const TABS_BASE_CLASS: &str = "data-[orientation=vertical]:flex data-[orientation=vertical]:gap-4";
const TABS_LIST_BASE_CLASS: &str = "inline-flex h-10 items-center justify-center rounded-md bg-muted p-1 text-muted-foreground data-[orientation=vertical]:h-auto data-[orientation=vertical]:flex-col data-[orientation=vertical]:items-stretch data-[orientation=vertical]:justify-start";
const TABS_TRIGGER_BASE_CLASS: &str = "inline-flex items-center justify-center whitespace-nowrap rounded-sm px-3 py-1.5 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50";
const TABS_CONTENT_BASE_CLASS: &str = "mt-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[orientation=vertical]:mt-0";

/// Whether moving focus to a trigger also selects it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabsActivation {
    /// Arrow, Home, and End keys select the trigger they move to.
    #[default]
    Automatic,
    /// Keys only move focus; Enter, Space, or a click selects.
    Manual,
}

impl TabsActivation {
    /// The roving group script's `data-dxui-roving-activation` value.
    pub fn roving_attribute(self) -> &'static str {
        match self {
            Self::Automatic => "focus",
            Self::Manual => "manual",
        }
    }
}

/// The direction the triggers are laid out and moved through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabsOrientation {
    /// Left and Right move between triggers.
    #[default]
    Horizontal,
    /// Up and Down move between triggers, and the list sits beside the panels.
    Vertical,
}

impl TabsOrientation {
    /// The value for `aria-orientation` and `data-orientation`.
    pub fn attribute(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// Classes for the root, which lays the list beside the panels when vertical.
pub fn tabs_class(class: &str) -> String {
    merge_classes(classes([Some(TABS_BASE_CLASS)]), class)
}

/// Classes for the list that holds the triggers.
pub fn tabs_list_class(class: &str) -> String {
    merge_classes(classes([Some(TABS_LIST_BASE_CLASS)]), class)
}

/// Classes for the trigger: base classes, raised while `active` and muted otherwise, with `class`
/// merged over them.
pub fn tabs_trigger_class(active: bool, class: &str) -> String {
    let active_class = if active {
        "bg-background text-foreground shadow-sm"
    } else {
        "text-muted-foreground hover:text-foreground"
    };

    merge_classes(classes([Some(TABS_TRIGGER_BASE_CLASS), Some(active_class)]), class)
}

/// Classes for the content panel.
pub fn tabs_content_class(class: &str) -> String {
    merge_classes(classes([Some(TABS_CONTENT_BASE_CLASS)]), class)
}

/// What `Tabs` shares with its parts (RFC 0077).
#[derive(Clone)]
struct TabsContext {
    base_id: String,
    value: Controllable<Option<String>>,
    select: Callback<String>,
    activation: TabsActivation,
    orientation: TabsOrientation,
}

/// The root of a tab set: it owns which tab is selected and links its parts.
/// Pass `value` to control it, or `default_value` to start it; the change
/// callback hears every change the user makes either way.
#[component]
pub fn Tabs(
    #[props(default)] value: ReadSignal<Option<String>>,
    #[props(default)] default_value: Option<String>,
    #[props(default)] on_value_change: Option<EventHandler<String>>,
    #[props(default)] activation: TabsActivation, #[props(default)] orientation: TabsOrientation,
    #[props(default)] class: String, children: Element,
) -> Element {
    let class = tabs_class(&class);
    let base_id = use_hook(|| format!("dxui-tabs-{}", next_element_id()));
    let change = use_callback(move |next: Option<String>| {
        if let (Some(handler), Some(next)) = (on_value_change, next) {
            handler.call(next);
        }
    });
    let value = use_controllable(move || value().map(Some), move || default_value, Some(change));
    let select = use_callback(move |next: String| value.set(Some(next)));
    use_context_provider(|| TabsContext { base_id, value, select, activation, orientation });

    rsx! {
      div {
        class,
        "data-orientation": orientation.attribute(),
        {children}
      }
    }
}

#[component]
pub fn TabsList(
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes, extends = div)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let context = use_root_context::<TabsContext>("TabsList", "Tabs");
    let class = tabs_list_class(&class);
    let orientation = context.orientation.attribute();
    let scope_id = use_roving_group(Some(context.select));

    rsx! {
      div {
        role: "tablist",
        class,
        "aria-orientation": orientation,
        "data-orientation": orientation,
        "data-dxui-roving-group": scope_id,
        "data-dxui-roving-orientation": orientation,
        "data-dxui-roving-loop": "true",
        "data-dxui-roving-activation": context.activation.roving_attribute(),
        ..attributes,
        {children}
      }
    }
}

#[component]
pub fn TabsTrigger(
    value: String, #[props(default)] disabled: bool, #[props(default)] class: String,
    children: Element,
) -> Element {
    let context = use_root_context::<TabsContext>("TabsTrigger", "Tabs");
    let active = context.value.get().as_deref() == Some(value.as_str());
    let class =
        tabs_trigger_class(active, &with_density(density_control_class(use_density()), &class));
    let id = group_part_id(&context.base_id, "trigger", &value);
    let controls = group_part_id(&context.base_id, "content", &value);

    rsx! {
      button {
        r#type: "button",
        role: "tab",
        id,
        class,
        disabled,
        "aria-selected": active.to_string(),
        "aria-controls": controls,
        "data-value": value,
        "data-dxui-roving-item": "",
        {children}
      }
    }
}

#[component]
pub fn TabsContent(value: String, #[props(default)] class: String, children: Element) -> Element {
    let context = use_root_context::<TabsContext>("TabsContent", "Tabs");
    let active = context.value.get().as_deref() == Some(value.as_str());
    let class = tabs_content_class(&class);
    let id = group_part_id(&context.base_id, "content", &value);
    let labelledby = group_part_id(&context.base_id, "trigger", &value);

    rsx! {
      div {
        role: "tabpanel",
        id,
        class,
        tabindex: "0",
        hidden: !active,
        "aria-labelledby": labelledby,
        "data-orientation": context.orientation.attribute(),
        "data-value": value,
        {children}
      }
    }
}
