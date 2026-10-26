use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DatabaseEvent::Table)
                    .if_not_exists()
                    .col(pk_auto(DatabaseEvent::Id))
                    .col(string(DatabaseEvent::SourceTable).not_null())
                    .col(string(DatabaseEvent::Operation).not_null())
                    .col(string(DatabaseEvent::OldRowJson).null())
                    .col(string(DatabaseEvent::NewRowJson).null())
                    .col(timestamp(DatabaseEvent::OccurredAt).default(Expr::current_timestamp()))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(DatabaseTriggerRegistry::Table)
                    .if_not_exists()
                    .col(string(DatabaseTriggerRegistry::SourceTable).primary_key())
                    .col(string(DatabaseTriggerRegistry::TriggerName).not_null())
                    .col(string(DatabaseTriggerRegistry::FunctionName).null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(DatabaseTriggerRegistry::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(DatabaseEvent::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum DatabaseEvent {
    #[sea_orm(iden = "database_events")]
    Table,
    Id,
    #[sea_orm(iden = "source_table")]
    SourceTable,
    Operation,
    #[sea_orm(iden = "old_row_json")]
    OldRowJson,
    #[sea_orm(iden = "new_row_json")]
    NewRowJson,
    #[sea_orm(iden = "occurred_at")]
    OccurredAt,
}

#[derive(DeriveIden)]
enum DatabaseTriggerRegistry {
    #[sea_orm(iden = "database_trigger_registry")]
    Table,
    #[sea_orm(iden = "source_table")]
    SourceTable,
    #[sea_orm(iden = "trigger_name")]
    TriggerName,
    #[sea_orm(iden = "function_name")]
    FunctionName,
}
