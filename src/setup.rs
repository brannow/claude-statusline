//! `--configure`: point Claude Code's `statusLine` setting at this binary.
//!
//! Done in Rust rather than in install.sh because editing JSON safely from a
//! shell needs jq or python3, and neither is guaranteed on a fresh machine.

use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Unchanged,
    Created,
    Updated {
        previous: Option<String>,
        backup: PathBuf,
    },
}

/// Entry point for `--configure`. Returns the process exit code.
pub fn run() -> i32 {
    let result = settings_path().and_then(|settings| {
        let command = own_command()?;
        let outcome = configure(&settings, &command)?;
        Ok((settings, command, outcome))
    });

    match result {
        Ok((settings, command, outcome)) => {
            match outcome {
                Outcome::Unchanged => {
                    println!("statusLine already set in {}", settings.display());
                }
                Outcome::Created => {
                    println!("Created {} with statusLine", settings.display());
                }
                Outcome::Updated { previous, backup } => {
                    println!("Updated statusLine in {}", settings.display());
                    if let Some(prev) = previous {
                        println!("  previous command: {}", prev);
                    }
                    println!("  backup: {}", backup.display());
                }
            }
            println!("  command: {}", command);
            0
        }
        Err(e) => {
            eprintln!("claude-statusline: {}", e);
            1
        }
    }
}

/// `$CLAUDE_CONFIG_DIR/settings.json`, falling back to `~/.claude/settings.json`
/// — the same lookup Claude Code itself does.
fn settings_path() -> Result<PathBuf, String> {
    let dir = match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => {
            let home = std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .ok_or("HOME is not set")?;
            PathBuf::from(home).join(".claude")
        }
    };
    Ok(dir.join("settings.json"))
}

/// Absolute path of the running binary, shell-quoted if needed — Claude Code
/// runs the statusLine command through a shell.
fn own_command() -> Result<String, String> {
    let exe = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|e| format!("cannot locate own binary: {}", e))?;
    Ok(shell_quote(&exe.to_string_lossy()))
}

fn shell_quote(s: &str) -> String {
    let safe = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+~".contains(c));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Set `statusLine.type`/`statusLine.command` in `settings`, keeping every
/// other key (including `padding`/`refreshInterval` of an existing statusLine).
pub fn configure(settings: &Path, command: &str) -> Result<Outcome, String> {
    // Write through symlinks: settings.json is often a link into a dotfiles
    // repo or shared between config dirs, and renaming over the link would
    // silently replace it with a regular file.
    let target = if settings.symlink_metadata().is_ok() {
        fs::canonicalize(settings)
            .map_err(|e| format!("cannot resolve {}: {}", settings.display(), e))?
    } else {
        settings.to_path_buf()
    };

    let existing = match fs::read_to_string(&target) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("cannot read {}: {}", target.display(), e)),
    };

    // Never fall back to `{}` on a parse error — that would wipe the user's settings.
    let mut root: Map<String, Value> = match &existing {
        None => Map::new(),
        Some(s) if s.trim().is_empty() => Map::new(),
        Some(s) => match serde_json::from_str(s) {
            Ok(Value::Object(m)) => m,
            Ok(_) => return Err(format!("{} is not a JSON object", target.display())),
            Err(e) => {
                return Err(format!(
                    "{} is not valid JSON ({}); fix it or add statusLine manually",
                    target.display(),
                    e
                ))
            }
        },
    };

    let mut previous = None;
    match root.get_mut("statusLine") {
        Some(Value::Object(sl)) => {
            let same = sl.get("type").and_then(Value::as_str) == Some("command")
                && sl.get("command").and_then(Value::as_str) == Some(command);
            if same {
                return Ok(Outcome::Unchanged);
            }
            previous = sl.get("command").and_then(Value::as_str).map(String::from);
            sl.insert("type".into(), "command".into());
            sl.insert("command".into(), command.into());
        }
        _ => {
            let mut sl = Map::new();
            sl.insert("type".into(), "command".into());
            sl.insert("command".into(), command.into());
            root.insert("statusLine".into(), Value::Object(sl));
        }
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }

    let backup = if existing.is_some() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut name = target.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".bak-{}", ts));
        let backup = target.with_file_name(name);
        fs::copy(&target, &backup)
            .map_err(|e| format!("cannot write backup {}: {}", backup.display(), e))?;
        Some(backup)
    } else {
        None
    };

    let mut out = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| format!("cannot serialize settings: {}", e))?;
    out.push('\n');

    // Temp file + rename so Claude Code never reads a half-written file.
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(target.file_name().unwrap_or_default());
    tmp_name.push(format!(".tmp-{}", std::process::id()));
    let tmp = target.with_file_name(tmp_name);
    let write = || -> std::io::Result<()> {
        fs::write(&tmp, &out)?;
        if let Ok(meta) = fs::metadata(&target) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, &target)
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cannot write {}: {}", target.display(), e));
    }

    Ok(match backup {
        None => Outcome::Created,
        Some(backup) => Outcome::Updated { previous, backup },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "/opt/bin/claude-statusline";

    fn read_json(p: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn creates_missing_file_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("nested/settings.json");
        assert_eq!(configure(&settings, CMD).unwrap(), Outcome::Created);
        let v = read_json(&settings);
        assert_eq!(v["statusLine"]["type"], "command");
        assert_eq!(v["statusLine"]["command"], CMD);
    }

    #[test]
    fn adds_to_existing_settings_preserving_order() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        fs::write(&settings, r#"{"zeta": 1, "alpha": {"b": 2, "a": 1}}"#).unwrap();

        let outcome = configure(&settings, CMD).unwrap();
        let Outcome::Updated { previous, backup } = outcome else {
            panic!("expected Updated, got {:?}", outcome)
        };
        assert_eq!(previous, None);
        assert!(backup.exists());

        let text = fs::read_to_string(&settings).unwrap();
        let (z, a, s) = (
            text.find("zeta").unwrap(),
            text.find("alpha").unwrap(),
            text.find("statusLine").unwrap(),
        );
        assert!(z < a && a < s, "key order changed:\n{}", text);
    }

    #[test]
    fn replaces_foreign_command_keeping_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        fs::write(
            &settings,
            r#"{"statusLine": {"type": "command", "command": "~/old.sh", "padding": 2}}"#,
        )
        .unwrap();

        let outcome = configure(&settings, CMD).unwrap();
        assert!(matches!(
            outcome,
            Outcome::Updated { previous: Some(ref p), .. } if p == "~/old.sh"
        ));
        let v = read_json(&settings);
        assert_eq!(v["statusLine"]["command"], CMD);
        assert_eq!(v["statusLine"]["padding"], 2);
    }

    #[test]
    fn already_configured_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let original = format!(r#"{{"statusLine":{{"type":"command","command":"{}"}}}}"#, CMD);
        fs::write(&settings, &original).unwrap();

        assert_eq!(configure(&settings, CMD).unwrap(), Outcome::Unchanged);
        assert_eq!(fs::read_to_string(&settings).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no backup expected");
    }

    #[test]
    fn invalid_json_is_rejected_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let original = "{ \"a\": 1, } // trailing comma";
        fs::write(&settings, original).unwrap();

        assert!(configure(&settings, CMD).is_err());
        assert_eq!(fs::read_to_string(&settings).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn non_object_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        fs::write(&settings, "[]").unwrap();
        assert!(configure(&settings, CMD).is_err());
    }

    #[test]
    fn writes_through_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.json");
        let link = dir.path().join("settings.json");
        fs::write(&real, r#"{"model": "opus"}"#).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        configure(&link, CMD).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        let v = read_json(&real);
        assert_eq!(v["model"], "opus");
        assert_eq!(v["statusLine"]["command"], CMD);
    }

    #[test]
    fn broken_symlink_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("settings.json");
        std::os::unix::fs::symlink(dir.path().join("missing.json"), &link).unwrap();
        assert!(configure(&link, CMD).is_err());
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn quotes_paths_with_spaces() {
        assert_eq!(shell_quote("/usr/local/bin/x"), "/usr/local/bin/x");
        assert_eq!(shell_quote("/Users/a b/x"), "'/Users/a b/x'");
        assert_eq!(shell_quote("/it's/x"), r"'/it'\''s/x'");
    }
}
