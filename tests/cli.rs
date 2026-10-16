use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static SOCKET_NUMBER: AtomicUsize = AtomicUsize::new(0);

// Use the same behavior tests for the reference executable and the Rust port.
fn executable() -> std::path::PathBuf {
    std::env::var_os("HERDR_POOL_TEST_BIN")
        .map(Into::into)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_herdr-shell-pool").into())
}

#[test]
fn status_reports_zero_without_a_pool_workspace() {
    let socket = std::env::temp_dir().join(format!(
        "hpool-{}-{}.sock",
        std::process::id(),
        SOCKET_NUMBER.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&socket).expect("bind test Herdr socket");
    listener.set_nonblocking(true).unwrap();

    let mut child = Command::new(executable())
        .arg("status")
        .env("HERDR_SOCKET_PATH", &socket)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run status");

    // Bound the fake server's wait so a missing client connection cannot hang the test.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let connection = loop {
        match listener.accept() {
            Ok((stream, _)) => break Some(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline || child.try_wait().unwrap().is_some() {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept test connection: {error}"),
        }
    };
    if let Some(mut stream) = connection {
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "session.snapshot");
        writeln!(
            stream,
            "{}",
            serde_json::json!({
                "id": request["id"],
                "result": {"snapshot": {"workspaces": [], "panes": []}}
            })
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    std::fs::remove_file(socket).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"0/2 ready\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_action_fails_with_usage() {
    let output = Command::new(executable()).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("usage: herdr-shell-pool")
    );
}

#[test]
fn worker_requires_a_valid_token() {
    for args in [
        vec!["fill-worker"],
        vec!["fill-worker", "short"],
        vec![
            "fill-worker",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ],
    ] {
        let output = Command::new(executable()).args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with("herdr-shell-pool: ")
        );
    }
}

#[test]
fn unknown_action_fails_with_usage() {
    let output = Command::new(executable())
        .arg("not-an-action")
        .output()
        .expect("run shell pool executable");

    assert!(!output.status.success(), "unknown action must fail");
    assert!(output.stdout.is_empty(), "errors must not use stdout");
    assert_eq!(
        String::from_utf8(output.stderr).expect("UTF-8 error"),
        "herdr-shell-pool: usage: herdr-shell-pool <fill|status|new-tab|split-left|split-down>\n"
    );
}
