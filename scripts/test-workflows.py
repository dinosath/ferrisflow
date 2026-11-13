#!/usr/bin/env python3
"""Execute declarative OWS test suites against disposable Ferrisflow servers."""
import argparse
import fnmatch
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def request(base, method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    call = urllib.request.Request(
        base + path, data=data, method=method,
        headers={"Content-Type": "application/json"},
    )
    try:
        with HTTP.open(call, timeout=90) as response:
            content = response.read()
            if not content:
                return None
            if "json" in response.headers.get("Content-Type", ""):
                return json.loads(content)
            return content.decode()
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"{method} {path}: HTTP {error.code}: {error.read().decode()}") from error


def prepare(value, base, functions=None):
    """Resolve server URLs and declaratively substitute function implementations."""
    if isinstance(value, list):
        return [prepare(item, base, functions) for item in value]
    if isinstance(value, str):
        return value.replace("@TEST_BASE_URL@", base)
    if not isinstance(value, dict):
        return value
    result = {key: prepare(item, base, functions) for key, item in value.items()}
    function = result.get("call")
    implementation = (functions or {}).get(function) if isinstance(function, str) else None
    if implementation:
        result.pop("call")
        result["run"] = {"workflow": {**implementation, "input": result.pop("with", {})}}
    return result


def load(path):
    return json.loads(path.read_text())


def execute_case(binary, directory, config, case):
    with tempfile.TemporaryDirectory(prefix="ferrisflow-workflow-tests-") as temporary:
        temporary = Path(temporary)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        environment = {
            **os.environ, "IP": "127.0.0.1", "PORT": str(port),
            "DATABASE_URL": f"sqlite://{temporary / 'test.db'}?mode=rwc",
        }
        with (temporary / "server.log").open("w+") as log:
            server = subprocess.Popen([str(binary)], cwd=ROOT, env=environment, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 30
                while True:
                    if server.poll() is not None:
                        raise RuntimeError(f"Test server exited with status {server.returncode}")
                    try:
                        request(base, "GET", "/api/health")
                        break
                    except (OSError, RuntimeError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError("Test server did not become ready")
                        time.sleep(0.05)
                functions = config.get("functions", {})
                for plugin in case.get("plugins", config.get("plugins", [])):
                    manifest = prepare(load(directory / plugin), base, functions)
                    request(base, "POST", "/api/plugins", manifest)
                inputs = {
                    key: prepare(load(directory / path), base, functions)
                    for key, path in config.get("inputs", {}).items()
                }
                definition = prepare(load(directory / case["workflow"]), base)
                result = request(base, "POST", "/api/workflows/execute", {
                    "definition_json": json.dumps(definition), "input": {"packages": inputs},
                })
                if not result.get("success"):
                    raise RuntimeError(json.dumps(result, indent=2))
                if result.get("output", {}).get("passed") is not True:
                    raise RuntimeError("Test workflow did not return {\"passed\": true}")
            except Exception:
                log.flush()
                log.seek(0)
                print("\n".join(log.read().splitlines()[-25:]), file=sys.stderr)
                raise
            finally:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", type=Path, default=ROOT / "plugins/tests/suite.json")
    parser.add_argument("--case", default="*", help="Glob matching a workflow path")
    parser.add_argument("--server-bin", type=Path, help="Use an existing API server binary")
    arguments = parser.parse_args()
    suite = arguments.suite.resolve()
    config = load(suite)
    cases = [case for case in config["cases"] if fnmatch.fnmatch(case["workflow"], arguments.case)]
    if not cases:
        parser.error("No workflow tests match --case")
    binary = arguments.server_bin
    if binary is None:
        subprocess.run([
            "cargo", "build", "--locked", "--no-default-features", "--features", "api-server",
            "--bin", "ferrisflow",
        ], cwd=ROOT, check=True)
        metadata = json.loads(subprocess.check_output([
            "cargo", "metadata", "--no-deps", "--format-version", "1",
        ], cwd=ROOT))
        binary = Path(metadata["target_directory"]) / "debug/ferrisflow"
    binary = binary.resolve()
    failures = 0
    for case in cases:
        try:
            execute_case(binary, suite.parent, config, case)
            print(f"PASS {case['workflow']}", flush=True)
        except Exception as error:
            failures += 1
            print(f"FAIL {case['workflow']}: {error}", file=sys.stderr, flush=True)
    print(f"{len(cases) - failures} passed; {failures} failed")
    return bool(failures)


if __name__ == "__main__":
    sys.exit(main())
