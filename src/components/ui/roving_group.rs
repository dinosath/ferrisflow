use dioxus::prelude::*;

use super::element_id::next_element_id;
use super::script::{Script, component_script};

// Runs for the group's lifetime and reads items from the DOM on every event.
// The root sets `data-dxui-roving-orientation` (horizontal, vertical, or both),
// `data-dxui-roving-loop`, and `data-dxui-roving-activation` ("focus" when
// selection follows focus, "manual" when items carry a selection that does
// not). With `data-dxui-roving-tab-stops="all"` the script
// leaves `tabindex` alone, so every enabled item stays in the Tab order. Sends
// the `data-value` of a clicked item, and of a newly focused item when
// selection follows focus.
component_script!(
    roving_group_script = r#"
export async function run(dioxus) {
  const scopeId = await dioxus.recv();
  const root = document.querySelector(`[data-dxui-roving-group="${scopeId}"]`);
  if (!root) return;
  const itemSelector = "[data-dxui-roving-item]";
  // Items inside a nested group belong to that group.
  const items = () =>
    Array.from(root.querySelectorAll(itemSelector)).filter(
      (item) => item.closest("[data-dxui-roving-group]") === root,
    );
  const enabledItems = () => items().filter((item) => !item.disabled);
  const followsFocus = () => root.dataset.dxuiRovingActivation === "focus";
  const hasSelection = () => ["focus", "manual"].includes(root.dataset.dxuiRovingActivation);
  const singleTabStop = () => root.dataset.dxuiRovingTabStops !== "all";
  const selected = (item) =>
    ["aria-selected", "aria-checked", "aria-pressed"].some((name) => item.getAttribute(name) === "true");
  let lastFocused = null;
  // One Tab stop: where items carry a selection, the selected item comes
  // first; otherwise the item that last had focus does.
  const setTabStop = () => {
    if (!singleTabStop()) return;
    const enabled = enabledItems();
    const chosen = enabled.find(selected);
    const last = enabled.includes(lastFocused) ? lastFocused : null;
    const stop = (hasSelection() ? chosen || last : last || chosen) || enabled[0];
    items().forEach((item) => {
      item.tabIndex = item === stop ? 0 : -1;
    });
  };
  const keys = {
    horizontal: [["ArrowRight"], ["ArrowLeft"]],
    vertical: [["ArrowDown"], ["ArrowUp"]],
    both: [["ArrowRight", "ArrowDown"], ["ArrowLeft", "ArrowUp"]],
  };
  // Returns the item to focus, the current item at an end without looping, or
  // null for keys the group does not handle.
  const step = (item, key) => {
    const enabled = enabledItems();
    const index = enabled.indexOf(item);
    if (index < 0) return null;
    const [nextKeys, previousKeys] = keys[root.dataset.dxuiRovingOrientation] || keys.both;
    const loop = root.dataset.dxuiRovingLoop !== "false";
    const last = enabled.length - 1;
    if (nextKeys.includes(key)) return index < last ? enabled[index + 1] : loop ? enabled[0] : item;
    if (previousKeys.includes(key)) return index > 0 ? enabled[index - 1] : loop ? enabled[last] : item;
    if (key === "Home") return enabled[0];
    if (key === "End") return enabled[last];
    return null;
  };
  // In a right-to-left layout, ArrowLeft points at the next item.
  const visualKey = (key) => {
    if (getComputedStyle(root).direction !== "rtl") return key;
    if (key === "ArrowLeft") return "ArrowRight";
    if (key === "ArrowRight") return "ArrowLeft";
    return key;
  };
  const onKeyDown = (event) => {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
    if (!items().includes(event.target)) return;
    const next = step(event.target, visualKey(event.key));
    if (!next) return;
    event.preventDefault();
    if (next === event.target) return;
    next.focus();
    if (followsFocus()) dioxus.send(next.dataset.value || "");
  };
  const onClick = (event) => {
    const item = event.target instanceof Element ? event.target.closest(itemSelector) : null;
    if (item && !item.disabled && items().includes(item)) dioxus.send(item.dataset.value || "");
  };
  const onFocusIn = (event) => {
    if (!items().includes(event.target)) return;
    lastFocused = event.target;
    if (!singleTabStop()) return;
    event.target.tabIndex = 0;
    items().forEach((item) => {
      if (item !== event.target) item.tabIndex = -1;
    });
  };
  // Leaving the group moves the Tab stop back to its preferred item. The
  // browser has already picked the next focus target.
  const onFocusOut = (event) => {
    if (items().includes(event.target) && !items().includes(event.relatedTarget)) setTabStop();
  };
  let finish;
  const ended = new Promise((resolve) => {
    finish = resolve;
  });
  const observer = new MutationObserver(() => {
    if (!root.isConnected) return finish();
    // Focus stays the Tab stop until selection catches up with it.
    if (!items().includes(document.activeElement)) setTabStop();
  });
  setTabStop();
  observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true, attributeFilter: ["aria-selected", "aria-checked", "aria-pressed", "disabled"] });
  root.addEventListener("keydown", onKeyDown);
  root.addEventListener("click", onClick);
  root.addEventListener("focusin", onFocusIn);
  root.addEventListener("focusout", onFocusOut);
  await ended;
  observer.disconnect();
}
"#
);

/// Builds a trigger or panel id for a group part. Characters that are not
/// ASCII letters, digits, `-`, or `_` become `-` so the id is a valid IDREF.
pub(crate) fn group_part_id(base_id: &str, part: &str, value: &str) -> String {
    let value: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect();

    format!("{base_id}-{part}-{value}")
}

/// Runs the roving group script for the component's lifetime. Clicks, and
/// focus moves where the root sets `data-dxui-roving-activation="focus"`, call
/// `on_activate` with the item's `data-value`.
///
/// Returns the value for the root's `data-dxui-roving-group` attribute.
pub(crate) fn use_roving_group(on_activate: Option<EventHandler<String>>) -> String {
    let scope_id = use_hook(|| format!("dxui-roving-group-{}", next_element_id()));
    let effect_scope_id = scope_id.clone();

    use_effect(move || {
        let script = roving_group_script::start();
        // A send error means the page already finished the script; nothing to track.
        let _ = script.send(effect_scope_id.as_str());
        spawn(async move {
            while let Ok(value) = script.recv::<String>().await {
                if let Some(handler) = on_activate {
                    handler.call(value);
                }
            }
        });
    });

    scope_id
}
