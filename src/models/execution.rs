use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "executions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub workflow_id: Option<i32>,
    pub workflow_name: String,
    pub status: String,
    pub started_at: DateTime,
    pub finished_at: Option<DateTime>,
    pub duration_ms: Option<i64>,
    pub input_json: String,
    pub output_json: Option<String>,
    pub error: Option<String>,
    pub definition_json: String,
}

impl ActiveModelBehavior for ActiveModel {}
