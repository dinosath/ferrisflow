pub use sea_orm_migration::prelude::*;

mod m20220101_000001_init;
mod m20261025_000002_executions;
mod m20261025_000003_execution_logs;
mod migrations {
    automod::dir!("src/migrations");
}

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_init::Migration),
            Box::new(m20261025_000002_executions::Migration),
            Box::new(m20261025_000003_execution_logs::Migration),
        ]
    }
}
