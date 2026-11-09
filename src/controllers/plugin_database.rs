use crate::models::{content_entry, content_type};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Field {
    name: String,
    label: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    options: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Schema {
    fields: Vec<Field>,
}

pub(crate) async fn execute<C: ConnectionTrait>(
    db: &C, resource: &str, operation: &str, uid: Option<&str>, id: Option<i32>, input: Value,
) -> Result<Value, String> {
    match resource {
        "content-types" => execute_content_type(db, operation, uid, input).await,
        "content-records" => {
            let uid = uid.ok_or_else(|| "content type UID is required".to_owned())?;
            execute_record(db, uid, operation, id, input).await
        }
        _ => Err(format!("unknown plugin database resource '{resource}'")),
    }
}

async fn execute_content_type<C: ConnectionTrait>(
    db: &C, operation: &str, path_uid: Option<&str>, input: Value,
) -> Result<Value, String> {
    match operation {
        "list" => {
            let rows = content_type::Entity::find()
                .filter(content_type::Column::Uid.ne("core.page"))
                .order_by_asc(content_type::Column::DisplayName)
                .all(db)
                .await
                .map_err(db_error)?;
            rows.into_iter().map(type_value).collect::<Result<Vec<_>, _>>().map(Value::Array)
        }
        "read" => {
            let uid = required_uid(path_uid, &input)?;
            let row = content_type::Entity::find_by_id(uid)
                .one(db)
                .await
                .map_err(db_error)?
                .ok_or_else(|| "Content type not found".to_owned())?;
            type_value(row)
        }
        "create" => {
            let uid = input.get("uid").and_then(Value::as_str).unwrap_or_default().trim();
            let name = input.get("display_name").and_then(Value::as_str).unwrap_or_default().trim();
            let schema: Schema =
                serde_json::from_value(input.get("schema").cloned().unwrap_or(Value::Null))
                    .map_err(|error| format!("Invalid content type schema: {error}"))?;
            validate_schema(&uid, name, &schema)?;
            let row = content_type::ActiveModel {
                uid: Set(uid.to_owned()),
                display_name: Set(name.to_owned()),
                schema_json: Set(serde_json::to_string(&schema).map_err(db_error)?),
                ..Default::default()
            }
            .insert(db)
            .await
            .map_err(db_error)?;
            type_value(row)
        }
        "update" => {
            let uid = required_uid(path_uid, &input)?;
            let name = input.get("display_name").and_then(Value::as_str).unwrap_or_default().trim();
            let schema: Schema =
                serde_json::from_value(input.get("schema").cloned().unwrap_or(Value::Null))
                    .map_err(|error| format!("Invalid content type schema: {error}"))?;
            validate_schema(&uid, name, &schema)?;
            let mut row = content_type::Entity::find_by_id(uid.clone())
                .one(db)
                .await
                .map_err(db_error)?
                .ok_or_else(|| "Content type not found".to_owned())?
                .into_active_model();
            row.display_name = Set(name.to_owned());
            row.schema_json = Set(serde_json::to_string(&schema).map_err(db_error)?);
            row.updated_at = Set(chrono::Utc::now().naive_utc());
            let row = row.update(db).await.map_err(db_error)?;
            type_value(row)
        }
        "delete" => {
            let uid = required_uid(path_uid, &input)?;
            content_entry::Entity::delete_many()
                .filter(content_entry::Column::ContentTypeUid.eq(uid.clone()))
                .exec(db)
                .await
                .map_err(db_error)?;
            if content_type::Entity::delete_by_id(uid.clone())
                .exec(db)
                .await
                .map_err(db_error)?
                .rows_affected
                == 0
            {
                return Err("Content type not found".to_owned());
            }
            Ok(serde_json::json!({"deleted":true,"uid":uid}))
        }
        _ => Err(format!("unsupported content type operation '{operation}'")),
    }
}

async fn execute_record<C: ConnectionTrait>(
    db: &C, uid: &str, operation: &str, path_id: Option<i32>, mut input: Value,
) -> Result<Value, String> {
    let schema = schema_for(db, uid).await?;
    let id =
        path_id.or_else(|| input.get("id").and_then(as_i64).and_then(|id| i32::try_from(id).ok()));
    match operation {
        "list" => {
            let rows = content_entry::Entity::find()
                .filter(content_entry::Column::ContentTypeUid.eq(uid.to_owned()))
                .order_by_desc(content_entry::Column::UpdatedAt)
                .all(db)
                .await
                .map_err(db_error)?;
            rows.into_iter().map(record_value).collect::<Result<Vec<_>, _>>().map(Value::Array)
        }
        "read" => {
            let id = id.ok_or_else(|| "Record id is required".to_owned())?;
            let row = content_entry::Entity::find_by_id(id)
                .filter(content_entry::Column::ContentTypeUid.eq(uid.to_owned()))
                .one(db)
                .await
                .map_err(db_error)?
                .ok_or_else(|| "Record not found".to_owned())?;
            record_value(row)
        }
        "create" | "update" => {
            if input.get("data").is_none() {
                let mut data = input.as_object().cloned().unwrap_or_default();
                data.remove("id");
                data.remove("uid");
                input = serde_json::json!({"data":data,"published":input.get("published").and_then(Value::as_bool).unwrap_or(false)});
            }
            let data = input
                .get("data")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or_else(|| "Record data must be a JSON object".to_owned())?;
            validate_data(&schema, &data)?;
            let published = input.get("published").and_then(Value::as_bool).unwrap_or(false);
            let row = if operation == "create" {
                content_entry::ActiveModel {
                    content_type_uid: Set(uid.to_owned()),
                    data_json: Set(data.to_string()),
                    published: Set(published),
                    ..Default::default()
                }
                .insert(db)
                .await
                .map_err(db_error)?
            } else {
                let id = id.ok_or_else(|| "Record id is required".to_owned())?;
                let mut row = content_entry::Entity::find_by_id(id)
                    .filter(content_entry::Column::ContentTypeUid.eq(uid.to_owned()))
                    .one(db)
                    .await
                    .map_err(db_error)?
                    .ok_or_else(|| "Record not found".to_owned())?
                    .into_active_model();
                row.data_json = Set(data.to_string());
                row.published = Set(published);
                row.updated_at = Set(chrono::Utc::now().naive_utc());
                row.update(db).await.map_err(db_error)?
            };
            record_value(row)
        }
        "delete" => {
            let id = id.ok_or_else(|| "Record id is required".to_owned())?;
            if content_entry::Entity::delete_by_id(id)
                .filter(content_entry::Column::ContentTypeUid.eq(uid.to_owned()))
                .exec(db)
                .await
                .map_err(db_error)?
                .rows_affected
                == 0
            {
                return Err("Record not found".to_owned());
            }
            Ok(serde_json::json!({"deleted":true,"id":id}))
        }
        _ => Err(format!("unsupported record operation '{operation}'")),
    }
}

async fn schema_for<C: ConnectionTrait>(db: &C, uid: &str) -> Result<Schema, String> {
    let row = content_type::Entity::find_by_id(uid.to_owned())
        .one(db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| "Content type not found".to_owned())?;
    serde_json::from_str(&row.schema_json).map_err(db_error)
}

fn type_value(row: content_type::Model) -> Result<Value, String> {
    let schema: Schema = serde_json::from_str(&row.schema_json).map_err(db_error)?;
    Ok(
        serde_json::json!({"uid":row.uid,"display_name":row.display_name,"schema":schema,"created_at":row.created_at,"updated_at":row.updated_at}),
    )
}

fn record_value(row: content_entry::Model) -> Result<Value, String> {
    let data: Value = serde_json::from_str(&row.data_json).map_err(db_error)?;
    Ok(
        serde_json::json!({"id":row.id,"data":data,"published":row.published,"created_at":row.created_at,"updated_at":row.updated_at}),
    )
}

fn required_uid(path_uid: Option<&str>, input: &Value) -> Result<String, String> {
    let uid = path_uid
        .map(str::to_owned)
        .or_else(|| input.get("uid").and_then(Value::as_str).map(str::to_owned))
        .ok_or_else(|| "Content type UID is required".to_owned())?;
    if uid == "core.page" {
        return Err("core.page is reserved for standalone page storage".into());
    }
    Ok(uid)
}

fn validate_schema(uid: &str, name: &str, schema: &Schema) -> Result<(), String> {
    if uid == "core.page" {
        return Err("core.page is reserved for standalone page storage".into());
    }
    if uid.is_empty() || uid.len() > 64 || !uid.split('.').all(valid_identifier) {
        return Err(
            "UID must contain letters, numbers, underscores, or dot-separated segments".into()
        );
    }
    if name.is_empty() {
        return Err("Display name is required".into());
    }
    if schema.fields.is_empty() {
        return Err("A content type needs at least one field".into());
    }
    let mut names = std::collections::HashSet::new();
    for field in &schema.fields {
        if !valid_identifier(&field.name) || !names.insert(field.name.to_lowercase()) {
            return Err(format!("Invalid or duplicate field name: {}", field.name));
        }
        if field.label.trim().is_empty() {
            return Err(format!("Field {} needs a label", field.name));
        }
        if !matches!(
            field.kind.as_str(),
            "text" | "richtext" | "number" | "boolean" | "date" | "email" | "json" | "enum"
        ) {
            return Err(format!("Unsupported field type: {}", field.kind));
        }
        if field.kind == "enum" && field.options.as_ref().is_none_or(Vec::is_empty) {
            return Err(format!("Enum field {} needs options", field.name));
        }
    }
    Ok(())
}

fn validate_data(schema: &Schema, data: &Value) -> Result<(), String> {
    let object = data.as_object().ok_or_else(|| "Record data must be a JSON object".to_owned())?;
    for field in &schema.fields {
        let value = object.get(&field.name);
        if field.required && value.is_none_or(Value::is_null) {
            return Err(format!("{} is required", field.label));
        }
        if let Some(value) = value {
            let valid = match field.kind.as_str() {
                "text" | "richtext" | "date" | "email" => value.is_string(),
                "number" => value.is_number(),
                "boolean" => value.is_boolean(),
                "json" => true,
                "enum" => value.as_str().is_some_and(|item| {
                    field.options.as_ref().is_some_and(|options| options.contains(&item.to_owned()))
                }),
                _ => false,
            };
            if !valid {
                return Err(format!("{} has an invalid value", field.label));
            }
        }
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && value.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

fn as_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str().and_then(|id| id.parse().ok()))
}

fn db_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
