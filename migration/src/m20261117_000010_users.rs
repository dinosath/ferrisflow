use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(pk_auto(Users::Id))
                    .col(string(Users::Username).not_null())
                    .col(string_null(Users::Email))
                    .col(string(Users::DisplayName).not_null())
                    .col(timestamp(Users::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(Users::UpdatedAt).default(Expr::current_timestamp()))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_users_username_unique")
                    .table(Users::Table)
                    .col(Users::Username)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Users::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Users {
    #[sea_orm(iden = "users")]
    Table,
    Id,
    Username,
    Email,
    #[sea_orm(iden = "display_name")]
    DisplayName,
    #[sea_orm(iden = "created_at")]
    CreatedAt,
    #[sea_orm(iden = "updated_at")]
    UpdatedAt,
}
