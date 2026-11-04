use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Plugins::Table)
                    .if_not_exists()
                    .col(string(Plugins::Uid).primary_key())
                    .col(string(Plugins::Name).not_null())
                    .col(string(Plugins::Version).not_null().default("0.1.0"))
                    .col(string(Plugins::Description).null())
                    .col(string(Plugins::ManifestJson).not_null())
                    .col(string(Plugins::FilesJson).not_null().default("{}"))
                    .col(string(Plugins::SourceUrl).null())
                    .col(boolean(Plugins::Enabled).not_null().default(true))
                    .col(timestamp(Plugins::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(Plugins::UpdatedAt).default(Expr::current_timestamp()))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_plugins_enabled")
                    .table(Plugins::Table)
                    .col(Plugins::Enabled)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Plugins::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Plugins {
    #[sea_orm(iden = "plugins")]
    Table,
    Uid,
    Name,
    Version,
    Description,
    #[sea_orm(iden = "manifest_json")]
    ManifestJson,
    #[sea_orm(iden = "files_json")]
    FilesJson,
    #[sea_orm(iden = "source_url")]
    SourceUrl,
    Enabled,
    #[sea_orm(iden = "created_at")]
    CreatedAt,
    #[sea_orm(iden = "updated_at")]
    UpdatedAt,
}
