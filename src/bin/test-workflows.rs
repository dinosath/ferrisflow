use reqwest::Client;
use serde_json::{Map, Value, json};
use std::{
    env,
    error::Error,
    fs::{self, File},
    io::{self, Read},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type AnyError = Box<dyn Error + Send + Sync>;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

#[derive(Default)]
struct Args {
    suite: Option<PathBuf>,
    case_pattern: String,
    server_bin: Option<PathBuf>,
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn create(label: &str) -> io::Result<Self> {
        let nonce =
            SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
        let path = env::temp_dir()
            .join(format!("ferrisflow-workflow-tests-{}-{nonce}-{label}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Case {
    workflow: String,
    plugins: Option<Vec<String>>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), AnyError> {
    let args = parse_args()?;
    let root = Path::new(ROOT);
    let suite_path =
        resolve_path(root, args.suite.unwrap_or_else(|| PathBuf::from("plugins/tests/suite.json")));
    let suite: Value = read_json(&suite_path)?;
    let directory = suite_path.parent().ok_or("suite path has no parent directory")?;
    let cases =
        suite.get("cases").and_then(Value::as_array).ok_or("suite must contain a cases array")?;
    let selected: Vec<Case> = cases
        .iter()
        .filter_map(parse_case)
        .filter(|case| wildcard_match(&args.case_pattern, &case.workflow))
        .collect();
    if selected.is_empty() {
        return Err(format!("No workflow tests match --case {}", args.case_pattern).into());
    }

    let binary = match args.server_bin {
        Some(path) => resolve_path(root, path),
        None => build_server(root)?,
    };
    let client = Client::builder().no_proxy().timeout(Duration::from_secs(90)).build()?;
    let mut failures = 0;
    for (index, case) in selected.iter().enumerate() {
        match execute_case(&client, &binary, directory, &suite, case, index).await {
            Ok(()) => println!("PASS {}", case.workflow),
            Err(error) => {
                failures += 1;
                eprintln!("FAIL {}: {error}", case.workflow);
            }
        }
    }
    println!("{} passed; {failures} failed", selected.len() - failures);
    if failures > 0 {
        return Err("workflow test failures".into());
    }
    Ok(())
}

fn parse_args() -> Result<Args, AnyError> {
    let mut args = Args { case_pattern: "*".to_owned(), ..Args::default() };
    let mut input = env::args().skip(1);
    while let Some(argument) = input.next() {
        match argument.as_str() {
            "--suite" => args.suite = Some(PathBuf::from(required_value(&mut input, "--suite")?)),
            "--case" => args.case_pattern = required_value(&mut input, "--case")?,
            "--server-bin" => {
                args.server_bin = Some(PathBuf::from(required_value(&mut input, "--server-bin")?))
            }
            "--help" | "-h" => {
                println!(
                    "Usage: test-workflows [--suite PATH] [--case GLOB] [--server-bin PATH]\n\
                     Runs declarative workflow cases against disposable Ferrisflow servers."
                );
                std::process::exit(0);
            }
            value => return Err(format!("unknown argument: {value}").into()),
        }
    }
    Ok(args)
}

fn required_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, AnyError> {
    args.next().ok_or_else(|| format!("{flag} requires a value").into())
}

fn build_server(root: &Path) -> Result<PathBuf, AnyError> {
    let status = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--no-default-features",
            "--features",
            "api-server",
            "--bin",
            "ferrisflow",
        ])
        .current_dir(root)
        .status()?;
    if !status.success() {
        return Err(format!("cargo build exited with {status}").into());
    }
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err("cargo metadata failed while locating the server binary".into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let target_dir = metadata
        .get("target_directory")
        .and_then(Value::as_str)
        .ok_or("cargo metadata did not return target_directory")?;
    let executable = if cfg!(windows) { "ferrisflow.exe" } else { "ferrisflow" };
    Ok(Path::new(target_dir).join("debug").join(executable))
}

fn parse_case(value: &Value) -> Option<Case> {
    Some(Case {
        workflow: value.get("workflow")?.as_str()?.to_owned(),
        plugins: value
            .get("plugins")
            .and_then(Value::as_array)
            .map(|plugins| plugins.iter().filter_map(Value::as_str).map(str::to_owned).collect()),
    })
}

async fn execute_case(
    client: &Client, binary: &Path, directory: &Path, suite: &Value, case: &Case, index: usize,
) -> Result<(), AnyError> {
    let temporary = TempDirectory::create(&format!("case-{index}"))?;
    let port = reserve_port()?;
    let base = format!("http://127.0.0.1:{port}");
    let log_path = temporary.0.join("server.log");
    let log = File::create(&log_path)?;
    let mut server = spawn_server(binary, &temporary.0, port, log.try_clone()?)?;
    drop(log);
    let result = execute_case_requests(client, &base, directory, suite, case, &mut server).await;
    if result.is_err() {
        eprintln!("{}", tail(&log_path, 25).unwrap_or_default());
    }
    stop_server(&mut server);
    result
}

fn spawn_server(binary: &Path, temporary: &Path, port: u16, log: File) -> io::Result<Child> {
    let database = temporary.join("test.db");
    Command::new(binary)
        .current_dir(ROOT)
        .env("IP", "127.0.0.1")
        .env("PORT", port.to_string())
        .env("DATABASE_URL", format!("sqlite://{}?mode=rwc", database.display()))
        .env("AUTH_REQUIRED", "false")
        .env_remove("OAUTH_INTROSPECTION_URL")
        .env_remove("OAUTH_CLIENT_ID")
        .env_remove("OAUTH_CLIENT_SECRET")
        .env_remove("ADMIN_PASSWORD")
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
}

async fn execute_case_requests(
    client: &Client, base: &str, directory: &Path, suite: &Value, case: &Case, server: &mut Child,
) -> Result<(), AnyError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = server.try_wait()? {
            return Err(format!("test server exited with status {status}").into());
        }
        match request(client, base, reqwest::Method::GET, "/api/health", None).await {
            Ok(_) => break,
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            Err(_) => return Err("test server did not become ready".into()),
        }
    }

    let functions = suite.get("functions").cloned().unwrap_or_else(|| json!({}));
    let plugin_paths: Vec<String> = match &case.plugins {
        Some(paths) => paths.clone(),
        None => suite
            .get("plugins")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default(),
    };
    for plugin in plugin_paths {
        let manifest = read_json(&directory.join(plugin))?;
        let manifest = prepare(manifest, base, &functions);
        request(client, base, reqwest::Method::POST, "/api/plugins", Some(manifest)).await?;
    }

    let mut inputs = Map::new();
    if let Some(configured_inputs) = suite.get("inputs").and_then(Value::as_object) {
        for (name, path) in configured_inputs {
            let path = path.as_str().ok_or("suite input paths must be strings")?;
            let value = read_json(&directory.join(path))?;
            inputs.insert(name.clone(), prepare(value, base, &functions));
        }
    }
    let definition_path = directory.join(&case.workflow);
    let definition = prepare(read_json(&definition_path)?, base, &json!({}));
    let body = json!({
        "definition_json": serde_json::to_string(&definition)?,
        "input": {"packages": inputs},
    });
    let result =
        request(client, base, reqwest::Method::POST, "/api/workflows/execute", Some(body)).await?;
    if result.get("success").and_then(Value::as_bool) != Some(true) {
        let status = result.get("status").and_then(Value::as_str).unwrap_or("unknown");
        let error = result.get("error").and_then(Value::as_str).unwrap_or("unknown error");
        return Err(format!("workflow execution failed (status={status}): {error}").into());
    }
    if result.pointer("/output/passed").and_then(Value::as_bool) != Some(true) {
        return Err("test workflow did not return {\"passed\": true}".into());
    }
    Ok(())
}

async fn request(
    client: &Client, base: &str, method: reqwest::Method, path: &str, body: Option<Value>,
) -> Result<Value, AnyError> {
    let mut request = client.request(method.clone(), format!("{base}{path}"));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response.bytes().await?;
    if !status.is_success() {
        return Err(format!(
            "{} {path}: HTTP {}: {}",
            method,
            status,
            String::from_utf8_lossy(&bytes)
        )
        .into());
    }
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    if content_type.contains("json") {
        Ok(serde_json::from_slice(&bytes)?)
    } else {
        Ok(Value::String(String::from_utf8(bytes.to_vec())?))
    }
}

fn prepare(value: Value, base: &str, functions: &Value) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|item| prepare(item, base, functions)).collect())
        }
        Value::String(value) => Value::String(value.replace("@TEST_BASE_URL@", base)),
        Value::Object(object) => {
            let mut result: Map<String, Value> = object
                .into_iter()
                .map(|(key, value)| (key, prepare(value, base, functions)))
                .collect();
            if let Some(function_name) = result.get("call").and_then(Value::as_str)
                && let Some(implementation) = functions.get(function_name).cloned()
            {
                result.remove("call");
                let arguments = result.remove("with").unwrap_or_else(|| json!({}));
                let input_expression = format!("${{ {} }}", expression(&arguments));
                let mut workflow = implementation.as_object().cloned().unwrap_or_default();
                workflow.insert("input".to_owned(), Value::String(input_expression));
                result.insert("run".to_owned(), json!({"workflow": Value::Object(workflow)}));
            }
            Value::Object(result)
        }
        other => other,
    }
}

fn expression(value: &Value) -> String {
    match value {
        Value::String(value) if value.starts_with("${") && value.ends_with('}') => {
            value[2..value.len() - 1].trim().to_owned()
        }
        Value::Object(object) => format!(
            "{{{}}}",
            object
                .iter()
                .map(|(key, value)| format!("{}:{}", json!(key), expression(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Array(items) => {
            format!("[{}]", items.iter().map(expression).collect::<Vec<_>>().join(","))
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_owned()),
    }
}

fn read_json(path: &Path) -> Result<Value, AnyError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn resolve_path(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() { path } else { root.join(path) }
}

fn reserve_port() -> io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn stop_server(server: &mut Child) {
    if server.try_wait().ok().flatten().is_none() {
        let _ = server.kill();
    }
    let _ = server.wait();
}

fn tail(path: &Path, count: usize) -> io::Result<String> {
    let mut content = String::new();
    File::open(path)?.read_to_string(&mut content)?;
    Ok(content
        .lines()
        .rev()
        .take(count)
        .map(redact_admin_password)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

fn redact_admin_password(line: &str) -> String {
    match line.find("admin_password=") {
        Some(position) => format!("{}admin_password=[redacted]", &line[..position]),
        None => line.to_owned(),
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut row = vec![false; value.len() + 1];
    row[0] = true;
    for token in pattern {
        let mut next = vec![false; value.len() + 1];
        match token {
            b'*' => {
                next[0] = row[0];
                for index in 1..=value.len() {
                    next[index] = row[index] || next[index - 1];
                }
            }
            b'?' => {
                next[1..].copy_from_slice(&row[..value.len()]);
            }
            byte => {
                for index in 1..=value.len() {
                    next[index] = row[index - 1] && value[index - 1] == *byte;
                }
            }
        }
        row = next;
    }
    row[value.len()]
}
