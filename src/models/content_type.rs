use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "content_types")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub uid: String,
    pub display_name: String,
    pub schema_json: String,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

impl ActiveModelBehavior for ActiveModel {}
