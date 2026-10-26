use sea_orm::DatabaseConnection;

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
        let state = Self { db };
        if let Err(error) = crate::db_triggers::reconcile(&state).await {
            tracing::warn!(%error, "could not install configured database triggers");
        }
        crate::db_triggers::spawn_listener(state.clone());
        state
    }
}
