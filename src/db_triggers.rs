use crate::{
    app_state::AppState,
    controllers::workflow::{ExecuteRequest, run_ows},
    models::workflow,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    QueryResult, Statement,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tracing::{error, warn};

#[derive(Clone, Debug)]
struct TargetTable {
    key: String,
    schema: Option<String>,
    table: String,
}

#[derive(Debug)]
struct ChangeEvent {
    id: i32,
    source_table: String,
    operation: String,
    old_row_json: Option<String>,
    new_row_json: Option<String>,
}

pub async fn list_tables(db: &DatabaseConnection) -> Result<(String, Vec<String>), String> {
    let backend = db.get_database_backend();
    let (label, sql) = match backend {
        DatabaseBackend::Sqlite => (
            "sqlite",
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT IN ('seaql_migrations', 'database_events', 'database_trigger_registry') ORDER BY name".to_string(),
        ),
        DatabaseBackend::Postgres => (
            "postgres",
            "SELECT table_schema || '.' || table_name AS name FROM information_schema.tables WHERE table_type = 'BASE TABLE' AND table_schema NOT IN ('pg_catalog', 'information_schema') AND table_name NOT IN ('seaql_migrations', 'database_events', 'database_trigger_registry') ORDER BY table_schema, table_name".to_string(),
        ),
        _ => return Err("Database triggers support SQLite and PostgreSQL only".into()),
    };
    let rows = db
        .query_all_raw(Statement::from_string(backend, sql))
        .await
        .map_err(|error| error.to_string())?;
    let tables = rows.iter().filter_map(|row| row.try_get::<String>("", "name").ok()).collect();
    Ok((label.into(), tables))
}

pub async fn reconcile(state: &AppState) -> Result<(), String> {
    let backend = state.db.get_database_backend();
    if !matches!(backend, DatabaseBackend::Sqlite | DatabaseBackend::Postgres) {
        return Err("Database triggers support SQLite and PostgreSQL only".into());
    }

    let workflows = workflow::Entity::find()
        .filter(workflow::Column::Active.eq(true))
        .all(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    let mut desired = BTreeMap::<String, TargetTable>::new();
    for workflow in &workflows {
        let Ok(definition) = serde_json::from_str::<Value>(&workflow.definition_json) else {
            continue;
        };
        let Some(triggers) =
            definition.pointer("/document/metadata/triggers").and_then(Value::as_array)
        else {
            continue;
        };
        for trigger in triggers.iter().filter(|trigger| trigger["type"] == "database") {
            let Some(table_name) =
                trigger["table"].as_str().map(str::trim).filter(|s| !s.is_empty())
            else {
                continue;
            };
            if let Some(target) = parse_table(backend, table_name) {
                desired.insert(target.key.clone(), target);
            }
        }
    }

    let registry = state
        .db
        .query_all_raw(Statement::from_string(
            backend,
            "SELECT source_table, trigger_name, function_name FROM database_trigger_registry",
        ))
        .await
        .map_err(|error| error.to_string())?;
    let mut existing = BTreeMap::new();
    for row in registry {
        let key: String = row.try_get("", "source_table").map_err(|error| error.to_string())?;
        let trigger_name: String =
            row.try_get("", "trigger_name").map_err(|error| error.to_string())?;
        let function_name: Option<String> = row.try_get("", "function_name").ok();
        existing.insert(key, (trigger_name, function_name));
    }

    for (key, (trigger_name, function_name)) in &existing {
        if desired.contains_key(key) {
            continue;
        }
        if let Some(table) = parse_table(backend, key) {
            drop_trigger(&state.db, &table, trigger_name, function_name.as_deref()).await?;
        }
        state
            .db
            .execute_unprepared(&format!(
                "DELETE FROM database_trigger_registry WHERE source_table = {}",
                sql_literal(key)
            ))
            .await
            .map_err(|error| error.to_string())?;
    }

    for (key, target) in desired {
        if existing.contains_key(&key) {
            continue;
        }
        let (trigger_name, function_name) = install_trigger(&state.db, &target).await?;
        let function = function_name.as_deref().map(sql_literal).unwrap_or_else(|| "NULL".into());
        state
            .db
            .execute_unprepared(&format!(
                "INSERT INTO database_trigger_registry (source_table, trigger_name, function_name) VALUES ({}, {}, {}) ON CONFLICT (source_table) DO UPDATE SET trigger_name = EXCLUDED.trigger_name, function_name = EXCLUDED.function_name",
                sql_literal(&key), sql_literal(&trigger_name), function
            ))
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn parse_table(backend: DatabaseBackend, key: &str) -> Option<TargetTable> {
    match backend {
        DatabaseBackend::Sqlite if !key.contains('.') && valid_identifier(key) => {
            Some(TargetTable { key: key.into(), schema: None, table: key.into() })
        }
        DatabaseBackend::Postgres => {
            let mut parts = key.split('.');
            let (schema, table) = (parts.next()?, parts.next()?);
            if parts.next().is_some() || !valid_identifier(schema) || !valid_identifier(table) {
                return None;
            }
            Some(TargetTable { key: key.into(), schema: Some(schema.into()), table: table.into() })
        }
        _ => None,
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && value.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

fn quoted_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn object_hash(value: &str) -> String {
    let hash = value.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

async fn install_trigger(
    db: &DatabaseConnection, target: &TargetTable,
) -> Result<(String, Option<String>), String> {
    let hash = object_hash(&target.key);
    let trigger_name = format!("ff_db_event_{hash}");
    let backend = db.get_database_backend();
    let schema = target.schema.as_deref().unwrap_or("public");
    let columns = table_columns(db, target).await?;
    if columns.is_empty() {
        return Err(format!("No columns found for table {}", target.key));
    }

    match backend {
        DatabaseBackend::Sqlite => {
            let object = sqlite_json_object(&columns, "NEW");
            let old_object = sqlite_json_object(&columns, "OLD");
            let table = quoted_identifier(&target.table);
            for (suffix, operation, old_row, new_row) in [
                ("insert", "INSERT", "NULL", object.as_str()),
                ("update", "UPDATE", old_object.as_str(), object.as_str()),
                ("delete", "DELETE", old_object.as_str(), "NULL"),
            ] {
                let name = quoted_identifier(&format!("{trigger_name}_{suffix}"));
                let sql = format!(
                    "CREATE TRIGGER IF NOT EXISTS {name} AFTER {operation} ON {table} BEGIN INSERT INTO database_events (source_table, operation, old_row_json, new_row_json, occurred_at) VALUES ({}, '{operation}', {old_row}, {new_row}, CURRENT_TIMESTAMP); END",
                    sql_literal(&target.key)
                );
                db.execute_unprepared(&sql).await.map_err(|error| error.to_string())?;
            }
            Ok((trigger_name, None))
        }
        DatabaseBackend::Postgres => {
            let function_name = format!("ff_db_event_fn_{hash}");
            let table =
                format!("{}.{}", quoted_identifier(schema), quoted_identifier(&target.table));
            let function =
                format!("{}.{}", quoted_identifier(schema), quoted_identifier(&function_name));
            let sql = format!(
                "CREATE OR REPLACE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN INSERT INTO database_events (source_table, operation, old_row_json, new_row_json, occurred_at) VALUES ({}, TG_OP, CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE to_jsonb(OLD)::text END, CASE WHEN TG_OP = 'DELETE' THEN NULL ELSE to_jsonb(NEW)::text END, CURRENT_TIMESTAMP); RETURN NULL; END; $$",
                sql_literal(&target.key)
            );
            db.execute_unprepared(&sql).await.map_err(|error| error.to_string())?;
            db.execute_unprepared(&format!(
                "CREATE TRIGGER {} AFTER INSERT OR UPDATE OR DELETE ON {table} FOR EACH ROW EXECUTE FUNCTION {function}()",
                quoted_identifier(&trigger_name)
            ))
            .await
            .map_err(|error| error.to_string())?;
            Ok((trigger_name, Some(function_name)))
        }
        _ => Err("Database triggers support SQLite and PostgreSQL only".into()),
    }
}

async fn drop_trigger(
    db: &DatabaseConnection, target: &TargetTable, trigger_name: &str, function_name: Option<&str>,
) -> Result<(), String> {
    let backend = db.get_database_backend();
    match backend {
        DatabaseBackend::Sqlite => {
            for suffix in ["insert", "update", "delete"] {
                db.execute_unprepared(&format!(
                    "DROP TRIGGER IF EXISTS {}",
                    quoted_identifier(&format!("{trigger_name}_{suffix}"))
                ))
                .await
                .map_err(|error| error.to_string())?;
            }
        }
        DatabaseBackend::Postgres => {
            let schema = target.schema.as_deref().unwrap_or("public");
            db.execute_unprepared(&format!(
                "DROP TRIGGER IF EXISTS {} ON {}.{}",
                quoted_identifier(trigger_name),
                quoted_identifier(schema),
                quoted_identifier(&target.table)
            ))
            .await
            .map_err(|error| error.to_string())?;
            if let Some(function_name) = function_name {
                db.execute_unprepared(&format!(
                    "DROP FUNCTION IF EXISTS {}.{}()",
                    quoted_identifier(schema),
                    quoted_identifier(function_name)
                ))
                .await
                .map_err(|error| error.to_string())?;
            }
        }
        _ => return Err("Database triggers support SQLite and PostgreSQL only".into()),
    }
    Ok(())
}

async fn table_columns(
    db: &DatabaseConnection, target: &TargetTable,
) -> Result<Vec<String>, String> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DatabaseBackend::Sqlite => format!(
            "SELECT name FROM pragma_table_info({}) ORDER BY cid",
            sql_literal(&target.table)
        ),
        DatabaseBackend::Postgres => format!(
            "SELECT column_name AS name FROM information_schema.columns WHERE table_schema = {} AND table_name = {} ORDER BY ordinal_position",
            sql_literal(target.schema.as_deref().unwrap_or("public")),
            sql_literal(&target.table)
        ),
        _ => return Err("Database triggers support SQLite and PostgreSQL only".into()),
    };
    let rows = db
        .query_all_raw(Statement::from_string(backend, sql))
        .await
        .map_err(|error| error.to_string())?;
    rows.iter()
        .map(|row| row.try_get::<String>("", "name").map_err(|error| error.to_string()))
        .collect()
}

fn sqlite_json_object(columns: &[String], row_name: &str) -> String {
    let fields = columns
        .iter()
        .map(|column| format!("{}, {row_name}.{}", sql_literal(column), quoted_identifier(column)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("json_object({fields})")
}

pub fn spawn_listener(state: AppState) {
    tokio::spawn(async move {
        let mut last_id = match latest_event_id(&state.db).await {
            Ok(id) => id,
            Err(error) => {
                error!(%error, "could not initialize database trigger listener");
                0
            }
        };
        let mut last_cleanup = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            match load_events_after(&state.db, last_id).await {
                Ok(events) => {
                    for event in events {
                        last_id = last_id.max(event.id);
                        dispatch_event(&state, event).await;
                    }
                }
                Err(error) => warn!(%error, "database trigger poll failed"),
            }
            if last_cleanup.elapsed() >= Duration::from_secs(60) {
                if let Err(error) = state
                    .db
                    .execute_unprepared(
                        "DELETE FROM database_events WHERE id <= (SELECT MAX(id) - 5000 FROM database_events)",
                    )
                    .await
                {
                    warn!(%error, "database trigger history cleanup failed");
                }
                last_cleanup = tokio::time::Instant::now();
            }
        }
    });
}

async fn latest_event_id(db: &DatabaseConnection) -> Result<i32, String> {
    let backend = db.get_database_backend();
    let row = db
        .query_one_raw(Statement::from_string(
            backend,
            "SELECT COALESCE(MAX(id), 0) AS id FROM database_events",
        ))
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "event cursor query returned no row".to_string())?;
    row.try_get("", "id").map_err(|error| error.to_string())
}

async fn load_events_after(db: &DatabaseConnection, id: i32) -> Result<Vec<ChangeEvent>, String> {
    let backend = db.get_database_backend();
    let rows = db
        .query_all_raw(Statement::from_string(
            backend,
            format!("SELECT id, source_table, operation, old_row_json, new_row_json FROM database_events WHERE id > {id} ORDER BY id ASC LIMIT 100"),
        ))
        .await
        .map_err(|error| error.to_string())?;
    rows.iter().map(parse_event).collect()
}

fn parse_event(row: &QueryResult) -> Result<ChangeEvent, String> {
    Ok(ChangeEvent {
        id: row.try_get("", "id").map_err(|error| error.to_string())?,
        source_table: row.try_get("", "source_table").map_err(|error| error.to_string())?,
        operation: row.try_get("", "operation").map_err(|error| error.to_string())?,
        old_row_json: row.try_get("", "old_row_json").ok(),
        new_row_json: row.try_get("", "new_row_json").ok(),
    })
}

async fn dispatch_event(state: &AppState, event: ChangeEvent) {
    let Ok(workflows) =
        workflow::Entity::find().filter(workflow::Column::Active.eq(true)).all(&state.db).await
    else {
        warn!("could not load workflows for database event");
        return;
    };
    for workflow in workflows {
        let Ok(definition) = serde_json::from_str::<Value>(&workflow.definition_json) else {
            continue;
        };
        let triggers = definition.pointer("/document/metadata/triggers").and_then(Value::as_array);
        let matches = triggers.is_some_and(|triggers| {
            triggers.iter().any(|trigger| {
                trigger["type"] == "database"
                    && trigger["table"].as_str() == Some(event.source_table.as_str())
                    && trigger["events"].as_array().is_some_and(|events| {
                        events.iter().any(|value| {
                            value
                                .as_str()
                                .is_some_and(|value| value.eq_ignore_ascii_case(&event.operation))
                        })
                    })
            })
        });
        if !matches {
            continue;
        }
        let parse_row = |value: Option<String>| {
            value
                .and_then(|value| serde_json::from_str::<Value>(&value).ok())
                .unwrap_or(Value::Null)
        };
        let input = json!({
            "database": {
                "table": event.source_table.clone(),
                "operation": event.operation.clone(),
                "old": parse_row(event.old_row_json.clone()),
                "new": parse_row(event.new_row_json.clone()),
            }
        });
        let request = ExecuteRequest {
            definition_json: workflow.definition_json.clone(),
            input: Some(input),
            workflow_id: Some(workflow.id),
            workflow_name: Some(workflow.name.clone()),
        };
        if let Err(error) = run_ows(state, request).await {
            error!(
                workflow_id = workflow.id,
                ?error,
                "database-triggered workflow failed to execute"
            );
        }
    }
}

pub async fn listen_for_event(
    db: &DatabaseConnection, table: &str,
) -> Result<Option<Value>, String> {
    let backend = db.get_database_backend();
    let target =
        parse_table(backend, table).ok_or_else(|| "invalid database table name".to_string())?;
    let start_id = latest_event_id(db).await?;
    let registry = db
        .query_one_raw(Statement::from_string(
            backend,
            format!(
                "SELECT source_table FROM database_trigger_registry WHERE source_table = {}",
                sql_literal(table)
            ),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let temporary_trigger = registry.is_none();
    if temporary_trigger {
        install_trigger(db, &target).await?;
    }

    let result = async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        while tokio::time::Instant::now() < deadline {
            let events = load_events_after(db, start_id).await?;
            if let Some(event) = events.into_iter().find(|event| event.source_table == table) {
                let parse_row = |value: Option<String>| {
                    value
                        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
                        .unwrap_or(Value::Null)
                };
                return Ok(Some(json!({
                    "id": event.id,
                    "table": event.source_table,
                    "operation": event.operation,
                    "old": parse_row(event.old_row_json),
                    "new": parse_row(event.new_row_json),
                })));
            }
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
        Ok(None)
    }
    .await;
    if temporary_trigger {
        let (trigger_name, function_name) = trigger_names(backend, table);
        drop_trigger(db, &target, &trigger_name, function_name.as_deref()).await?;
    }
    result
}

fn trigger_names(backend: DatabaseBackend, table: &str) -> (String, Option<String>) {
    let hash = object_hash(table);
    (
        format!("ff_db_event_{hash}"),
        (backend == DatabaseBackend::Postgres).then(|| format!("ff_db_event_fn_{hash}")),
    )
}
