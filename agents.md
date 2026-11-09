# Ferrisflow contributor guide

This repository contains Ferrisflow, a Rust and Dioxus workflow application.

## Sidebar conventions

- Build sidebar sections with the shared `SidebarSection` component in `src/pages/shared.rs`.
- Build every navigable sidebar item with the shared `SidebarItem` component. Do not duplicate link markup, active-state classes, or icon layout in the sidebar.
- Keep menu item styling unified through the `sidebar-link` and `sidebar-icon` classes in `assets/workspace.css`. Add a shared component prop when an item needs new behavior instead of creating a one-off item style.
- Keep section heading styles unified through `sidebar-section-label`. Use `SidebarSection` for all sidebar groups, including Settings.
- Add new settings destinations as normal `SidebarItem`s in the Settings section.
- Remove obsolete sidebar-specific styles when their markup is removed.

## List pages and imports

- Build record lists with the shared `DataTable`, `DataTableRow`, and `DataTableCell` components. Put search and import controls in the table toolbar, and keep the primary create action in `Shell.actions`.
- Use the shared `ImportSourceDialog` for import flows that accept a URL, file, or pasted text. When a JSON package can reference companion files, support selecting those files alongside the primary JSON file; keep import controls shared rather than creating page-specific source controls.
- Add tests for import source parsing and request routing when introducing or changing an import flow.
