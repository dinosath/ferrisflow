//! Database and workflow primitives. Resource semantics belong to plugin workflows.
use crate::models::{plugin, workflow};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseBackend, EntityTrait, IntoActiveModel, Set,
    Statement,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn ident(s: &str) -> Result<String, String> {
    if s.is_empty()
        || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || !s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
    {
        return Err(format!("Invalid database identifier: {s}"));
    }
    Ok(format!("\"{s}\""))
}
fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| format!("{key} is required"))
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) async fn columns<C: ConnectionTrait>(
    db: &C, table: &str,
) -> Result<Vec<String>, String> {
    let quoted = ident(table)?;
    let backend = db.get_database_backend();
    let statement = if backend == DatabaseBackend::Postgres {
        Statement::from_sql_and_values(
            backend,
            "SELECT column_name AS name FROM information_schema.columns WHERE table_schema = 'public' AND table_name = $1 ORDER BY ordinal_position",
            [table.into()],
        )
    } else {
        Statement::from_string(backend, format!("PRAGMA table_info({quoted})"))
    };
    db.query_all_raw(statement)
        .await
        .map_err(err)?
        .into_iter()
        .map(|row| row.try_get("", "name").map_err(err))
        .collect()
}

pub(crate) async fn database<C: ConnectionTrait>(db: &C, args: &Value) -> Result<Value, String> {
    let table = required(args, "table")?;
    let quoted = ident(table)?;
    let operation = required(args, "operation")?;
    let backend = db.get_database_backend();
    if let Some(fallback) = args.get("fallback")
        && columns(db, table).await?.is_empty()
    {
        return Box::pin(database(db, fallback)).await;
    }
    let mut input = args.get("input").cloned().unwrap_or(json!({}));
    if let Some(request) = input.get("request").cloned() {
        input = request.get("body").filter(|v| v.is_object()).cloned().unwrap_or(json!({}));
        if let (Some(object), Some(params)) = (input.as_object_mut(), request["params"].as_object())
        {
            object.extend(params.clone());
        }
    }
    let original = input.clone();
    if let (Some(object), Some(fields)) = (input.as_object_mut(), args["ignoreFields"].as_array()) {
        for field in fields {
            if let Some(field) = field.as_str() {
                object.remove(field);
            }
        }
    }
    let json_fields = args["jsonFields"].as_object().cloned().unwrap_or_default();
    if let Some(object) = input.as_object_mut() {
        for (alias, column) in &json_fields {
            if let Some(value) = object.remove(alias) {
                object.insert(
                    column.as_str().ok_or("Invalid JSON field mapping")?.to_owned(),
                    Value::String(value.to_string()),
                );
            }
        }
    }
    if operation == "ensure" {
        let columns = args["columns"].as_array().ok_or("columns must be an array")?;
        if columns.is_empty() {
            return Err("At least one column is required".into());
        }
        let mut definitions = Vec::new();
        for column in columns {
            let name = ident(required(column, "name")?)?;
            let kind = required(column, "type")?;
            let primary = column["primaryKey"].as_bool() == Some(true);
            let sql_type = match kind {
                "integer" if primary && backend == DatabaseBackend::Postgres => "SERIAL",
                "integer" => "INTEGER",
                "number" => "DOUBLE PRECISION",
                "boolean" => "BOOLEAN",
                "string" | "json" => "TEXT",
                _ => return Err(format!("Unsupported column type: {kind}")),
            };
            definitions.push(format!(
                "{name} {sql_type}{}{}",
                if primary { " PRIMARY KEY" } else { "" },
                if column["required"].as_bool() == Some(true) { " NOT NULL" } else { "" }
            ));
        }
        db.execute_unprepared(&format!(
            "CREATE TABLE IF NOT EXISTS {quoted} ({})",
            definitions.join(",")
        ))
        .await
        .map_err(err)?;
        return Ok(if args["returnInput"] == true { original } else { json!({"table":table}) });
    }
    let primary = args["primaryKey"].as_str().unwrap_or("id");
    let pk = ident(primary)?;
    let mut key = args.get("key").cloned().or_else(|| input.get(primary).cloned());
    if args["keyType"] == "integer"
        && let Some(value) = key.as_ref().and_then(Value::as_str)
    {
        key = Some(json!(value.parse::<i64>().map_err(|_| "Record key must be an integer")?));
    }
    let mut bindings: Vec<sea_orm::Value> = Vec::new();
    let mut bind = |value: &Value| -> Result<String, String> {
        bindings.push(match value {
            Value::Null => sea_orm::Value::String(None),
            Value::Bool(v) => (*v).into(),
            Value::Number(v) if v.is_i64() => v.as_i64().unwrap().into(),
            Value::Number(v) => v.as_f64().unwrap().into(),
            Value::String(v) => v.clone().into(),
            _ => value.to_string().into(),
        });
        Ok(if backend == DatabaseBackend::Postgres {
            format!("${}", bindings.len())
        } else {
            format!("?{}", bindings.len())
        })
    };
    let mut filters = Vec::new();
    if let Some(filter) = args["filter"].as_object() {
        for (column, value) in filter {
            filters.push(format!("{} = {}", ident(column)?, bind(value)?));
        }
    }
    let where_filter = if filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", filters.join(" AND "))
    };
    let and_filter =
        if filters.is_empty() { String::new() } else { format!(" AND {}", filters.join(" AND ")) };
    let sql = match operation {
        "list" => format!("SELECT * FROM {quoted}{where_filter} LIMIT 1000"),
        "read" => format!(
            "SELECT * FROM {quoted} WHERE {pk} = {}{and_filter}",
            bind(key.as_ref().ok_or("Record key is required")?)?
        ),
        "delete" => format!(
            "DELETE FROM {quoted} WHERE {pk} = {}{and_filter} RETURNING *",
            bind(key.as_ref().ok_or("Record key is required")?)?
        ),
        "create" | "upsert" | "update" => {
            let object = input.as_object().ok_or("Record must be an object")?;
            if object.is_empty() {
                return Err("Record must have fields".into());
            }
            let mut names = Vec::new();
            let mut values = Vec::new();
            let mut updates = Vec::new();
            for (name, value) in object {
                names.push(ident(name)?);
                if operation != "update" {
                    values.push(bind(value)?);
                }
                if name != primary {
                    updates.push(format!("{} = EXCLUDED.{}", ident(name)?, ident(name)?));
                }
            }
            if operation == "update" {
                let sets = object
                    .iter()
                    .filter(|(name, _)| name.as_str() != primary)
                    .map(|(name, value)| Ok(format!("{} = {}", ident(name)?, bind(value)?)))
                    .collect::<Result<Vec<_>, String>>()?;
                if sets.is_empty() {
                    return Err("No fields to update".into());
                }
                format!(
                    "UPDATE {quoted} SET {} WHERE {pk} = {}{and_filter} RETURNING *",
                    sets.join(","),
                    bind(key.as_ref().ok_or("Record key is required")?)?
                )
            } else {
                format!(
                    "INSERT INTO {quoted} ({}) VALUES ({}){} RETURNING *",
                    names.join(","),
                    values.join(","),
                    if operation == "upsert" {
                        if updates.is_empty() {
                            format!(" ON CONFLICT ({pk}) DO NOTHING")
                        } else {
                            format!(" ON CONFLICT ({pk}) DO UPDATE SET {}", updates.join(","))
                        }
                    } else {
                        String::new()
                    }
                )
            }
        }
        _ => return Err(format!("Unsupported database operation: {operation}")),
    };
    // Convert rows to JSON in the database so column types and nulls are preserved.
    let sql = if matches!(operation, "list" | "read") {
        if backend == DatabaseBackend::Postgres {
            format!("SELECT row_to_json(record)::text AS payload FROM ({sql}) record")
        } else {
            let columns = db
                .query_all_raw(Statement::from_string(
                    backend,
                    format!("PRAGMA table_info({quoted})"),
                ))
                .await
                .map_err(err)?;
            let fields = columns
                .iter()
                .map(|row| {
                    let name: String = row.try_get("", "name").map_err(err)?;
                    Ok(format!("'{}', {}", name, ident(&name)?))
                })
                .collect::<Result<Vec<_>, String>>()?;
            format!("SELECT json_object({}) AS payload FROM ({sql})", fields.join(","))
        }
    } else {
        sql
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(backend, sql, bindings))
        .await
        .map_err(err)?;
    let mut output = Vec::new();
    if matches!(operation, "list" | "read") {
        for row in rows {
            let raw: String = row.try_get("", "payload").map_err(err)?;
            let mut value: Value = serde_json::from_str(&raw).map_err(err)?;
            for (alias, column) in &json_fields {
                if let Some(raw) = value
                    .as_object_mut()
                    .unwrap()
                    .remove(column.as_str().ok_or("Invalid JSON field mapping")?)
                {
                    value[alias] = if let Some(raw) = raw.as_str() {
                        serde_json::from_str(raw).map_err(err)?
                    } else {
                        raw
                    };
                }
            }
            output.push(value);
        }
        if operation == "read" {
            output.into_iter().next().ok_or("Record not found".into())
        } else {
            Ok(Value::Array(output))
        }
    } else if args["returnInput"] == true {
        Ok(original)
    } else if rows.is_empty() {
        Err("Record not found".into())
    } else {
        let key = rows[0]
            .try_get::<i64>("", primary)
            .map(|key| json!(key))
            .or_else(|_| rows[0].try_get::<i32>("", primary).map(|key| json!(key)))
            .or_else(|_| rows[0].try_get::<String>("", primary).map(|key| json!(key)))
            .map_err(err)?;
        Ok(json!({"success":true,"key":key,"record":original}))
    }
}

pub(crate) async fn save_workflow<C: ConnectionTrait>(
    db: &C, definition: Value,
) -> Result<Value, String> {
    let normalized = crate::ows::normalize_ows_json(&definition.to_string())?;
    let definition: Value = serde_json::from_str(&normalized).map_err(err)?;
    let namespace = required(&definition["document"], "namespace")?;
    let name = required(&definition["document"], "name")?;
    let owner =
        definition["document"]["metadata"]["ferrisflow"]["plugin"].as_str().unwrap_or(namespace);
    let identity = format!("{owner} · {name}");
    let existing = workflow::Entity::find().all(db).await.map_err(err)?.into_iter().find(|row| {
        serde_json::from_str::<Value>(&row.definition_json).is_ok_and(|saved| {
            saved["document"]["namespace"] == definition["document"]["namespace"]
                && saved["document"]["name"] == definition["document"]["name"]
        })
    });
    let mut active = existing.map(IntoActiveModel::into_active_model).unwrap_or_default();
    active.name = Set(identity);
    active.definition_json = Set(normalized);
    active.active = Set(true);
    active.version = Set(1);
    active.description = Set(definition["document"]["metadata"]["ferrisflow"]["description"]
        .as_str()
        .map(str::to_owned));
    let row = active.save(db).await.map_err(err)?;
    Ok(json!({"id":row.id.unwrap()}))
}
fn render(value: &Value, parameters: &Value) -> Result<Value, String> {
    Ok(match value {
        Value::String(s) => {
            let mut s = s.clone();
            if let Some(parameters) = parameters.as_object() {
                for (k, v) in parameters {
                    if let Some(v) = v.as_str() {
                        s = s.replace(&format!("{{{{{k}}}}}"), v);
                    }
                }
            }
            if s.contains("{{") {
                return Err(format!("Unresolved template parameter: {s}"));
            }
            Value::String(s)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| render(v, parameters)).collect::<Result<_, _>>()?)
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("$parameter") => parameters
            .get(required(value, "$parameter")?)
            .cloned()
            .ok_or("Missing template parameter")?,
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), render(v, parameters)?)))
                .collect::<Result<_, String>>()?,
        ),
        v => v.clone(),
    })
}

// Runtimes retain their invokers; a weak reference keeps completed hooks from
// retaining a transaction and preventing commit or rollback.
pub(crate) struct Function<C>(pub std::sync::Weak<C>);
#[async_trait::async_trait]
impl<C: ConnectionTrait + Send + Sync + 'static> ows::service::FunctionInvoker for Function<C> {
    async fn invoke(
        &self, request: ows::service::FunctionRequest<'_>,
    ) -> Result<Value, ows_core::WorkflowError> {
        self.invoke_inner(&json!(request.args)).await.map_err(|e| {
            ows_core::WorkflowError::new(
                ows_core::ErrorKind::Runtime,
                ows_core::ProblemDetails::new("about:blank", "Primitive failed", 400)
                    .with_detail(e),
            )
        })
    }
}
impl<C: ConnectionTrait> Function<C> {
    async fn invoke_inner(&self, args: &Value) -> Result<Value, String> {
        let db = self.0.upgrade().ok_or("Primitive database context expired")?;
        match required(args, "primitive")? {
            "database" => database(&*db, args).await,
            "workflow.save" => save_workflow(&*db, args["definition"].clone()).await,
            "workflow.instantiate" => {
                let row = plugin::Entity::find_by_id(required(args, "plugin")?)
                    .one(&*db)
                    .await
                    .map_err(err)?
                    .ok_or("Template plugin not installed")?;
                if !row.enabled {
                    return Err("Template plugin is disabled".into());
                }
                let manifest: Value = serde_json::from_str(&row.manifest_json).map_err(err)?;
                let templates = manifest["workflowTemplates"][required(args, "template")?]
                    .as_array()
                    .ok_or("Workflow template not found")?;
                let mut results = Vec::new();
                for template in templates {
                    results
                        .push(save_workflow(&*db, render(template, &args["parameters"])?).await?);
                }
                Ok(Value::Array(results))
            }
            other => Err(format!("Unknown primitive: {other}")),
        }
    }
}

pub(crate) async fn run<C: ConnectionTrait + Send + Sync + 'static>(
    db: Arc<C>, definition: &Value, input: Value,
) -> Result<Value, String> {
    let definition = crate::ows::parse_ows_document(&definition.to_string())?;
    let runtime = ows::Runtime::builder()
        .register_function("ferrisflow.primitive", Arc::new(Function(Arc::downgrade(&db))))
        .build()
        .map_err(err)?;
    let workflow = runtime.register_definition(&definition).map_err(err)?;
    runtime.execute(workflow, input).await.map_err(err)?.wait().await.map_err(|error| {
        error
            .problem
            .detail
            .as_ref()
            .map(|detail| format!("{error}: {detail}"))
            .unwrap_or_else(|| error.to_string())
    })
}
