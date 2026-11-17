use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "app_settings")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub key: String,
    pub value: String,
}

impl ActiveModelBehavior for ActiveModel {}
