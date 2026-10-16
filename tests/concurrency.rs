//! Controlled request overlap and background-refill gates, not timing-based stress tests.
//! No live Herdr resources are used.
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{
    fs::PermissionsExt,
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);
const CWD: &str = "/tmp/project with space";

fn executable() -> PathBuf {
    std::env::var_os("HERDR_POOL_TEST_BIN")
        .map(Into::into)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_herdr-shell-pool").into())
}

#[derive(Debug)]
struct Model {
    panes: Vec<Value>,
    moves: Vec<String>,
    cold_tabs: usize,
    focused: Vec<String>,
    setup_acknowledged: bool,
    refills: usize,
}
impl Model {
    fn new(shells: usize) -> Self {
        let mut panes = vec![
            json!({"pane_id":"source","workspace_id":"work","tab_id":"source-tab","foreground_cwd":CWD}),
        ];
        for n in 1..=shells {
            panes.push(json!({"pane_id":format!("pool-{n}"),"terminal_id":format!("warm-terminal-{n}"),"workspace_id":"pool","tab_id":format!("pool-tab-{n}")}));
        }
        Self {
            panes,
            moves: vec![],
            cold_tabs: 0,
            focused: vec![],
            setup_acknowledged: false,
            refills: 0,
        }
    }
    fn snapshot(&self) -> Value {
        json!({"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"},{"workspace_id":"work"}],"panes":self.panes})
    }
    fn commit_move(&mut self) -> Value {
        let pane = self
            .panes
            .iter_mut()
            .find(|p| p["pane_id"] == "pool-1")
            .unwrap();
        pane["pane_id"] = json!("moved-pane");
        pane["workspace_id"] = json!("work");
        pane["tab_id"] = json!("warm-destination");
        pane.clone()
    }
}

#[derive(Clone, Copy)]
enum Scenario {
    OverlappingTabs,
    BlockedRefill,
}
enum Control {
    ReleaseMove,
    ReleaseRefill,
    Stop,
}
struct PendingMove {
    stream: UnixStream,
    id: Value,
}
fn reply(stream: &mut UnixStream, id: &Value, result: Value) {
    write!(stream, "{}", json!({"id":id,"result":result})).unwrap();
}

// The first move response is parked, not waited on: B and refill workers can
// still connect and receive responses while A is blocked inside its pool lock.
fn serve(
    listener: UnixListener,
    controls: Receiver<Control>,
    gate: Sender<()>,
    scenario: Scenario,
) -> Model {
    let mut model = Model::new(match scenario {
        Scenario::OverlappingTabs => 3,
        Scenario::BlockedRefill => 2,
    });
    let mut pending: Option<PendingMove> = None;
    let mut refill: Option<PendingMove> = None;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match controls.try_recv() {
            Ok(Control::ReleaseMove) => {
                let mut parked = pending.take().expect("A must have a pending move");
                let pane = model.commit_move();
                reply(
                    &mut parked.stream,
                    &parked.id,
                    json!({"move_result":{"changed":true,"pane":pane}}),
                );
            }
            Ok(Control::ReleaseRefill) => {
                let mut parked = refill.take().expect("refill must be pending");
                model.panes.push(json!({"pane_id":"refilled-pane","terminal_id":"refilled-terminal","workspace_id":"pool","tab_id":"refilled-tab"}));
                reply(
                    &mut parked.stream,
                    &parked.id,
                    json!({"tab":{"tab_id":"refilled-tab"}}),
                );
            }
            Ok(Control::Stop) | Err(mpsc::TryRecvError::Disconnected) => return model,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(WAIT)).unwrap();
                stream.set_write_timeout(Some(WAIT)).unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let params = &request["params"];
                let id = &request["id"];
                let result = match request["method"].as_str().unwrap() {
                    "session.snapshot" => json!({"snapshot":model.snapshot()}),
                    "pane.process_info" => {
                        assert!(
                            model
                                .panes
                                .iter()
                                .any(|p| p["pane_id"] == params["pane_id"])
                        );
                        json!({"process_info":{"foreground_processes":[{"name":"zsh"}]}})
                    }
                    "pane.move" => {
                        let pane = params["pane_id"].as_str().unwrap().to_owned();
                        model.moves.push(pane.clone());
                        if pending.is_some() {
                            // Keep the selected pane visible until commit: a missing lock
                            // must expose duplicate selection, not benefit from fake serialization.
                            write!(stream, "{}", json!({"id":id,"error":{"message":"duplicate warm-shell move while A is pending"}})).unwrap();
                            continue;
                        }
                        assert_eq!(pane, "pool-1");
                        assert_eq!(
                            params["destination"],
                            json!({"type":"new_tab","workspace_id":"work"})
                        );
                        assert_eq!(params["focus"], false);
                        match scenario {
                            Scenario::OverlappingTabs => {
                                pending = Some(PendingMove {
                                    stream,
                                    id: id.clone(),
                                });
                                gate.send(()).unwrap();
                                continue;
                            }
                            Scenario::BlockedRefill => {
                                let pane = model.commit_move();
                                json!({"move_result":{"changed":true,"pane":pane}})
                            }
                        }
                    }
                    "tab.create" => {
                        if let Scenario::BlockedRefill = scenario {
                            assert!(model.setup_acknowledged);
                            assert_eq!(model.focused, ["moved-pane"]);
                            assert_eq!(params["workspace_id"], "pool");
                            assert_eq!(params["focus"], false);
                            model.refills += 1;
                            assert_eq!(model.refills, 1);
                            refill = Some(PendingMove {
                                stream,
                                id: id.clone(),
                            });
                            gate.send(()).unwrap();
                            continue;
                        }
                        assert!(
                            pending.is_some(),
                            "cold recovery must finish before A is released"
                        );
                        assert_eq!(params["workspace_id"], "work");
                        assert_eq!(params["cwd"], CWD);
                        assert_eq!(params["focus"], true);
                        model.cold_tabs += 1;
                        assert_eq!(model.cold_tabs, 1);
                        model.panes.push(json!({"pane_id":"cold-pane","terminal_id":"cold-terminal","workspace_id":"work","tab_id":"cold-destination","cwd":CWD}));
                        model.focused.push("cold-pane".into());
                        json!({"tab":{"tab_id":"cold-destination"}})
                    }
                    "pane.wait_for_output" => {
                        assert!(pending.is_none());
                        if params["pane_id"] == "refilled-pane" {
                            assert!(refill.is_none());
                            assert_eq!(model.refills, 1);
                        } else {
                            assert_eq!(params["pane_id"], "moved-pane");
                            model.setup_acknowledged = true;
                        }
                        json!({})
                    }
                    "pane.focus" => {
                        assert!(
                            model.setup_acknowledged,
                            "warm destination must finish setup before focus"
                        );
                        assert_eq!(params["pane_id"], "moved-pane");
                        model.focused.push("moved-pane".into());
                        json!({})
                    }
                    other => panic!("unexpected API request: {other}"),
                };
                reply(&mut stream, id, result);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "test server timed out");
                // Polling only delivers events; ordering is established by the gate
                // event and B's observed exit, never by a fixed sleep interval.
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("accept test API connection: {error}"),
        }
    }
}

fn identity(pid: u32) -> Option<String> {
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart=", "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    // A zombie cannot run or access the fixture; its command no longer matches.
    if text.len() < 26 {
        return None;
    }
    Some(text)
}
fn process_records(dir: &Path) -> Vec<(String, u32, String)> {
    fs::read_to_string(dir.join("processes.log"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '|');
            Some((
                fields.next()?.into(),
                fields.next()?.parse().ok()?,
                fields.next()?.into(),
            ))
        })
        .collect()
}

// A private executable path makes even a not-yet-registered detached launcher
// discoverable for failure cleanup, without matching any live shell-pool process.
fn owned_processes(executable: &Path) -> Vec<(u32, String)> {
    let Ok(output) = Command::new("/bin/ps")
        .args(["-ax", "-o", "pid=", "-o", "lstart=", "-o", "command="])
        .output()
    else {
        return vec![];
    };
    let prefix = format!("{} ", executable.display());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim().split_once(char::is_whitespace)?;
            let identity = rest.trim();
            let command = identity.get(24..)?.trim();
            let action = command.strip_prefix(&prefix)?;
            if action != "new-tab" && action != "fill" && !action.starts_with("fill-worker ") {
                return None;
            }
            Some((pid.parse().ok()?, identity.to_owned()))
        })
        .collect()
}

struct Fixture {
    dir: tempfile::TempDir,
    executable: PathBuf,
    controls: Sender<Control>,
    pending: Receiver<()>,
    server: Option<JoinHandle<Model>>,
    children: Vec<Option<Child>>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_scenario(Scenario::OverlappingTabs)
    }
    fn with_scenario(scenario: Scenario) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("hp-overlap")
            .tempdir_in("/tmp")
            .unwrap();
        let executable_path = dir.path().join("shell-pool");
        fs::copy(executable(), &executable_path).unwrap();
        fs::set_permissions(&executable_path, fs::Permissions::from_mode(0o700)).unwrap();
        let executable = fs::canonicalize(executable_path).unwrap();
        let listener = UnixListener::bind(dir.path().join("api.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (controls, rx) = mpsc::channel();
        let (tx, pending) = mpsc::channel();
        let server = thread::spawn(move || serve(listener, rx, tx, scenario));
        fs::write(
            dir.path().join("herdr"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HERDR_TEST_LOG\"\n",
        )
        .unwrap();
        // Observe real background worker and launcher identities at the existing
        // process boundary. Delegate to real ps so identity checks stay intact.
        fs::write(dir.path().join("ps"), r#"#!/bin/sh
worker="$2"
parent=$(/bin/ps -p "$worker" -o ppid= | tr -d ' ')
worker_identity=$(/bin/ps -p "$worker" -o lstart= -o command=)
parent_identity=$(/bin/ps -p "$parent" -o lstart= -o command=)
printf 'worker|%s|%s\nparent|%s|%s\n' "$worker" "$worker_identity" "$parent" "$parent_identity" >> "$HERDR_PROCESS_LOG"
exec /bin/ps "$@"
"#).unwrap();
        for name in ["herdr", "ps"] {
            fs::set_permissions(dir.path().join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            dir,
            executable,
            controls,
            pending,
            server: Some(server),
            children: vec![],
        }
    }
    fn spawn(&mut self) -> usize {
        let child = Command::new(&self.executable)
            .arg("new-tab")
            .env("HERDR_SOCKET_PATH", self.dir.path().join("api.sock"))
            .env("HERDR_ACTIVE_PANE_ID", "source")
            .env_remove("HERDR_PANE_ID")
            .env_remove("HERDR_SESSION")
            .env("HOME", self.dir.path())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("XDG_STATE_HOME", self.dir.path().join("state"))
            .env("HERDR_TEST_LOG", self.dir.path().join("shell.log"))
            .env("HERDR_PROCESS_LOG", self.dir.path().join("processes.log"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.dir.path().display()),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        self.children.push(Some(child));
        self.children.len() - 1
    }
    fn finish_child(&mut self, index: usize) -> Output {
        let deadline = Instant::now() + WAIT;
        let child = self.children[index].as_mut().unwrap();
        while child.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "CLI did not finish before its deadline"
            );
            thread::sleep(Duration::from_millis(5));
        }
        self.children[index]
            .take()
            .unwrap()
            .wait_with_output()
            .unwrap()
    }
    fn wait_for_refills(&self, expected_workers: usize) {
        let deadline = Instant::now() + WAIT;
        loop {
            let records = process_records(self.dir.path());
            let workers = records
                .iter()
                .filter(|(role, _, _)| role == "worker")
                .count();
            let running = records
                .iter()
                .any(|(_, pid, recorded)| identity(*pid).as_deref() == Some(recorded.trim()));
            if workers == expected_workers && !running {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected_workers} detached refills to exit; records: {records:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn finish_server(&mut self) -> Model {
        self.controls.send(Control::Stop).unwrap();
        self.server.take().unwrap().join().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Foreground children are owned Child handles, not unverified PIDs.
        for child in self.children.iter_mut().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Include launchers that have not reached the ps shim or registry yet.
        // Recheck start time and command immediately before signalling a PID.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let processes = owned_processes(&self.executable);
            if processes.is_empty() || Instant::now() >= deadline {
                break;
            }
            for (pid, recorded) in processes {
                if identity(pid).as_deref() == Some(recorded.as_str()) {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.controls.send(Control::Stop);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.is_empty(),
        "existing cold recovery must remain silent"
    );
}

#[test]
fn overlapping_new_tabs_do_not_move_the_same_warm_shell_twice() {
    let mut fixture = Fixture::new();
    let a = fixture.spawn();
    fixture
        .pending
        .recv_timeout(WAIT)
        .expect("A must reach its move while holding the pool lock");
    let b = fixture.spawn();
    // B must complete while A's response remains parked. A serial fake server
    // or a blocking lock would fail this bounded wait instead of passing accidentally.
    assert_success(fixture.finish_child(b));
    assert!(
        fixture.children[a]
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none()
    );
    fixture.controls.send(Control::ReleaseMove).unwrap();
    assert_success(fixture.finish_child(a));
    fixture.wait_for_refills(2);
    let model = fixture.finish_server();
    assert_eq!(
        model.moves,
        ["pool-1"],
        "only one original pool pane may be selected"
    );
    assert_eq!(model.cold_tabs, 1);
    assert_eq!(model.focused, ["cold-pane", "moved-pane"]);
    let destination: Vec<_> = model
        .panes
        .iter()
        .filter(|p| p["workspace_id"] == "work" && p["pane_id"] != "source")
        .collect();
    assert_eq!(destination.len(), 2);
    assert_eq!(destination[0]["terminal_id"], "warm-terminal-1");
    assert_eq!(destination[0]["tab_id"], "warm-destination");
    assert_eq!(destination[1]["terminal_id"], "cold-terminal");
    assert_eq!(destination[1]["tab_id"], "cold-destination");
    let remaining: Vec<_> = model
        .panes
        .iter()
        .filter(|p| p["workspace_id"] == "pool")
        .map(|p| p["terminal_id"].as_str().unwrap())
        .collect();
    assert_eq!(remaining, ["warm-terminal-2", "warm-terminal-3"]);
    let setup = fs::read_to_string(fixture.dir.path().join("shell.log")).unwrap();
    assert!(setup.contains("HERDR_PANE_ID=moved-pane HERDR_ACTIVE_PANE_ID=moved-pane HERDR_WORKSPACE_ID=work HERDR_TAB_ID=warm-destination"));
    assert!(setup.contains("cd -- '/tmp/project with space'"));
}

#[test]
fn new_tab_exits_while_background_replenishment_is_blocked() {
    let mut fixture = Fixture::with_scenario(Scenario::BlockedRefill);
    let request = fixture.spawn();
    fixture
        .pending
        .recv_timeout(WAIT)
        .expect("background replenishment must request a new pool shell");

    // The refill's tab.create response is still parked. Finishing here proves
    // foreground completion is independent of refill completion, not just fast.
    assert_success(fixture.finish_child(request));
    let records = process_records(fixture.dir.path());
    assert_eq!(
        records
            .iter()
            .filter(|(role, _, _)| role == "worker")
            .count(),
        1
    );
    for role in ["worker", "parent"] {
        let (_, pid, recorded) = records.iter().find(|(kind, _, _)| kind == role).unwrap();
        assert_eq!(
            identity(*pid).as_deref(),
            Some(recorded.trim()),
            "refill {role} must still be running"
        );
    }

    fixture.controls.send(Control::ReleaseRefill).unwrap();
    fixture.wait_for_refills(1);
    let model = fixture.finish_server();
    assert_eq!(model.moves, ["pool-1"]);
    assert_eq!(model.cold_tabs, 0);
    assert_eq!(model.refills, 1);
    assert_eq!(model.focused, ["moved-pane"]);
    let remaining: Vec<_> = model
        .panes
        .iter()
        .filter(|p| p["workspace_id"] == "pool")
        .map(|p| p["terminal_id"].as_str().unwrap())
        .collect();
    assert_eq!(remaining, ["warm-terminal-2", "refilled-terminal"]);
}
