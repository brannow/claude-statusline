use std::process::Command;

/// Read the Claude Code OAuth token from the OS credential store.
/// Token is held only in memory — never written to disk or logs.
///
/// The keychain service name is "Claude Code-credentials-{hash}" where {hash}
/// is the first 8 hex chars of SHA256(config_dir_path). When CLAUDE_CONFIG_DIR
/// is not set, the default config dir (~/.claude) is used.
///
/// Only subscription-based installs store OAuth tokens; API-key installs won't
/// have a matching keychain entry and this returns Err.

/// Derive the keychain service name for the active Claude Code installation.
/// Format: "Claude Code-credentials-{sha256(config_dir)[:8]}"
fn credential_service_name() -> String {
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
        format!("{home}/.claude")
    });
    let hash = sha256_hex(&config_dir);
    format!("Claude Code-credentials-{}", &hash[..8])
}

/// SHA-256 producing a hex string — pure Rust, no external dependencies.
fn sha256_hex(input: &str) -> String {
    sha256_software(input)
}

#[cfg(target_os = "macos")]
pub fn get_oauth_token() -> Result<String, String> {
    let service = credential_service_name();
    let output = Command::new("security")
        .args(["find-generic-password", "-s", &service, "-w"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("failed to invoke security: {e}"))?;

    if !output.status.success() {
        return Err(format!("Claude Code credentials not found (service: {service})"));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    extract_access_token(raw.trim())
        .ok_or_else(|| "could not parse access token from credentials".into())
}

#[cfg(target_os = "linux")]
pub fn get_oauth_token() -> Result<String, String> {
    // Try credentials file first (Linux/WSL2)
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
        format!("{home}/.claude")
    });
    let creds_path = std::path::Path::new(&config_dir).join(".credentials.json");
    if let Ok(contents) = std::fs::read_to_string(&creds_path) {
        if let Some(token) = extract_access_token(contents.trim()) {
            return Ok(token);
        }
    }

    // Fallback: secret-tool with hashed service name
    let service = credential_service_name();
    let output = Command::new("secret-tool")
        .args(["lookup", "service", &service])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("failed to invoke secret-tool: {e}"))?;

    if !output.status.success() {
        return Err("Claude Code credentials not found".into());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    extract_access_token(raw.trim())
        .ok_or_else(|| "could not parse access token from credentials".into())
}

fn extract_access_token(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let token = v.get("claudeAiOauth")?.get("accessToken")?.as_str()?.to_string();
    if token.is_empty() { None } else { Some(token) }
}

/// Pure-Rust SHA-256 implementation — no external dependencies.
fn sha256_software(input: &str) -> String {
    let msg = input.as_bytes();

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Pre-processing: padding
    let bit_len = (msg.len() as u64) * 8;
    let mut data = msg.to_vec();
    data.push(0x80);
    while (data.len() % 64) != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 64-byte chunk
    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1);
            d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SHA-256 ──────────────────────────────────────────────────────────────

    #[test]
    fn sha256_empty_string() {
        // NIST test vector: SHA-256("") = e3b0c44298fc1c149afbf4c8996fb924...
        let hash = sha256_hex("");
        assert_eq!(&hash[..8], "e3b0c442");
    }

    #[test]
    fn sha256_known_value() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223...
        let hash = sha256_hex("abc");
        assert_eq!(&hash[..8], "ba7816bf");
    }

    #[test]
    fn sha256_config_dir_private() {
        // Verified against Python hashlib and macOS Keychain entry
        let hash = sha256_hex("/Users/b.rannow/.claude-private");
        assert_eq!(&hash[..8], "11b1338c");
    }

    #[test]
    fn sha256_longer_input() {
        // SHA-256("The quick brown fox jumps over the lazy dog")
        let hash = sha256_hex("The quick brown fox jumps over the lazy dog");
        assert_eq!(
            &hash[..16],
            "d7a8fbb307d78094",
        );
    }

    // ── credential_service_name ──────────────────────────────────────────────

    #[test]
    fn service_name_uses_claude_config_dir_env() {
        // Temporarily set CLAUDE_CONFIG_DIR for this test
        let orig = std::env::var("CLAUDE_CONFIG_DIR").ok();
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", "/tmp/test-claude-config") };

        let name = credential_service_name();
        let expected_hash = &sha256_hex("/tmp/test-claude-config")[..8];
        assert_eq!(name, format!("Claude Code-credentials-{expected_hash}"));

        // Restore
        match orig {
            Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
            None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
        }
    }

    // ── extract_access_token ─────────────────────────────────────────────────

    #[test]
    fn extract_valid_token() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abc123","refreshToken":"rt","expiresAt":9999}}"#;
        assert_eq!(
            extract_access_token(json),
            Some("sk-ant-oat01-abc123".to_string()),
        );
    }

    #[test]
    fn extract_missing_oauth_field() {
        let json = r#"{"someOther":{"key":"value"}}"#;
        assert_eq!(extract_access_token(json), None);
    }

    #[test]
    fn extract_empty_token() {
        let json = r#"{"claudeAiOauth":{"accessToken":""}}"#;
        assert_eq!(extract_access_token(json), None);
    }

    #[test]
    fn extract_invalid_json() {
        assert_eq!(extract_access_token("not json at all"), None);
    }

    #[test]
    fn extract_missing_access_token_key() {
        let json = r#"{"claudeAiOauth":{"refreshToken":"rt"}}"#;
        assert_eq!(extract_access_token(json), None);
    }
}
