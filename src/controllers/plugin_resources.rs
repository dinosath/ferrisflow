//! Apply custom resources through handlers declared by installed plugins.
use super::primitives;
use crate::models::plugin;
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, TransactionTrait};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

async fn apply<C: ConnectionTrait + TransactionTrait + Send + Sync + 'static>(
    db: Arc<C>, manifest: &Value,
) -> Result<(), String> {
    let rows = plugin::Entity::find().all(&*db).await.map_err(error)?;
    let mut providers = HashMap::new();
    let mut installed = HashMap::new();
    for row in rows {
        if !row.enabled {
            continue;
        }
        installed.insert(row.uid.clone(), row.version.clone());
        let provider: Value = serde_json::from_str(&row.manifest_json).map_err(error)?;
        if let Some(types) = provider["resourceTypes"].as_array() {
            for kind in types {
                let name = kind["name"].as_str().ok_or("Resource type needs a name")?;
                if providers
                    .insert(name.to_owned(), (row.uid.clone(), provider.clone(), kind.clone()))
                    .is_some()
                {
                    return Err(format!("Multiple plugins define resource type '{name}'"));
                }
            }
        }
    }
    if let Some(dependencies) = manifest.get("dependencies") {
        let dependencies = dependencies.as_array().ok_or("dependencies must be an array")?;
        for dep in dependencies {
            let id = dep["id"].as_str().ok_or("Dependency id is required")?;
            let version = installed
                .get(id)
                .ok_or_else(|| format!("Required plugin '{id}' is not installed or enabled"))?;
            let requirement = semver::VersionReq::parse(
                dep["version"].as_str().ok_or("Dependency version is required")?,
            )
            .map_err(error)?;
            if !requirement.matches(&semver::Version::parse(version).map_err(error)?) {
                return Err(format!(
                    "Plugin '{id}' version {version} does not satisfy {requirement}"
                ));
            }
        }
    }
    if let Some(workflows) = manifest["workflows"].as_array() {
        for definition in workflows {
            primitives::save_workflow(&*db, definition.clone()).await?;
        }
    }
    let mut resources = manifest
        .get("resources")
        .map(|v| v.as_array().cloned().ok_or("resources must be an array"))
        .transpose()?
        .unwrap_or_default();
    // A provider can name a manifest path for its resource collection.
    for (name, (_, _, kind)) in &providers {
        if let Some(source) = kind["source"].as_str() {
            let mut value = manifest;
            for key in source.split('.') {
                value = &value[key];
            }
            if let Some(items) = value.as_array() {
                for definition in items {
                    resources.push(json!({"type":name,"definition":definition}));
                }
            }
        }
    }
    for resource in resources {
        let name = resource["type"].as_str().ok_or("Resource type is required")?;
        let (provider_id, provider, kind) = providers
            .get(name)
            .ok_or_else(|| format!("No installed plugin handles resource type '{name}'"))?;
        if provider_id != manifest["id"].as_str().unwrap_or_default()
            && !manifest["dependencies"]
                .as_array()
                .is_some_and(|deps| deps.iter().any(|d| d["id"].as_str() == Some(provider_id)))
        {
            return Err(format!(
                "Resource '{name}' requires an explicit dependency on '{provider_id}'"
            ));
        }
        let input =
            json!({"definition":resource["definition"],"resource":resource,"plugin":manifest});
        for hook in ["create", "postInstall"] {
            if let Some(workflow_name) = kind[hook].as_str() {
                let definition = provider["workflows"]
                    .as_array()
                    .and_then(|items| items.iter().find(|w| w["document"]["name"] == workflow_name))
                    .ok_or_else(|| format!("Missing resource {hook} workflow '{workflow_name}'"))?;
                primitives::run(db.clone(), definition, input.clone()).await?;
            } else if hook == "create" {
                return Err(format!("Resource '{name}' needs a create workflow"));
            }
        }
    }
    if let Some(name) = manifest["hooks"]["postInstall"].as_str() {
        let definition = manifest["workflows"]
            .as_array()
            .and_then(|items| items.iter().find(|w| w["document"]["name"] == name))
            .ok_or("Post-install workflow not found")?;
        primitives::run(db, definition, json!({"plugin":manifest})).await?;
    }
    Ok(())
}

pub(crate) async fn persist(
    db: &sea_orm::DatabaseConnection, active: plugin::ActiveModel,
) -> Result<plugin::Model, String> {
    let transaction = Arc::new(db.begin().await.map_err(error)?);
    let uid = active.uid.clone().unwrap();
    let exists =
        plugin::Entity::find_by_id(&uid).one(&*transaction).await.map_err(error)?.is_some();
    let row = if exists {
        active.update(&*transaction).await
    } else {
        active.insert(&*transaction).await
    }
    .map_err(error)?;
    let manifest: Value = serde_json::from_str(&row.manifest_json).map_err(error)?;
    apply(transaction.clone(), &manifest).await?;
    Arc::try_unwrap(transaction)
        .map_err(|_| "Plugin workflow retained the database transaction")?
        .commit()
        .await
        .map_err(error)?;
    Ok(row)
}

pub(crate) async fn reconcile(db: &sea_orm::DatabaseConnection) -> Result<(), String> {
    let rows = plugin::Entity::find().all(db).await.map_err(error)?;
    for row in rows {
        if row.enabled {
            let uid = row.uid.clone();
            if let Err(error) = persist(db, row.into()).await {
                tracing::warn!(%uid, %error, "Could not reconcile plugin resources");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use migration::MigratorTrait;
    use sea_orm::{Database, DatabaseConnection, Set};
    async fn database() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        db
    }
    fn active(manifest: Value) -> plugin::ActiveModel {
        plugin::ActiveModel {
            uid: Set(manifest["id"].as_str().unwrap().to_owned()),
            name: Set(manifest["name"].as_str().unwrap().to_owned()),
            version: Set(manifest["version"].as_str().unwrap().to_owned()),
            manifest_json: Set(manifest.to_string()),
            files_json: Set("{}".into()),
            enabled: Set(true),
            ..Default::default()
        }
    }
    fn provider() -> Value {
        serde_json::from_str(include_str!("../../plugins/ferrisflow.content-types.json")).unwrap()
    }
    fn novels() -> Value {
        serde_json::from_str(include_str!("../../plugins/example.novels.json")).unwrap()
    }
    #[tokio::test]
    async fn plugin_workflows_provision_table_and_crud_idempotently() {
        let db = database().await;
        persist(&db, active(provider())).await.unwrap();
        persist(&db, active(novels())).await.unwrap();
        let (_, tables) = crate::db_triggers::list_tables(&db).await.unwrap();
        assert!(tables.contains(&"novels".into()));
        let row = plugin::Entity::find_by_id("example.novels").one(&db).await.unwrap().unwrap();
        persist(&db, row.into()).await.unwrap();
        let rows = crate::models::workflow::Entity::find().all(&db).await.unwrap();
        let crud = rows.iter().filter(|row| row.name.contains("novels-")).collect::<Vec<_>>();
        assert_eq!(crud.len(), 5);
        let definition: Value = serde_json::from_str(
            &crud.iter().find(|w| w.name.ends_with("novels-create")).unwrap().definition_json,
        )
        .unwrap();
        primitives::run(
            Arc::new(db.clone()),
            &definition,
            json!({"title":"Dune","author":"Frank Herbert","synopsis":"Desert planet"}),
        )
        .await
        .unwrap();
        let definition: Value = serde_json::from_str(
            &crud.iter().find(|w| w.name.ends_with("novels-list")).unwrap().definition_json,
        )
        .unwrap();
        let rows = primitives::run(Arc::new(db.clone()), &definition, json!({})).await.unwrap();
        assert_eq!(rows[0]["title"], "Dune");
        let record_id = rows[0]["id"].clone();
        for (op, input) in [
            ("read", json!({"request":{"params":{"id":record_id},"body":null}})),
            ("update", json!({"id":record_id,"title":"Dune Messiah"})),
            ("delete", json!({"id":record_id})),
        ] {
            let definition: Value = serde_json::from_str(
                &crud
                    .iter()
                    .find(|w| w.name.ends_with(&format!("novels-{op}")))
                    .unwrap()
                    .definition_json,
            )
            .unwrap();
            primitives::run(Arc::new(db.clone()), &definition, input).await.unwrap();
        }
        let registry=primitives::database(&db,&json!({"table":"content_types","operation":"read","primaryKey":"uid","key":"novels","jsonFields":{"schema":"schema_json"}})).await.unwrap();
        assert_eq!(registry["schema"]["fields"][0]["name"], "title");
    }
    #[tokio::test]
    async fn missing_dependency_rejects_import_and_rolls_back() {
        let db = database().await;
        let error = persist(&db, active(novels())).await.unwrap_err();
        assert!(error.contains("not installed"), "{error}");
        assert!(plugin::Entity::find_by_id("example.novels").one(&db).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn incompatible_dependency_is_rejected() {
        let db = database().await;
        persist(&db, active(provider())).await.unwrap();
        let mut manifest = novels();
        manifest["dependencies"][0]["version"] = json!("^2.0");
        assert!(persist(&db, active(manifest)).await.unwrap_err().contains("does not satisfy"));
    }
    #[tokio::test]
    async fn failed_post_install_rolls_back_table_registry_and_plugin() {
        let db = database().await;
        let mut manifest = provider();
        manifest["resourceTypes"][0]["postInstall"] = json!("missing-hook");
        persist(&db, active(manifest)).await.unwrap();
        assert!(persist(&db, active(novels())).await.is_err());
        let (_, tables) = crate::db_triggers::list_tables(&db).await.unwrap();
        assert!(!tables.contains(&"novels".into()));
        assert!(plugin::Entity::find_by_id("example.novels").one(&db).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn unknown_resource_and_unsafe_identifier_are_rejected() {
        let db = database().await;
        let mut manifest = novels();
        manifest["dependencies"] = json!([]);
        manifest["resources"][0]["type"] = json!("unknown");
        assert!(persist(&db, active(manifest)).await.unwrap_err().contains("No installed plugin"));
        assert!(
            primitives::database(
                &db,
                &json!({"table":"novels; DROP TABLE plugins","operation":"list"})
            )
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn import_api_exposes_generated_crud_and_datatables() {
        let db = database().await;
        persist(&db, active(provider())).await.unwrap();
        let app = crate::build_router(crate::app_state::AppState { db });
        let server = axum_test::TestServer::new(app);
        server
            .post("/api/plugins")
            .json(&novels())
            .await
            .assert_status(axum::http::StatusCode::CREATED);
        let result: Value = server.get("/api/database/tables").await.json();
        assert!(result["tables"].as_array().unwrap().contains(&json!("novels")));
        let result: Value = server
            .post("/api/collections/novels")
            .json(&json!({"title":"Dune","author":"Frank Herbert"}))
            .await
            .json();
        assert_eq!(result["success"], true, "{result}");
        let result: Value = server.get("/api/collections/novels").await.json();
        assert_eq!(result["output"][0]["title"], "Dune", "{result}");
        server.get("/api/database/tables/novels/records").await.assert_status_ok();
        let result: Value = server.get("/api/collections/novels/1").await.json();
        assert_eq!(result["output"]["author"], "Frank Herbert", "{result}");
        let result: Value = server
            .put("/api/collections/novels/1")
            .json(&json!({"title":"Dune Messiah"}))
            .await
            .json();
        assert_eq!(result["success"], true, "{result}");
        let result: Value = server.delete("/api/collections/novels/1").await.json();
        assert_eq!(result["success"], true, "{result}");
    }
    #[tokio::test]
    async fn novels_package_creates_both_collections() {
        let db = database().await;
        persist(&db, active(provider())).await.unwrap();
        let manifest: Value =
            serde_json::from_str(include_str!("../../plugins/local.novels.json")).unwrap();
        persist(&db, active(manifest)).await.unwrap();
        let (_, tables) = crate::db_triggers::list_tables(&db).await.unwrap();
        assert!(tables.contains(&"local_novels_novel".into()));
        assert!(tables.contains(&"local_novels_chapter".into()));
    }
    #[tokio::test]
    async fn arbitrary_resource_type_is_handled_by_its_provider_workflow() {
        let db = database().await;
        let provider = json!({
            "id":"example.settings","name":"Settings provider","version":"1.0.0",
            "resourceTypes":[{"name":"setting","create":"apply-setting"}],
            "workflows":[{"document":{"dsl":"1.0.3","namespace":"settings","name":"apply-setting","version":"1.0.0"},"do":[
                {"table":{"call":"ferrisflow.primitive","with":{"primitive":"database","operation":"ensure","table":"settings","columns":[{"name":"id","type":"integer","primaryKey":true},{"name":"value","type":"string"}],"input":"${ . }","returnInput":true}}},
                {"record":{"call":"ferrisflow.primitive","with":{"primitive":"database","operation":"create","table":"settings","input":"${ .definition }"}}}
            ]}]
        });
        persist(&db, active(provider)).await.unwrap();
        persist(&db,active(json!({"id":"example.consumer","name":"Consumer","version":"1.0.0","dependencies":[{"id":"example.settings","version":"^1.0"}],"resources":[{"type":"setting","definition":{"value":"configured"}}]}))).await.unwrap();
        let records = primitives::database(&db, &json!({"operation":"list","table":"settings"}))
            .await
            .unwrap();
        assert_eq!(records[0]["value"], "configured");
    }
    #[tokio::test]
    async fn plugin_workflows_preserve_legacy_json_records() {
        let db = database().await;
        persist(&db, active(provider())).await.unwrap();
        primitives::database(&db,&json!({"operation":"create","table":"content_types","primaryKey":"uid","jsonFields":{"schema":"schema_json"},"input":{"uid":"articles","display_name":"Articles","schema":{"fields":[]}}})).await.unwrap();
        primitives::database(&db,&json!({"operation":"create","table":"content_entries","jsonFields":{"data":"data_json"},"input":{"content_type_uid":"articles","data":{"title":"Legacy article"},"published":false}})).await.unwrap();
        let manifest = provider();
        for operation in ["list", "read", "update", "delete"] {
            let name = format!("content-records-{operation}");
            let definition = manifest["workflows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["document"]["name"] == name)
                .unwrap();
            let output=primitives::run(Arc::new(db.clone()),definition,json!({"request":{"params":{"uid":"articles","id":"1"},"body":{"data":{"title":"Updated article"}}}})).await.unwrap();
            if operation == "list" {
                assert_eq!(output[0]["data"]["title"], "Legacy article");
            }
            if operation == "read" {
                assert_eq!(output["data"]["title"], "Legacy article");
            }
        }
    }
}
