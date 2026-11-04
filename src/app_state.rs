use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    Set,
};

const CONTENT_TYPES_PLUGIN_ID: &str = "ferrisflow.content-types";

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
}

impl AppState {
    pub async fn new() -> Self {
        let default_database_url =
            concat!("sqlite://", env!("CARGO_MANIFEST_DIR"), "/ferris-workflow.db?mode=rwc");
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| default_database_url.to_string());
        let db =
            sea_orm::Database::connect(&database_url).await.expect("failed to connect to database");
        {
            use migration::MigratorTrait;
            migration::Migrator::up(&db, None).await.expect("seaorm migrations failed");
        }
        let mut manifest: serde_json::Value =
            serde_json::from_str(include_str!("../plugins/ferrisflow.content-types.json"))
                .expect("built-in content types plugin definition must be valid JSON");
        let workflows = manifest["workflows"]
            .as_array_mut()
            .expect("built-in content types plugin must declare workflow definitions");
        let mut content_type_workflows = Vec::with_capacity(workflows.len());
        for workflow in workflows {
            let definition_json = crate::ows::normalize_ows_json(&workflow.to_string())
                .expect("built-in plugin workflow must be valid OWS");
            *workflow = serde_json::from_str(&definition_json)
                .expect("normalized plugin workflow must be valid JSON");
            let metadata = workflow["document"]["metadata"]["ferrisflow"].clone();
            let name =
                format!("Content types · {}", metadata["name"].as_str().unwrap_or("Workflow"));
            let description = metadata["description"].as_str().unwrap_or_default().to_owned();
            content_type_workflows.push((name, description, workflow.clone()));
        }
        manifest["builtin"] = serde_json::Value::Bool(true);
        if let Some(row) = crate::models::plugin::Entity::find_by_id(CONTENT_TYPES_PLUGIN_ID)
            .one(&db)
            .await
            .expect("could not check built-in content types plugin")
        {
            if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&row.manifest_json) {
                if let (Some(new_views), Some(old_views)) = (
                    manifest.get_mut("views").and_then(serde_json::Value::as_array_mut),
                    existing.get("views").and_then(serde_json::Value::as_array),
                ) {
                    for view in new_views {
                        let id = view.get("id").and_then(serde_json::Value::as_str);
                        if let Some(old_view) = old_views
                            .iter()
                            .find(|old| old.get("id").and_then(serde_json::Value::as_str) == id)
                        {
                            if let Some(old_config) = old_view.get("config") {
                                if let Some(old_list) = old_config.get("listView") {
                                    for key in ["api", "columns", "settings"] {
                                        if let Some(value) = old_list.get(key) {
                                            view["config"]["listView"][key] = value.clone();
                                        }
                                    }
                                    if let Some(by_type) =
                                        old_list.get("byType").filter(|by_type| by_type.is_object())
                                    {
                                        view["config"]["listView"]["byType"] = by_type.clone();
                                    }
                                }
                                if let Some(form_view) = old_config
                                    .get("formView")
                                    .filter(|form_view| form_view.is_object())
                                {
                                    view["config"]["formView"] = form_view.clone();
                                }
                            }
                        }
                    }
                }
            }
            let mut plugin = row.into_active_model();
            plugin.manifest_json = Set(manifest.to_string());
            plugin.enabled = Set(true);
            plugin.update(&db).await.expect("could not update built-in content types plugin");
        } else {
            crate::models::plugin::ActiveModel {
                uid: Set(CONTENT_TYPES_PLUGIN_ID.to_owned()),
                name: Set("Content types".to_owned()),
                version: Set("1.0.0".to_owned()),
                description: Set(Some(
                    "Built-in content schema and CRUD workflow tools".to_owned(),
                )),
                manifest_json: Set(manifest.to_string()),
                files_json: Set("{}".to_owned()),
                source_url: Set(None),
                enabled: Set(true),
                ..Default::default()
            }
            .insert(&db)
            .await
            .expect("could not register built-in content types plugin");
        }
        for (name, description, definition) in content_type_workflows {
            let definition_json = crate::ows::normalize_ows_json(&definition.to_string())
                .expect("built-in content type workflow must be valid OWS");
            let exists = crate::models::workflow::Entity::find()
                .filter(crate::models::workflow::Column::Name.eq(name.clone()))
                .one(&db)
                .await
                .expect("could not check built-in content type workflows");
            if let Some(row) = exists {
                let mut workflow = row.into_active_model();
                workflow.active = Set(true);
                workflow.definition_json = Set(definition_json);
                workflow.description = Set(Some(description));
                workflow
                    .update(&db)
                    .await
                    .expect("could not update built-in content type workflow");
            } else {
                crate::models::workflow::ActiveModel {
                    active: Set(true),
                    definition_json: Set(definition_json),
                    description: Set(Some(description)),
                    name: Set(name),
                    version: Set(1),
                    ..Default::default()
                }
                .insert(&db)
                .await
                .expect("could not register built-in content type workflow");
            }
        }
        let saved_workflows = crate::models::workflow::Entity::find()
            .all(&db)
            .await
            .expect("could not inspect saved workflows");
        for row in saved_workflows {
            let Ok(mut definition) =
                serde_json::from_str::<serde_json::Value>(&row.definition_json)
            else {
                continue;
            };
            let uses_removed_content_crud =
                definition.to_string().contains("ferrisflow.content_crud");
            let Some(metadata) = definition
                .get_mut("document")
                .and_then(|document| document.get_mut("metadata"))
                .and_then(serde_json::Value::as_object_mut)
            else {
                continue;
            };
            if metadata.remove("content_crud").is_none() && !uses_removed_content_crud {
                continue;
            }
            let mut workflow = row.into_active_model();
            workflow.active = Set(false);
            workflow.definition_json = Set(definition.to_string());
            workflow.update(&db).await.expect("could not disable legacy content CRUD workflow");
        }
        let state = Self { db };
        let content_types = crate::models::content_type::Entity::find()
            .all(&state.db)
            .await
            .expect("could not load content types for view generation");
        for content_type in content_types {
            if content_type.uid == "core.page" {
                continue;
            }
            if let Err(error) =
                crate::controllers::content_views::generate_for_type(&state.db, &content_type.uid)
                    .await
            {
                tracing::warn!(uid = %content_type.uid, %error, "could not regenerate generated content type views");
            }
        }
        if let Err(error) = crate::db_triggers::reconcile(&state).await {
            tracing::warn!(%error, "could not install configured database triggers");
        }
        crate::db_triggers::spawn_listener(state.clone());
        state
    }
}
