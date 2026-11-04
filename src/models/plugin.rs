use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "plugins")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub uid: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest_json: String,
    pub files_json: String,
    pub source_url: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

impl ActiveModelBehavior for ActiveModel {}
