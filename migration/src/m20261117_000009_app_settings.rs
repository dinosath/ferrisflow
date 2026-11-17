use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AppSettings::Table)
                    .if_not_exists()
                    .col(string(AppSettings::Key).primary_key())
                    .col(text(AppSettings::Value).not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(AppSettings::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum AppSettings {
    #[sea_orm(iden = "app_settings")]
    Table,
    Key,
    Value,
}
