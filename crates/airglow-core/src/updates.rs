//! Native Rust GitHub auto-updater: release checking, chunked HTTPS download with progress,
//! SHA-256 verification via Windows PowerShell `Get-FileHash`, and detached self-swap restart.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::automations::local_time_hm_wday;
use crate::config::UpdateChannel;

pub const GITHUB_RELEASES_API: &str =
    "https://api.github.com/repos/Heavenlystorm/airglow/releases";
pub const GITHUB_RELEASES_WEB: &str = "https://github.com/Heavenlystorm/airglow/releases";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate {
        current: String,
        checked_at: String,
    },
    Available {
        current: String,
        latest: String,
        notes: String,
        html_url: String,
        exe_url: Option<String>,
        sha256_url: Option<String>,
        is_prerelease: bool,
    },
    Downloading {
        latest: String,
        progress_pct: u8,
    },
    ReadyToRestart {
        latest: String,
        staged_exe: String,
    },
    Error(String),
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

/// Returns the compiled Airglow crate version (`CARGO_PKG_VERSION`).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreIdent {
    Num(u64),
    Alpha(String),
}

impl Ord for PreIdent {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Num(a), Self::Num(b)) => a.cmp(b),
            (Self::Alpha(a), Self::Alpha(b)) => a.cmp(b),
            // SemVer 2.0.0 §11.4.3: numeric identifiers always have lower precedence than non-numeric.
            (Self::Num(_), Self::Alpha(_)) => Ordering::Less,
            (Self::Alpha(_), Self::Num(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for PreIdent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSemver {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<PreIdent>,
}

impl Ord for ParsedSemver {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => Ordering::Equal,
                // Stable release has higher precedence than a pre-release of the same core version.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.prerelease.iter().zip(other.prerelease.iter()) {
                        let ord = a.cmp(b);
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                    self.prerelease.len().cmp(&other.prerelease.len())
                }
            })
    }
}

impl PartialOrd for ParsedSemver {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_semver(raw: &str) -> Option<ParsedSemver> {
    let trimmed = raw.trim();
    let without_v = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    if without_v.is_empty() {
        return None;
    }
    // Strip optional build metadata (+build.123) per SemVer 2.0.0.
    let without_build = without_v.split('+').next().unwrap_or(without_v);
    let (core, pre_str) = match without_build.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (without_build, None),
    };
    let mut parts = core.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = match parts.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    let mut prerelease = Vec::new();
    if let Some(pre) = pre_str {
        if pre.is_empty() {
            return None;
        }
        for seg in pre.split('.') {
            if seg.is_empty() {
                return None;
            }
            if seg.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(n) = seg.parse::<u64>() {
                    prerelease.push(PreIdent::Num(n));
                } else {
                    prerelease.push(PreIdent::Alpha(seg.to_ascii_lowercase()));
                }
            } else {
                prerelease.push(PreIdent::Alpha(seg.to_ascii_lowercase()));
            }
        }
    }
    Some(ParsedSemver {
        major,
        minor,
        patch,
        prerelease,
    })
}

/// Returns `true` when `candidate` is strictly newer than `current` according to SemVer 2.0.0
/// (handling optional leading `v`/`V` prefixes, pre-releases, and build metadata).
pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let Some(cur) = parse_semver(current) else {
        return false;
    };
    let Some(cand) = parse_semver(candidate) else {
        return false;
    };
    cand > cur
}

fn clean_version_label(tag: &str) -> String {
    let trimmed = tag.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
        .to_string()
}

fn now_checked_label() -> String {
    let (h, m, _) = local_time_hm_wday();
    format!("{h:02}:{m:02}")
}

fn select_release_for_channel(
    releases: &[GitHubRelease],
    current: &str,
    channel: UpdateChannel,
    checked_at: String,
) -> UpdateStatus {
    let mut best: Option<(&GitHubRelease, ParsedSemver)> = None;
    for rel in releases {
        if rel.draft {
            continue;
        }
        let Some(ver) = parse_semver(&rel.tag_name) else {
            continue;
        };
        let is_pre = rel.prerelease || !ver.prerelease.is_empty();
        if channel == UpdateChannel::Stable && is_pre {
            continue;
        }
        let replace = match &best {
            None => true,
            Some((_, best_ver)) => ver > *best_ver,
        };
        if replace {
            best = Some((rel, ver));
        }
    }

    let Some((rel, ver)) = best else {
        return UpdateStatus::UpToDate {
            current: clean_version_label(current),
            checked_at,
        };
    };

    if !is_newer_version(current, &rel.tag_name) {
        return UpdateStatus::UpToDate {
            current: clean_version_label(current),
            checked_at,
        };
    }

    let exe_url = rel
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("airglow.exe"))
        .or_else(|| {
            rel.assets.iter().find(|a| {
                let lower = a.name.to_ascii_lowercase();
                lower.ends_with(".exe") && lower.contains("airglow")
            })
        })
        .map(|a| a.browser_download_url.clone());

    let sha256_url = rel
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("sha256sums.txt"))
        .or_else(|| {
            rel.assets.iter().find(|a| {
                let lower = a.name.to_ascii_lowercase();
                lower.contains("sha256") && lower.ends_with(".txt")
            })
        })
        .map(|a| a.browser_download_url.clone());

    let html_url = if rel.html_url.trim().is_empty() {
        GITHUB_RELEASES_WEB.to_string()
    } else {
        rel.html_url.clone()
    };

    let notes = rel
        .body
        .as_deref()
        .unwrap_or("")
        .trim()
        .chars()
        .take(2000)
        .collect::<String>();

    UpdateStatus::Available {
        current: clean_version_label(current),
        latest: clean_version_label(&rel.tag_name),
        notes,
        html_url,
        exe_url,
        sha256_url,
        is_prerelease: rel.prerelease || !ver.prerelease.is_empty(),
    }
}

fn build_http_client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent(format!("Airglow/{}", current_version()))
        .build()
        .context("build HTTPS update client")
}

/// Checks GitHub Releases for a newer Airglow build on the chosen `channel`.
pub async fn check_github_updates_async(channel: UpdateChannel) -> UpdateStatus {
    let client = match build_http_client(12) {
        Ok(c) => c,
        Err(e) => return UpdateStatus::Error(format!("Update client error: {e}")),
    };
    let url = format!("{GITHUB_RELEASES_API}?per_page=15");
    let resp = match client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return UpdateStatus::Error(format!("Could not reach GitHub: {e}")),
    };

    let status = resp.status();
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return UpdateStatus::Error(
            "GitHub API rate limit reached — try again in a few minutes.".to_string(),
        );
    }
    if status.as_u16() == 404 {
        return UpdateStatus::UpToDate {
            current: current_version().to_string(),
            checked_at: now_checked_label(),
        };
    }
    if !status.is_success() {
        return UpdateStatus::Error(format!("GitHub returned HTTP {}", status.as_u16()));
    }

    let releases: Vec<GitHubRelease> = match resp.json().await {
        Ok(list) => list,
        Err(e) => return UpdateStatus::Error(format!("Invalid GitHub release data: {e}")),
    };

    select_release_for_channel(&releases, current_version(), channel, now_checked_label())
}

/// Resolves the staging path for a downloaded update (`%LOCALAPPDATA%\Airglow\Airglow.new.exe`,
/// falling back alongside the current executable).
pub fn staged_update_path() -> PathBuf {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let dir = PathBuf::from(local_app_data).join("Airglow");
        return dir.join("Airglow.new.exe");
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        return parent.join("Airglow.new.exe");
    }
    std::env::temp_dir().join("Airglow.new.exe")
}

/// Parses a `SHA256SUMS.txt` file and returns the lowercase 64-hex-char SHA-256 hash matching
/// `target_filename` (case-insensitive), or a sole standalone 64-hex-char hash if only one exists.
pub fn parse_sha256_sums(contents: &str, target_filename: &str) -> Option<String> {
    let target_lower = target_filename.trim().to_ascii_lowercase();
    let mut single_hash: Option<String> = None;
    let mut hash_count = 0usize;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let Some(first) = tokens.next() else {
            continue;
        };
        let is_hex64 = first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit());
        if !is_hex64 {
            continue;
        }
        let digest = first.to_ascii_lowercase();
        hash_count += 1;
        single_hash = Some(digest.clone());

        if let Some(rest) = tokens.next() {
            let file_token = rest
                .trim_start_matches('*')
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(rest)
                .to_ascii_lowercase();
            if file_token == target_lower {
                return Some(digest);
            }
        }
    }

    if hash_count == 1 {
        single_hash
    } else {
        None
    }
}

fn ps_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(windows)]
fn verify_sha256_windows(path: &Path, expected_hex: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let path_str = path.to_string_lossy();
    let script = format!(
        "(Get-FileHash -LiteralPath {} -Algorithm SHA256).Hash",
        ps_single_quote(&path_str)
    );
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("run PowerShell Get-FileHash")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("SHA-256 verification failed: {}", err.trim());
    }
    let actual = String::from_utf8_lossy(&out.stdout)
        .trim()
        .to_ascii_lowercase();
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual != expected {
        bail!("SHA-256 mismatch (expected {expected}, got {actual})");
    }
    Ok(())
}

#[cfg(not(windows))]
fn verify_sha256_windows(_path: &Path, _expected_hex: &str) -> Result<()> {
    Ok(())
}

/// Downloads `exe_url` to `%LOCALAPPDATA%\Airglow\Airglow.new.exe`, verifies SHA-256 if
/// `sha256_url` is provided, and returns `UpdateStatus::ReadyToRestart` or `UpdateStatus::Error`.
pub async fn download_update_async(
    exe_url: String,
    sha256_url: Option<String>,
    latest: String,
) -> UpdateStatus {
    download_update_with_progress_async(exe_url, sha256_url, latest, |_| {}).await
}

/// Same as [`download_update_async`], invoking `on_progress(0..=100)` as chunks arrive.
pub async fn download_update_with_progress_async<F>(
    exe_url: String,
    sha256_url: Option<String>,
    latest: String,
    mut on_progress: F,
) -> UpdateStatus
where
    F: FnMut(u8),
{
    if !exe_url.starts_with("https://") {
        return UpdateStatus::Error("Update download URL must use HTTPS.".to_string());
    }
    if let Some(ref s_url) = sha256_url
        && !s_url.starts_with("https://")
    {
        return UpdateStatus::Error("Checksum URL must use HTTPS.".to_string());
    }

    let client = match build_http_client(120) {
        Ok(c) => c,
        Err(e) => return UpdateStatus::Error(format!("Update client error: {e}")),
    };

    on_progress(0);

    // Fetch SHA256SUMS.txt first if available.
    let expected_sha256 = if let Some(ref s_url) = sha256_url {
        match client.get(s_url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.text().await {
                Ok(text) => match parse_sha256_sums(&text, "Airglow.exe") {
                    Some(hash) => Some(hash),
                    None => {
                        return UpdateStatus::Error(
                            "SHA256SUMS.txt did not contain a valid Airglow.exe hash.".to_string(),
                        );
                    }
                },
                Err(e) => {
                    return UpdateStatus::Error(format!("Failed reading SHA256SUMS.txt: {e}"));
                }
            },
            Ok(resp) => {
                return UpdateStatus::Error(format!(
                    "Failed fetching SHA256SUMS.txt (HTTP {})",
                    resp.status().as_u16()
                ));
            }
            Err(e) => {
                return UpdateStatus::Error(format!("Failed downloading SHA256SUMS.txt: {e}"));
            }
        }
    } else {
        None
    };

    let mut resp = match client.get(&exe_url).send().await {
        Ok(r) => r,
        Err(e) => return UpdateStatus::Error(format!("Download failed: {e}")),
    };
    if !resp.status().is_success() {
        return UpdateStatus::Error(format!(
            "Download returned HTTP {}",
            resp.status().as_u16()
        ));
    }

    let total_bytes = resp.content_length();
    let dest = staged_update_path();
    if let Some(parent) = dest.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return UpdateStatus::Error(format!("Could not prepare update folder: {e}"));
    }
    let part_path = dest.with_extension("exe.part");
    let mut file = match fs::File::create(&part_path) {
        Ok(f) => f,
        Err(e) => return UpdateStatus::Error(format!("Could not create staging file: {e}")),
    };

    let mut downloaded: u64 = 0;
    let mut last_pct: u8 = 0;
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(e) = file.write_all(&chunk) {
                    let _ = fs::remove_file(&part_path);
                    return UpdateStatus::Error(format!("Failed writing update file: {e}"));
                }
                downloaded += chunk.len() as u64;
                if let Some(total) = total_bytes
                    && total > 0
                {
                    let pct = ((downloaded.saturating_mul(100)) / total).min(99) as u8;
                    if pct != last_pct {
                        last_pct = pct;
                        on_progress(pct);
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                let _ = fs::remove_file(&part_path);
                return UpdateStatus::Error(format!("Download interrupted: {e}"));
            }
        }
    }
    if let Err(e) = file.flush() {
        let _ = fs::remove_file(&part_path);
        return UpdateStatus::Error(format!("Failed flushing update file: {e}"));
    }
    drop(file);

    if downloaded < 1024 {
        let _ = fs::remove_file(&part_path);
        return UpdateStatus::Error("Downloaded file was unexpectedly small.".to_string());
    }

    if let Some(expected) = expected_sha256
        && let Err(e) = verify_sha256_windows(&part_path, &expected)
    {
        let _ = fs::remove_file(&part_path);
        return UpdateStatus::Error(format!("{e}"));
    }

    if dest.exists() {
        let _ = fs::remove_file(&dest);
    }
    if let Err(e) = fs::rename(&part_path, &dest) {
        let _ = fs::remove_file(&part_path);
        return UpdateStatus::Error(format!("Could not stage downloaded update: {e}"));
    }

    on_progress(100);
    UpdateStatus::ReadyToRestart {
        latest,
        staged_exe: dest.to_string_lossy().to_string(),
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const UPDATE_SPAWNER_CREATION_FLAGS: u32 = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;

#[cfg(windows)]
fn build_swap_script(pid: u32, staged: &Path, target: &Path, self_script: &Path) -> String {
    let staged_q = ps_single_quote(&staged.to_string_lossy());
    let target_q = ps_single_quote(&target.to_string_lossy());
    let self_q = ps_single_quote(&self_script.to_string_lossy());
    let backup_q = ps_single_quote(&format!("{}.bak", target.to_string_lossy()));

    format!(
        "$ErrorActionPreference = 'Stop'\r\n\
$appPid = {pid}\r\n\
$staged = {staged_q}\r\n\
$target = {target_q}\r\n\
$backup = {backup_q}\r\n\
$self = {self_q}\r\n\
$targetDir = Split-Path -Parent $target\r\n\
$errLog = Join-Path $targetDir 'update-last-error.log'\r\n\
for ($i = 0; $i -lt 50; $i++) {{\r\n\
    if (-not (Get-Process -Id $appPid -ErrorAction SilentlyContinue)) {{ break }}\r\n\
    Start-Sleep -Milliseconds 300\r\n\
}}\r\n\
Start-Sleep -Milliseconds 300\r\n\
$swapped = $false\r\n\
$lastErr = ''\r\n\
for ($attempt = 0; $attempt -lt 16; $attempt++) {{\r\n\
    try {{\r\n\
        if (Test-Path -LiteralPath $target) {{\r\n\
            if (Test-Path -LiteralPath $backup) {{ Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue }}\r\n\
            Move-Item -LiteralPath $target -Destination $backup -Force\r\n\
        }}\r\n\
        Move-Item -LiteralPath $staged -Destination $target -Force\r\n\
        $swapped = $true\r\n\
        break\r\n\
    }} catch {{\r\n\
        $lastErr = $_.Exception.Message\r\n\
        Start-Sleep -Milliseconds 350\r\n\
    }}\r\n\
}}\r\n\
if ($swapped) {{\r\n\
    Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue\r\n\
    Remove-Item -LiteralPath $errLog -Force -ErrorAction SilentlyContinue\r\n\
    Start-Process -FilePath $target -WorkingDirectory $targetDir\r\n\
}} else {{\r\n\
    if ($lastErr) {{ Set-Content -LiteralPath $errLog -Value $lastErr -Encoding UTF8 -ErrorAction SilentlyContinue }}\r\n\
    if (Test-Path -LiteralPath $backup) {{\r\n\
        Move-Item -LiteralPath $backup -Destination $target -Force -ErrorAction SilentlyContinue\r\n\
        Start-Process -FilePath $target -WorkingDirectory $targetDir\r\n\
    }}\r\n\
}}\r\n\
Remove-Item -LiteralPath $self -Force -ErrorAction SilentlyContinue\r\n"
    )
}

/// Spawns a detached PowerShell swap script that waits for the current Airglow process to exit,
/// replaces `current_exe()` with `staged_exe`, and relaunches Airglow cleanly.
pub fn apply_staged_update_and_restart(staged_exe: &str) -> Result<()> {
    let staged = PathBuf::from(staged_exe);
    if !staged.is_file() {
        bail!("Staged update binary not found at {}", staged.display());
    }
    let current = std::env::current_exe().context("resolve current executable path")?;
    let pid = std::process::id();

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let script_path =
            std::env::temp_dir().join(format!("airglow-apply-update-{pid}.ps1"));
        let script = build_swap_script(pid, &staged, &current, &script_path);

        fs::write(&script_path, script).context("write update swap script")?;
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
                &script_path.to_string_lossy(),
            ])
            .creation_flags(UPDATE_SPAWNER_CREATION_FLAGS)
            .spawn()
            .context("launch detached update swapper")?;
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = (staged, current, pid);
        bail!("In-place restart updates are only supported on Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_v_prefix_prereleases_and_build_metadata() {
        assert!(is_newer_version("2.0.0", "2.0.1"));
        assert!(is_newer_version("2.0.0", "v2.1.0"));
        assert!(is_newer_version("v2.0.0", "V3.0.0"));
        assert!(!is_newer_version("2.0.0", "2.0.0"));
        assert!(!is_newer_version("2.0.0", "v2.0.0"));
        assert!(!is_newer_version("2.1.0", "2.0.9"));

        // Pre-release vs stable
        assert!(is_newer_version("2.1.0-beta.1", "2.1.0"));
        assert!(!is_newer_version("2.1.0", "2.1.0-beta.1"));
        assert!(is_newer_version("2.0.0", "2.1.0-beta.1"));
        assert!(is_newer_version("2.1.0-alpha.1", "2.1.0-beta.1"));
        assert!(is_newer_version("2.1.0-beta.2", "2.1.0-beta.10"));
        assert!(is_newer_version("2.1.0-beta.1", "2.1.0-rc.1"));
        assert!(is_newer_version("2.1.0-beta", "2.1.0-beta.1"));

        // Build metadata ignored
        assert!(!is_newer_version("2.0.0+abc", "2.0.0+def"));
        assert!(is_newer_version("2.0.0+abc", "2.0.1+xyz"));

        // Invalid versions return false
        assert!(!is_newer_version("2.0.0", ""));
        assert!(!is_newer_version("2.0.0", "not-a-version"));
        assert!(!is_newer_version("invalid", "2.1.0"));
    }

    #[test]
    fn release_selection_respects_stable_vs_beta_channel_and_extracts_assets() {
        let releases = vec![
            GitHubRelease {
                tag_name: "v2.2.0-beta.1".to_string(),
                body: Some("Beta notes".to_string()),
                html_url: "https://github.com/Heavenlystorm/airglow/releases/tag/v2.2.0-beta.1"
                    .to_string(),
                draft: false,
                prerelease: true,
                assets: vec![
                    GitHubAsset {
                        name: "Airglow.exe".to_string(),
                        browser_download_url: "https://example.com/Airglow-beta.exe".to_string(),
                    },
                    GitHubAsset {
                        name: "SHA256SUMS.txt".to_string(),
                        browser_download_url: "https://example.com/SHA256SUMS.txt".to_string(),
                    },
                ],
            },
            GitHubRelease {
                tag_name: "v2.1.0".to_string(),
                body: Some("Stable release notes".to_string()),
                html_url: "https://github.com/Heavenlystorm/airglow/releases/tag/v2.1.0"
                    .to_string(),
                draft: false,
                prerelease: false,
                assets: vec![GitHubAsset {
                    name: "Airglow.exe".to_string(),
                    browser_download_url: "https://example.com/Airglow.exe".to_string(),
                }],
            },
            GitHubRelease {
                tag_name: "v9.9.9".to_string(),
                body: Some("Draft".to_string()),
                html_url: String::new(),
                draft: true,
                prerelease: false,
                assets: vec![],
            },
        ];

        // Stable channel skips prereleases and drafts
        let stable =
            select_release_for_channel(&releases, "2.0.0", UpdateChannel::Stable, "10:00".into());
        match stable {
            UpdateStatus::Available {
                current,
                latest,
                exe_url,
                sha256_url,
                is_prerelease,
                ..
            } => {
                assert_eq!(current, "2.0.0");
                assert_eq!(latest, "2.1.0");
                assert_eq!(exe_url.as_deref(), Some("https://example.com/Airglow.exe"));
                assert_eq!(sha256_url, None);
                assert!(!is_prerelease);
            }
            other => panic!("expected Available, got {other:?}"),
        }

        // Beta channel picks 2.2.0-beta.1
        let beta =
            select_release_for_channel(&releases, "2.0.0", UpdateChannel::Beta, "10:00".into());
        match beta {
            UpdateStatus::Available {
                latest,
                exe_url,
                sha256_url,
                is_prerelease,
                ..
            } => {
                assert_eq!(latest, "2.2.0-beta.1");
                assert_eq!(
                    exe_url.as_deref(),
                    Some("https://example.com/Airglow-beta.exe")
                );
                assert_eq!(
                    sha256_url.as_deref(),
                    Some("https://example.com/SHA256SUMS.txt")
                );
                assert!(is_prerelease);
            }
            other => panic!("expected Available, got {other:?}"),
        }

        // Already on 2.1.0 on Stable -> UpToDate
        let up_to_date =
            select_release_for_channel(&releases, "2.1.0", UpdateChannel::Stable, "10:05".into());
        assert_eq!(
            up_to_date,
            UpdateStatus::UpToDate {
                current: "2.1.0".to_string(),
                checked_at: "10:05".to_string(),
            }
        );
    }

    #[test]
    fn parses_sha256_sums_for_airglow_exe() {
        let hash_a = "a".repeat(64);
        let hash_b = "b".repeat(64);
        let manifest = format!(
            "# checksums\n{hash_a}  other.zip\n{hash_b} *dist/Airglow.exe\n"
        );
        assert_eq!(
            parse_sha256_sums(&manifest, "Airglow.exe"),
            Some(hash_b.clone())
        );
        assert_eq!(parse_sha256_sums(&hash_a, "Airglow.exe"), Some(hash_a));
        assert_eq!(parse_sha256_sums("invalid line", "Airglow.exe"), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_updater_spawn_flags_are_valid_and_swap_script_preserves_backup() {
        use std::os::windows::process::CommandExt;

        // Mutually exclusive CREATE_NO_WINDOW | DETACHED_PROCESS causes ERROR_INVALID_PARAMETER (87).
        assert_eq!(
            UPDATE_SPAWNER_CREATION_FLAGS & 0x0000_0008,
            0,
            "DETACHED_PROCESS must never be combined with CREATE_NO_WINDOW"
        );

        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "exit 0"])
            .creation_flags(UPDATE_SPAWNER_CREATION_FLAGS)
            .status()
            .expect("powershell.exe spawn with UPDATE_SPAWNER_CREATION_FLAGS must succeed");
        assert!(status.success());

        let script = build_swap_script(
            4242,
            Path::new(r"C:\Users\demo\AppData\Local\Airglow\Airglow.new.exe"),
            Path::new(r"C:\Users\demo\AppData\Local\Airglow\Airglow.exe"),
            Path::new(r"C:\Temp\airglow-apply-update-4242.ps1"),
        );
        // Must only delete old $backup inside `if (Test-Path -LiteralPath $target)` so retries stay safe
        let target_if = script.find("if (Test-Path -LiteralPath $target)").unwrap();
        let backup_rm = script.find("Remove-Item -LiteralPath $backup").unwrap();
        assert!(backup_rm > target_if);
        assert!(script.contains("-WorkingDirectory $targetDir"));
        assert!(script.contains("update-last-error.log"));
    }
}
