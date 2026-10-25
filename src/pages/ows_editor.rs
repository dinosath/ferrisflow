use dioxus::prelude::*;
use ferris_workflow_editor::{ValidationState, WorkflowEditor, WorkflowExecution};
use std::{cell::Cell, rc::Rc};

use crate::pages::api::{execute_workflow, validate_workflow};

#[component]
pub(crate) fn OwsEditor(
    definition: String, on_change: EventHandler<String>,
    on_validation: EventHandler<ValidationState>, validation: ValidationState,
    notice: Option<String>, workflow_id: Option<i32>, workflow_name: String,
) -> Element {
    let mut last_definition = use_signal(String::new);
    let mut execution = use_signal(WorkflowExecution::default);
    let validation_generation = use_hook(|| Rc::new(Cell::new(0_u64)));
    let generation_for_effect = validation_generation.clone();
    let definition_for_effect = definition.clone();

    use_effect(move || {
        let current = definition_for_effect.clone();
        if last_definition() == current {
            return;
        }
        last_definition.set(current.clone());
        let generation = generation_for_effect.get() + 1;
        generation_for_effect.set(generation);
        let checking = ValidationState { checking: true, ..Default::default() };
        on_validation.call(checking);
        let validation_generation = generation_for_effect.clone();
        spawn(async move {
            let result = validate_workflow(current).await;
            if validation_generation.get() != generation {
                return;
            }
            let result = validation_result(result);
            on_validation.call(result);
        });
    });

    let generation_for_change = validation_generation.clone();
    let generation_for_change_async = validation_generation.clone();
    let on_change = move |next_definition: String| {
        let generation = generation_for_change.get() + 1;
        generation_for_change.set(generation);
        let checking = ValidationState { checking: true, ..Default::default() };
        on_validation.call(checking);
        on_change.call(next_definition.clone());
        let validation_generation = generation_for_change_async.clone();
        let on_validation = on_validation;
        spawn(async move {
            let result = validate_workflow(next_definition).await;
            if validation_generation.get() != generation {
                return;
            }
            on_validation.call(validation_result(result));
        });
    };
    let generation_for_validate = validation_generation.clone();
    let on_validate = move |definition: String| {
        let generation = generation_for_validate.get() + 1;
        generation_for_validate.set(generation);
        let checking = ValidationState { checking: true, ..Default::default() };
        on_validation.call(checking);
        let validation_generation = generation_for_validate.clone();
        spawn(async move {
            let result = validate_workflow(definition).await;
            if validation_generation.get() != generation {
                return;
            }
            let result = validation_result(result);
            on_validation.call(result);
        });
    };
    let definition_for_execution = definition.clone();
    let workflow_name_for_execution = workflow_name.clone();
    let on_execute = move |_| {
        let current = definition_for_execution.clone();
        let name = workflow_name_for_execution.clone();
        execution.set(WorkflowExecution { running: true, ..Default::default() });
        spawn(async move {
            let result = execute_workflow(current, workflow_id, Some(name)).await;
            execution.set(match result {
                Ok(result) if result.success => WorkflowExecution {
                    execution_id: Some(result.execution_id),
                    output: result.output.map(|output| {
                        serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string())
                    }),
                    ..Default::default()
                },
                Ok(result) => WorkflowExecution {
                    execution_id: Some(result.execution_id),
                    error: result.error.or_else(|| Some("Workflow execution failed".into())),
                    ..Default::default()
                },
                Err(error) => WorkflowExecution { error: Some(error), ..Default::default() },
            });
        });
    };

    rsx! {
        WorkflowEditor {
            definition,
            on_change,
            on_validate,
            validation,
            execution: execution(),
            on_execute,
            notice,
        }
    }
}

fn validation_result(result: Result<Vec<String>, String>) -> ValidationState {
    match result {
        Ok(errors) if errors.is_empty() => ValidationState { valid: true, ..Default::default() },
        Ok(errors) => ValidationState { errors, ..Default::default() },
        Err(error) => ValidationState { errors: vec![error], ..Default::default() },
    }
}
