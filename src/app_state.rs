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
        if let Some(password) = ensure_admin_password(&db).await {
            tracing::warn!(
                admin_password = %password,
                "Generated the initial admin password. Save it securely; it is stored in app config."
            );
        }
        ensure_admin_user(&db).await;
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
        crate::workflow_schedules::spawn(state.clone());
        state
    }
}

async fn ensure_admin_password(db: &DatabaseConnection) -> Option<String> {
    use crate::models::app_setting;
    use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};

    const ADMIN_PASSWORD_KEY: &str = "auth.admin_password";
    let configured = std::env::var("ADMIN_PASSWORD").ok().filter(|value| !value.trim().is_empty());
    let existing = app_setting::Entity::find_by_id(ADMIN_PASSWORD_KEY)
        .one(db)
        .await
        .expect("load admin password config");
    if configured.is_none()
        && let Some(record) = existing.as_ref()
        && !record.value.is_empty()
    {
        if record.value.starts_with("sha256:") || record.value.starts_with("$argon2") {
            return None;
        }
        // Upgrade a password written by an earlier build that stored it directly.
        let encoded = hash_admin_password(&record.value);
        let mut active = record.clone().into_active_model();
        active.value = Set(encoded);
        active.update(db).await.expect("upgrade admin password config");
        return None;
    }

    let password = configured.clone().unwrap_or_else(generate_admin_password);
    let value = hash_admin_password(&password);
    match existing {
        Some(record) => {
            let mut active = record.into_active_model();
            active.value = Set(value);
            active.update(db).await.expect("save admin password config");
        }
        None => {
            (app_setting::ActiveModel {
                key: Set(ADMIN_PASSWORD_KEY.to_owned()),
                value: Set(value),
            })
            .insert(db)
            .await
            .expect("save admin password config");
        }
    }
    configured.is_none().then_some(password)
}

fn generate_admin_password() -> String {
    let bytes: [u8; 32] = rand::random();
    let mut password = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(password, "{byte:02x}");
    }
    password
}

pub(crate) fn hash_admin_password(password: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};

    let salt_bytes: [u8; 16] = rand::random();
    let salt = SaltString::encode_b64(&salt_bytes).expect("encode random admin password salt");
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hash admin password")
        .to_string()
}

async fn ensure_admin_user(db: &DatabaseConnection) {
    use crate::models::user;
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

    if user::Entity::find()
        .filter(user::Column::Username.eq("admin"))
        .one(db)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    let now = chrono::Utc::now().naive_utc();
    let admin = user::ActiveModel {
        username: Set("admin".to_owned()),
        email: Set(None),
        display_name: Set("Administrator".to_owned()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    if let Err(error) = admin.insert(db).await {
        tracing::warn!(%error, "could not create the default admin user record");
    }
}
