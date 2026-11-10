//! Keep native OWS schedules in sync with enabled, saved workflows.
use crate::{app_state::AppState, controllers::primitives};
use std::{sync::Arc, time::Duration};

pub(crate) fn spawn(state: AppState) {
    tokio::spawn(watch(state, Duration::from_secs(5)));
}

async fn watch(state: AppState, refresh: Duration) {
    // Keep a strong reference while function invokers hold weak references.
    let db = Arc::new(state.db);
    let mut signature = String::new();
    let mut running: Vec<(ows::Runtime, ows::ScheduleSet)> = Vec::new();
    let mut interval = tokio::time::interval(refresh);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let result = async {
            let definitions = primitives::definitions(&*db).await?;
            let next = serde_json::to_string(&definitions).map_err(|error| error.to_string())?;
            if next == signature {
                return Ok::<_, String>(());
            }
            // Dropping the previous ScheduleSet cancels removed/disabled schedules.
            running.clear();
            for definition in
                definitions.into_iter().filter(|definition| definition.get("schedule").is_some())
            {
                let runtime = primitives::runtime(db.clone(), definition).await?;
                let schedules =
                    runtime.start_schedules().await.map_err(|error| error.to_string())?;
                running.push((runtime, schedules));
            }
            signature = next;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(%error, "Could not reconcile workflow schedules");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{controllers::plugin_resources, models::plugin};
    use migration::MigratorTrait;
    use sea_orm::{ConnectionTrait, Database, EntityTrait, Set};
    use serde_json::json;

    #[tokio::test]
    async fn schedules_start_after_import_and_stop_when_plugin_is_disabled() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        db.execute_unprepared("CREATE TABLE ticks (id INTEGER PRIMARY KEY, value INTEGER)")
            .await
            .unwrap();
        let state = AppState { db };
        let watcher = tokio::spawn(watch(state.clone(), Duration::from_millis(10)));
        // Starting the watcher before installation exercises live discovery.
        let manifest = json!({"id":"example.ticks","version":"1.0.0","workflows":[{
            "document":{"dsl":"1.0.3","namespace":"tests","name":"tick","version":"1.0.0","metadata":{"ferrisflow":{"plugin":"example.ticks"}}},
            "schedule":{"every":{"milliseconds":20}},
            "do":[{"tick":{"call":"ferrisflow.primitive","with":{"primitive":"database","operation":"create","table":"ticks","input":{"value":1}}}}]
        }]});
        plugin_resources::persist(
            &state.db,
            plugin::ActiveModel {
                uid: Set("example.ticks".into()),
                name: Set("Ticks".into()),
                version: Set("1.0.0".into()),
                manifest_json: Set(manifest.to_string()),
                files_json: Set("{}".into()),
                enabled: Set(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let count = || async {
            primitives::database(&state.db, &json!({"operation":"list","table":"ticks"}))
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .len()
        };
        tokio::time::timeout(Duration::from_secs(3), async {
            while count().await < 2 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut active: plugin::ActiveModel = plugin::Entity::find_by_id("example.ticks")
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .into();
        active.enabled = Set(false);
        plugin_resources::persist(&state.db, active).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let stopped = count().await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(count().await, stopped);
        watcher.abort();
        let _ = watcher.await;
    }
}
