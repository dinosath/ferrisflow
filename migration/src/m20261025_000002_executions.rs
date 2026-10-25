use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Execution::Table)
                    .if_not_exists()
                    .col(pk_auto(Execution::Id))
                    .col(integer_null(Execution::WorkflowId))
                    .col(string(Execution::WorkflowName).not_null())
                    .col(string(Execution::Status).not_null())
                    .col(timestamp(Execution::StartedAt).default(Expr::current_timestamp()))
                    .col(timestamp_null(Execution::FinishedAt))
                    .col(big_integer_null(Execution::DurationMs))
                    .col(string(Execution::InputJson).not_null())
                    .col(string_null(Execution::OutputJson))
                    .col(string_null(Execution::Error))
                    .col(string(Execution::DefinitionJson).not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_executions_started_at")
                    .table(Execution::Table)
                    .col(Execution::StartedAt)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Execution::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Execution {
    #[sea_orm(iden = "executions")]
    Table,
    Id,
    #[sea_orm(iden = "workflow_id")]
    WorkflowId,
    #[sea_orm(iden = "workflow_name")]
    WorkflowName,
    Status,
    #[sea_orm(iden = "started_at")]
    StartedAt,
    #[sea_orm(iden = "finished_at")]
    FinishedAt,
    #[sea_orm(iden = "duration_ms")]
    DurationMs,
    #[sea_orm(iden = "input_json")]
    InputJson,
    #[sea_orm(iden = "output_json")]
    OutputJson,
    Error,
    #[sea_orm(iden = "definition_json")]
    DefinitionJson,
}
