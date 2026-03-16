use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::platform;

const CACHE_FILE: &str = "/tmp/claude-statusline-cache";
const CACHE_TTL: u64 = 5; // seconds
const CACHE_EVICT: u64 = 3600; // drop entries older than 1 hour

pub struct GitInfo {
    pub branch: Option<String>,
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
}

impl Default for GitInfo {
    fn default() -> Self {
        Self {
            branch: None,
            staged: 0,
            modified: 0,
            untracked: 0,
        }
    }
}

impl GitInfo {
    pub fn has_status(&self) -> bool {
        self.staged > 0 || self.modified > 0 || self.untracked > 0
    }
}

/// Cache key for a given cwd
fn cache_key(cwd: &str) -> String {
    let hash = platform::sha256_hex(cwd);
    hash[..12].to_string()
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read all entries from the single cache file.
/// Format: one line per entry: "key timestamp branch|staged|modified|untracked"
fn read_all_entries() -> HashMap<String, (u64, String)> {
    let mut map = HashMap::new();
    let Ok(content) = fs::read_to_string(CACHE_FILE) else { return map };
    for line in content.lines() {
        let mut parts = line.splitn(3, ' ');
        let Some(key) = parts.next() else { continue };
        let Some(ts_str) = parts.next() else { continue };
        let Some(data) = parts.next() else { continue };
        let Ok(ts) = ts_str.parse::<u64>() else { continue };
        map.insert(key.to_string(), (ts, data.to_string()));
    }
    map
}

/// Write entries back, evicting anything older than CACHE_EVICT.
fn write_all_entries(entries: &HashMap<String, (u64, String)>) {
    let now = now_epoch();
    let mut content = String::new();
    for (key, (ts, data)) in entries {
        if now.saturating_sub(*ts) < CACHE_EVICT {
            content.push_str(key);
            content.push(' ');
            content.push_str(&ts.to_string());
            content.push(' ');
            content.push_str(data);
            content.push('\n');
        }
    }
    let tmp = format!("{}.tmp", CACHE_FILE);
    if fs::write(&tmp, &content).is_ok() {
        let _ = fs::rename(&tmp, CACHE_FILE);
    }
}

fn read_cache(cwd: &str) -> Option<GitInfo> {
    let key = cache_key(cwd);
    let entries = read_all_entries();
    let (ts, data) = entries.get(&key)?;
    let now = now_epoch();
    if now.saturating_sub(*ts) > CACHE_TTL {
        return None;
    }
    parse_cache(data)
}

fn write_cache(cwd: &str, info: &GitInfo) {
    let key = cache_key(cwd);
    let branch = info.branch.as_deref().unwrap_or("");
    let data = format!("{}|{}|{}|{}", branch, info.staged, info.modified, info.untracked);
    let mut entries = read_all_entries();
    entries.insert(key, (now_epoch(), data));
    write_all_entries(&entries);
}

fn parse_cache(content: &str) -> Option<GitInfo> {
    let mut parts = content.splitn(4, '|');
    let branch_str = parts.next()?;
    let staged: u32 = parts.next()?.parse().ok()?;
    let modified: u32 = parts.next()?.parse().ok()?;
    let untracked: u32 = parts.next()?.trim().parse().ok()?;

    let branch = if branch_str.is_empty() {
        None
    } else {
        Some(branch_str.to_string())
    };

    Some(GitInfo {
        branch,
        staged,
        modified,
        untracked,
    })
}

pub fn get_info(cwd: &str) -> GitInfo {
    if let Some(info) = read_cache(cwd) {
        return info;
    }
    let info = fetch_git_info(cwd);
    write_cache(cwd, &info);
    info
}

fn fetch_git_info(cwd: &str) -> GitInfo {
    let mut info = GitInfo::default();

    // Get branch name
    let branch_out = Command::new("git")
        .args(["-c", "core.useBuiltinFSMonitor=false", "branch", "--show-current"])
        .current_dir(cwd)
        .output();

    match branch_out {
        Ok(out) if out.status.success() => {
            let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !branch.is_empty() {
                info.branch = Some(branch);
            }
        }
        _ => return info, // not a git repo
    }

    // Get status in one call
    let status_out = Command::new("git")
        .args([
            "-c",
            "core.useBuiltinFSMonitor=false",
            "status",
            "--porcelain=v1",
        ])
        .current_dir(cwd)
        .output();

    if let Ok(out) = status_out {
        if out.status.success() {
            let output = String::from_utf8_lossy(&out.stdout);
            for line in output.lines() {
                if line.len() < 2 {
                    continue;
                }
                let bytes = line.as_bytes();
                let x = bytes[0]; // index status (staged)
                let y = bytes[1]; // worktree status (modified)

                if x == b'?' && y == b'?' {
                    info.untracked += 1;
                } else {
                    if x != b' ' && x != b'?' {
                        info.staged += 1;
                    }
                    if y != b' ' && y != b'?' {
                        info.modified += 1;
                    }
                }
            }
        }
    }

    info
}

// Timeout wrapper — kill git if it takes too long
pub fn get_info_with_timeout(cwd: &str, timeout: Duration) -> GitInfo {
    if let Some(info) = read_cache(cwd) {
        return info;
    }

    let cwd_owned = cwd.to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let info = fetch_git_info(&cwd_owned);
        let _ = tx.send(info);
    });

    match rx.recv_timeout(timeout) {
        Ok(info) => {
            write_cache(cwd, &info);
            info
        }
        Err(_) => GitInfo::default(),
    }
}
