use sea_orm::{DatabaseConnection, EntityTrait, IntoActiveModel, Set};

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
        // Built-in packages use the same resource lifecycle as imported packages.
        for package in [include_str!("../plugins/ferrisflow.content-types.json")] {
            let manifest: serde_json::Value =
                serde_json::from_str(package).expect("invalid bundled plugin");
            let uid = manifest["id"].as_str().expect("bundled plugin ID");
            let existing = crate::models::plugin::Entity::find_by_id(uid)
                .one(&db)
                .await
                .expect("load bundled plugin");
            let mut active = existing.map(IntoActiveModel::into_active_model).unwrap_or_default();
            active.uid = Set(uid.to_owned());
            active.name = Set(manifest["name"].as_str().unwrap().to_owned());
            active.version = Set(manifest["version"].as_str().unwrap().to_owned());
            active.description = Set(manifest["description"].as_str().map(str::to_owned));
            active.manifest_json = Set(manifest.to_string());
            active.files_json = Set("{}".to_owned());
            active.enabled = Set(true);
            crate::controllers::plugin_resources::persist(&db, active)
                .await
                .expect("register bundled plugin");
        }
        crate::controllers::plugin_resources::reconcile(&db)
            .await
            .expect("reconcile plugin resources");
        let state = Self { db };
        if let Err(error) = crate::db_triggers::reconcile(&state).await {
            tracing::warn!(%error, "could not install configured database triggers");
        }
        crate::db_triggers::spawn_listener(state.clone());
        state
    }
}
