use std::fs;
use std::process::Command;
use std::time::{Duration, SystemTime};

const CACHE_FILE: &str = "/tmp/claude-statusline-git";
const CACHE_TTL: u64 = 5; // seconds

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

pub fn get_info(cwd: &str) -> GitInfo {
    // Try cache first
    if let Some(info) = read_cache() {
        return info;
    }

    let info = fetch_git_info(cwd);
    write_cache(&info);
    info
}

fn cache_age() -> Option<u64> {
    let meta = fs::metadata(CACHE_FILE).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    Some(age.as_secs())
}

fn read_cache() -> Option<GitInfo> {
    let age = cache_age()?;
    if age > CACHE_TTL {
        return None;
    }
    let content = fs::read_to_string(CACHE_FILE).ok()?;
    parse_cache(&content)
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

fn write_cache(info: &GitInfo) {
    let branch = info.branch.as_deref().unwrap_or("");
    let content = format!("{}|{}|{}|{}", branch, info.staged, info.modified, info.untracked);
    // Atomic write: tmp + rename
    let tmp = format!("{}.tmp", CACHE_FILE);
    if fs::write(&tmp, &content).is_ok() {
        let _ = fs::rename(&tmp, CACHE_FILE);
    }
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
    // Try cache first (no timeout needed for cache)
    if let Some(info) = read_cache() {
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
            write_cache(&info);
            info
        }
        Err(_) => GitInfo::default(),
    }
}
