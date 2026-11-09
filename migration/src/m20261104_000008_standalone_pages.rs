use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let insert = Query::insert()
            .into_table(ContentTypes::Table)
            .columns([ContentTypes::Uid, ContentTypes::DisplayName, ContentTypes::SchemaJson])
            .values_panic([
                Expr::value("core.page"),
                Expr::value("Standalone pages"),
                Expr::value(r#"{"fields":[]}"#),
            ])
            .on_conflict(OnConflict::column(ContentTypes::Uid).do_nothing().to_owned())
            .to_owned();
        manager.exec_stmt(insert).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(ContentTypes::Table)
                    .and_where(Expr::col(ContentTypes::Uid).eq("core.page"))
                    .to_owned(),
            )
            .await?;
        Ok(())
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
}
