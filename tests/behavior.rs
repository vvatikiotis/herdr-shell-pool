use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

fn executable() -> std::path::PathBuf {
    std::env::var_os("HERDR_POOL_TEST_BIN")
        .map(Into::into)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_herdr-shell-pool").into())
}

struct Step {
    method: &'static str,
    params: Value,
    response: Value,
}
fn step(method: &'static str, params: Value, result: Value) -> Step {
    Step {
        method,
        params,
        response: json!({"result":result}),
    }
}
fn snapshot(state: Value) -> Step {
    step("session.snapshot", json!({}), json!({"snapshot":state}))
}
fn processes(id: &str, names: &[&str]) -> Step {
    step(
        "pane.process_info",
        json!({"pane_id":id}),
        json!({"process_info":{"foreground_processes":names.iter().map(|n|json!({"name":n})).collect::<Vec<_>>()}}),
    )
}
fn state() -> Value {
    json!({"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"}],"panes":[
        {"pane_id":"source","workspace_id":"work","tab_id":"tab","foreground_cwd":"/tmp/a b","cwd":"/tmp/old"},
        {"pane_id":"warm","workspace_id":"pool","tab_id":"warm-tab"}
    ]})
}
fn check_subset(actual: &Value, expected: &Value) {
    for (key, value) in expected.as_object().unwrap() {
        assert_eq!(&actual[key], value, "parameter {key}");
    }
}
fn scenario(action: &str, steps: Vec<Step>, success: bool) -> (String, String) {
    let (out, log, _) = scenario_with(action, steps, success, &[], false);
    (out, log)
}
fn scenario_with(
    action: &str,
    steps: Vec<Step>,
    success: bool,
    environment: &[(&str, &str)],
    busy: bool,
) -> (String, String, String) {
    scenario_arguments(&[action], steps, success, environment, busy)
}
fn scenario_arguments(
    arguments: &[&str],
    steps: Vec<Step>,
    success: bool,
    environment: &[(&str, &str)],
    busy: bool,
) -> (String, String, String) {
    let dir = tempfile::Builder::new()
        .prefix("hp")
        .tempdir_in("/tmp")
        .unwrap();
    if let Some((_, record)) = environment
        .iter()
        .find(|(key, _)| *key == "HERDR_TEST_RECORD")
    {
        let registry = dir.path().join("state/herdr-shell-pool/workers");
        std::fs::create_dir_all(&registry).unwrap();
        std::fs::write(registry.join("older.json"), record).unwrap();
    }
    let socket_mode = environment
        .iter()
        .find(|(key, _)| *key == "HERDR_TEST_SOCKET_MODE")
        .map(|(_, value)| *value)
        .unwrap_or("explicit");
    let socket = match socket_mode {
        "home" => dir.path().join(".config/herdr/herdr.sock"),
        "xdg" => dir.path().join("config/herdr/herdr.sock"),
        "session" => dir.path().join("config/herdr/sessions/test/herdr.sock"),
        _ => dir.path().join("api.sock"),
    };
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let bin = dir.path().join("herdr");
    std::fs::write(
        &bin,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HERDR_TEST_LOG\"\nif [ \"$1\" = pane ] && [ \"$2\" = run ]; then\n    count=0\n    if [ -f \"$HERDR_TEST_RUN_COUNT\" ]; then count=$(cat \"$HERDR_TEST_RUN_COUNT\"); fi\n    count=$((count + 1))\n    printf '%s' \"$count\" > \"$HERDR_TEST_RUN_COUNT\"\n    if [ \"${HERDR_TEST_FAIL_RUN:-0}\" = \"$count\" ]; then exit 7; fi\nfi\nexit \"${HERDR_TEST_EXIT:-0}\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
    let log = dir.path().join("shell.log");
    let done = Arc::new(AtomicBool::new(false));
    let server_done = done.clone();
    let server_log = log.clone();
    let server = std::thread::spawn(move || {
        let mut steps = steps.into_iter();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    let req: Value = serde_json::from_str(&line).unwrap();
                    if req["method"] == "pane.wait_for_output" {
                        let marker = req["params"]["match"]["value"].as_str().unwrap();
                        let submitted = std::fs::read_to_string(&server_log).unwrap();
                        let setup = submitted.lines().last().unwrap();
                        assert!(
                            setup.starts_with(" if [[ -o zle ]];"),
                            "shell setup must start with a space for history exclusion"
                        );
                        assert!(
                            !submitted.contains(marker),
                            "literal marker must not occur in shell input"
                        );
                        let marker_command =
                            &submitted[submitted.rfind("printf '%b\\n'").unwrap()..];
                        let decoded = Command::new("/bin/bash")
                            .args(["-c", marker_command])
                            .output()
                            .unwrap();
                        assert!(decoded.status.success());
                        assert_eq!(
                            String::from_utf8(decoded.stdout).unwrap(),
                            format!("{marker}\n")
                        );
                    }
                    let response = if let Some(step) = steps.next() {
                        assert_eq!(req["method"], step.method);
                        check_subset(&req["params"], &step.params);
                        step.response
                    } else {
                        // Background refill sees an already-full pool and terminates.
                        match req["method"].as_str().unwrap() {
                            "session.snapshot" => {
                                json!({"result":{"snapshot":{"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"}],"panes":[{"pane_id":"p1","workspace_id":"pool"},{"pane_id":"p2","workspace_id":"pool"}]}}})
                            }
                            "pane.process_info" => {
                                json!({"result":{"process_info":{"foreground_processes":[{"name":"zsh"}]}}})
                            }
                            other => panic!("unexpected request {other}"),
                        }
                    };
                    write!(stream, "{response}").unwrap();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if server_done.load(Ordering::SeqCst) {
                        break;
                    }
                    assert!(Instant::now() < deadline, "server timed out");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert!(steps.next().is_none(), "unconsumed expected requests");
    });
    let _lock = if busy {
        use std::os::fd::AsRawFd;
        let path = dir.path().join("state/herdr-shell-pool");
        std::fs::create_dir_all(&path).unwrap();
        let file = std::fs::File::create(path.join("lock")).unwrap();
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        Some(file)
    } else {
        None
    };
    let mut command = Command::new(executable());
    command
        .args(arguments)
        .env("HERDR_SOCKET_PATH", &socket)
        .env("HERDR_ACTIVE_PANE_ID", "source")
        .env("HOME", "/tmp/herdr-test-home")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SESSION")
        .env_remove("HERDR_TEST_EXIT")
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env("HERDR_TEST_LOG", &log)
        .env("HERDR_TEST_RUN_COUNT", dir.path().join("run.count"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if socket_mode != "explicit" {
        command.env_remove("HERDR_SOCKET_PATH");
        if socket_mode == "home" {
            command.env("HOME", dir.path()).env("XDG_CONFIG_HOME", "");
        } else {
            command.env("XDG_CONFIG_HOME", dir.path().join("config"));
        }
        if socket_mode == "session" {
            command.env("HERDR_SESSION", "test");
        }
    }
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("CLI timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    // Let detached refill finish before disposing of the isolated environment.
    std::thread::sleep(Duration::from_millis(200));
    done.store(true, Ordering::SeqCst);
    if !output.status.success() {
        eprintln!("CLI error: {}", String::from_utf8_lossy(&output.stderr));
    }
    server.join().unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8(output.stdout).unwrap(),
        std::fs::read_to_string(log).unwrap_or_default(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn status_counts_shells_but_not_busy_or_unknown_processes() {
    let s = json!({"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"}],"panes":[
        {"pane_id":"a","workspace_id":"pool","agent":"agent"},
        {"pane_id":"b","workspace_id":"pool"},{"pane_id":"c","workspace_id":"pool"},
        {"pane_id":"outside","workspace_id":"other"}
    ]});
    let (out, _) = scenario(
        "status",
        vec![
            snapshot(s),
            processes("a", &["/bin/-ZSH"]),
            processes("b", &["vim"]),
            processes("c", &[]),
        ],
        true,
    );
    assert_eq!(out, "1/2 ready\n");
}

#[test]
fn warm_new_tab_updates_context_before_focus() {
    let s = state();
    let (_, log) = scenario(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            step(
                "pane.move",
                json!({"pane_id":"warm","focus":false,"destination":{"type":"new_tab","workspace_id":"work"}}),
                json!({"move_result":{"changed":true,"pane":{"pane_id":"warm","tab_id":"new-tab"}}}),
            ),
            step(
                "pane.wait_for_output",
                json!({"pane_id":"warm","timeout_ms":3000,"source":"recent"}),
                json!({}),
            ),
            step("pane.focus", json!({"pane_id":"warm"}), json!({})),
        ],
        true,
    );
    assert!(log.contains("HERDR_WORKSPACE_ID=work"));
    assert!(log.contains("HERDR_TAB_ID=new-tab"));
    assert!(log.contains("HERDR_PANE_ID=warm"));
    assert!(log.contains("cd -- '/tmp/a b'"));
    let lines: Vec<_> = log.lines().collect();
    let setup = lines
        .iter()
        .position(|line| line.starts_with(" if [[ -o zle ]];"))
        .unwrap();
    let advancement = lines.iter().position(|line| *line == " ").unwrap();
    assert!(advancement > setup, "history advancement must follow setup");
}

mod cases;
