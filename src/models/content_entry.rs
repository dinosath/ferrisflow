use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "content_entries")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub content_type_uid: String,
    pub data_json: String,
    pub published: bool,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

impl ActiveModelBehavior for ActiveModel {}
