//! LibreHardwareMonitor motherboard & AIO fan control client matching `lhmfans.py`.
//!
//! Controls motherboard Super I/O fan headers and LHM-supported cooler/hub pumps & fans
//! via LHM's local web server (`POST /Sensor?action=Set&id=...&value=...`), and cleanly
//! hands channels back to BIOS/hardware control (`value=null`) when deselected or on exit.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub const MAX_LHM_CHANNELS: usize = 32;
pub const RESEND_INTERVAL: Duration = Duration::from_secs(30);
pub const RETRY_BACKOFF: Duration = Duration::from_secs(15);
pub const SENDS_PER_TICK: usize = 6;
pub const HTTP_TIMEOUT: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ChannelMode {
    /// Leave the channel under BIOS / cooler firmware control.
    #[default]
    Auto,
    /// Follow Airglow's active fan curve.
    Curve,
    /// Hold a constant duty cycle (20..=100%), suited for AIO pumps.
    Steady(u8),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LhmFanChannel {
    pub id: String,
    pub name: String,
    pub hardware: String,
    pub rpm: Option<u32>,
    pub duty: Option<f32>,
    pub is_pump: bool,
    pub mode: ChannelMode,
}

/// Validates an LHM control sensor ID (must start with `/`, 1..=200 chars, no control chars).
pub fn validate_sensor_id(sensor_id: &str) -> Result<()> {
    if sensor_id.is_empty()
        || sensor_id.len() > 200
        || !sensor_id.starts_with('/')
        || sensor_id.bytes().any(|b| b < 32)
    {
        bail!("invalid LibreHardwareMonitor sensor id");
    }
    Ok(())
}

/// Percent-encodes a query parameter component per RFC 3986.
pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0x0F) as usize] as char);
            }
        }
    }
    out
}

/// Extracts all controllable non-GPU fan and pump channels from an LHM `data.json` tree.
pub fn parse_lhm_fan_channels(
    root: &Value,
    configured: &BTreeMap<String, ChannelMode>,
) -> Vec<LhmFanChannel> {
    #[derive(Clone)]
    struct FlatSensor {
        id: String,
        name: String,
        sensor_type: String,
        hardware: String,
        is_gpu: bool,
        value: Option<f32>,
    }

    fn parse_val(s: &str) -> Option<f32> {
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
        buf.parse::<f32>().ok().filter(|v| v.is_finite())
    }

    fn walk(node: &Value, hw_name: &str, is_gpu: bool, out: &mut Vec<FlatSensor>) {
        let text = node.get("Text").and_then(Value::as_str).unwrap_or("");
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

        let (next_hw, next_gpu) = if !hid.is_empty() {
            let gpu = hid.contains("gpu")
                || img.contains("nvidia")
                || img.contains("ati")
                || img.contains("gpu");
            (if text.is_empty() { hw_name } else { text }, gpu)
        } else {
            (hw_name, is_gpu)
        };

        if let Some(children) = node.get("Children").and_then(Value::as_array)
            && !children.is_empty()
        {
            for child in children {
                walk(child, next_hw, next_gpu, out);
            }
            return;
        }

        let Some(id) = node.get("SensorId").and_then(Value::as_str) else {
            return;
        };
        let sensor_type = node
            .get("Type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let value = node
            .get("Value")
            .and_then(Value::as_str)
            .and_then(parse_val);

        out.push(FlatSensor {
            id: id.to_string(),
            name: text.to_string(),
            sensor_type,
            hardware: next_hw.to_string(),
            is_gpu: next_gpu || id.to_ascii_lowercase().contains("/gpu"),
            value,
        });
    }

    let mut flat = Vec::new();
    walk(root, "", false, &mut flat);

    let by_id: HashMap<&str, &FlatSensor> = flat.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut channels = Vec::new();

    for s in &flat {
        if !matches!(s.sensor_type.as_str(), "control" | "controls") || s.is_gpu {
            continue;
        }
        let paired_fan = if s.id.contains("/control/") {
            let fan_id = s.id.replace("/control/", "/fan/");
            by_id.get(fan_id.as_str()).copied()
        } else {
            None
        };
        let rpm = paired_fan
            .and_then(|f| f.value)
            .filter(|v| *v >= 0.0)
            .map(|v| v.round() as u32);
        let is_pump = s.name.to_ascii_lowercase().contains("pump")
            || paired_fan.is_some_and(|f| f.name.to_ascii_lowercase().contains("pump"));
        let mode = configured.get(&s.id).copied().unwrap_or(ChannelMode::Auto);

        channels.push(LhmFanChannel {
            id: s.id.clone(),
            name: s.name.clone(),
            hardware: s.hardware.clone(),
            rpm,
            duty: s.value,
            is_pump,
            mode,
        });
    }

    channels
}

/// Sends a duty percentage (`0..=100`) to a single LibreHardwareMonitor control sensor.
pub fn set_channel_duty(
    lhm_url: &str,
    auth_header: Option<&str>,
    sensor_id: &str,
    duty: u8,
) -> Result<()> {
    validate_sensor_id(sensor_id)?;
    let clamped = duty.min(100);
    post_lhm_sensor_set(lhm_url, auth_header, sensor_id, &clamped.to_string())
}

/// Hands a LibreHardwareMonitor control sensor back to its own BIOS/firmware control (`value=null`).
pub fn restore_auto(lhm_url: &str, auth_header: Option<&str>, sensor_id: &str) -> Result<()> {
    validate_sensor_id(sensor_id)?;
    post_lhm_sensor_set(lhm_url, auth_header, sensor_id, "null")
}

fn build_sensor_set_request(
    lhm_url: &str,
    auth_header: Option<&str>,
    sensor_id: &str,
    value_param: &str,
) -> Result<(SocketAddr, String)> {
    let trimmed = lhm_url.trim().trim_end_matches('/');
    let host_port_path = trimmed
        .strip_prefix("http://")
        .context("LHM URL must use http://")?;
    let (host_port, base_path) = match host_port_path.split_once('/') {
        Some((hp, p)) => (hp, format!("/{}", p.trim_matches('/'))),
        None => (host_port_path, String::new()),
    };

    let normalized_host_port = if let Some(rest) = host_port.strip_prefix("localhost")
        && (rest.is_empty() || rest.starts_with(':'))
    {
        format!("127.0.0.1{rest}")
    } else {
        host_port.to_string()
    };

    let addr_str = if normalized_host_port.contains(':') {
        normalized_host_port.clone()
    } else {
        format!("{normalized_host_port}:8085")
    };

    let addr: SocketAddr = addr_str
        .parse()
        .or_else(|_| {
            addr_str
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "no addr"))
        })
        .with_context(|| format!("resolve LHM address '{addr_str}'"))?;

    let query = format!(
        "action=Set&id={}&value={}",
        url_encode(sensor_id),
        url_encode(value_param)
    );
    let target = format!("{base_path}/Sensor?{query}");

    let mut req = format!(
        "POST {target} HTTP/1.1\r\nHost: {normalized_host_port}\r\nContent-Length: 0\r\nConnection: close\r\n"
    );
    if let Some(auth) = auth_header.filter(|a| !a.trim().is_empty()) {
        req.push_str(&format!("Authorization: {}\r\n", auth.trim()));
    }
    req.push_str("\r\n");

    Ok((addr, req))
}

fn parse_lhm_http_response(raw: &[u8]) -> Result<()> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("malformed HTTP response from LibreHardwareMonitor")?;
    let header_text = String::from_utf8_lossy(&raw[..split]);
    let status_line = header_text.lines().next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    if status_code != 200 {
        bail!("LibreHardwareMonitor answered HTTP {status_code}");
    }

    let body_bytes = if header_text
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        decode_chunked(&raw[split + 4..])
    } else {
        raw[split + 4..].to_vec()
    };

    let reply: Value = serde_json::from_slice(&body_bytes)
        .context("LibreHardwareMonitor returned invalid JSON")?;
    if reply.get("result").and_then(Value::as_str) != Some("ok") {
        let msg = reply
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("LibreHardwareMonitor refused")
            .lines()
            .next()
            .unwrap_or("LibreHardwareMonitor refused");
        let short: String = msg.chars().take(200).collect();
        bail!("{short}");
    }
    Ok(())
}

fn post_lhm_sensor_set(
    lhm_url: &str,
    auth_header: Option<&str>,
    sensor_id: &str,
    value_param: &str,
) -> Result<()> {
    let (addr, req) = build_sensor_set_request(lhm_url, auth_header, sensor_id, value_param)?;

    let mut stream = TcpStream::connect_timeout(&addr, HTTP_TIMEOUT)
        .context("LibreHardwareMonitor didn't answer (connect)")?;
    let _ = stream.set_read_timeout(Some(HTTP_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HTTP_TIMEOUT));
    stream
        .write_all(req.as_bytes())
        .context("LibreHardwareMonitor didn't answer (write)")?;

    let mut raw = Vec::with_capacity(4096);
    stream
        .read_to_end(&mut raw)
        .context("LibreHardwareMonitor didn't answer (read)")?;

    parse_lhm_http_response(&raw)
}

fn decode_chunked(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(pos) = data.windows(2).position(|w| w == b"\r\n") {
        let size_str = String::from_utf8_lossy(&data[..pos]);
        let size_hex = size_str.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        let start = pos + 2;
        if start + size > data.len() {
            out.extend_from_slice(&data[start..]);
            break;
        }
        out.extend_from_slice(&data[start..start + size]);
        let next = start + size;
        data = if data.get(next..next + 2) == Some(b"\r\n") {
            &data[next + 2..]
        } else {
            &data[next..]
        };
    }
    out
}

/// Stateful controller that sends duties only on change (or every 30 s), backs off dead channels,
/// and hands every driven channel back to BIOS/firmware control on shutdown.
pub struct LhmFanController {
    sent: HashMap<String, (Option<u8>, Option<Instant>)>,
    errors: HashMap<String, String>,
    retry_at: HashMap<String, Instant>,
    release_retry_at: HashMap<String, Instant>,
}

impl Default for LhmFanController {
    fn default() -> Self {
        Self::new()
    }
}

impl LhmFanController {
    pub fn new() -> Self {
        Self {
            sent: HashMap::new(),
            errors: HashMap::new(),
            retry_at: HashMap::new(),
            release_retry_at: HashMap::new(),
        }
    }

    pub fn errors(&self) -> &HashMap<String, String> {
        &self.errors
    }

    pub fn driven_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sent.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Marks all currently driven channels as stale so they will be resent on the next `apply`.
    pub fn resend_all(&mut self) {
        for (_, ts) in self.sent.values_mut() {
            *ts = None;
        }
    }

    /// Applies `{sensor_id -> target_duty}`. Any previously driven channel missing from `targets`
    /// is handed back to Auto (`value=null`).
    pub fn apply(
        &mut self,
        lhm_url: &str,
        auth_header: Option<&str>,
        targets: &BTreeMap<String, u8>,
    ) {
        self.apply_with(targets, false, |sid, val| match val {
            Some(duty) => set_channel_duty(lhm_url, auth_header, sid, duty),
            None => restore_auto(lhm_url, auth_header, sid),
        });
    }

    /// Immediately pushes every currently driven channel to at least `duty` (e.g. shutdown failsafe).
    pub fn failsafe(&mut self, lhm_url: &str, auth_header: Option<&str>, duty: u8) -> usize {
        self.failsafe_with(duty, |sid, val| match val {
            Some(d) => set_channel_duty(lhm_url, auth_header, sid, d),
            None => restore_auto(lhm_url, auth_header, sid),
        })
    }

    /// Hands every channel Airglow drove back to its own control (`value=null`).
    pub fn release_all(&mut self, lhm_url: &str, auth_header: Option<&str>) {
        self.release_all_with(|sid| restore_auto(lhm_url, auth_header, sid));
    }

    /// Hands one specific channel back to Auto (`value=null`).
    pub fn release(&mut self, lhm_url: &str, auth_header: Option<&str>, sid: &str) {
        self.release_one_with(sid, |s| restore_auto(lhm_url, auth_header, s));
    }

    fn failsafe_with<F>(&mut self, duty: u8, mut send: F) -> usize
    where
        F: FnMut(&str, Option<u8>) -> Result<()>,
    {
        let targets: BTreeMap<String, u8> = self
            .sent
            .iter()
            .map(|(sid, (last_duty, _))| {
                let target = match last_duty {
                    Some(d) => (*d).max(duty),
                    None => duty,
                };
                (sid.clone(), target)
            })
            .collect();
        self.resend_all();
        self.apply_with(&targets, true, &mut send);
        targets
            .iter()
            .filter(|(sid, target)| self.sent.get(*sid).is_some_and(|(d, _)| *d == Some(**target)))
            .count()
    }

    fn release_all_with<F>(&mut self, mut release_fn: F)
    where
        F: FnMut(&str) -> Result<()>,
    {
        let sids: Vec<String> = self.sent.keys().cloned().collect();
        for sid in sids {
            self.release_one_with(&sid, &mut release_fn);
        }
    }

    fn apply_with<F>(&mut self, targets: &BTreeMap<String, u8>, force: bool, mut send: F)
    where
        F: FnMut(&str, Option<u8>) -> Result<()>,
    {
        let now = Instant::now();
        let mut budget = SENDS_PER_TICK;

        let mut pending: Vec<(String, Option<u8>)> = self
            .sent
            .keys()
            .filter(|sid| !targets.contains_key(*sid))
            .map(|sid| (sid.clone(), None))
            .collect();
        for (sid, &duty) in targets {
            pending.push((sid.clone(), Some(duty)));
        }

        // Urgent (100% thermal safety or forced failsafe) first, then oldest-sent first.
        pending.sort_by_key(|(sid, duty)| {
            let urgent = duty.is_some_and(|d| force || d >= 100);
            let last_ts = self.sent.get(sid).and_then(|(_, ts)| *ts);
            (!urgent, last_ts)
        });

        for (sid, maybe_duty) in pending {
            let Some(duty) = maybe_duty else {
                let can_try = self
                    .release_retry_at
                    .get(&sid)
                    .is_none_or(|&at| now >= at);
                if budget > 0 && can_try {
                    budget -= 1;
                    self.release_one_with(&sid, |s| send(s, None));
                }
                continue;
            };

            self.release_retry_at.remove(&sid);
            let urgent = force || duty >= 100;
            if let Some((Some(last_duty), Some(last_ts))) = self.sent.get(&sid)
                && *last_duty == duty
                && now.duration_since(*last_ts) < RESEND_INTERVAL
            {
                continue;
            }
            if !urgent && self.retry_at.get(&sid).is_some_and(|&at| now < at) {
                continue;
            }
            if !urgent {
                if budget == 0 {
                    continue;
                }
                budget -= 1;
            }

            match send(&sid, Some(duty)) {
                Ok(()) => {
                    self.sent.insert(sid.clone(), (Some(duty), Some(now)));
                    self.errors.remove(&sid);
                    self.retry_at.remove(&sid);
                }
                Err(e) => {
                    self.errors.insert(sid.clone(), e.to_string());
                    self.sent.insert(sid.clone(), (None, None));
                    self.retry_at.insert(sid, now + RETRY_BACKOFF);
                }
            }
        }
    }

    fn release_one_with<F>(&mut self, sid: &str, mut release_fn: F)
    where
        F: FnMut(&str) -> Result<()>,
    {
        self.retry_at.remove(sid);
        match release_fn(sid) {
            Ok(()) => {
                self.sent.remove(sid);
                self.errors.remove(sid);
                self.release_retry_at.remove(sid);
            }
            Err(e) => {
                let now = Instant::now();
                self.errors
                    .insert(sid.to_string(), format!("couldn't hand it back: {e}"));
                self.sent.insert(sid.to_string(), (None, Some(now)));
                self.release_retry_at
                    .insert(sid.to_string(), now + RETRY_BACKOFF);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_sensor_ids() {
        assert_eq!(
            url_encode("/lpc/nct6701d/0/control/1"),
            "%2Flpc%2Fnct6701d%2F0%2Fcontrol%2F1"
        );
        assert_eq!(url_encode("Fan #1"), "Fan%20%231");
    }

    #[test]
    fn parses_non_gpu_control_channels_and_pairs_rpm_and_pump() {
        let tree = serde_json::json!({
            "Text": "Computer",
            "Children": [
                {
                    "Text": "ASUS ROG STRIX B850",
                    "HardwareId": "/lpc/nct6701d/0",
                    "Children": [
                        { "Text": "Fan #1", "SensorId": "/lpc/nct6701d/0/fan/0", "Type": "Fan", "Value": "1142 RPM" },
                        { "Text": "Fan #1", "SensorId": "/lpc/nct6701d/0/control/0", "Type": "Control", "Value": "42.5 %" },
                        { "Text": "AIO Pump", "SensorId": "/lpc/nct6701d/0/fan/1", "Type": "Fan", "Value": "2400 RPM" },
                        { "Text": "AIO Pump", "SensorId": "/lpc/nct6701d/0/control/1", "Type": "Control", "Value": "80.0 %" }
                    ]
                },
                {
                    "Text": "NVIDIA GeForce RTX 5080",
                    "HardwareId": "/gpu-nvidia/0",
                    "Children": [
                        { "Text": "GPU Fan", "SensorId": "/gpu-nvidia/0/control/0", "Type": "Control", "Value": "35.0 %" }
                    ]
                }
            ]
        });

        let mut cfg = BTreeMap::new();
        cfg.insert("/lpc/nct6701d/0/control/0".to_string(), ChannelMode::Curve);
        cfg.insert(
            "/lpc/nct6701d/0/control/1".to_string(),
            ChannelMode::Steady(80),
        );

        let channels = parse_lhm_fan_channels(&tree, &cfg);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].id, "/lpc/nct6701d/0/control/0");
        assert_eq!(channels[0].rpm, Some(1142));
        assert_eq!(channels[0].duty, Some(42.5));
        assert!(!channels[0].is_pump);
        assert_eq!(channels[0].mode, ChannelMode::Curve);

        assert_eq!(channels[1].id, "/lpc/nct6701d/0/control/1");
        assert_eq!(channels[1].rpm, Some(2400));
        assert!(channels[1].is_pump);
        assert_eq!(channels[1].mode, ChannelMode::Steady(80));
    }

    #[test]
    fn builds_http_post_requests_and_parses_lhm_responses() {
        let (addr, req_set) = build_sensor_set_request(
            "http://localhost:8085/",
            Some("Basic dGVzdDp0ZXN0"),
            "/lpc/nct6701d/0/control/1",
            "65",
        )
        .expect("build set request");
        assert_eq!(addr.to_string(), "127.0.0.1:8085");
        assert!(req_set.starts_with(
            "POST /Sensor?action=Set&id=%2Flpc%2Fnct6701d%2F0%2Fcontrol%2F1&value=65 HTTP/1.1\r\n"
        ));
        assert!(req_set.contains("Host: 127.0.0.1:8085\r\n"));
        assert!(req_set.contains("Authorization: Basic dGVzdDp0ZXN0\r\n"));

        let (_, req_auto) = build_sensor_set_request(
            "http://127.0.0.1:8085",
            None,
            "/lpc/nct6701d/0/control/1",
            "null",
        )
        .expect("build auto request");
        assert!(req_auto.starts_with(
            "POST /Sensor?action=Set&id=%2Flpc%2Fnct6701d%2F0%2Fcontrol%2F1&value=null HTTP/1.1\r\n"
        ));
        assert!(!req_auto.contains("Authorization:"));

        // Valid JSON OK response
        let ok_resp = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"result\":\"ok\"}";
        assert!(parse_lhm_http_response(ok_resp).is_ok());

        // Chunked OK response
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nF\r\n{\"result\":\"ok\"}\r\n0\r\n\r\n";
        assert!(parse_lhm_http_response(chunked).is_ok());

        // Refused JSON response (takes first line only)
        let err_resp = b"HTTP/1.1 200 OK\r\n\r\n{\"result\":\"error\",\"message\":\"sensor locked\\nstack trace\"}";
        let err = parse_lhm_http_response(err_resp).unwrap_err();
        assert_eq!(err.to_string(), "sensor locked");

        // Non-200 status
        let unauth = b"HTTP/1.1 401 Unauthorized\r\n\r\n";
        assert!(parse_lhm_http_response(unauth).is_err());
    }

    #[test]
    fn controller_applies_dedups_failsafes_and_releases_channels() {
        let mut ctrl = LhmFanController::new();
        let mut log = Vec::<(String, Option<u8>)>::new();

        let mut targets = BTreeMap::new();
        targets.insert("/lpc/nct6701d/0/control/0".to_string(), 45);
        targets.insert("/lpc/nct6701d/0/control/1".to_string(), 80);

        ctrl.apply_with(&targets, false, |sid, val| {
            log.push((sid.to_string(), val));
            Ok(())
        });
        assert_eq!(log.len(), 2);
        assert!(ctrl.errors().is_empty());

        // Re-applying identical duties within 30s sends nothing
        ctrl.apply_with(&targets, false, |sid, val| {
            log.push((sid.to_string(), val));
            Ok(())
        });
        assert_eq!(log.len(), 2);

        // Failsafe at 60% raises 45% -> 60% while keeping 80% at 80%
        let updated = ctrl.failsafe_with(60, |sid, val| {
            log.push((sid.to_string(), val));
            Ok(())
        });
        assert_eq!(updated, 2);
        assert!(log.contains(&("/lpc/nct6701d/0/control/0".to_string(), Some(60))));
        assert!(log.contains(&("/lpc/nct6701d/0/control/1".to_string(), Some(80))));

        // Release all hands both channels back to Auto
        let mut released = Vec::new();
        ctrl.release_all_with(|sid| {
            released.push(sid.to_string());
            Ok(())
        });
        released.sort();
        assert_eq!(
            released,
            vec!["/lpc/nct6701d/0/control/0", "/lpc/nct6701d/0/control/1"]
        );
        assert!(ctrl.driven_ids().is_empty());
    }
}
