use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Execution::Table)
                    .add_column(string(Execution::LogsJson).not_null().default("[]"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter().table(Execution::Table).drop_column(Execution::LogsJson).to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Execution {
    #[sea_orm(iden = "executions")]
    Table,
    #[sea_orm(iden = "logs_json")]
    LogsJson,
}
