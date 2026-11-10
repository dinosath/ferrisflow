use super::{plugin_resources, primitives};
use crate::models::plugin;
use migration::MigratorTrait;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Set};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Brightnovels {
    chapters: Mutex<Vec<u32>>,
    requests: Mutex<Vec<String>>,
    fail: Mutex<Option<u32>>,
}

#[async_trait::async_trait]
impl ows::service::FunctionInvoker for Brightnovels {
    async fn invoke(
        &self, request: ows::service::FunctionRequest<'_>,
    ) -> Result<Value, ows_core::WorkflowError> {
        let endpoint = request.args["endpoint"].as_str().unwrap().to_owned();
        self.requests.lock().unwrap().push(endpoint.clone());
        if endpoint.ends_with("/chapters") {
            return Ok(json!({"chapters": self.chapters.lock().unwrap().iter().map(|number| {
                json!({"number":number,"slug":format!("chapter-{number}"),"name":format!("Chapter {number}"),"is_premium": *number == 3})
            }).collect::<Vec<_>>()}));
        }
        let page = if let Some((_, number)) = endpoint.rsplit_once("/chapter-") {
            let number = number.parse::<u32>().unwrap();
            if *self.fail.lock().unwrap() == Some(number) {
                return Err(ows_core::WorkflowError::new(
                    ows_core::ErrorKind::Runtime,
                    ows_core::ProblemDetails::new("about:blank", "Chapter fetch failed", 503),
                ));
            }
            json!({"props":{"chapter":{"content":format!("&lt;p&gt;Chapter {number} &amp; text&lt;/p&gt;")}}})
        } else {
            json!({"props":{"series":{"title":"Dune","user":{"name":"Frank Herbert"},"description":"Desert planet","cover":{"path":"dune.png"}}}})
        };
        Ok(json!(format!("<div data-page=\"{}\"></div>", page.to_string().replace('"', "&quot;"))))
    }
}

async fn installed() -> Arc<DatabaseConnection> {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    for text in [
        include_str!("../../plugins/ferrisflow.content-types.json"),
        include_str!("../../plugins/local.novels.json"),
    ] {
        let manifest: Value = serde_json::from_str(text).unwrap();
        plugin_resources::persist(
            &db,
            plugin::ActiveModel {
                uid: Set(manifest["id"].as_str().unwrap().into()),
                name: Set(manifest["name"].as_str().unwrap().into()),
                version: Set(manifest["version"].as_str().unwrap().into()),
                manifest_json: Set(manifest.to_string()),
                files_json: Set("{}".into()),
                enabled: Set(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    Arc::new(db)
}

async fn runtime(db: &Arc<DatabaseConnection>, http: Arc<Brightnovels>) -> ows::Runtime {
    let runtime = ows::Runtime::builder()
        .register_function(
            "ferrisflow.primitive",
            Arc::new(primitives::Function(Arc::downgrade(db))),
        )
        .register_function("http", http)
        .build()
        .unwrap();
    for definition in primitives::definitions(&**db).await.unwrap() {
        runtime
            .register_definition(&crate::ows::parse_ows_document(&definition.to_string()).unwrap())
            .unwrap();
    }
    runtime
}

async fn run(runtime: &ows::Runtime, name: &str, input: Value) -> Result<Value, String> {
    let manifest: Value =
        serde_json::from_str(include_str!("../../plugins/local.novels.json")).unwrap();
    let definition = manifest["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["document"]["name"] == name)
        .unwrap();
    let workflow = runtime
        .register_definition(&crate::ows::parse_ows_document(&definition.to_string()).unwrap())
        .unwrap();
    runtime.run(workflow, input).await.map_err(|error| format!("{error:?}"))
}

async fn records(db: &DatabaseConnection, table: &str) -> Vec<Value> {
    primitives::database(db, &json!({"operation":"list","table":table}))
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn crawl_and_refresh_save_records_without_duplicates_and_keep_progress() {
    let db = installed().await;
    let http = Arc::new(Brightnovels::default());
    *http.chapters.lock().unwrap() = vec![3, 2, 1];
    let runtime = runtime(&db, http.clone()).await;
    let input = json!({"novel_url":"https://brightnovels.com/series/dune/","chapter_from":1,"chapter_to":3,"auto_update":true});
    for _ in 0..2 {
        run(&runtime, "crawl-brightnovels", input.clone()).await.unwrap();
    }
    let novels = records(&db, "local_novels_novel").await;
    assert_eq!(novels.len(), 1);
    assert_eq!(novels[0]["last_downloaded_chapter"], 2);
    assert_eq!(novels[0]["auto_update"], true);
    assert_eq!(novels[0]["source_site"], "brightnovels.com");
    chrono::DateTime::parse_from_rfc3339(novels[0]["last_checked_at"].as_str().unwrap()).unwrap();
    let chapters = records(&db, "local_novels_chapter").await;
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0]["body"], "<p>Chapter 1 & text</p>");
    assert_eq!(chapters[0]["novel_source"], novels[0]["source"]);
    chrono::DateTime::parse_from_rfc3339(chapters[0]["downloaded_at"].as_str().unwrap()).unwrap();
    *http.chapters.lock().unwrap() = vec![4, 3, 2, 1];
    http.requests.lock().unwrap().clear();
    run(
        &runtime,
        "refresh-brightnovels",
        json!({"novel_url":"https://brightnovels.com/series/dune","last_downloaded_chapter":999}),
    )
    .await
    .unwrap();
    let requests = http.requests.lock().unwrap().clone();
    assert!(requests.iter().any(|url| url.ends_with("/chapter-4")));
    assert!(!requests.iter().any(|url| url.ends_with("/chapter-1")
        || url.ends_with("/chapter-2")
        || url.ends_with("/chapter-3")));
    assert_eq!(records(&db, "local_novels_chapter").await.len(), 3);
    assert_eq!(records(&db, "local_novels_novel").await[0]["last_downloaded_chapter"], 4);
    run(&runtime, "crawl-brightnovels", input).await.unwrap();
    assert_eq!(records(&db, "local_novels_novel").await[0]["last_downloaded_chapter"], 4);
    // An older in-flight crawl cannot regress progress or undo a later opt-out.
    primitives::database(&*db,&json!({"operation":"update","table":"local_novels_novel","key":novels[0]["id"],"input":{"auto_update":false}})).await.unwrap();
    run(&runtime,"save-brightnovels",json!({"title":"Dune","source":"https://brightnovels.com/series/dune","last_downloaded_chapter":2,"auto_update":true,"auto_update_explicit":false,"chapters":[]})).await.unwrap();
    let saved = records(&db, "local_novels_novel").await;
    assert_eq!(saved[0]["last_downloaded_chapter"], 4);
    assert_eq!(saved[0]["auto_update"], false);
}

#[tokio::test]
async fn bad_input_and_failed_download_do_not_write_records() {
    let db = installed().await;
    let http = Arc::new(Brightnovels::default());
    *http.chapters.lock().unwrap() = vec![2, 1];
    let runtime = runtime(&db, http.clone()).await;
    for input in [
        json!({"novel_url":"https://other.example/series/dune"}),
        json!({"novel_url":"https://brightnovels.com/series/dune","chapter_from":4,"chapter_to":1}),
        json!({"novel_url":"https://brightnovels.com/series/dune","chapter_from":1.5}),
    ] {
        assert!(run(&runtime, "crawl-brightnovels", input).await.is_err());
    }
    assert!(http.requests.lock().unwrap().is_empty());
    *http.fail.lock().unwrap() = Some(2);
    assert!(
        run(
            &runtime,
            "crawl-brightnovels",
            json!({"novel_url":"https://brightnovels.com/series/dune"})
        )
        .await
        .is_err()
    );
    assert!(records(&db, "local_novels_novel").await.is_empty());
    assert!(records(&db, "local_novels_chapter").await.is_empty());
}

#[tokio::test]
async fn save_rolls_back_every_record_when_a_chapter_write_fails() {
    let db = installed().await;
    db.execute_unprepared("CREATE TRIGGER fail_chapter BEFORE INSERT ON local_novels_chapter WHEN NEW.number = 2 BEGIN SELECT RAISE(ABORT, 'chapter write failed'); END").await.unwrap();
    let runtime = runtime(&db, Arc::new(Brightnovels::default())).await;
    let input = json!({"title":"Dune","source":"https://brightnovels.com/series/dune","last_downloaded_chapter":2,"chapters":[
        {"number":1,"name":"One","url":"https://brightnovels.com/series/dune/one","body":"One"},
        {"number":2,"name":"Two","url":"https://brightnovels.com/series/dune/two","body":"Two"}
    ]});
    assert!(run(&runtime, "save-brightnovels", input).await.is_err());
    assert!(records(&db, "local_novels_novel").await.is_empty());
    assert!(records(&db, "local_novels_chapter").await.is_empty());
}

#[tokio::test]
async fn auto_refresh_respects_opt_in_and_continues_after_a_novel_fails() {
    let db = installed().await;
    let http = Arc::new(Brightnovels::default());
    *http.chapters.lock().unwrap() = vec![1];
    let runtime = runtime(&db, http.clone()).await;
    for (slug, enabled) in [("broken", true), ("dune", true), ("disabled", false)] {
        primitives::database(&*db,&json!({"operation":"create","table":"local_novels_novel","input":{"title":slug,"source":format!("https://brightnovels.com/series/{slug}"),"auto_update":enabled}})).await.unwrap();
    }
    // A failing input is caught per novel, allowing the next eligible novel to run.
    primitives::database(&*db,&json!({"operation":"update","table":"local_novels_novel","key":1,"input":{"source":"bad-url"}})).await.unwrap();
    run(&runtime, "refresh-auto-novels", Value::Null).await.unwrap();
    let requests = http.requests.lock().unwrap().clone();
    assert!(requests.iter().any(|url| url.ends_with("/dune/chapter-1")));
    assert!(!requests.iter().any(|url| url.contains("/disabled")));
    assert_eq!(records(&db, "local_novels_chapter").await.len(), 1);
}

#[tokio::test]
async fn generated_crud_validates_chapter_numbers_and_renders_record_forms() {
    let db = installed().await;
    let server = axum_test::TestServer::new(crate::build_router(crate::app_state::AppState {
        db: (*db).clone(),
    }));
    for number in [json!(1.5), json!(0), json!(-1)] {
        let result: Value = server.post("/api/collections/local_novels_chapter")
            .json(&json!({"novel_source":"novel","number":number,"title":"Chapter","source_url":"chapter","body":"Text"}))
            .await.json();
        assert_eq!(result["success"], false, "{result}");
    }
    assert!(records(&db, "local_novels_chapter").await.is_empty());
    let definitions = primitives::definitions(&*db).await.unwrap();
    let create = definitions
        .iter()
        .find(|definition| definition["document"]["name"] == "local_novels_chapter-create")
        .unwrap();
    assert_eq!(create["document"]["metadata"]["ui"]["view"]["type"], "form");
    assert_eq!(
        create["document"]["metadata"]["ui"]["input_schema"]["properties"]["number"]["type"],
        "integer"
    );
    let read = definitions
        .iter()
        .find(|definition| definition["document"]["name"] == "local_novels_chapter-read")
        .unwrap();
    assert_eq!(read["document"]["metadata"]["ui"]["output_view"]["bodyField"], "body");
    // Direct registry creation must still supply the new template parameters.
    let created: Value = server.post("/api/content-type-registry")
        .json(&json!({"uid":"reviews","display_name":"Reviews","schema":{"fields":[{"name":"title","label":"Title","type":"text","required":true}]}}))
        .await.json();
    assert_eq!(created["success"], true, "{created}");
    let created: Value =
        server.post("/api/collections/reviews").json(&json!({"title":"Great"})).await.json();
    assert_eq!(created["success"], true, "{created}");
}

#[tokio::test]
async fn upgrading_existing_tables_preserves_records_and_adds_unique_sources() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    // Recreate the previous physical schema, which had no unique source index.
    let mut legacy: Value =
        serde_json::from_str(include_str!("../../plugins/local.novels.json")).unwrap();
    for resource in legacy["resources"].as_array_mut().unwrap() {
        for field in resource["definition"]["schema"]["fields"].as_array_mut().unwrap() {
            field.as_object_mut().unwrap().remove("unique");
        }
    }
    for manifest in [
        serde_json::from_str::<Value>(include_str!("../../plugins/ferrisflow.content-types.json"))
            .unwrap(),
        legacy.clone(),
    ] {
        plugin_resources::persist(
            &db,
            plugin::ActiveModel {
                uid: Set(manifest["id"].as_str().unwrap().into()),
                name: Set("Test".into()),
                version: Set(manifest["version"].as_str().unwrap().into()),
                manifest_json: Set(manifest.to_string()),
                files_json: Set("{}".into()),
                enabled: Set(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    primitives::database(&db,&json!({"operation":"create","table":"local_novels_novel","input":{"title":"Existing","source":"https://brightnovels.com/series/dune"}})).await.unwrap();
    use sea_orm::EntityTrait;
    let mut active: plugin::ActiveModel =
        plugin::Entity::find_by_id("local.novels").one(&db).await.unwrap().unwrap().into();
    active.manifest_json = Set(include_str!("../../plugins/local.novels.json").into());
    plugin_resources::persist(&db, active).await.unwrap();
    assert_eq!(records(&db, "local_novels_novel").await[0]["title"], "Existing");
    assert!(primitives::database(&db,&json!({"operation":"create","table":"local_novels_novel","input":{"title":"Duplicate","source":"https://brightnovels.com/series/dune"}})).await.is_err());
}

#[tokio::test]
async fn disabled_plugins_cannot_run_and_unrelated_workflows_do_not_break_execution() {
    use crate::models::workflow;
    use sea_orm::{ActiveModelTrait, EntityTrait};
    let db = installed().await;
    workflow::ActiveModel {
        name:Set("Unrelated".into()), active:Set(true), version:Set(1),
        definition_json:Set(json!({"document":{"dsl":"1.0.3","namespace":"tests","name":"unrelated","version":"1.0.0"},"do":[{"broken":{"set":"${ this is not a valid expression ( }"}}]}).to_string()),
        ..Default::default()
    }.insert(&*db).await.unwrap();
    let root = json!({"document":{"dsl":"1.0.3","namespace":"tests","name":"list","version":"1.0.0"},"do":[{"records":{"call":"ferrisflow.primitive","with":{"primitive":"database","operation":"list","table":"local_novels_novel"}}}]});
    assert_eq!(primitives::run(db.clone(), &root, json!({})).await.unwrap(), json!([]));
    let mut active: plugin::ActiveModel =
        plugin::Entity::find_by_id("local.novels").one(&*db).await.unwrap().unwrap().into();
    active.enabled = Set(false);
    active.update(&*db).await.unwrap();
    let manifest: Value =
        serde_json::from_str(include_str!("../../plugins/local.novels.json")).unwrap();
    let save = manifest["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["document"]["name"] == "save-brightnovels")
        .unwrap();
    assert!(
        primitives::run(db.clone(), save, json!({}))
            .await
            .unwrap_err()
            .contains("not installed or enabled")
    );
}
