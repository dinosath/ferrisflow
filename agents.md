# Ferrisflow contributor guide

This repository contains Ferrisflow, a Rust and Dioxus workflow application.

## Sidebar conventions

- Build sidebar sections with the shared `SidebarSection` component in `src/pages/shared.rs`.
- Build every navigable sidebar item with the shared `SidebarItem` component. Do not duplicate link markup, active-state classes, or icon layout in the sidebar.
- Keep menu item styling unified through the `sidebar-link` and `sidebar-icon` classes in `assets/workspace.css`. Add a shared component prop when an item needs new behavior instead of creating a one-off item style.
- Keep section heading styles unified through `sidebar-section-label`. Use `SidebarSection` for all sidebar groups, including Settings.
- Add new settings destinations as normal `SidebarItem`s in the Settings section.
- Remove obsolete sidebar-specific styles when their markup is removed.
