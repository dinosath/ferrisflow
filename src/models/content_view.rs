use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "content_views")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub content_type_uid: String,
    pub name: String,
    pub view_type: String,
    pub config_json: String,
    pub is_default: bool,
    pub position: i32,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

impl ActiveModelBehavior for ActiveModel {}
