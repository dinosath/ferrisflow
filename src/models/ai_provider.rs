use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ai_providers")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub active: bool,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

impl ActiveModelBehavior for ActiveModel {}
