use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{
    fs::PermissionsExt,
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

fn executable() -> PathBuf {
    std::env::var_os("HERDR_POOL_TEST_BIN")
        .map(Into::into)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_herdr-shell-pool").into())
}

fn driver_request(socket: &Path, request: Value) -> Value {
    let mut stream = UnixStream::connect(socket).unwrap();
    writeln!(stream, "{request}").unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn make_executable(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn warm_shell_history_excludes_setup_from_actual_interactive_history() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::Builder::new()
        .prefix("history-regression")
        .tempdir_in(root)
        .unwrap();
    let driver_socket = directory.path().join("driver.sock");
    let driver_ready = directory.path().join("driver.ready");
    let api_socket = directory.path().join("api.sock");
    let driver = directory.path().join("pty-driver.py");
    make_executable(
        &driver,
        r##"#!/usr/bin/env python3
import json, os, pty, select, socket, sys

socket_path = os.environ['HERDR_DRIVER_SOCKET']
pid, fd = pty.fork()
if pid == 0:
    os.environ['TERM'] = 'xterm'
    os.execv('/bin/zsh', ['zsh', '-f'])

pending = b''
def read_until(needle, timeout=5):
    global pending
    deadline = __import__('time').monotonic() + timeout
    while True:
        position = pending.find(needle)
        if position >= 0:
            end = position + len(needle)
            result = pending[:end]
            pending = pending[end:]
            return result
        remaining = deadline - __import__('time').monotonic()
        if remaining <= 0:
            raise RuntimeError('timed out waiting for ' + repr(needle) + ': ' + repr(pending[-500:]))
        ready, _, _ = select.select([fd], [], [], min(remaining, .05))
        if ready:
            chunk = os.read(fd, 4096)
            if not chunk:
                raise RuntimeError('shell exited')
            pending += chunk

def send(command):
    os.write(fd, command.encode() + b'\n')

def reply(connection, value):
    connection.sendall((json.dumps(value) + '\n').encode())

# Isolated history, an actual hook, and a known prompt make the result observable.
read_until(b'%')
send("HISTSIZE=100; setopt HIST_IGNORE_SPACE; bindkey -e; zshaddhistory() { [[ $1 != *'stty -echo'* ]]; }; precmd() { sleep 0.2; }; PS1='PROBE> '")
read_until(b'PROBE> ')
send('echo PRIOR_USER_COMMAND')
read_until(b'PROBE> ')

server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(socket_path)
server.listen(8)
ready_path = os.environ['HERDR_DRIVER_READY']
ready_tmp = ready_path + '.tmp'
with open(ready_tmp, 'w') as ready:
    ready.write('ready\\n')
os.replace(ready_tmp, ready_path)
while True:
    connection, _ = server.accept()
    with connection:
        line = b''
        while not line.endswith(b'\n'):
            line += connection.recv(4096)
        request = json.loads(line)
        operation = request['operation']
        if operation == 'run':
            send(request['command'])
            reply(connection, {'ok': True})
        elif operation == 'wait_marker':
            try:
                read_until(request['marker'].encode(), 2)
                reply(connection, {'ok': True})
            except Exception as error:
                reply(connection, {'ok': False, 'error': str(error)})
        elif operation == 'drain_prompt':
            read_until(b'PROBE> ')
            reply(connection, {'ok': True})
        elif operation == 'clear':
            os.write(fd, b'\x15')
            reply(connection, {'ok': True})
        elif operation == 'user':
            send(request['command'])
            read_until(b'PROBE> ')
            reply(connection, {'ok': True})
        elif operation == 'up':
            os.write(fd, b'\x1b[A')
            output = b''
            deadline = __import__('time').monotonic() + 1
            while __import__('time').monotonic() < deadline:
                ready, _, _ = select.select([fd], [], [], .05)
                if ready:
                    chunk = os.read(fd, 4096)
                    if not chunk:
                        break
                    output += chunk
            reply(connection, {'output': output.decode(errors='replace')})
        elif operation == 'stop':
            reply(connection, {'ok': True})
            break
        else:
            raise RuntimeError(operation)
"##,
    );
    let fake_herdr = directory.path().join("herdr");
    make_executable(
        &fake_herdr,
        r##"#!/usr/bin/env python3
import json, os, socket, sys
if sys.argv[1:3] != ['pane', 'run']:
    raise SystemExit('unexpected herdr command: ' + repr(sys.argv))
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
    stream.connect(os.environ['HERDR_DRIVER_SOCKET'])
    stream.sendall((json.dumps({'operation': 'run', 'command': sys.argv[4]}) + '\n').encode())
    if not stream.recv(4096):
        raise SystemExit('driver did not acknowledge input')
"##,
    );

    let mut driver_child = Command::new(&driver)
        .env("HERDR_DRIVER_SOCKET", &driver_socket)
        .env("HERDR_DRIVER_READY", &driver_ready)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if driver_ready.exists() {
            break;
        }
        if driver_child.try_wait().unwrap().is_some() {
            let output = driver_child.wait_with_output().unwrap();
            panic!(
                "PTY driver exited before readiness: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if Instant::now() >= ready_deadline {
            let _ = driver_child.kill();
            let output = driver_child.wait_with_output().unwrap();
            panic!(
                "timed out waiting for PTY driver readiness: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let listener = UnixListener::bind(&api_socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let driver_for_server = driver_socket.clone();
    let server_done = Arc::new(AtomicBool::new(false));
    let server_done_thread = server_done.clone();
    let server = thread::spawn(move || {
        let mut requests = 0;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if server_done_thread.load(Ordering::SeqCst) {
                        return;
                    }
                    assert!(Instant::now() < deadline, "API server timed out");
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("accept API connection: {error}"),
            };
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let method = request["method"].as_str().unwrap();
            let result = match requests {
                0 | 1 => {
                    assert_eq!(method, "session.snapshot");
                    json!({"snapshot":{"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"}],"panes":[{"pane_id":"source","workspace_id":"work","tab_id":"source-tab","foreground_cwd":"/tmp"},{"pane_id":"warm","workspace_id":"pool","tab_id":"pool-tab"},{"pane_id":"other","workspace_id":"pool","tab_id":"other-tab"}]}})
                }
                2 => {
                    assert_eq!(method, "pane.process_info");
                    json!({"process_info":{"foreground_processes":[{"name":"zsh"}]}})
                }
                3 => {
                    assert_eq!(method, "pane.move");
                    assert_eq!(request["params"]["focus"], false);
                    json!({"move_result":{"changed":true,"pane":{"pane_id":"warm","workspace_id":"work","tab_id":"new-tab"}}})
                }
                4 => {
                    assert_eq!(method, "pane.wait_for_output");
                    let marker = request["params"]["match"]["value"].clone();
                    let response = driver_request(
                        &driver_for_server,
                        json!({"operation":"wait_marker","marker":marker}),
                    );
                    assert_eq!(response["ok"], true, "PTY driver: {response}");
                    json!({})
                }
                5 => {
                    assert_eq!(method, "pane.focus");
                    json!({})
                }
                _ => {
                    // Detached refill sees an already-full pool and exits.
                    match method {
                        "session.snapshot" => {
                            json!({"snapshot":{"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"}],"panes":[{"pane_id":"one","workspace_id":"pool"},{"pane_id":"two","workspace_id":"pool"}]}})
                        }
                        "pane.process_info" => {
                            json!({"process_info":{"foreground_processes":[{"name":"zsh"}]}})
                        }
                        other => panic!("unexpected API request after focus: {other}"),
                    }
                }
            };
            requests += 1;
            writeln!(stream, "{}", json!({"id":request["id"],"result":result})).unwrap();
        }
    });

    let mut child = Command::new(executable())
        .arg("new-tab")
        .env("HERDR_SOCKET_PATH", &api_socket)
        .env("HERDR_ACTIVE_PANE_ID", "source")
        .env("HOME", directory.path())
        .env("XDG_STATE_HOME", directory.path().join("state"))
        .env("HERDR_DRIVER_SOCKET", &driver_socket)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", directory.path().display()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "CLI timed out");
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        driver_request(&driver_socket, json!({"operation":"drain_prompt"}))["ok"],
        true
    );
    // This is the regression assertion: no input is accepted between the
    // executable's completion and the first Up key.
    let first_up = driver_request(&driver_socket, json!({"operation":"up"}))["output"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        first_up.contains("PRIOR_USER_COMMAND"),
        "first Up must recall the prior user command: {first_up:?}"
    );
    assert!(!first_up.contains("stty -echo"));
    assert!(!first_up.contains("HERDR_POOL_READY_"));

    // Clear the recalled line without accepting it, then verify subsequent
    // user history independently of the first-Up regression.
    assert_eq!(
        driver_request(&driver_socket, json!({"operation":"clear"}))["ok"],
        true
    );
    assert_eq!(
        driver_request(
            &driver_socket,
            json!({"operation":"user","command":"echo SUBSEQUENT_USER_COMMAND"})
        )["ok"],
        true
    );
    let subsequent_up = driver_request(&driver_socket, json!({"operation":"up"}))["output"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        subsequent_up.contains("SUBSEQUENT_USER_COMMAND"),
        "subsequent Up: {subsequent_up:?}"
    );
    assert!(!subsequent_up.contains("stty -echo"));
    assert!(!subsequent_up.contains("HERDR_POOL_READY_"));

    let _ = driver_request(&driver_socket, json!({"operation":"stop"}));
    let _ = driver_child.kill();
    let _ = driver_child.wait();
    server_done.store(true, Ordering::SeqCst);
    server.join().unwrap();
}
