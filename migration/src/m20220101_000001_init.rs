use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create table workflows (without foreign keys)
        manager
            .create_table(
                Table::create()
                    .table(Workflow::Table)
                    .if_not_exists()
                    .col(pk_auto(Workflow::Id))
                    .col(timestamp(Workflow::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(Workflow::LastUpdated).default(Expr::current_timestamp()))
                    .col(boolean(Workflow::Active).not_null())
                    .col(string(Workflow::DefinitionJson).not_null())
                    .col(string(Workflow::Description).null())
                    .col(string(Workflow::Name).not_null())
                    .col(integer(Workflow::Version).not_null())
                    .to_owned(),
            )
            .await?;

        // Now add all foreign key constraints (after all tables exist)
        // Create many-to-many junction tables
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop many-to-many junction tables first
        // Drop foreign keys first (in reverse order)
        // Drop entity tables in reverse order
        manager.drop_table(Table::drop().table(Workflow::Table).to_owned()).await?;
        Ok(())
    }
}

// Define Iden enums for tables and columns
#[derive(DeriveIden)]
#[sea_orm(iden = "workflows")]
enum Workflow {
    #[sea_orm(iden = "workflows")]
    Table,
    Id,
    #[sea_orm(iden = "created_at")]
    CreatedAt,
    #[sea_orm(iden = "last_updated")]
    LastUpdated,
    #[sea_orm(iden = "active")]
    Active,
    #[sea_orm(iden = "definition_json")]
    DefinitionJson,
    #[sea_orm(iden = "description")]
    Description,
    #[sea_orm(iden = "name")]
    Name,
    #[sea_orm(iden = "version")]
    Version,
}

// Define Iden enums for junction tables
