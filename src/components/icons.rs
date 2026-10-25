use dioxus::prelude::*;

use dioxus_icons::lucide::{
    activity::Activity, archive::Archive, arrow_left::ArrowLeft, arrow_up_down::ArrowUpDown,
    arrow_up_right::ArrowUpRight, braces::Braces, check::Check, chevron_down::ChevronDown,
    chevron_left::ChevronLeft, chevron_right::ChevronRight, chevrons_left::ChevronsLeft,
    chevrons_right::ChevronsRight, circle_check::CircleCheck, circle_minus::CircleMinus,
    circle_question_mark::CircleQuestionMark, clock::Clock, command::Command, download::Download,
    ellipsis::Ellipsis, file_text::FileText, history::History, kanban::Kanban, key_round::KeyRound,
    layout_dashboard::LayoutDashboard, list::List, pencil::Pencil, play::Play, plus::Plus,
    save::Save, search::Search, settings::Settings, sliders_horizontal::SlidersHorizontal,
    sparkles::Sparkles, table::Table, upload::Upload, workflow::Workflow, x::X,
};

/// Reuses the project's Lucide icon set through semantic names used by the UI.
#[component]
pub fn Icon(name: &'static str) -> Element {
    match name {
        "activity" => rsx! { Activity { size: 16, class: "ui-icon" } },
        "archive" => rsx! { Archive { size: 16, class: "ui-icon" } },
        "arrow-left" => rsx! { ArrowLeft { size: 16, class: "ui-icon" } },
        "arrow-up-right" => rsx! { ArrowUpRight { size: 16, class: "ui-icon" } },
        "check" => rsx! { Check { size: 16, class: "ui-icon" } },
        "chevron-down" => rsx! { ChevronDown { size: 16, class: "ui-icon" } },
        "chevron-left" => rsx! { ChevronLeft { size: 16, class: "ui-icon" } },
        "chevron-right" => rsx! { ChevronRight { size: 16, class: "ui-icon" } },
        "chevrons-left" => rsx! { ChevronsLeft { size: 16, class: "ui-icon" } },
        "chevrons-right" => rsx! { ChevronsRight { size: 16, class: "ui-icon" } },
        "circle-check" => rsx! { CircleCheck { size: 16, class: "ui-icon" } },
        "circle-minus" => rsx! { CircleMinus { size: 16, class: "ui-icon" } },
        "close" => rsx! { X { size: 16, class: "ui-icon" } },
        "clock" => rsx! { Clock { size: 16, class: "ui-icon" } },
        "command" => rsx! { Command { size: 16, class: "ui-icon" } },
        "download" => rsx! { Download { size: 16, class: "ui-icon" } },
        "form" => rsx! { FileText { size: 16, class: "ui-icon" } },
        "help" => rsx! { CircleQuestionMark { size: 16, class: "ui-icon" } },
        "history" => rsx! { History { size: 16, class: "ui-icon" } },
        "home" => rsx! { LayoutDashboard { size: 16, class: "ui-icon" } },
        "kanban" => rsx! { Kanban { size: 16, class: "ui-icon" } },
        "key" => rsx! { KeyRound { size: 16, class: "ui-icon" } },
        "layout" => rsx! { LayoutDashboard { size: 16, class: "ui-icon" } },
        "list" => rsx! { List { size: 16, class: "ui-icon" } },
        "more" => rsx! { Ellipsis { size: 16, class: "ui-icon" } },
        "plus" => rsx! { Plus { size: 16, class: "ui-icon" } },
        "pencil" => rsx! { Pencil { size: 16, class: "ui-icon" } },
        "play" => rsx! { Play { size: 16, class: "ui-icon" } },
        "search" => rsx! { Search { size: 16, class: "ui-icon" } },
        "save" => rsx! { Save { size: 16, class: "ui-icon" } },
        "settings" => rsx! { Settings { size: 16, class: "ui-icon" } },
        "sliders" => rsx! { SlidersHorizontal { size: 16, class: "ui-icon" } },
        "sort" => rsx! { ArrowUpDown { size: 16, class: "ui-icon" } },
        "sparkles" => rsx! { Sparkles { size: 16, class: "ui-icon" } },
        "table" => rsx! { Table { size: 16, class: "ui-icon" } },
        "variable" => rsx! { Braces { size: 16, class: "ui-icon" } },
        "workflow" => rsx! { Workflow { size: 16, class: "ui-icon" } },
        "upload" => rsx! { Upload { size: 16, class: "ui-icon" } },
        _ => rsx! { CircleQuestionMark { size: 16, class: "ui-icon" } },
    }
}
