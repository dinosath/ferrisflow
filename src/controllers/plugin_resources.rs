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
