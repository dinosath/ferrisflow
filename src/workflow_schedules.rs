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
