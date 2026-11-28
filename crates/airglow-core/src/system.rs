//! Windows system integration: conflicting RGB process detection & closing (`conflicts.py`),
//! idle time & workstation lock detection (`system.py`), Start-with-Windows autostart toggle
//! (`autostart.py`), and privacy-scrubbed diagnostics report export (`diagnostics.py`).

use crate::config::{AppConfig, default_data_dir};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

// ------------------------------------------------------------------ Conflicting RGB software

struct KnownConflict {
    exe: &'static str,
    name: &'static str,
    conflicts_with: &'static str,
    is_service: bool,
    path_must_have: Option<&'static str>,
    only_for_vendor: Option<&'static str>,
}

const KNOWN_CONFLICTS: &[KnownConflict] = &[
    KnownConflict {
        exe: "signalrgb.exe",
        name: "SignalRGB",
        conflicts_with: "lighting on every device",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "signalrgblauncher.exe",
        name: "SignalRGB",
        conflicts_with: "lighting on every device",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "icue.exe",
        name: "Corsair iCUE",
        conflicts_with: "Corsair lighting and fans",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "nzxt cam.exe",
        name: "NZXT CAM",
        conflicts_with: "the NZXT hub's lighting and fans",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "lightingservice.exe",
        name: "ASUS Aura service",
        conflicts_with: "motherboard lighting",
        is_service: true,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "armourycrate.exe",
        name: "ASUS Armoury Crate",
        conflicts_with: "motherboard lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "armourycrate.service.exe",
        name: "ASUS Armoury Crate",
        conflicts_with: "motherboard lighting",
        is_service: true,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "rgbfusion.exe",
        name: "Gigabyte RGB Fusion",
        conflicts_with: "Gigabyte graphics card and board lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "gcc.exe",
        name: "Gigabyte Control Center",
        conflicts_with: "Gigabyte graphics card and board lighting",
        is_service: false,
        path_must_have: Some("gigabyte"),
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "mysticlight.exe",
        name: "MSI Mystic Light",
        conflicts_with: "MSI board lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "msi.centralserver.exe",
        name: "MSI Center",
        conflicts_with: "MSI board lighting",
        is_service: true,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "lconnect3.exe",
        name: "Lian Li L-Connect",
        conflicts_with: "Lian Li fans and lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "l-connect 3.exe",
        name: "Lian Li L-Connect",
        conflicts_with: "Lian Li fans and lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "ledkeeper2.exe",
        name: "MSI Mystic Light",
        conflicts_with: "MSI board and graphics card lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "polychrome rgb.exe",
        name: "ASRock Polychrome",
        conflicts_with: "ASRock board lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: None,
    },
    KnownConflict {
        exe: "lghub.exe",
        name: "Logitech G HUB",
        conflicts_with: "Logitech keyboard and mouse lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: Some("logitech"),
    },
    KnownConflict {
        exe: "lghub_agent.exe",
        name: "Logitech G HUB",
        conflicts_with: "Logitech keyboard and mouse lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: Some("logitech"),
    },
    KnownConflict {
        exe: "steelseriesgg.exe",
        name: "SteelSeries GG",
        conflicts_with: "SteelSeries keyboard and mouse lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: Some("steelseries"),
    },
    KnownConflict {
        exe: "steelseriesengine.exe",
        name: "SteelSeries GG",
        conflicts_with: "SteelSeries keyboard and mouse lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: Some("steelseries"),
    },
    KnownConflict {
        exe: "steelseriesengine3.exe",
        name: "SteelSeries Engine",
        conflicts_with: "SteelSeries keyboard and mouse lighting",
        is_service: false,
        path_must_have: None,
        only_for_vendor: Some("steelseries"),
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictingApp {
    pub name: String,
    pub conflicts_with: String,
    pub exes: Vec<String>,
    pub is_service: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CloseConflictResult {
    pub closed: Vec<String>,
    pub failed: Vec<String>,
    pub services: Vec<String>,
}

fn path_matches(entry: &KnownConflict, exe_path: Option<&Path>) -> bool {
    match entry.path_must_have {
        None => true,
        Some(needle) => exe_path
            .and_then(|p| p.to_str())
            .is_some_and(|s| s.to_ascii_lowercase().contains(needle)),
    }
}

/// Detects running RGB software that may conflict with Airglow's device control.
///
/// Pass `lit_vendors = Some(&["logitech", ...])` to suppress peripheral apps whose brand
/// is not currently lit by Airglow; pass `None` to report every running conflict (diagnostics).
pub fn detect_conflicting_apps(lit_vendors: Option<&[&str]>) -> Vec<ConflictingApp> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    let running: Vec<(String, Option<PathBuf>)> = sys
        .processes()
        .values()
        .map(|p| {
            (
                p.name().to_string_lossy().to_ascii_lowercase(),
                p.exe().map(Path::to_path_buf),
            )
        })
        .collect();
    detect_conflicting_apps_from_list(&running, lit_vendors)
}

/// Pure helper for conflict detection over a `(lowercase_exe_name, optional_full_path)` list.
pub fn detect_conflicting_apps_from_list(
    running: &[(String, Option<PathBuf>)],
    lit_vendors: Option<&[&str]>,
) -> Vec<ConflictingApp> {
    let lit_lower: Option<BTreeSet<String>> = lit_vendors
        .map(|vs| vs.iter().map(|v| v.to_ascii_lowercase()).collect());

    let mut found: BTreeMap<&'static str, ConflictingApp> = BTreeMap::new();
    for entry in KNOWN_CONFLICTS {
        if let (Some(lit), Some(vendor)) = (&lit_lower, entry.only_for_vendor)
            && !lit.contains(vendor)
        {
            continue;
        }
        let matched = running
            .iter()
            .any(|(exe, path)| exe == entry.exe && path_matches(entry, path.as_deref()));
        if !matched {
            continue;
        }
        let app = found.entry(entry.name).or_insert_with(|| ConflictingApp {
            name: entry.name.to_string(),
            conflicts_with: entry.conflicts_with.to_string(),
            exes: Vec::new(),
            is_service: false,
        });
        if !app.exes.iter().any(|e| e == entry.exe) {
            app.exes.push(entry.exe.to_string());
        }
        app.is_service |= entry.is_service;
    }
    found.into_values().collect()
}

/// Terminates running processes belonging to a known conflicting RGB product or executable name
/// (user-initiated). Windows services are left untouched and listed in `services`.
pub fn close_conflicting_process(name: &str) -> Result<CloseConflictResult> {
    let needle = name.trim().to_ascii_lowercase();
    let targets: Vec<&KnownConflict> = KNOWN_CONFLICTS
        .iter()
        .filter(|c| c.name.to_ascii_lowercase() == needle || c.exe == needle)
        .collect();
    if targets.is_empty() {
        bail!("'{name}' isn't a known RGB app");
    }

    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );

    let mut closed = Vec::new();
    let mut failed = Vec::new();
    let mut services = BTreeSet::new();

    for proc_info in sys.processes().values() {
        let exe_name = proc_info.name().to_string_lossy().to_string();
        let exe_lower = exe_name.to_ascii_lowercase();
        let Some(entry) = targets.iter().find(|c| c.exe == exe_lower) else {
            continue;
        };
        if entry.is_service {
            services.insert(exe_name);
            continue;
        }
        if !path_matches(entry, proc_info.exe()) {
            continue;
        }
        if proc_info.kill() {
            closed.push(exe_name);
        } else {
            failed.push(exe_name);
        }
    }

    Ok(CloseConflictResult {
        closed,
        failed,
        services: services.into_iter().collect(),
    })
}

// ------------------------------------------------------------------ Windows idle & lock detection

/// Seconds elapsed since the last keyboard or mouse input (`GetLastInputInfo` Win32 FFI).
#[cfg(windows)]
pub fn idle_seconds() -> f64 {
    #[repr(C)]
    struct LastInputInfo {
        cb_size: u32,
        dw_time: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetLastInputInfo(plii: *mut LastInputInfo) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount() -> u32;
    }

    let mut info = LastInputInfo {
        cb_size: std::mem::size_of::<LastInputInfo>() as u32,
        dw_time: 0,
    };
    let ok = unsafe { GetLastInputInfo(&mut info) };
    if ok == 0 {
        return 0.0;
    }
    let now = unsafe { GetTickCount() };
    now.wrapping_sub(info.dw_time) as f64 / 1000.0
}

#[cfg(not(windows))]
pub fn idle_seconds() -> f64 {
    0.0
}

/// Returns `true` when the Windows lock screen or another secure desktop holds input
/// (`OpenInputDesktop` Win32 FFI).
#[cfg(windows)]
pub fn is_workstation_locked() -> bool {
    const DESKTOP_SWITCHDESKTOP: u32 = 0x0100;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenInputDesktop(
            dw_flags: u32,
            f_inherit: i32,
            dw_desired_access: u32,
        ) -> *mut core::ffi::c_void;
        fn CloseDesktop(h_desktop: *mut core::ffi::c_void) -> i32;
    }

    let handle = unsafe { OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP) };
    if handle.is_null() {
        true
    } else {
        unsafe {
            CloseDesktop(handle);
        }
        false
    }
}

#[cfg(not(windows))]
pub fn is_workstation_locked() -> bool {
    false
}

/// Lowercase executable filename of the currently focused foreground window, if readable.
#[cfg(windows)]
pub fn foreground_exe() -> Option<String> {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut core::ffi::c_void;
        fn GetWindowThreadProcessId(
            hwnd: *mut core::ffi::c_void,
            lpdw_process_id: *mut u32,
        ) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(
            dw_desired_access: u32,
            b_inherit_handle: i32,
            dw_process_id: u32,
        ) -> *mut core::ffi::c_void;
        fn QueryFullProcessImageNameW(
            h_process: *mut core::ffi::c_void,
            dw_flags: u32,
            lp_exe_name: *mut u16,
            lpdw_size: *mut u32,
        ) -> i32;
        fn CloseHandle(h_object: *mut core::ffi::c_void) -> i32;
    }

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
    unsafe {
        CloseHandle(handle);
    }
    if ok == 0 || size == 0 {
        return None;
    }
    let full = String::from_utf16_lossy(&buf[..size as usize]);
    Path::new(&full)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
}

#[cfg(not(windows))]
pub fn foreground_exe() -> Option<String> {
    None
}

// ------------------------------------------------------------------ Start with Windows autostart

pub const AUTOSTART_REG_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const AUTOSTART_VALUE_NAME: &str = "Airglow";

/// Builds the startup command line for `exe_path`, rewriting Scoop versioned paths
/// (`...\scoop\apps\airglow\<ver>\...`) to `current` like Python Airglow's `autostart.py`.
pub fn autostart_command_for_exe(exe_path: &Path) -> String {
    let stable = stabilize_scoop_path(exe_path);
    format!("\"{}\" --background", stable.display())
}

fn stabilize_scoop_path(exe_path: &Path) -> PathBuf {
    let parts: Vec<std::ffi::OsString> = exe_path
        .components()
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    let lower: Vec<String> = parts
        .iter()
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .collect();
    for i in 0..lower.len().saturating_sub(3) {
        if lower[i] == "scoop" && lower[i + 1] == "apps" && lower[i + 2] == "airglow" {
            let mut out = PathBuf::new();
            for (idx, part) in parts.iter().enumerate() {
                if idx == i + 3 {
                    out.push("current");
                } else {
                    out.push(part);
                }
            }
            return out;
        }
    }
    exe_path.to_path_buf()
}

#[cfg(windows)]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
mod win_reg {
    use super::{AUTOSTART_REG_SUBKEY, to_wide_null};
    use anyhow::{Result, bail};

    type HKey = isize;
    const HKEY_CURRENT_USER: HKey = 0x80000001u32 as i32 as isize;
    const KEY_READ: u32 = 0x20019;
    const KEY_SET_VALUE: u32 = 0x0002;
    const REG_SZ: u32 = 1;
    const ERROR_SUCCESS: i32 = 0;
    const ERROR_FILE_NOT_FOUND: i32 = 2;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegOpenKeyExW(
            h_key: HKey,
            lp_sub_key: *const u16,
            ul_options: u32,
            sam_desired: u32,
            phk_result: *mut HKey,
        ) -> i32;
        fn RegQueryValueExW(
            h_key: HKey,
            lp_value_name: *const u16,
            lp_reserved: *mut u32,
            lp_type: *mut u32,
            lp_data: *mut u8,
            lpcb_data: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            h_key: HKey,
            lp_value_name: *const u16,
            reserved: u32,
            dw_type: u32,
            lp_data: *const u8,
            cb_data: u32,
        ) -> i32;
        fn RegDeleteValueW(h_key: HKey, lp_value_name: *const u16) -> i32;
        fn RegCloseKey(h_key: HKey) -> i32;
    }

    pub fn read_run_value(value_name: &str) -> Option<String> {
        let subkey = to_wide_null(AUTOSTART_REG_SUBKEY);
        let name = to_wide_null(value_name);
        let mut hkey: HKey = 0;
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let mut reg_type: u32 = 0;
        let mut size_bytes: u32 = 0;
        let rc_len = unsafe {
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut reg_type,
                std::ptr::null_mut(),
                &mut size_bytes,
            )
        };
        if rc_len != ERROR_SUCCESS || size_bytes < 2 {
            unsafe { RegCloseKey(hkey) };
            return None;
        }
        let mut buf = vec![0u16; (size_bytes as usize).div_ceil(2)];
        let rc_val = unsafe {
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut reg_type,
                buf.as_mut_ptr() as *mut u8,
                &mut size_bytes,
            )
        };
        unsafe { RegCloseKey(hkey) };
        if rc_val != ERROR_SUCCESS {
            return None;
        }
        while buf.last() == Some(&0) {
            buf.pop();
        }
        Some(String::from_utf16_lossy(&buf))
    }

    pub fn set_run_value(value_name: &str, command: &str) -> Result<()> {
        let subkey = to_wide_null(AUTOSTART_REG_SUBKEY);
        let name = to_wide_null(value_name);
        let wide_cmd = to_wide_null(command);
        let mut hkey: HKey = 0;
        let rc = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            )
        };
        if rc != ERROR_SUCCESS {
            bail!("Could not open HKCU Run registry key (Win32 error {rc})");
        }
        let rc_set = unsafe {
            RegSetValueExW(
                hkey,
                name.as_ptr(),
                0,
                REG_SZ,
                wide_cmd.as_ptr() as *const u8,
                (wide_cmd.len() * 2) as u32,
            )
        };
        unsafe { RegCloseKey(hkey) };
        if rc_set != ERROR_SUCCESS {
            bail!("Could not write autostart registry value (Win32 error {rc_set})");
        }
        Ok(())
    }

    pub fn delete_run_value(value_name: &str) -> Result<()> {
        let subkey = to_wide_null(AUTOSTART_REG_SUBKEY);
        let name = to_wide_null(value_name);
        let mut hkey: HKey = 0;
        let rc = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            )
        };
        if rc != ERROR_SUCCESS {
            bail!("Could not open HKCU Run registry key (Win32 error {rc})");
        }
        let rc_del = unsafe { RegDeleteValueW(hkey, name.as_ptr()) };
        unsafe { RegCloseKey(hkey) };
        if rc_del != ERROR_SUCCESS && rc_del != ERROR_FILE_NOT_FOUND {
            bail!("Could not delete autostart registry value (Win32 error {rc_del})");
        }
        Ok(())
    }
}

/// Returns the current `HKCU\...\Run\Airglow` command line, if configured.
pub fn current_autostart_command() -> Option<String> {
    #[cfg(windows)]
    {
        win_reg::read_run_value(AUTOSTART_VALUE_NAME)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Returns `true` when Airglow is registered to start with Windows.
pub fn is_autostart_enabled() -> bool {
    current_autostart_command().is_some()
}

/// Enables or disables Start-with-Windows for the current executable (`Airglow.exe`).
pub fn set_autostart(enabled: bool) -> Result<()> {
    let exe = std::env::current_exe().context("resolve current executable path")?;
    set_autostart_for_exe(enabled, &exe)
}

/// Enables or disables Start-with-Windows for an explicit executable path.
pub fn set_autostart_for_exe(enabled: bool, exe_path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        if enabled {
            let cmd = autostart_command_for_exe(exe_path);
            win_reg::set_run_value(AUTOSTART_VALUE_NAME, &cmd)
        } else {
            win_reg::delete_run_value(AUTOSTART_VALUE_NAME)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (enabled, exe_path);
        bail!("Start with Windows is only supported on Windows")
    }
}

// ------------------------------------------------------------------ Diagnostics report export

fn collect_env_secrets() -> Vec<String> {
    let mut set = BTreeSet::new();
    for key in [
        "USERNAME",
        "COMPUTERNAME",
        "USERDOMAIN",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "ONEDRIVE",
    ] {
        if let Ok(val) = std::env::var(key)
            && val.len() > 2
        {
            set.insert(val.clone());
            set.insert(val.replace('\\', "/"));
            set.insert(val.replace('\\', "\\\\"));
        }
    }
    let mut list: Vec<String> = set.into_iter().filter(|s| s.len() > 2).collect();
    // Longest first so full paths are redacted before bare usernames.
    list.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    list
}

fn replace_ascii_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let hay_lower = haystack.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    if hay_lower.len() != haystack.len() {
        // Fallback for non-ASCII case changes: exact match
        return haystack.replace(needle, replacement);
    }
    let mut out = String::with_capacity(haystack.len());
    let mut pos = 0;
    while let Some(found) = hay_lower[pos..].find(&needle_lower) {
        let abs = pos + found;
        out.push_str(&haystack[pos..abs]);
        out.push_str(replacement);
        pos = abs + needle.len();
    }
    out.push_str(&haystack[pos..]);
    out
}

/// Deterministic 64-bit FNV-1a short hex digest (8 hex chars) for anonymizing OpenRGB serials
/// while keeping device IDs correlated across sections of a diagnostics report.
fn short_serial_hash(serial: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in serial.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("s{:08x}", (hash as u32) ^ ((hash >> 32) as u32))
}

/// Replaces the serial segment of `openrgb:<slug>:<serial>` with a stable short hash.
fn scrub_openrgb_serials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find("openrgb:") {
        out.push_str(&rest[..idx]);
        let after_prefix = &rest[idx + "openrgb:".len()..];
        let slug_len = after_prefix
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
            .count();
        if slug_len > 0 && after_prefix.as_bytes().get(slug_len) == Some(&b':') {
            let after_slug = &after_prefix[slug_len + 1..];
            let serial_len = after_slug
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
                .count();
            if serial_len > 0 {
                let slug = &after_prefix[..slug_len];
                let serial = &after_slug[..serial_len];
                out.push_str("openrgb:");
                out.push_str(slug);
                out.push(':');
                out.push_str(&short_serial_hash(serial));
                rest = &after_slug[serial_len..];
                continue;
            }
        }
        out.push_str("openrgb:");
        rest = after_prefix;
    }
    out.push_str(rest);
    out
}

/// Scrubs user/PC environment strings and OpenRGB serial numbers from diagnostics text.
pub fn scrub_diagnostics_text(text: &str) -> String {
    let secrets = collect_env_secrets();
    scrub_diagnostics_with_secrets(text, &secrets)
}

/// Scrubs explicit secret strings (longest-first recommended) and OpenRGB serial numbers.
pub fn scrub_diagnostics_with_secrets(text: &str, secrets: &[String]) -> String {
    let mut ordered: Vec<&String> = secrets.iter().filter(|s| s.len() > 2).collect();
    ordered.sort_by(|a, b| b.len().cmp(&a.len()));
    let mut cleaned = text.to_string();
    for secret in ordered {
        cleaned = replace_ascii_case_insensitive(&cleaned, secret, "<redacted>");
    }
    scrub_openrgb_serials(&cleaned)
}

#[cfg(windows)]
fn local_timestamp_compact() -> (String, String) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTimeWin {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(lp_system_time: *mut SystemTimeWin);
    }
    let mut st = SystemTimeWin::default();
    unsafe { GetLocalTime(&mut st) };
    let compact = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        st.year, st.month, st.day, st.hour, st.minute, st.second
    );
    let iso = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        st.year, st.month, st.day, st.hour, st.minute, st.second
    );
    (compact, iso)
}

#[cfg(not(windows))]
fn local_timestamp_compact() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (format!("{secs}"), format!("{secs}"))
}

/// Writes a privacy-scrubbed timestamped JSON diagnostics report to `dir` and returns its path.
pub fn export_diagnostics_report(dir: &Path, summary_json: &Value) -> Result<PathBuf> {
    fs::create_dir_all(dir).context("create diagnostics directory")?;
    let (stamp, created_iso) = local_timestamp_compact();
    let conflicts = detect_conflicting_apps(None);
    let report = json!({
        "format": "airglow-diagnostics",
        "version": 1,
        "created": created_iso,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "autostart_enabled": is_autostart_enabled(),
        "conflicts": conflicts,
        "summary": summary_json,
    });
    let pretty = serde_json::to_string_pretty(&report)?;
    let scrubbed = scrub_diagnostics_text(&pretty);
    let path = dir.join(format!("airglow-diagnostics-{stamp}.json"));
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, scrubbed).context("write temp diagnostics report")?;
    fs::rename(&tmp, &path).context("save diagnostics report")?;
    Ok(path)
}

// ------------------------------------------------------------------ Native Windows 11 DWM Titlebar

/// Converts an `[r, g, b]` triple into a Win32 `COLORREF` (`0x00BBGGRR`).
#[inline]
pub fn rgb_to_colorref(rgb: [u8; 3]) -> u32 {
    (rgb[0] as u32) | ((rgb[1] as u32) << 8) | ((rgb[2] as u32) << 16)
}

/// Applies native Windows 11 DWM titlebar tinting (dark mode, caption fill, caption text, and
/// window border color) to all visible top-level windows belonging to the current process with
/// width > 200 px (skipping tiny hidden tray message windows).
///
/// Returns `true` if at least one main application window was found and updated.
#[cfg(windows)]
pub fn apply_window_titlebar_theme(
    caption_rgb: [u8; 3],
    text_rgb: [u8; 3],
    border_rgb: [u8; 3],
    dark_mode: bool,
) -> bool {
    use std::ffi::c_void;

    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const DWMWA_TEXT_COLOR: u32 = 36;

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    struct EnumState {
        pid: u32,
        dark: u32,
        caption: u32,
        text: u32,
        border: u32,
        applied: bool,
    }

    type EnumWindowsProc = unsafe extern "system" fn(hwnd: *mut c_void, lparam: isize) -> i32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcessId() -> u32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(lp_enum_func: EnumWindowsProc, lparam: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, lpdw_process_id: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn GetWindowRect(hwnd: *mut c_void, lp_rect: *mut Rect) -> i32;
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            dw_attribute: u32,
            pv_attribute: *const c_void,
            cb_attribute: u32,
        ) -> i32;
    }

    unsafe extern "system" fn enum_cb(hwnd: *mut c_void, lparam: isize) -> i32 {
        let state = unsafe { &mut *(lparam as *mut EnumState) };
        let mut win_pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut win_pid) };
        if win_pid != state.pid {
            return 1;
        }
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let mut rect = Rect::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 || (rect.right - rect.left) <= 200 {
            return 1;
        }
        let sz = std::mem::size_of::<u32>() as u32;
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&state.dark as *const u32).cast::<c_void>(),
                sz,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                (&state.border as *const u32).cast::<c_void>(),
                sz,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                (&state.caption as *const u32).cast::<c_void>(),
                sz,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR,
                (&state.text as *const u32).cast::<c_void>(),
                sz,
            );
        }
        state.applied = true;
        1
    }

    let mut state = EnumState {
        pid: unsafe { GetCurrentProcessId() },
        dark: u32::from(dark_mode),
        caption: rgb_to_colorref(caption_rgb),
        text: rgb_to_colorref(text_rgb),
        border: rgb_to_colorref(border_rgb),
        applied: false,
    };
    unsafe {
        let _ = EnumWindows(enum_cb, (&mut state as *mut EnumState) as isize);
    }
    state.applied
}

#[cfg(not(windows))]
pub fn apply_window_titlebar_theme(
    _caption_rgb: [u8; 3],
    _text_rgb: [u8; 3],
    _border_rgb: [u8; 3],
    _dark_mode: bool,
) -> bool {
    false
}

// ------------------------------------------------------------------ Rig Config Backup & Shell Open

fn preferred_backup_dir() -> PathBuf {
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let desktop = PathBuf::from(profile).join("Desktop");
        if desktop.is_dir() {
            return desktop;
        }
    }
    default_data_dir()
}

/// Exports the full `AppConfig` as a timestamped `airglow-rig-backup-*.json` bundle to the
/// user's Desktop (falling back to Airglow's config folder) and returns the saved file path.
pub fn export_rig_bundle(cfg: &AppConfig) -> Result<PathBuf> {
    let dir = preferred_backup_dir();
    export_rig_bundle_to_dir(cfg, &dir)
}

/// Exports the full `AppConfig` as a timestamped `airglow-rig-backup-*.json` bundle into `dir`.
pub fn export_rig_bundle_to_dir(cfg: &AppConfig, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir).context("create rig backup directory")?;
    let (stamp, created_iso) = local_timestamp_compact();
    let bundle = json!({
        "format": "airglow-rig-bundle",
        "version": 1,
        "app_version": env!("CARGO_PKG_VERSION"),
        "created": created_iso,
        "config": cfg,
    });
    let pretty = serde_json::to_string_pretty(&bundle).context("serialize rig backup")?;
    let path = dir.join(format!("airglow-rig-backup-{stamp}.json"));
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, pretty).context("write temp rig backup")?;
    fs::rename(&tmp, &path).context("save rig backup")?;
    Ok(path)
}

/// Imports an `AppConfig` from either an `airglow-rig-bundle` backup JSON or a raw `config.json`,
/// normalizing all fields before returning.
pub fn import_rig_bundle(path: &Path) -> Result<AppConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read rig backup {}", path.display()))?;
    let val: Value = serde_json::from_str(&raw).context("parse rig backup JSON")?;
    let mut cfg: AppConfig = if let Some(inner) = val.get("config") {
        serde_json::from_value(inner.clone()).context("decode rig backup config payload")?
    } else {
        serde_json::from_value(val).context("decode raw Airglow config JSON")?
    };
    cfg.normalize();
    Ok(cfg)
}

#[cfg(windows)]
fn shell_open_path_or_url(target: &str) -> Result<()> {
    const SW_SHOWNORMAL: i32 = 1;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            lp_operation: *const u16,
            lp_file: *const u16,
            lp_parameters: *const u16,
            lp_directory: *const u16,
            n_show_cmd: i32,
        ) -> isize;
    }
    let op = to_wide_null("open");
    let file = to_wide_null(target);
    let rc = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if rc <= 32 {
        bail!("ShellExecuteW failed with code {rc}");
    }
    Ok(())
}

/// Opens an `http://` or `https://` URL in the user's default web browser.
pub fn open_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        bail!("Only http:// and https:// URLs can be opened");
    }
    #[cfg(windows)]
    {
        shell_open_path_or_url(trimmed)
    }
    #[cfg(not(windows))]
    {
        let _ = trimmed;
        bail!("open_url is only supported on Windows")
    }
}

/// Opens Airglow's `%APPDATA%\Airglow-rs` config folder in Windows Explorer.
pub fn open_config_folder() -> Result<()> {
    let dir = default_data_dir();
    fs::create_dir_all(&dir).context("ensure config folder exists")?;
    #[cfg(windows)]
    {
        shell_open_path_or_url(&dir.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        bail!("open_config_folder is only supported on Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_conflicts_with_gcc_path_guard_and_vendor_filter() {
        let running = vec![
            ("signalrgb.exe".to_string(), Some(PathBuf::from(r"C:\SignalRGB\SignalRGB.exe"))),
            ("gcc.exe".to_string(), Some(PathBuf::from(r"C:\msys64\mingw64\bin\gcc.exe"))),
            ("lightingservice.exe".to_string(), None),
            ("lghub.exe".to_string(), None),
        ];

        // With no Logitech device lit, G HUB is not flagged; GNU gcc.exe is never flagged.
        let filtered = detect_conflicting_apps_from_list(&running, Some(&["nzxt", "asus"]));
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|c| c.name == "SignalRGB" && !c.is_service));
        assert!(filtered.iter().any(|c| c.name == "ASUS Aura service" && c.is_service));
        assert!(!filtered.iter().any(|c| c.name == "Gigabyte Control Center"));
        assert!(!filtered.iter().any(|c| c.name == "Logitech G HUB"));

        // Diagnostics (lit_vendors = None) includes G HUB; Gigabyte gcc.exe path matches.
        let mut with_giga = running;
        with_giga.push((
            "gcc.exe".to_string(),
            Some(PathBuf::from(r"C:\Program Files\GIGABYTE\Control Center\GCC.exe")),
        ));
        let all = detect_conflicting_apps_from_list(&with_giga, None);
        assert_eq!(all.len(), 4);
        assert!(all.iter().any(|c| c.name == "Gigabyte Control Center"));
        assert!(all.iter().any(|c| c.name == "Logitech G HUB"));
    }

    #[test]
    fn refuses_unknown_app_in_close_conflicting_process() {
        assert!(close_conflicting_process("notepad.exe").is_err());
    }

    #[test]
    fn stabilizes_scoop_autostart_paths() {
        let cmd = autostart_command_for_exe(Path::new(
            r"C:\Users\alice\scoop\apps\airglow\2.0.0\Airglow.exe",
        ));
        assert!(cmd.contains(r"scoop\apps\airglow\current\Airglow.exe"));
        assert!(cmd.ends_with(" --background"));
    }

    #[test]
    fn scrubs_user_pc_names_and_openrgb_serials() {
        let secrets = vec![
            r"C:\Users\Alice".to_string(),
            r"C:/Users/Alice".to_string(),
            "Alice".to_string(),
            "DESKTOP-9999".to_string(),
        ];
        let raw = r#"{"path":"C:\Users\Alice\AppData","host":"desktop-9999","zone":"openrgb:keychron-k2-he:1d0068000b51343436363936/z0"}"#;
        let scrubbed = scrub_diagnostics_with_secrets(raw, &secrets);
        assert!(!scrubbed.contains("Alice"));
        assert!(!scrubbed.to_ascii_lowercase().contains("desktop-9999"));
        assert!(!scrubbed.contains("1d0068000b51343436363936"));
        assert!(scrubbed.contains("openrgb:keychron-k2-he:s"));
        assert!(scrubbed.contains("/z0"));
    }

    #[test]
    fn idle_and_lock_queries_execute_safely() {
        let idle = idle_seconds();
        assert!(idle.is_finite() && idle >= 0.0);
        let _locked = is_workstation_locked();
    }

    #[test]
    fn exports_scrubbed_diagnostics_report_file() {
        let dir = std::env::temp_dir().join(format!(
            "airglow_diag_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let summary = json!({
            "zone": "openrgb:keychron-k2-he:1d0068000b51343436363936/z0",
            "user_path": std::env::var("USERPROFILE").unwrap_or_default()
        });
        let path = export_diagnostics_report(&dir, &summary).expect("export report");
        assert!(path.is_file());
        let content = fs::read_to_string(&path).expect("read report");
        assert!(!content.contains("1d0068000b51343436363936"));
        if let Ok(user) = std::env::var("USERNAME")
            && user.len() > 2
        {
            assert!(!content.to_ascii_lowercase().contains(&user.to_ascii_lowercase()));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rgb_to_colorref_packs_bgr_little_endian() {
        assert_eq!(rgb_to_colorref([0x22, 0x1d, 0x30]), 0x0030_1d22);
        assert_eq!(rgb_to_colorref([0xff, 0x00, 0x00]), 0x0000_00ff);
        assert_eq!(rgb_to_colorref([0x00, 0xff, 0x00]), 0x0000_ff00);
        assert_eq!(rgb_to_colorref([0x00, 0x00, 0xff]), 0x00ff_0000);
        assert_eq!(rgb_to_colorref([0xf4, 0x8f, 0xb1]), 0x00b1_8ff4);
    }

    #[test]
    fn rig_bundle_export_and_import_roundtrip_and_raw_fallback() {
        let dir = std::env::temp_dir().join(format!(
            "airglow_bundle_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);

        let mut cfg = AppConfig::default();
        cfg.fullscreen_fill_width = true;
        cfg.update_channel = crate::config::UpdateChannel::Beta;
        let ws_id = cfg.create_workspace("Studio Backup", Some("quiet-studio"));
        cfg.add_overview_tile(crate::config::OverviewTileKind::NetUp);

        let exported = export_rig_bundle_to_dir(&cfg, &dir).expect("export rig bundle");
        assert!(exported.is_file());
        assert!(
            exported
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("airglow-rig-backup-")
        );

        let imported = import_rig_bundle(&exported).expect("import rig bundle");
        assert_eq!(imported, cfg);
        assert_eq!(imported.active_workspace_id, ws_id);

        // Also supports raw AppConfig JSON and normalizes empty workspaces
        let mut raw_cfg = AppConfig::default();
        raw_cfg.overview_workspaces.clear();
        raw_cfg.active_workspace_id = "missing".to_string();
        let raw_path = dir.join("raw-config.json");
        fs::write(&raw_path, serde_json::to_string_pretty(&raw_cfg).unwrap()).unwrap();
        let restored = import_rig_bundle(&raw_path).expect("import raw config");
        assert_eq!(restored.overview_workspaces.len(), 1);
        assert_eq!(restored.active_workspace_id, "default");

        // Reject unsafe URL schemes
        assert!(open_url("file:///C:/Windows/System32/cmd.exe").is_err());
        assert!(open_url("javascript:alert(1)").is_err());

        let _ = fs::remove_dir_all(&dir);
    }
}
