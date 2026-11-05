# Ferrisflow contributor guide

This repository contains Ferrisflow, a Rust and Dioxus workflow application.

## Sidebar conventions

- Build sidebar sections with the shared `SidebarSection` component in `src/pages/shared.rs`.
- Build every navigable sidebar item with the shared `SidebarItem` component. Do not duplicate link markup, active-state classes, or icon layout in the sidebar.
- Keep menu item styling unified through the `sidebar-link` and `sidebar-icon` classes in `assets/workspace.css`. Add a shared component prop when an item needs new behavior instead of creating a one-off item style.
- Keep section heading styles unified through `sidebar-section-label`. Use `SidebarSection` for all sidebar groups, including Settings.
- Add new settings destinations as normal `SidebarItem`s in the Settings section.
- Remove obsolete sidebar-specific styles when their markup is removed.

## Cargo launch modes

- The supported local web experience is the Dioxus fullstack app: `dx serve --fullstack --features fullstack`. It serves the UI and REST API from one Axum server.
- The `server` Cargo feature is API-only and must not imply `ui`. Its minimal build is `cargo check --no-default-features --features server`.
- The `fullstack` feature combines `server` and `ui`, and adds Dioxus fullstack/server support. The `router(app)` entry point belongs only to this mode; the API-only entry point must serve `api` directly.
- `api-server` is a compatibility alias for `server` and must stay API-only.
- The web client feature set is `cargo check --no-default-features --features web`.
- When changing entry points or Cargo features, preserve all four modes and verify these feature combinations: `server`, `fullstack`, `api-server`, and `web`, each with `--no-default-features`.
- Do not fix an unresolved `app` in `dioxus::server::router(app)` by substituting the local `api` variable. Make sure the fullstack entry point is gated by both server and UI features; the API-only entry point serves `api` directly.
- Keep these feature-mode checks in CI so `--no-default-features` builds cannot regress silently.

## cargo-dist release workflow

- `.github/workflows/release.yml` is generated from `dist-workspace.toml`; regenerate it with `dist generate --mode ci` after changing dist configuration.
- Verify generated CI with `dist generate --mode ci --check`. Do not set `allow-dirty = ["ci"]` unless intentionally maintaining a hand-edited workflow; it disables this check and can leave a custom release workflow in place instead of the plan/build/host/announce pipeline.
- Keep release-plz version/tag preparation separate from cargo-dist artifact build and GitHub Release hosting. Avoid duplicate custom workflows that build and publish the same tags.
