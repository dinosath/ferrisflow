//! Table: styled semantic table parts for tabular data.
use super::utils::{classes, merge_classes};
use dioxus::prelude::*;

const TABLE_CONTAINER_BASE_CLASS: &str = "relative w-full overflow-auto";
const TABLE_BASE_CLASS: &str = "w-full caption-bottom text-sm";
const TABLE_HEADER_BASE_CLASS: &str = "[&_tr]:border-b";
const TABLE_BODY_BASE_CLASS: &str = "[&_tr:last-child]:border-0";
const TABLE_FOOTER_BASE_CLASS: &str =
    "border-t border-border bg-muted font-medium [&>tr]:last:border-b-0";
const TABLE_ROW_BASE_CLASS: &str = "border-b border-border transition-colors hover:bg-muted";
const TABLE_HEAD_BASE_CLASS: &str =
    "h-12 px-4 text-left align-middle font-medium text-muted-foreground";
const TABLE_CELL_BASE_CLASS: &str = "p-4 align-middle";
const TABLE_CAPTION_BASE_CLASS: &str = "mt-4 text-sm text-muted-foreground";

fn table_container_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_CONTAINER_BASE_CLASS)]), class)
}

/// Classes for the `table` element, with `class` merged over them.
pub fn table_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_BASE_CLASS)]), class)
}

fn table_header_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_HEADER_BASE_CLASS)]), class)
}

fn table_body_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_BODY_BASE_CLASS)]), class)
}

fn table_footer_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_FOOTER_BASE_CLASS)]), class)
}

fn table_row_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_ROW_BASE_CLASS)]), class)
}

fn table_head_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_HEAD_BASE_CLASS)]), class)
}

fn table_cell_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_CELL_BASE_CLASS)]), class)
}

fn table_caption_class(class: &str) -> String {
    merge_classes(classes([Some(TABLE_CAPTION_BASE_CLASS)]), class)
}

#[component]
pub fn Table(#[props(default)] class: String, children: Element) -> Element {
    let container_class = table_container_class("");
    let class = table_class(&class);

    rsx! {
      div {
        class: container_class,
        table {
          class,
          {children}
        }
      }
    }
}

#[component]
pub fn TableHeader(#[props(default)] class: String, children: Element) -> Element {
    let class = table_header_class(&class);

    rsx! {
      thead {
        class,
        {children}
      }
    }
}

#[component]
pub fn TableBody(#[props(default)] class: String, children: Element) -> Element {
    let class = table_body_class(&class);

    rsx! {
      tbody {
        class,
        {children}
      }
    }
}

#[component]
pub fn TableFooter(#[props(default)] class: String, children: Element) -> Element {
    let class = table_footer_class(&class);

    rsx! {
      tfoot {
        class,
        {children}
      }
    }
}

#[component]
pub fn TableRow(#[props(default)] class: String, children: Element) -> Element {
    let class = table_row_class(&class);

    rsx! {
      tr {
        class,
        {children}
      }
    }
}

#[component]
pub fn TableHead(#[props(default)] class: String, children: Element) -> Element {
    let class = table_head_class(&class);

    rsx! {
      th {
        class,
        {children}
      }
    }
}

#[component]
pub fn TableCell(#[props(default)] class: String, children: Element) -> Element {
    let class = table_cell_class(&class);

    rsx! {
      td {
        class,
        {children}
      }
    }
}

#[component]
pub fn TableCaption(#[props(default)] class: String, children: Element) -> Element {
    let class = table_caption_class(&class);

    rsx! {
      caption {
        class,
        {children}
      }
    }
}
