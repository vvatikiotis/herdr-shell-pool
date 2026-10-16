use crate::{Result, fill_locked, pool_lock, state_root};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
struct FillWorker {
    role: String,
    pid: i32,
    token: String,
    generation: String,
    started: String,
    created: i64,
    executable: String,
}

fn worker_dir() -> PathBuf {
    state_root().join("herdr-shell-pool").join("workers")
}

pub fn launch() -> Result<()> {
    let executable = std::env::current_exe()?;
    let mut bytes = [0u8; 24];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token = hex::encode(bytes);
    let status = Command::new(&executable)
        .arg("fill-worker")
        .arg(token)
        .stdin(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exit status {}", status.code().unwrap_or(-1)).into())
    }
}

pub fn run(token: &str) -> Result<()> {
    if token.len() != 48 {
        return Err("invalid fill worker token".into());
    }
    let mut decoded = [0u8; 24];
    hex::decode_to_slice(token, &mut decoded)
        .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })?;

    let executable = std::env::current_exe()?;
    let generation = {
        let contents = fs::read(&executable)?;
        format!("{:x}", Sha256::digest(contents))
    };
    let (started, command) = process_identity(std::process::id() as i32)?;
    let executable_string = executable.to_string_lossy().into_owned();
    if command != format!("{executable_string} fill-worker {token}") {
        return Err("cannot verify fill worker identity".into());
    }

    let dir = worker_dir();
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let lock_path = dir.join("registry.lock");
    let registry_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(lock_path)?;
    flock(&registry_lock, libc::LOCK_EX)?;
    let _registry_lock = FlockGuard(&registry_lock);

    let self_record = FillWorker {
        role: "fill".into(),
        pid: std::process::id() as i32,
        token: token.into(),
        generation,
        started,
        created: SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as i64,
        executable: executable_string,
    };
    let path = dir.join(format!("{token}.json"));
    let data = serde_json::to_vec(&self_record)?;
    let _registration = RegistrationCleanup { path: path.clone() };
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(&data)?;
    drop(file);

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(true)
                || entry.file_name().to_string_lossy() == format!("{token}.json")
                || entry.path().extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                continue;
            }
            let Ok(data) = fs::read(&entry_path) else {
                continue;
            };
            let Ok(other) = serde_json::from_slice::<FillWorker>(&data) else {
                continue;
            };
            if entry.file_name().to_string_lossy() != format!("{}.json", other.token)
                || !retirable_worker(&self_record, &other)
            {
                continue;
            }
            if let Ok((birth, args)) = process_identity(other.pid)
                && birth == other.started
                && args == format!("{} fill-worker {}", other.executable, other.token)
            {
                unsafe {
                    libc::kill(other.pid, libc::SIGTERM);
                }
            }
        }
    }
    drop(_registry_lock);

    let Some(_pool_lock) = pool_lock()? else {
        return Ok(());
    };
    fill_locked()
}

fn process_identity(pid: i32) -> Result<(String, String)> {
    if pid <= 0 {
        return Err("invalid pid".into());
    }
    let output = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("lstart=")
        .arg("-o")
        .arg("command=")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("ps failed").into());
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if line.len() < 26 || !line.is_char_boundary(24) {
        return Err("incomplete process identity".into());
    }
    Ok((line[..24].into(), line[24..].trim().into()))
}

fn retirable_worker(self_record: &FillWorker, other: &FillWorker) -> bool {
    other.role == "fill"
        && other.pid > 0
        && other.pid != self_record.pid
        && !other.token.is_empty()
        && !other.started.is_empty()
        && !other.executable.is_empty()
        && other.generation != self_record.generation
        && !other.generation.is_empty()
        && other.created < self_record.created
}

fn flock(file: &File, operation: libc::c_int) -> Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().into())
    }
}

struct FlockGuard<'a>(&'a File);
impl Drop for FlockGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

struct RegistrationCleanup {
    path: PathBuf,
}
impl Drop for RegistrationCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
