//! Smoke coverage for every retained runnable example (issue #388).
//!
//! `cargo build --examples --workspace --all-features` (run separately in
//! CI) proves every example *compiles*. This test additionally *runs* each
//! one-shot example to completion and asserts a clean exit, and exercises
//! the two long-running workspace applications (the axum and tonic example
//! apps) by starting them, interacting with them, and shutting them down.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// One-shot examples that run to completion on their own.
///
/// `None` means the example lives in the root `tower-resilience-tests`
/// package (`examples/*.rs`); `Some(pkg)` means it lives in that package's
/// own `examples/` directory.
const ONE_SHOT_EXAMPLES: &[(&str, Option<&str>)] = &[
    // Root examples: canonical for crates with no crate-local example.
    ("adaptive", None),
    ("coalesce", None),
    ("executor", None),
    ("hedge", None),
    ("outlier", None),
    ("router", None),
    // Root examples: meaningful compositions / cross-cutting concerns.
    ("composition_outbound", None),
    ("server_api", None),
    ("healthcheck_circuitbreaker", None),
    ("observability_metrics", None),
    // Crate-local examples.
    ("bulkhead_basic", Some("tower-resilience-bulkhead")),
    ("bulkhead_advanced", Some("tower-resilience-bulkhead")),
    ("cache_example", Some("tower-resilience-cache")),
    ("chaos_example", Some("tower-resilience-chaos")),
    (
        "circuitbreaker_example",
        Some("tower-resilience-circuitbreaker"),
    ),
    (
        "circuitbreaker_fallback",
        Some("tower-resilience-circuitbreaker"),
    ),
    (
        "circuitbreaker_health_check",
        Some("tower-resilience-circuitbreaker"),
    ),
    ("fallback_example", Some("tower-resilience-fallback")),
    ("healthcheck_basic", Some("tower-resilience-healthcheck")),
    ("ratelimiter_example", Some("tower-resilience-ratelimiter")),
    ("reconnect_basic", Some("tower-resilience-reconnect")),
    (
        "reconnect_custom_policy",
        Some("tower-resilience-reconnect"),
    ),
    ("retry_example", Some("tower-resilience-retry")),
    ("timelimiter_example", Some("tower-resilience-timelimiter")),
    // Facade crate: proves the `tower_resilience::` re-export surface works.
    ("combined", Some("tower-resilience")),
];

const PER_EXAMPLE_TIMEOUT: Duration = Duration::from_secs(120);

#[test]
fn one_shot_examples_run_to_completion() {
    for (name, package) in ONE_SHOT_EXAMPLES {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.arg("run").arg("--locked").arg("--quiet");
        if let Some(pkg) = package {
            cmd.arg("-p").arg(pkg);
        }
        cmd.arg("--example").arg(name).arg("--all-features");
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn example `{name}`: {e}"));

        let status = wait_with_timeout(&mut child, PER_EXAMPLE_TIMEOUT, name);
        assert!(status.success(), "example `{name}` exited with {status}");
    }
}

#[test]
fn axum_resilient_kv_store_smoke() {
    let bin = build_and_locate_bin("axum-resilient-kv-store", "axum-resilient-kv-store");

    let mut server = Command::new(&bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to start axum-resilient-kv-store: {e}"));

    let addr = "127.0.0.1:3000";
    wait_for_port(addr, Duration::from_secs(15));

    let health = http_request(addr, "GET", "/health/live", None);
    assert!(
        health.starts_with("HTTP/1.1 200"),
        "unexpected /health/live response: {health}"
    );

    let post = http_request(addr, "POST", "/smoke-key", Some("hello-from-smoke-test"));
    assert!(
        post.starts_with("HTTP/1.1 200"),
        "unexpected POST /smoke-key response: {post}"
    );

    let get = http_request(addr, "GET", "/smoke-key", None);
    assert!(
        get.starts_with("HTTP/1.1 200") && get.contains("hello-from-smoke-test"),
        "unexpected GET /smoke-key response: {get}"
    );

    let _ = server.kill();
    let _ = server.wait();
}

#[test]
fn tonic_resilient_greeter_smoke() {
    let server_bin = build_and_locate_bin("tonic-resilient-greeter", "server");
    let client_bin = build_and_locate_bin("tonic-resilient-greeter", "client");

    let mut server = Command::new(&server_bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to start tonic greeter server: {e}"));

    wait_for_port("[::1]:50051", Duration::from_secs(15));

    let mut client = Command::new(&client_bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to start tonic greeter client: {e}"));

    let mut stdout_pipe = client.stdout.take().expect("client stdout not piped");
    let mut stderr_pipe = client.stderr.take().expect("client stderr not piped");
    let stdout_handle = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout_pipe.read_to_string(&mut s);
        s
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr_pipe.read_to_string(&mut s);
        s
    });

    let status = wait_with_timeout(&mut client, Duration::from_secs(60), "tonic greeter client");
    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();

    let _ = server.kill();
    let _ = server.wait();

    let combined = format!("{stdout}{stderr}");
    assert!(
        status.success(),
        "tonic greeter client exited with {status}\noutput:\n{combined}"
    );
    assert!(
        combined.contains("Successful requests"),
        "client output missing expected summary line:\n{combined}"
    );
}

fn wait_with_timeout(child: &mut Child, timeout: Duration, name: &str) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("`{name}` did not finish within {timeout:?}");
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("failed to wait on `{name}`: {e}"),
        }
    }
}

fn wait_for_port(addr: &str, timeout: Duration) {
    let start = Instant::now();
    loop {
        if TcpStream::connect(addr).is_ok() {
            return;
        }
        if start.elapsed() > timeout {
            panic!("timed out waiting for {addr} to accept connections");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Sends a minimal raw HTTP/1.1 request over a plain TCP connection and
/// returns the full response text. Good enough for smoke-checking a local
/// example server without pulling in an HTTP client dependency.
fn http_request(addr: &str, method: &str, path: &str, body: Option<&str>) -> String {
    let socket_addr = addr
        .to_socket_addrs()
        .unwrap_or_else(|e| panic!("failed to resolve {addr}: {e}"))
        .next()
        .unwrap_or_else(|| panic!("no socket address resolved for {addr}"));

    let mut stream = TcpStream::connect(socket_addr)
        .unwrap_or_else(|e| panic!("failed to connect to {addr}: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("failed to set read timeout");

    let body = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream
        .write_all(request.as_bytes())
        .expect("failed to write HTTP request");

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

/// Builds `pkg` and returns the path to the `bin_name` binary it produced,
/// by parsing cargo's JSON build-artifact messages. This avoids assuming a
/// fixed `target/debug/...` layout (custom `CARGO_TARGET_DIR`, profile,
/// etc.).
fn build_and_locate_bin(pkg: &str, bin_name: &str) -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "-p",
            pkg,
            "--message-format=json-render-diagnostics",
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to run `cargo build -p {pkg}`: {e}"));

    assert!(
        output.status.success(),
        "`cargo build -p {pkg}` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if !line.contains("\"reason\":\"compiler-artifact\"") {
            continue;
        }
        if !line.contains(&format!("\"name\":\"{bin_name}\"")) {
            continue;
        }
        let marker = "\"executable\":\"";
        if let Some(start) = line.find(marker) {
            let rest = &line[start + marker.len()..];
            if let Some(end) = rest.find('"') {
                return PathBuf::from(rest[..end].replace("\\\\", "\\"));
            }
        }
    }
    panic!("could not locate built executable `{bin_name}` for package `{pkg}`");
}
