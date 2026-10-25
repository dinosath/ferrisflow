use dioxus::prelude::*;
use ferris_workflow_editor::{ValidationState, WorkflowEditor};

use crate::pages::api::validate_workflow;

#[component]
pub(crate) fn OwsEditor(definition: String, on_change: EventHandler<String>) -> Element {
    let mut validation = use_signal(ValidationState::default);
    let on_change = move |next_definition: String| {
        validation.set(ValidationState::default());
        on_change.call(next_definition);
    };
    let on_validate = move |definition: String| {
        validation.set(ValidationState { checking: true, ..Default::default() });
        spawn(async move {
            let result = validate_workflow(definition).await;
            validation.set(match result {
                Ok(errors) if errors.is_empty() => {
                    ValidationState { valid: true, ..Default::default() }
                }
                Ok(errors) => ValidationState { errors, ..Default::default() },
                Err(error) => ValidationState { errors: vec![error], ..Default::default() },
            });
        });
    };

    rsx! {
        WorkflowEditor {
            definition,
            on_change,
            on_validate,
            validation: validation(),
        }
    }
}
