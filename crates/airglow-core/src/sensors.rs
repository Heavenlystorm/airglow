//! Sensor polling: LibreHardwareMonitor JSON tree (with Basic Auth) + native Windows sysinfo + demo fallback.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};
use sysinfo::{Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};

use crate::config::TempUnit;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorReading {
    pub id: String,
    pub name: String,
    pub hardware: String,
    pub hw_kind: String,
    pub sensor_type: String,
    pub unit: String,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LhmFanReading {
    pub id: String,
    pub name: String,
    pub hardware: String,
    pub fan_id: Option<String>,
    pub rpm: Option<u16>,
    pub duty: Option<f32>,
    pub is_pump: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SensorSnapshot {
    pub online: bool,
    pub lhm_connected: bool,
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub gpu_hotspot: Option<f32>,
    pub liquid_temp: Option<f32>,
    pub ssd_temp: Option<f32>,
    pub cpu_load: f32,
    pub gpu_load: f32,
    pub ram_load: f32,
    pub ram_used_gb: Option<f32>,
    pub ram_total_gb: Option<f32>,
    pub net_down_bps: Option<f32>,
    pub net_up_bps: Option<f32>,
    pub cpu_power_w: Option<f32>,
    pub gpu_power_w: Option<f32>,
    pub total_power_w: Option<f32>,
    pub cpu_name: String,
    pub gpu_name: String,
    pub lhm_fan_channels: Vec<LhmFanReading>,
    pub readings: Vec<SensorReading>,
    pub app_cpu_pct: f32,
    pub app_mem_mb: f32,
}

impl Default for SensorSnapshot {
    fn default() -> Self {
        Self {
            online: false,
            lhm_connected: false,
            cpu_temp: None,
            gpu_temp: None,
            gpu_hotspot: None,
            liquid_temp: None,
            ssd_temp: None,
            cpu_load: 0.0,
            gpu_load: 0.0,
            ram_load: 0.0,
            ram_used_gb: None,
            ram_total_gb: None,
            net_down_bps: None,
            net_up_bps: None,
            cpu_power_w: None,
            gpu_power_w: None,
            total_power_w: None,
            cpu_name: "Processor".to_string(),
            gpu_name: "Graphics".to_string(),
            lhm_fan_channels: Vec::new(),
            readings: Vec::new(),
            app_cpu_pct: 0.0,
            app_mem_mb: 0.0,
        }
    }
}

pub fn format_temp(temp_c: Option<f32>, unit: TempUnit) -> String {
    match temp_c {
        Some(v) => unit.format_temp(v),
        None => "—".to_string(),
    }
}

pub fn format_throughput(bps: Option<f32>) -> String {
    let Some(v) = bps else {
        return "—".to_string();
    };
    const GB: f32 = 1024.0 * 1024.0 * 1024.0;
    const MB: f32 = 1024.0 * 1024.0;
    const KB: f32 = 1024.0;
    if v >= GB {
        format!("{:.1} GB/s", v / GB)
    } else if v >= MB {
        format!("{:.1} MB/s", v / MB)
    } else if v >= KB {
        format!("{:.1} KB/s", v / KB)
    } else {
        format!("{:.0} B/s", v.max(0.0))
    }
}

pub struct SensorPoller {
    sys: System,
    networks: Networks,
    last_net_poll: Instant,
    current_pid: Option<Pid>,
    ever_connected_lhm: bool,
    missed_lhm_polls: u8,
    last_lhm_snap: Option<SensorSnapshot>,
}

impl SensorPoller {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let current_pid = get_current_pid().ok();
        if let Some(pid) = current_pid {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );
        }
        let networks = Networks::new_with_refreshed_list();
        Self {
            sys,
            networks,
            last_net_poll: Instant::now(),
            current_pid,
            ever_connected_lhm: false,
            missed_lhm_polls: 0,
            last_lhm_snap: None,
        }
    }

    fn sample_self_process(&mut self) -> (f32, f32) {
        let Some(pid) = self.current_pid else {
            return (0.0, 0.0);
        };
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let logical_cpus = self.sys.cpus().len().max(1) as f32;
        if let Some(proc_info) = self.sys.process(pid) {
            let cpu = (proc_info.cpu_usage() / logical_cpus).clamp(0.0, 100.0);
            let mem_mb = proc_info.memory() as f32 / (1024.0 * 1024.0);
            (cpu, mem_mb)
        } else {
            (0.0, 0.0)
        }
    }

    pub fn poll(
        &mut self,
        lhm_url: &str,
        auth_header: Option<&str>,
        demo_mode: bool,
        elapsed_s: f32,
    ) -> SensorSnapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let (app_cpu_pct, app_mem_mb) = self.sample_self_process();
        let cpu_load = self.sys.global_cpu_usage();
        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let ram_load = if total_mem > 0 {
            (used_mem as f32 / total_mem as f32) * 100.0
        } else {
            0.0
        };
        let ram_total_gb = (total_mem > 0).then(|| total_mem as f32 / (1024.0 * 1024.0 * 1024.0));
        let ram_used_gb = (total_mem > 0).then(|| used_mem as f32 / (1024.0 * 1024.0 * 1024.0));

        let now = Instant::now();
        let net_dt = now.duration_since(self.last_net_poll).as_secs_f32().max(0.1);
        self.last_net_poll = now;
        self.networks.refresh(false);
        let mut rx_bytes = 0u64;
        let mut tx_bytes = 0u64;
        for (_, data) in self.networks.iter() {
            rx_bytes = rx_bytes.saturating_add(data.received());
            tx_bytes = tx_bytes.saturating_add(data.transmitted());
        }
        let sys_net_down = rx_bytes as f32 / net_dt;
        let sys_net_up = tx_bytes as f32 / net_dt;

        let cpu_name = self
            .sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Processor".to_string());

        if let Some(tree) = fetch_lhm_json(lhm_url, auth_header) {
            let mut snap = parse_lhm_tree(&tree);
            snap.online = true;
            snap.lhm_connected = true;
            if snap.cpu_load == 0.0 {
                snap.cpu_load = cpu_load;
            }
            if snap.ram_load == 0.0 {
                snap.ram_load = ram_load;
            }
            if snap.ram_used_gb.is_none() {
                snap.ram_used_gb = ram_used_gb;
            }
            if snap.ram_total_gb.is_none() {
                snap.ram_total_gb = ram_total_gb;
            }
            if snap.net_down_bps.is_none() {
                snap.net_down_bps = Some(sys_net_down);
            }
            if snap.net_up_bps.is_none() {
                snap.net_up_bps = Some(sys_net_up);
            }
            if snap.cpu_name == "Processor" {
                snap.cpu_name = cpu_name;
            }
            snap.app_cpu_pct = app_cpu_pct;
            snap.app_mem_mb = app_mem_mb;
            self.ever_connected_lhm = true;
            self.missed_lhm_polls = 0;
            self.last_lhm_snap = Some(snap.clone());
            return snap;
        }

        // Once real LHM telemetry has been observed, never fall back to synthetic demo sine waves:
        // hold the last real LHM reading across up to 2 transient HTTP misses, then report offline.
        if self.ever_connected_lhm {
            self.missed_lhm_polls = self.missed_lhm_polls.saturating_add(1);
            if self.missed_lhm_polls <= 2
                && let Some(mut prev) = self.last_lhm_snap.clone()
            {
                prev.cpu_load = cpu_load;
                prev.ram_load = ram_load;
                prev.ram_used_gb = ram_used_gb;
                prev.ram_total_gb = ram_total_gb;
                prev.net_down_bps = Some(sys_net_down);
                prev.net_up_bps = Some(sys_net_up);
                prev.app_cpu_pct = app_cpu_pct;
                prev.app_mem_mb = app_mem_mb;
                return prev;
            }
            return SensorSnapshot {
                online: false,
                lhm_connected: false,
                cpu_load,
                ram_load,
                ram_used_gb,
                ram_total_gb,
                net_down_bps: Some(sys_net_down),
                net_up_bps: Some(sys_net_up),
                cpu_name,
                app_cpu_pct,
                app_mem_mb,
                ..SensorSnapshot::default()
            };
        }

        if demo_mode {
            // Smooth simulated telemetry mixed with real CPU/RAM/Network stats from sysinfo.
            let wave_a = (elapsed_s * 0.35).sin();
            let wave_b = (elapsed_s * 0.22 + 1.1).cos();
            let cpu_temp = 47.0 + wave_a * 7.5 + (cpu_load * 0.22);
            let gpu_temp = 43.0 + wave_b * 6.0;
            let ssd_temp = 38.0 + wave_a * 2.5;
            let cpu_power = 42.0 + cpu_load * 0.75 + wave_a * 6.0;
            let gpu_power = 58.0 + (wave_b + 1.0) * 18.0;
            let net_down = if sys_net_down > 100.0 {
                sys_net_down
            } else {
                (1.8 + (wave_b + 1.0) * 2.4) * 1024.0 * 1024.0
            };
            return SensorSnapshot {
                online: true,
                lhm_connected: false,
                cpu_temp: Some(cpu_temp),
                gpu_temp: Some(gpu_temp),
                gpu_hotspot: Some(gpu_temp + 10.5),
                liquid_temp: Some(33.4 + wave_a * 1.4),
                ssd_temp: Some(ssd_temp),
                cpu_load,
                gpu_load: (22.0 + (wave_b + 1.0) * 14.0).clamp(0.0, 100.0),
                ram_load,
                ram_used_gb,
                ram_total_gb,
                net_down_bps: Some(net_down),
                net_up_bps: Some(sys_net_up),
                cpu_power_w: Some(cpu_power),
                gpu_power_w: Some(gpu_power),
                total_power_w: Some(cpu_power + gpu_power + 35.0),
                cpu_name,
                gpu_name: "Demo GPU".to_string(),
                lhm_fan_channels: Vec::new(),
                readings: Vec::new(),
                app_cpu_pct,
                app_mem_mb,
            };
        }

        SensorSnapshot {
            online: false,
            lhm_connected: false,
            cpu_load,
            ram_load,
            ram_used_gb,
            ram_total_gb,
            net_down_bps: Some(sys_net_down),
            net_up_bps: Some(sys_net_up),
            cpu_name,
            app_cpu_pct,
            app_mem_mb,
            ..SensorSnapshot::default()
        }
    }

    pub fn running_exe_names(&mut self) -> HashSet<String> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        self.sys
            .processes()
            .values()
            .map(|p| p.name().to_string_lossy().to_ascii_lowercase())
            .collect()
    }
}

pub fn fetch_lhm_json(base_url: &str, auth_header: Option<&str>) -> Option<Value> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let host_port = trimmed.strip_prefix("http://")?;
    let (host, path) = match host_port.split_once('/') {
        Some((hp, p)) => (hp, format!("/{p}/data.json")),
        None => (host_port, "/data.json".to_string()),
    };
    let addr_str = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:8085")
    };
    let addr: SocketAddr = addr_str.parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(350)).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(700)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(350)));
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(auth) = auth_header {
        if !auth.is_empty() && !auth.contains('\r') && !auth.contains('\n') {
            req.push_str(&format!("Authorization: {auth}\r\n"));
        }
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).ok()?;

    let mut raw = Vec::with_capacity(64 * 1024);
    stream.read_to_end(&mut raw).ok()?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let headers = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let body = &raw[split + 4..];
    if headers.contains("transfer-encoding: chunked") {
        let decoded = decode_chunked(body)?;
        serde_json::from_slice(&decoded).ok()
    } else {
        serde_json::from_slice(body).ok()
    }
}

fn decode_chunked(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    while !data.is_empty() {
        let crlf = data.windows(2).position(|w| w == b"\r\n")?;
        let size_str = std::str::from_utf8(&data[..crlf]).ok()?.trim();
        let size_hex = size_str.split(';').next().unwrap_or(size_str).trim();
        let size = usize::from_str_radix(size_hex, 16).ok()?;
        data = &data[crlf + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size..];
        if data.starts_with(b"\r\n") {
            data = &data[2..];
        }
    }
    Some(out)
}

impl Default for SensorPoller {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_number(s: &str) -> Option<f32> {
    let mut buf = String::new();
    let mut started = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() || (ch == '-' && !started) {
            started = true;
            buf.push(ch);
        } else if (ch == '.' || ch == ',') && started {
            buf.push('.');
        } else if started {
            break;
        }
    }
    buf.parse::<f32>().ok()
}

fn throughput_multiplier(val_str: &str) -> f32 {
    let lower = val_str.to_ascii_lowercase();
    if lower.contains("gb/s") {
        1024.0 * 1024.0 * 1024.0
    } else if lower.contains("mb/s") {
        1024.0 * 1024.0
    } else if lower.contains("kb/s") {
        1024.0
    } else {
        1.0
    }
}

fn sensor_unit(sensor_type: &str) -> &'static str {
    match sensor_type {
        "temperature" | "temperatures" => "°C",
        "load" | "control" | "controls" | "level" => "%",
        "power" => "W",
        "clock" => "MHz",
        "data" => "GB",
        "smalldata" => "MB",
        "fan" | "fans" => "RPM",
        "voltage" => "V",
        "throughput" => "B/s",
        "flow" => "L/h",
        "current" => "A",
        "frequency" => "Hz",
        _ => "",
    }
}

fn is_igpu_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("radeon(tm) graphics")
        || n.contains("intel(r) graphics")
        || n.contains("intel(r) uhd")
        || n.contains("intel(r) iris")
        || n.contains("arc(tm) graphics")
}

fn classify_hw(node: &Value) -> Option<&'static str> {
    let hid = node
        .get("HardwareId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let img = node
        .get("ImageURL")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let text = node.get("Text").and_then(Value::as_str).unwrap_or("");
    if hid.contains("cpu") || img.ends_with("cpu.png") {
        return Some("cpu");
    }
    if hid.contains("gpu")
        || img.ends_with("nvidia.png")
        || img.ends_with("ati.png")
        || img.ends_with("amd.png")
    {
        return Some(if is_igpu_name(text) { "igpu" } else { "gpu" });
    }
    if hid.contains("motherboard")
        || hid.contains("lpc")
        || img.ends_with("mainboard.png")
        || img.ends_with("chip.png")
    {
        return Some("board");
    }
    if hid.starts_with("/ram")
        || hid.starts_with("/vram")
        || hid.starts_with("/memory")
        || img.ends_with("ram.png")
    {
        return Some("memory");
    }
    if hid.starts_with("/nvme")
        || hid.starts_with("/hdd")
        || hid.starts_with("/ssd")
        || hid.starts_with("/storage")
        || img.ends_with("hdd.png")
    {
        return Some("storage");
    }
    if hid.starts_with("/nic") || img.ends_with("nic.png") {
        return Some("network");
    }
    if hid.contains("cooler") || hid.contains("kraken") || hid.contains("nzxt") {
        return Some("cooler");
    }
    if !hid.is_empty() {
        return Some("other");
    }
    None
}

fn flatten_lhm(
    node: &Value,
    hw_kind: &str,
    hw_name: &str,
    group: &str,
    out: &mut Vec<SensorReading>,
) {
    let text = node.get("Text").and_then(Value::as_str).unwrap_or("");
    let children = node.get("Children").and_then(Value::as_array);
    let has_children = children.is_some_and(|c| !c.is_empty());

    let mut cur_hw = hw_kind;
    let mut cur_name = hw_name;
    if has_children {
        if let Some(kind) = classify_hw(node) {
            cur_hw = kind;
            cur_name = text;
        }
        for child in children.unwrap() {
            let child_has_kids = child
                .get("Children")
                .and_then(Value::as_array)
                .is_some_and(|c| !c.is_empty());
            let child_hid = child.get("HardwareId").and_then(Value::as_str);
            let next_group = if child_has_kids && child_hid.is_none() {
                child.get("Text").and_then(Value::as_str).unwrap_or(group)
            } else {
                group
            };
            flatten_lhm(child, cur_hw, cur_name, next_group, out);
        }
        return;
    }

    let Some(val_str) = node.get("Value").and_then(Value::as_str) else {
        return;
    };
    let Some(mut val) = parse_number(val_str) else {
        return;
    };
    let sensor_type = node
        .get("Type")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(group)
        .to_ascii_lowercase();
    if sensor_type == "throughput" {
        val *= throughput_multiplier(val_str);
    }
    let id = node
        .get("SensorId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{cur_name}/{group}/{text}"));

    out.push(SensorReading {
        id,
        name: text.to_string(),
        hardware: cur_name.to_string(),
        hw_kind: cur_hw.to_string(),
        sensor_type: sensor_type.clone(),
        unit: sensor_unit(&sensor_type).to_string(),
        value: val,
    });
}

fn pick_sensor<'a>(
    readings: &'a [SensorReading],
    hw_kind: &str,
    type_word: &str,
    prefer: &[&str],
) -> Option<&'a SensorReading> {
    let pool: Vec<&SensorReading> = readings
        .iter()
        .filter(|r| r.hw_kind == hw_kind && r.sensor_type.contains(type_word))
        .collect();
    for word in prefer {
        if let Some(found) = pool
            .iter()
            .find(|r| r.name.to_ascii_lowercase().contains(word))
        {
            return Some(*found);
        }
    }
    pool.into_iter()
        .max_by(|a, b| a.value.total_cmp(&b.value))
}

pub fn parse_lhm_tree(root: &Value) -> SensorSnapshot {
    let mut flat = Vec::new();
    flatten_lhm(root, "other", "", "", &mut flat);

    let mut snap = SensorSnapshot::default();
    let has_discrete_gpu = flat.iter().any(|r| r.hw_kind == "gpu");
    let gpu_kind = if has_discrete_gpu { "gpu" } else { "igpu" };

    if let Some(cpu_r) = flat.iter().find(|r| r.hw_kind == "cpu" && !r.hardware.is_empty()) {
        snap.cpu_name = cpu_r.hardware.clone();
    }
    if let Some(gpu_r) = flat
        .iter()
        .find(|r| r.hw_kind == gpu_kind && !r.hardware.is_empty())
    {
        snap.gpu_name = gpu_r.hardware.clone();
    }

    snap.cpu_temp = pick_sensor(
        &flat,
        "cpu",
        "temperature",
        &["tctl/tdie", "tdie", "package", "core max"],
    )
    .map(|r| r.value);
    snap.cpu_load = pick_sensor(&flat, "cpu", "load", &["total"])
        .map(|r| r.value)
        .unwrap_or(0.0);
    snap.cpu_power_w = pick_sensor(&flat, "cpu", "power", &["package"]).map(|r| r.value);

    snap.gpu_temp =
        pick_sensor(&flat, gpu_kind, "temperature", &["gpu core", "edge", "core"]).map(|r| r.value);
    snap.gpu_hotspot = pick_sensor(
        &flat,
        gpu_kind,
        "temperature",
        &["hot spot", "hotspot", "junction"],
    )
    .map(|r| r.value);
    snap.gpu_load = pick_sensor(&flat, gpu_kind, "load", &["gpu core", "core", "d3d 3d"])
        .map(|r| r.value)
        .unwrap_or(0.0);
    snap.gpu_power_w = pick_sensor(
        &flat,
        gpu_kind,
        "power",
        &["gpu package", "gpu power", "board power", "package", "total"],
    )
    .map(|r| r.value);

    // Liquid coolant: never CPU/GPU/storage/memory/network, must be in 5..=85 C
    snap.liquid_temp = flat
        .iter()
        .find(|r| {
            r.sensor_type.contains("temperature")
                && !matches!(
                    r.hw_kind.as_str(),
                    "cpu" | "gpu" | "igpu" | "storage" | "memory" | "network"
                )
                && {
                    let n = r.name.to_ascii_lowercase();
                    n.contains("liquid") || n.contains("coolant") || n.contains("water")
                }
                && (5.0..=85.0).contains(&r.value)
        })
        .map(|r| r.value);

    // Drive temperature (SSD/NVMe)
    snap.ssd_temp = pick_sensor(
        &flat,
        "storage",
        "temperature",
        &["composite", "temperature"],
    )
    .map(|r| r.value);

    // Network download/upload throughput from LHM (if present)
    snap.net_down_bps = pick_sensor(&flat, "network", "throughput", &["download"]).map(|r| r.value);
    snap.net_up_bps = pick_sensor(&flat, "network", "throughput", &["upload"]).map(|r| r.value);

    if let (Some(c), Some(g)) = (snap.cpu_power_w, snap.gpu_power_w) {
        snap.total_power_w = Some(c + g + 30.0);
    }

    // LHM controllable fan/pump channels (excluding GPU fans)
    let by_id: HashMap<&str, &SensorReading> =
        flat.iter().map(|r| (r.id.as_str(), r)).collect();
    for r in &flat {
        if !matches!(r.sensor_type.as_str(), "control" | "controls")
            || matches!(r.hw_kind.as_str(), "gpu" | "igpu")
        {
            continue;
        }
        let fan_candidate = if r.id.contains("/control/") {
            Some(r.id.replace("/control/", "/fan/"))
        } else {
            None
        };
        let paired_fan = fan_candidate
            .as_deref()
            .and_then(|fid| by_id.get(fid).copied());
        let is_pump = r.name.to_ascii_lowercase().contains("pump")
            || paired_fan.is_some_and(|f| f.name.to_ascii_lowercase().contains("pump"));
        snap.lhm_fan_channels.push(LhmFanReading {
            id: r.id.clone(),
            name: r.name.clone(),
            hardware: r.hardware.clone(),
            fan_id: paired_fan.map(|f| f.id.clone()),
            rpm: paired_fan.map(|f| f.value.round().clamp(0.0, u16::MAX as f32) as u16),
            duty: Some(r.value),
            is_pump,
        });
    }

    snap.readings = flat;
    snap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lhm_cpu_gpu_ssd_net_and_fan_channels() {
        let json = serde_json::json!({
            "Text": "Computer",
            "Children": [
                {
                    "Text": "AMD Ryzen 7 7800X3D",
                    "HardwareId": "/amdcpu/0",
                    "Children": [
                        { "SensorId": "/amdcpu/0/temperature/2", "Text": "Core (Tctl/Tdie)", "Type": "Temperature", "Value": "62,5 °C" },
                        { "SensorId": "/amdcpu/0/power/0", "Text": "Package", "Type": "Power", "Value": "54.2 W" }
                    ]
                },
                {
                    "Text": "NVIDIA GeForce RTX 4080",
                    "HardwareId": "/gpu-nvidia/0",
                    "Children": [
                        { "SensorId": "/gpu-nvidia/0/temperature/0", "Text": "GPU Core", "Type": "Temperature", "Value": "51.0 °C" },
                        { "SensorId": "/gpu-nvidia/0/temperature/2", "Text": "GPU Hot Spot", "Type": "Temperature", "Value": "63.4 °C" },
                        { "SensorId": "/gpu-nvidia/0/power/0", "Text": "GPU Package", "Type": "Power", "Value": "145.0 W" }
                    ]
                },
                {
                    "Text": "Samsung 990 PRO",
                    "HardwareId": "/nvme/0",
                    "Children": [
                        { "SensorId": "/nvme/0/temperature/0", "Text": "Composite Temperature", "Type": "Temperature", "Value": "41.0 °C" }
                    ]
                },
                {
                    "Text": "Ethernet",
                    "HardwareId": "/nic/0",
                    "Children": [
                        { "SensorId": "/nic/0/throughput/7", "Text": "Download Speed", "Type": "Throughput", "Value": "2.5 MB/s" }
                    ]
                },
                {
                    "Text": "Nuvoton NCT6798D",
                    "HardwareId": "/lpc/nct6798d",
                    "Children": [
                        { "SensorId": "/lpc/nct6798d/fan/0", "Text": "Fan #1", "Type": "Fan", "Value": "1150 RPM" },
                        { "SensorId": "/lpc/nct6798d/control/0", "Text": "Fan #1", "Type": "Control", "Value": "45.0 %" },
                        { "SensorId": "/lpc/nct6798d/fan/1", "Text": "AIO Pump", "Type": "Fan", "Value": "2600 RPM" },
                        { "SensorId": "/lpc/nct6798d/control/1", "Text": "AIO Pump", "Type": "Control", "Value": "80.0 %" }
                    ]
                }
            ]
        });
        let snap = parse_lhm_tree(&json);
        assert_eq!(snap.cpu_temp, Some(62.5));
        assert_eq!(snap.gpu_temp, Some(51.0));
        assert_eq!(snap.gpu_hotspot, Some(63.4));
        assert_eq!(snap.ssd_temp, Some(41.0));
        assert_eq!(snap.net_down_bps, Some(2.5 * 1024.0 * 1024.0));
        assert_eq!(snap.lhm_fan_channels.len(), 2);
        assert_eq!(snap.lhm_fan_channels[0].rpm, Some(1150));
        assert!(!snap.lhm_fan_channels[0].is_pump);
        assert!(snap.lhm_fan_channels[1].is_pump);
        assert!(snap.readings.len() >= 10);
    }

    #[test]
    fn prefers_discrete_gpu_over_integrated_gpu() {
        let json = serde_json::json!({
            "Text": "Computer",
            "Children": [
                {
                    "Text": "NVIDIA GeForce RTX 5080",
                    "HardwareId": "/gpu-nvidia/0",
                    "Children": [
                        { "Text": "GPU Core", "Type": "Temperature", "Value": "48.0 °C" },
                        { "Text": "GPU Core", "Type": "Load", "Value": "14 %" }
                    ]
                },
                {
                    "Text": "AMD Radeon(TM) Graphics",
                    "HardwareId": "/gpu-amd/0",
                    "Children": [
                        { "Text": "GPU Core", "Type": "Temperature", "Value": "39.0 °C" },
                        { "Text": "GPU Core", "Type": "Load", "Value": "99 %" }
                    ]
                }
            ]
        });
        let snap = parse_lhm_tree(&json);
        assert_eq!(snap.gpu_name, "NVIDIA GeForce RTX 5080");
        assert_eq!(snap.gpu_temp, Some(48.0));
        assert_eq!(snap.gpu_load, 14.0);
    }

    #[test]
    fn formats_temp_and_throughput() {
        assert_eq!(format_temp(Some(60.0), TempUnit::C), "60°C");
        assert_eq!(format_temp(Some(60.0), TempUnit::F), "140°F");
        assert_eq!(format_temp(None, TempUnit::C), "—");
        assert_eq!(format_throughput(Some(1536.0)), "1.5 KB/s");
        assert_eq!(format_throughput(Some(5.0 * 1024.0 * 1024.0)), "5.0 MB/s");
    }

    #[test]
    fn lhm_live_never_reverts_to_synthetic_demo_telemetry() {
        let mut poller = SensorPoller::new();
        // Simulate having already connected to real LHM with a known real reading
        poller.ever_connected_lhm = true;
        poller.last_lhm_snap = Some(SensorSnapshot {
            online: true,
            lhm_connected: true,
            cpu_temp: Some(54.0),
            gpu_temp: Some(41.0),
            cpu_power_w: Some(68.0),
            gpu_power_w: Some(42.0),
            total_power_w: Some(140.0),
            gpu_name: "RTX 5080".to_string(),
            ..SensorSnapshot::default()
        });

        // Unreachable URL: first 2 transient misses hold last real LHM reading (never Demo GPU)
        let s1 = poller.poll("http://127.0.0.1:1", None, true, 5.0);
        assert!(s1.lhm_connected);
        assert_eq!(s1.gpu_name, "RTX 5080");
        assert_eq!(s1.cpu_power_w, Some(68.0));

        let s2 = poller.poll("http://127.0.0.1:1", None, true, 7.0);
        assert!(s2.lhm_connected);
        assert_eq!(s2.gpu_power_w, Some(42.0));

        // 3rd consecutive miss reports offline cleanly — NEVER synthetic "Demo GPU" sine waves
        let s3 = poller.poll("http://127.0.0.1:1", None, true, 9.0);
        assert!(!s3.lhm_connected);
        assert!(!s3.online);
        assert_ne!(s3.gpu_name, "Demo GPU");
        assert_eq!(s3.cpu_power_w, None);
        assert_eq!(s3.gpu_power_w, None);
        assert!(s3.app_cpu_pct.is_finite() && s3.app_cpu_pct >= 0.0 && s3.app_cpu_pct <= 100.0);
        assert!(s3.app_mem_mb.is_finite() && s3.app_mem_mb > 0.0);
    }
}
