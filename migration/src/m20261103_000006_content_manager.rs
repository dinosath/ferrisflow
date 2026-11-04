use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ContentTypes::Table)
                    .if_not_exists()
                    .col(string(ContentTypes::Uid).primary_key())
                    .col(string(ContentTypes::DisplayName).not_null())
                    .col(string(ContentTypes::SchemaJson).not_null())
                    .col(timestamp(ContentTypes::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(ContentTypes::UpdatedAt).default(Expr::current_timestamp()))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(ContentEntries::Table)
                    .if_not_exists()
                    .col(pk_auto(ContentEntries::Id))
                    .col(string(ContentEntries::ContentTypeUid).not_null())
                    .col(string(ContentEntries::DataJson).not_null())
                    .col(boolean(ContentEntries::Published).not_null().default(false))
                    .col(timestamp(ContentEntries::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(ContentEntries::UpdatedAt).default(Expr::current_timestamp()))
                    .foreign_key(
                        ForeignKey::create()
                            .from(ContentEntries::Table, ContentEntries::ContentTypeUid)
                            .to(ContentTypes::Table, ContentTypes::Uid)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_content_entries_type")
                    .table(ContentEntries::Table)
                    .col(ContentEntries::ContentTypeUid)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(ContentViews::Table)
                    .if_not_exists()
                    .col(pk_auto(ContentViews::Id))
                    .col(string(ContentViews::ContentTypeUid).not_null())
                    .col(string(ContentViews::Name).not_null())
                    .col(string(ContentViews::ViewType).not_null().default("grid"))
                    .col(string(ContentViews::ConfigJson).not_null().default("{}"))
                    .col(boolean(ContentViews::IsDefault).not_null().default(false))
                    .col(integer(ContentViews::Position).not_null().default(0))
                    .col(timestamp(ContentViews::CreatedAt).default(Expr::current_timestamp()))
                    .col(timestamp(ContentViews::UpdatedAt).default(Expr::current_timestamp()))
                    .foreign_key(
                        ForeignKey::create()
                            .from(ContentViews::Table, ContentViews::ContentTypeUid)
                            .to(ContentTypes::Table, ContentTypes::Uid)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_content_views_type")
                    .table(ContentViews::Table)
                    .col(ContentViews::ContentTypeUid)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ContentViews::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ContentEntries::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ContentTypes::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum ContentTypes {
    #[sea_orm(iden = "content_types")]
    Table,
    Uid,
    #[sea_orm(iden = "display_name")]
    DisplayName,
    #[sea_orm(iden = "schema_json")]
    SchemaJson,
    #[sea_orm(iden = "created_at")]
    CreatedAt,
    #[sea_orm(iden = "updated_at")]
    UpdatedAt,
}
#[derive(DeriveIden)]
enum ContentEntries {
    #[sea_orm(iden = "content_entries")]
    Table,
    Id,
    #[sea_orm(iden = "content_type_uid")]
    ContentTypeUid,
    #[sea_orm(iden = "data_json")]
    DataJson,
    Published,
    #[sea_orm(iden = "created_at")]
    CreatedAt,
    #[sea_orm(iden = "updated_at")]
    UpdatedAt,
}
#[derive(DeriveIden)]
enum ContentViews {
    #[sea_orm(iden = "content_views")]
    Table,
    Id,
    #[sea_orm(iden = "content_type_uid")]
    ContentTypeUid,
    Name,
    #[sea_orm(iden = "view_type")]
    ViewType,
    #[sea_orm(iden = "config_json")]
    ConfigJson,
    #[sea_orm(iden = "is_default")]
    IsDefault,
    Position,
    #[sea_orm(iden = "created_at")]
    CreatedAt,
    #[sea_orm(iden = "updated_at")]
    UpdatedAt,
}
