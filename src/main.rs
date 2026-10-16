mod workers;

use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::{fs::OpenOptionsExt, net::UnixStream, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const POOL_LABEL: &str = "__herdr_shell_pool__";
const USAGE: &str =
    "usage: herdr-shell-pool <fill|status|new-tab|split-left|split-down|new-workspace>";

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn items(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
fn home() -> PathBuf {
    let value = env("HOME");
    if !value.is_empty() {
        return value.into();
    }
    // Go's UserHomeDir uses HOME on these platforms as well.
    PathBuf::new()
}
fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
pub(crate) fn state_root() -> PathBuf {
    let root = env("XDG_STATE_HOME");
    if root.is_empty() {
        home().join(".local/state")
    } else {
        root.into()
    }
}
fn socket_path() -> PathBuf {
    let path = env("HERDR_SOCKET_PATH");
    if !path.is_empty() {
        return path.into();
    }
    let root = env("XDG_CONFIG_HOME");
    let mut path = if root.is_empty() {
        home().join(".config")
    } else {
        root.into()
    };
    path.push("herdr");
    let session = env("HERDR_SESSION");
    if !session.is_empty() {
        path = path.join("sessions").join(session);
    }
    path.join("herdr.sock")
}
fn request_timeout(method: &str, params: Value, timeout: Duration) -> Result<Value> {
    let result = (|| -> Result<Value> {
        let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
        socket.connect_timeout(&socket2::SockAddr::unix(socket_path())?, timeout)?;
        let mut stream = UnixStream::from(std::os::fd::OwnedFd::from(socket));
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("read timeout: {e}"))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| format!("write timeout: {e}"))?;
        let deadline = std::time::Instant::now() + timeout;
        writeln!(
            stream,
            "{}",
            json!({"id":format!("shell-pool-{}",nanos()),"method":method,"params":params})
        )?;
        // Incremental decoding also accepts JSON replies without a trailing newline.
        let mut bytes = Vec::new();
        let response: Value = loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or("API timeout")?;
            let mut pollfd = libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis().max(1) as i32) };
            if ready == 0 {
                return Err("API timeout".into());
            }
            if ready < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut buffer = [0u8; 4096];
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                return Err("unexpected end of API response".into());
            }
            bytes.extend_from_slice(&buffer[..n]);
            match serde_json::from_slice(&bytes) {
                Ok(value) => break value,
                Err(error) if error.is_eof() => continue,
                Err(error) => return Err(error.into()),
            }
        };
        if !response.is_object() && !response.is_null() {
            return Err("API response must be an object".into());
        }
        if let Some(result) = response.get("result").filter(|v| !v.is_null())
            && !result.is_object()
        {
            return Err("API result must be an object".into());
        }
        if let Some(error) = response.get("error").filter(|v| !v.is_null()) {
            if !error.is_object() || (!error["message"].is_null() && !error["message"].is_string())
            {
                return Err("invalid API error object".into());
            }
            return Err(format!("Herdr API {method}: {}", text(&error["message"])).into());
        }
        let result = response
            .get("result")
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or(json!({}));
        let nested = match method {
            "pane.move" => Some("move_result"),
            "pane.swap" => Some("swap"),
            _ => None,
        };
        Ok(nested
            .and_then(|key| result.get(key))
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or(result))
    })();
    result.map_err(|e| {
        if e.to_string().starts_with(&format!("Herdr API {method}:")) {
            e
        } else {
            format!("Herdr API {method} failed: {e}").into()
        }
    })
}
fn request(method: &str, params: Value) -> Result<Value> {
    request_timeout(method, params, Duration::from_secs(8))
}
fn snapshot() -> Result<Value> {
    Ok(request("session.snapshot", json!({}))?["snapshot"].clone())
}
fn pool(state: &Value) -> Option<&Value> {
    items(&state["workspaces"])
        .iter()
        .find(|w| text(&w["label"]) == POOL_LABEL)
}
fn panes<'a>(state: &'a Value, workspace: &str) -> Vec<&'a Value> {
    items(&state["panes"])
        .iter()
        .filter(|p| text(&p["workspace_id"]) == workspace)
        .collect()
}
fn idle(pane: &str) -> bool {
    let Ok(result) = request("pane.process_info", json!({"pane_id":pane})) else {
        return false;
    };
    let processes = items(&result["process_info"]["foreground_processes"]);
    !processes.is_empty()
        && processes.iter().all(|p| {
            let name = Path::new(text(&p["name"]))
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            matches!(
                name.strip_prefix('-').unwrap_or(&name),
                "zsh" | "bash" | "sh"
            )
        })
}
pub(crate) fn pool_lock() -> Result<Option<File>> {
    let dir = state_root().join("herdr-shell-pool");
    let mut builder = fs::DirBuilder::new();
    use std::os::unix::fs::DirBuilderExt;
    builder.recursive(true).mode(0o700).create(dir.clone())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(dir.join("lock"))?;
    // Closing the file releases the lock, including on early returns.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) || error.raw_os_error() == Some(libc::EAGAIN)
    {
        Ok(None)
    } else {
        Err(error.into())
    }
}
fn quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    if !value.contains([
        '\t', '\n', '\r', ' ', '\'', '"', '$', '`', '\\', '!', '*', '?', '[', ']', '{', '}', '(',
        ')', '<', '>', '|', '&', ';', '~',
    ]) {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn run_shell(pane: &str, command: &str) -> Result<()> {
    let marker = format!("HERDR_POOL_READY_{}", nanos());
    let escaped = marker
        .bytes()
        .map(|b| format!("\\{b:03o}"))
        .collect::<String>();
    let setup = format!(
        "if [[ -o zle ]]; then zle -I; fi; stty -echo; {command} >/dev/null 2>&1; clear; stty echo; printf '%b\\n' $'{escaped}'"
    );
    let status = Command::new("herdr")
        .args(["pane", "run", pane, &setup])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(format!("exit status {}", status.code().unwrap_or(-1)).into());
    }
    request_timeout(
        "pane.wait_for_output",
        json!({"pane_id":pane,"source":"recent","match":{"type":"substring","value":marker},"timeout_ms":3000}),
        Duration::from_secs(4),
    )?;
    Ok(())
}
fn create_pool_shell(workspace: &str, label: &str) -> Result<()> {
    let pane = if label.is_empty() {
        let state = snapshot()?;
        text(
            &panes(&state, workspace)
                .first()
                .ok_or("new pool workspace has no shell pane")?["pane_id"],
        )
        .to_owned()
    } else {
        let result = request(
            "tab.create",
            json!({"workspace_id":workspace,"cwd":home(),"label":label,"focus":false}),
        )?;
        let tab = text(&result["tab"]["tab_id"]);
        if tab.is_empty() {
            return Err("Herdr did not return the new pool tab id".into());
        }
        let state = snapshot()?;
        let pane = panes(&state, workspace)
            .into_iter()
            .find(|p| text(&p["tab_id"]) == tab)
            .map(|p| text(&p["pane_id"]).to_owned())
            .unwrap_or_default();
        if pane.is_empty() {
            return Err("Herdr did not create a pool shell pane".into());
        }
        pane
    };
    run_shell(&pane, ":")
}
pub(crate) fn fill_locked() -> Result<()> {
    let mut state = snapshot()?;
    if pool(&state).is_none() {
        request(
            "workspace.create",
            json!({"cwd":home(),"label":POOL_LABEL,"focus":false}),
        )?;
        state = snapshot()?;
        let workspace=text(&pool(&state).ok_or("Herdr did not create the reserved shell-pool workspace")?["workspace_id"]).to_owned();
        create_pool_shell(&workspace, "")?;
        state = snapshot()?;
    }
    let workspace = pool(&state)
        .map(|w| text(&w["workspace_id"]))
        .unwrap_or("")
        .to_owned();
    loop {
        let state = snapshot()?;
        let ready = panes(&state, &workspace)
            .into_iter()
            .filter(|p| text(&p["agent"]).is_empty() && idle(text(&p["pane_id"])))
            .count();
        if ready >= 2 {
            return Ok(());
        }
        create_pool_shell(&workspace, &format!("warm-{:04x}", nanos() & 0xffff))?;
    }
}
fn schedule_fill() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = Command::new(exe);
    command
        .arg("fill")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    if let Ok(mut child) = command.spawn() {
        // The short-lived CLI must not wait for background replenishment.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}
fn focus(pane: &str) -> Result<()> {
    request("pane.focus", json!({"pane_id":pane}))?;
    Ok(())
}
fn cold(action: &str, workspace: &str, source: &str, cwd: &str) -> Result<()> {
    if action == "new-tab" {
        request(
            "tab.create",
            json!({"workspace_id":workspace,"cwd":cwd,"focus":true}),
        )?;
        return Ok(());
    }
    let mut params = json!({"target_pane_id":source,"direction":if action=="split-left" {"right"} else {"down"},"focus":true,"cwd":cwd});
    if action == "split-left" {
        params["ratio"] = json!(0.61);
    }
    let result = request("pane.split", params)?;
    let mut pane = text(&result["pane"]["pane_id"]);
    if pane.is_empty() {
        pane = text(&result["pane_id"]);
    }
    if pane.is_empty() {
        return Err("Herdr did not return the new split pane id".into());
    }
    if action == "split-left"
        && request(
            "pane.swap",
            json!({"source_pane_id":source,"target_pane_id":pane}),
        )?["changed"]
            != true
    {
        return Err("new split created, but Herdr could not place it on the left".into());
    }
    focus(pane)
}
fn invoke(action: &str) -> Result<()> {
    let mut source = env("HERDR_ACTIVE_PANE_ID");
    if source.is_empty() {
        source = env("HERDR_PANE_ID");
    }
    if source.is_empty() {
        return Err("Herdr did not provide HERDR_ACTIVE_PANE_ID".into());
    }
    let state = snapshot()?;
    let pane = items(&state["panes"])
        .iter()
        .find(|p| text(&p["pane_id"]) == source)
        .ok_or_else(|| format!("active pane not found: {source}"))?;
    let mut cwd = text(&pane["foreground_cwd"]).to_owned();
    if cwd.is_empty() {
        cwd = text(&pane["cwd"]).to_owned();
    }
    if cwd.is_empty() {
        cwd = home().to_string_lossy().into_owned();
    }
    let workspace = text(&pane["workspace_id"]);
    let tab = text(&pane["tab_id"]);
    let mut fallback = true;
    if let Some(lock) = pool_lock()? {
        let state = snapshot()?;
        let candidate = pool(&state).and_then(|w| {
            panes(&state, text(&w["workspace_id"]))
                .into_iter()
                .find(|p| text(&p["agent"]).is_empty() && idle(text(&p["pane_id"])))
        });
        if let Some(candidate) = candidate {
            let mut destination = if action == "new-tab" {
                json!({"type":"new_tab","workspace_id":workspace})
            } else {
                json!({"type":"tab","tab_id":tab,"target_pane_id":source,"split":"down"})
            };
            if action == "split-left" {
                destination["split"] = json!("right");
                destination["ratio"] = json!(0.61);
            }
            let moved = request(
                "pane.move",
                json!({"pane_id":text(&candidate["pane_id"]),"destination":destination,"focus":false}),
            )?;
            if moved["changed"] == true {
                fallback = false;
                let pane = text(&moved["pane"]["pane_id"]);
                if action == "split-left"
                    && request(
                        "pane.swap",
                        json!({"source_pane_id":source,"target_pane_id":pane}),
                    )?["changed"]
                        != true
                {
                    return Err("warm shell moved, but Herdr could not place it on the left".into());
                }
                let command = format!(
                    "export HERDR_PANE_ID={} HERDR_ACTIVE_PANE_ID={} HERDR_WORKSPACE_ID={} HERDR_TAB_ID={} && cd -- {}",
                    quote(pane),
                    quote(pane),
                    quote(workspace),
                    quote(text(&moved["pane"]["tab_id"])),
                    quote(&cwd)
                );
                run_shell(pane, &command)?;
                focus(pane)?;
            }
        }
        drop(lock);
    }
    if fallback {
        cold(action, workspace, &source, &cwd)?;
    }
    schedule_fill();
    Ok(())
}
fn new_workspace(cwd: &str, label: &str) -> Result<()> {
    // Check the selected directory before any move. The shared shell setup
    // marker alone does not prove that cd succeeded.
    let check = Command::new("/bin/sh")
        .args(["-c", ":"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            format!("cannot enter workspace directory {cwd}: {error}; workspace was not created")
        })?;
    if !check.success() {
        return Err(
            format!("cannot enter workspace directory {cwd}; workspace was not created").into(),
        );
    }
    let _lock = pool_lock()?.ok_or("shell pool is busy; workspace was not created")?;
    let state = snapshot()?;
    let candidate = pool(&state)
        .and_then(|workspace| {
            panes(&state, text(&workspace["workspace_id"]))
                .into_iter()
                .find(|pane| text(&pane["agent"]).is_empty() && idle(text(&pane["pane_id"])))
        })
        .ok_or("no eligible warm shell; workspace was not created")?;
    let moved = request(
        "pane.move",
        json!({
            "pane_id": text(&candidate["pane_id"]),
            "destination": {"type":"new_workspace", "label":label, "tab_label":label},
            "focus": false,
        }),
    )?;
    if moved["changed"] != true {
        return Err("Herdr did not move the warm shell into a new workspace".into());
    }
    let pane = &moved["pane"];
    let pane_id = text(&pane["pane_id"]);
    let workspace_id = text(&pane["workspace_id"]);
    let tab_id = text(&pane["tab_id"]);
    if pane_id.is_empty() || workspace_id.is_empty() || tab_id.is_empty() {
        return Err(
            "warm shell moved, but Herdr did not return its destination identifiers".into(),
        );
    }
    let command = format!(
        "export HERDR_PANE_ID={} HERDR_ACTIVE_PANE_ID={} HERDR_WORKSPACE_ID={} HERDR_TAB_ID={} && cd -- {}",
        quote(pane_id),
        quote(pane_id),
        quote(workspace_id),
        quote(tab_id),
        quote(cwd)
    );
    run_shell(pane_id, &command)?;
    focus(pane_id)?;
    drop(_lock);
    schedule_fill();
    Ok(())
}

fn run() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("status") => {
            let state = snapshot()?;
            let count = pool(&state)
                .map(|w| {
                    panes(&state, text(&w["workspace_id"]))
                        .into_iter()
                        .filter(|p| idle(text(&p["pane_id"])))
                        .count()
                })
                .unwrap_or(0);
            println!("{count}/2 ready");
            Ok(())
        }
        Some("new-workspace") => {
            const WORKSPACE_USAGE: &str =
                "usage: herdr-shell-pool new-workspace --cwd <absolute-path> --label <name>";
            let mut cwd = None;
            let mut label = None;
            let mut pairs = args[2..].chunks_exact(2);
            for pair in &mut pairs {
                match pair[0].as_str() {
                    "--cwd" if cwd.is_none() => cwd = Some(pair[1].as_str()),
                    "--label" if label.is_none() => label = Some(pair[1].as_str()),
                    _ => return Err(WORKSPACE_USAGE.into()),
                }
            }
            let cwd = cwd
                .filter(|path| Path::new(path).is_absolute())
                .ok_or(WORKSPACE_USAGE)?;
            let label = label
                .filter(|name| !name.is_empty())
                .ok_or(WORKSPACE_USAGE)?;
            if !pairs.remainder().is_empty() {
                return Err(WORKSPACE_USAGE.into());
            }
            new_workspace(cwd, label)
        }
        Some("fill") => workers::launch(),
        Some("fill-worker") => {
            if args.len() != 3 {
                Err("missing fill worker token".into())
            } else {
                workers::run(&args[2])
            }
        }
        Some(action @ ("new-tab" | "split-left" | "split-down")) => invoke(action),
        _ => Err(USAGE.into()),
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("herdr-shell-pool: {error}");
        std::process::exit(1);
    }
}
