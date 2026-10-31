use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AiProvider::Table)
                    .if_not_exists()
                    .col(pk_auto(AiProvider::Id))
                    .col(string(AiProvider::Name).not_null())
                    .col(string(AiProvider::Provider).not_null())
                    .col(string(AiProvider::BaseUrl).not_null())
                    .col(string(AiProvider::Model).not_null())
                    .col(string(AiProvider::ApiKey).not_null().default(""))
                    .col(boolean(AiProvider::Active).not_null().default(false))
                    .col(timestamp(AiProvider::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(AiProvider::UpdatedAt).default(Expr::current_timestamp()))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ai_providers_active")
                    .table(AiProvider::Table)
                    .col(AiProvider::Active)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(AiProvider::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum AiProvider {
    #[sea_orm(iden = "ai_providers")]
    Table,
    Id,
    Name,
    Provider,
    #[sea_orm(iden = "base_url")]
    BaseUrl,
    Model,
    #[sea_orm(iden = "api_key")]
    ApiKey,
    Active,
    #[sea_orm(iden = "created_at")]
    CreatedAt,
    #[sea_orm(iden = "updated_at")]
    UpdatedAt,
}
