use dioxus::prelude::*;

use super::icons::Icon;

#[derive(Clone, PartialEq)]
struct TableState {
    selected: Vec<String>,
    visible_columns: Vec<bool>,
    view_open: bool,
    page: usize,
    page_size: usize,
}

#[component]
pub fn DataTable(
    columns: Vec<String>, row_ids: Vec<String>, toolbar: Element,
    #[props(default)] empty_message: String, #[props(default)] class: String, children: Element,
) -> Element {
    let mut state = use_signal(|| TableState {
        selected: Vec::new(),
        visible_columns: vec![true; columns.len()],
        view_open: false,
        page: 1,
        page_size: 10,
    });
    use_context_provider(|| state);

    let count = row_ids.len();
    let page_count = count.div_ceil(state().page_size).max(1);
    let page = state().page.min(page_count);
    let start = (page - 1) * state().page_size;
    let visible_ids: Vec<String> =
        row_ids.iter().skip(start).take(state().page_size).cloned().collect();
    let selected_count = visible_ids.iter().filter(|id| state().selected.contains(id)).count();
    let all_visible_selected = !visible_ids.is_empty() && selected_count == visible_ids.len();
    let page_numbers: Vec<usize> = if page_count <= 5 {
        (1..=page_count).collect()
    } else if page <= 3 {
        (1..=4).collect()
    } else if page >= page_count - 2 {
        (page_count - 3..=page_count).collect()
    } else {
        (page - 1..=page + 1).collect()
    };
    let show_ellipsis = page_numbers.last().is_some_and(|last| *last < page_count);
    let empty_message =
        if empty_message.is_empty() { "No records found.".to_string() } else { empty_message };
    let visible_count = state().visible_columns.iter().filter(|visible| **visible).count() + 2;

    rsx! {
        div { class: "shared-table-region {class}",
            div { class: "shared-table-toolbar",
                div { class: "shared-table-toolbar__filters", {toolbar} }
                div { class: "shared-table-view",
                    button {
                        class: "shared-table-view__trigger",
                        r#type: "button",
                        aria_expanded: "{state().view_open}",
                        onclick: move |_| { let mut current = state(); current.view_open = !current.view_open; state.set(current); },
                        Icon { name: "sliders" }
                        "View"
                    }
                    if state().view_open {
                        div { class: "shared-table-view__menu",
                            div { class: "shared-table-view__title", "Toggle columns" }
                            div { class: "shared-table-view__separator" }
                            for (index, column) in columns.iter().enumerate() {
                                {
                                    let visible = state().visible_columns.get(index).copied().unwrap_or(true);
                                    rsx! { button {
                                        class: "shared-table-column-toggle",
                                        r#type: "button",
                                        role: "menuitemcheckbox",
                                        aria_checked: "{visible}",
                                        onclick: move |_| {
                                            let mut current = state();
                                            if let Some(column_visible) = current.visible_columns.get_mut(index) {
                                                *column_visible = !*column_visible;
                                            }
                                            state.set(current);
                                        },
                                        span { class: if visible { "shared-table-column-toggle__check checked" } else { "shared-table-column-toggle__check" }, if visible { Icon { name: "check" } } }
                                        span { "{column}" }
                                    } }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "shared-table-shell",
            div { class: "shared-table-scroll",
                table { class: "shared-data-table",
                    thead {
                        tr {
                            th { class: "shared-table-select-cell",
                                input {
                                    r#type: "checkbox",
                                    aria_label: "Select all rows on this page",
                                    checked: all_visible_selected,
                                    onchange: move |event| {
                                        let mut current = state();
                                        if event.checked() {
                                            for id in &visible_ids {
                                                if !current.selected.contains(id) {
                                                    current.selected.push(id.clone());
                                                }
                                            }
                                        } else {
                                            current.selected.retain(|id| !visible_ids.contains(id));
                                        }
                                        state.set(current);
                                    },
                                }
                            }
                            for (index, column) in columns.iter().enumerate() {
                                th {
                                    class: "shared-table-heading",
                                    style: if state().visible_columns.get(index).copied().unwrap_or(true) { "" } else { "display:none" },
                                    span { "{column}" }
                                    Icon { name: "sort" }
                                }
                            }
                            th { class: "shared-table-action-heading" }
                        }
                    }
                    tbody {
                        if count == 0 {
                            tr { class: "shared-table-empty-row",
                                td { colspan: visible_count, "{empty_message}" }
                            }
                        } else {
                            {children}
                        }
                    }
                }
            }
            footer { class: "shared-table-footer",
                div { class: "shared-table-page-size",
                    select {
                        aria_label: "Rows per page",
                        value: "{state().page_size}",
                        onchange: move |event| {
                            let mut current = state();
                            current.page_size = event.value().parse().unwrap_or(10);
                            current.page = 1;
                            state.set(current);
                        },
                        option { value: "10", "10" }
                        option { value: "20", "20" }
                        option { value: "50", "50" }
                    }
                    span { "Rows per page" }
                }
                div { class: "shared-table-pagination",
                    span { class: "shared-table-page-label", "Page {page} of {page_count}" }
                    div { class: "shared-table-page-buttons",
                        button { r#type: "button", title: "Go to first page", aria_label: "Go to first page", disabled: page <= 1, onclick: move |_| { let mut current = state(); current.page = 1; state.set(current); }, Icon { name: "chevrons-left" } }
                        button { r#type: "button", title: "Go to previous page", aria_label: "Go to previous page", disabled: page <= 1, onclick: move |_| { let mut current = state(); current.page = current.page.saturating_sub(1).max(1); state.set(current); }, Icon { name: "chevron-left" } }
                        for target in page_numbers.iter() {
                            {
                                let target = *target;
                                rsx! { button {
                                    class: if target == page { "shared-table-current-page" } else { "shared-table-page-number" },
                                    r#type: "button",
                                    aria_label: "Go to page {target}",
                                    onclick: move |_| { let mut current = state(); current.page = target; state.set(current); },
                                    "{target}"
                                } }
                            }
                        }
                        if show_ellipsis { span { class: "shared-table-ellipsis", "…" } }
                        if show_ellipsis {
                            button { class: "shared-table-page-number", r#type: "button", aria_label: "Go to page {page_count}", onclick: move |_| { let mut current = state(); current.page = page_count; state.set(current); }, "{page_count}" }
                        }
                        button { r#type: "button", title: "Go to next page", aria_label: "Go to next page", disabled: page >= page_count, onclick: move |_| { let mut current = state(); current.page = (current.page + 1).min(page_count); state.set(current); }, Icon { name: "chevron-right" } }
                        button { r#type: "button", title: "Go to last page", aria_label: "Go to last page", disabled: page >= page_count, onclick: move |_| { let mut current = state(); current.page = page_count; state.set(current); }, Icon { name: "chevrons-right" } }
                    }
                }
            }
            }
        }
    }
}

#[component]
pub fn DataTableRow(id: String, index: usize, children: Element) -> Element {
    let mut state = use_context::<Signal<TableState>>();
    let current = state();
    let selected = current.selected.contains(&id);
    let visible =
        index >= (current.page - 1) * current.page_size && index < current.page * current.page_size;
    let row_id = id.clone();

    rsx! {
        tr {
            class: if selected { "shared-table-row selected" } else { "shared-table-row" },
            style: if visible { "" } else { "display: none" },
            key: "{id}",
            td { class: "shared-table-select-cell",
                input {
                    r#type: "checkbox",
                    aria_label: "Select row",
                    checked: selected,
                    onchange: move |event| {
                        let mut current = state();
                        if event.checked() {
                            if !current.selected.contains(&row_id) {
                                current.selected.push(row_id.clone());
                            }
                        } else {
                            current.selected.retain(|selected_id| selected_id != &row_id);
                        }
                        state.set(current);
                    },
                }
            }
            {children}
            td { class: "shared-table-action-cell",
                button { r#type: "button", title: "More actions", aria_label: "More actions", Icon { name: "more" } }
            }
        }
    }
}

#[component]
pub fn DataTableCell(index: usize, #[props(default)] class: String, children: Element) -> Element {
    let state = use_context::<Signal<TableState>>();
    let visible = state().visible_columns.get(index).copied().unwrap_or(true);
    rsx! {
        td {
            class,
            style: if visible { "" } else { "display:none" },
            {children}
        }
    }
}
