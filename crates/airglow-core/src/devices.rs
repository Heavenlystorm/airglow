//! USB HID packet builders, device discovery, and active output writer for NZXT HUE 2 and ASUS Aura USB.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::config::{ColorOrder, ZoneConfig};
use crate::engine::ZoneFrame;

pub const NZXT_VID: u16 = 0x1E71;
pub const NZXT_CONTROL_HUB_PID: u16 = 0x2022;
pub const ASUS_VID: u16 = 0x0B05;
pub const ASUS_AURA_19AF_PID: u16 = 0x19AF;

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);

/// Build 64-byte HID packets for NZXT HUE 2 direct streaming (20 LEDs/packet, commit packet).
pub fn nzxt_stream_packets(colors: &[[u8; 3]], channel: u8, order: ColorOrder) -> Vec<[u8; 64]> {
    let mut data = Vec::with_capacity(colors.len().min(40) * 3);
    for &rgb in colors.iter().take(40) {
        data.extend_from_slice(&order.reorder(rgb));
    }
    let mut packets = Vec::new();
    for (n, chunk) in data.chunks(60).enumerate() {
        let mut pkt = [0u8; 64];
        pkt[0] = 0x22;
        pkt[1] = 0x10 | (n as u8);
        pkt[2] = 1 << channel;
        pkt[3] = 0x00;
        pkt[4..4 + chunk.len()].copy_from_slice(chunk);
        packets.push(pkt);
    }
    let mut commit = [0u8; 64];
    commit[..16].copy_from_slice(&[
        0x22,
        0xA0,
        1 << channel,
        0x00,
        0x01,
        0x00,
        0x00,
        0x28,
        0x00,
        0x00,
        0x80,
        0x00,
        0x32,
        0x00,
        0x00,
        0x01,
    ]);
    packets.push(commit);
    packets
}

pub fn nzxt_fan_packet(duties: &[(u8, u8)], ports: u8) -> [u8; 64] {
    let mut pkt = [0u8; 64];
    pkt[0] = 0x62;
    pkt[1] = 0x01;
    for &(port, duty) in duties {
        if port < ports {
            pkt[2] |= 1 << port;
            pkt[3 + port as usize] = duty.min(100);
        }
    }
    pkt
}

pub fn nzxt_parse_fans(report: &[u8], ports: usize) -> Vec<(u16, u8)> {
    (0..ports)
        .filter_map(|i| {
            let rpm_idx = 24 + i * 2;
            let duty_idx = 40 + i;
            if rpm_idx + 1 < report.len() && duty_idx < report.len() {
                let rpm = (report[rpm_idx] as u16) | ((report[rpm_idx + 1] as u16) << 8);
                Some((rpm, report[duty_idx]))
            } else {
                None
            }
        })
        .collect()
}

pub fn aura_direct_packets(channel: u8, colors: &[[u8; 3]], order: ColorOrder) -> Vec<[u8; 65]> {
    let mut flat = Vec::with_capacity(colors.len() * 3);
    for &c in colors {
        flat.extend_from_slice(&order.reorder(c));
    }
    let mut packets = Vec::new();
    let total = colors.len();
    let mut sent = 0usize;
    while sent < total {
        let count = (total - sent).min(20);
        let apply_flag = if sent + count == total { 0x80 } else { 0x00 };
        let mut pkt = [0u8; 65];
        pkt[0] = 0xEC; // HID report ID
        pkt[1] = 0x40;
        pkt[2] = apply_flag | channel;
        pkt[3] = sent as u8;
        pkt[4] = count as u8;
        let slice = &flat[sent * 3..(sent + count) * 3];
        pkt[5..5 + slice.len()].copy_from_slice(slice);
        packets.push(pkt);
        sent += count;
    }
    packets
}

#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub online: bool,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Nzxt { channels: u8, fan_ports: u8 },
    AsusAura,
}

struct OpenHidController {
    id: String,
    name: String,
    family: Family,
    dev: hidapi::HidDevice,
    last_fan_rpms: Vec<(u16, u8)>,
    last_error: Option<String>,
}

pub struct HidOutputManager {
    controllers: Vec<OpenHidController>,
    last_sent: HashMap<(String, u8), (Vec<[u8; 3]>, Instant)>,
    last_duty: Option<u8>,
}

impl HidOutputManager {
    pub fn new() -> Self {
        Self {
            controllers: Vec::new(),
            last_sent: HashMap::new(),
            last_duty: None,
        }
    }

    pub fn rescan(&mut self) -> Vec<DiscoveredDevice> {
        let Ok(api) = hidapi::HidApi::new() else {
            return Vec::new();
        };

        let mut existing: HashMap<String, OpenHidController> = self
            .controllers
            .drain(..)
            .filter(|c| c.last_error.is_none())
            .map(|c| (c.id.clone(), c))
            .collect();

        let mut discovered = Vec::new();

        for info in api.device_list() {
            let vid = info.vendor_id();
            let pid = info.product_id();
            let (id, default_name, family) = if vid == NZXT_VID
                && let Some((ch, fans)) = nzxt_spec(pid)
            {
                (
                    format!("nzxt-{pid:04x}"),
                    "NZXT HUE 2 Controller",
                    Family::Nzxt {
                        channels: ch,
                        fan_ports: fans,
                    },
                )
            } else if vid == ASUS_VID
                && matches!(pid, 0x19AF | 0x18F3 | 0x1939 | 0x1AA6 | 0x1BED)
                && info.interface_number() == 2
            {
                (format!("asus-{pid:04x}"), "ASUS Aura USB", Family::AsusAura)
            } else {
                continue;
            };

            let label = info
                .product_string()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(default_name)
                .to_string();

            if let Some(ctrl) = existing.remove(&id) {
                discovered.push(DiscoveredDevice {
                    id: ctrl.id.clone(),
                    name: ctrl.name.clone(),
                    kind: "USB HID".to_string(),
                    online: true,
                    note: "Active direct USB stream".to_string(),
                });
                self.controllers.push(ctrl);
                continue;
            }

            match info.open_device(&api) {
                Ok(dev) => {
                    let _ = dev.set_blocking_mode(false);
                    init_controller(&dev, family);
                    discovered.push(DiscoveredDevice {
                        id: id.clone(),
                        name: label.clone(),
                        kind: "USB HID".to_string(),
                        online: true,
                        note: "Active direct USB stream".to_string(),
                    });
                    self.controllers.push(OpenHidController {
                        id,
                        name: label,
                        family,
                        dev,
                        last_fan_rpms: Vec::new(),
                        last_error: None,
                    });
                }
                Err(e) => {
                    discovered.push(DiscoveredDevice {
                        id,
                        name: label,
                        kind: "USB HID".to_string(),
                        online: false,
                        note: format!("Busy or inaccessible ({e})"),
                    });
                }
            }
        }

        discovered
    }

    pub fn write_zone_frames(&mut self, frames: &[ZoneFrame], zones: &[ZoneConfig]) {
        if self.controllers.is_empty() {
            return;
        }
        let now = Instant::now();

        for ctrl in &mut self.controllers {
            if ctrl.last_error.is_some() {
                continue;
            }
            match ctrl.family {
                Family::Nzxt { channels, .. } => {
                    for ch in 0..channels {
                        let Some(frame) = frames.get(ch as usize) else {
                            break;
                        };
                        let order = zones
                            .get(ch as usize)
                            .map(|z| z.color_order)
                            .unwrap_or(ColorOrder::Grb);
                        let key = (ctrl.id.clone(), ch);
                        let unchanged = self.last_sent.get(&key).is_some_and(|(prev, ts)| {
                            prev == &frame.colors && now.duration_since(*ts) < KEEPALIVE_INTERVAL
                        });
                        if unchanged {
                            continue;
                        }
                        for pkt in nzxt_stream_packets(&frame.colors, ch, order) {
                            if let Err(e) = ctrl.dev.write(&pkt) {
                                ctrl.last_error = Some(e.to_string());
                                break;
                            }
                        }
                        if ctrl.last_error.is_none() {
                            self.last_sent.insert(key, (frame.colors.clone(), now));
                        }
                    }
                }
                Family::AsusAura => {
                    for (idx, frame) in frames.iter().take(3).enumerate() {
                        let order = zones
                            .get(idx)
                            .map(|z| z.color_order)
                            .unwrap_or(ColorOrder::Rgb);
                        let key = (ctrl.id.clone(), idx as u8);
                        let unchanged = self.last_sent.get(&key).is_some_and(|(prev, ts)| {
                            prev == &frame.colors && now.duration_since(*ts) < KEEPALIVE_INTERVAL
                        });
                        if unchanged {
                            continue;
                        }
                        for pkt in aura_direct_packets(idx as u8, &frame.colors, order) {
                            if let Err(e) = ctrl.dev.write(&pkt) {
                                ctrl.last_error = Some(e.to_string());
                                break;
                            }
                        }
                        if ctrl.last_error.is_none() {
                            self.last_sent.insert(key, (frame.colors.clone(), now));
                        }
                    }
                }
            }
        }
    }

    pub fn write_fan_duty(&mut self, duty: u8) {
        if self.last_duty == Some(duty) {
            return;
        }
        let mut any_sent = false;
        for ctrl in &mut self.controllers {
            if ctrl.last_error.is_some() {
                continue;
            }
            if let Family::Nzxt { fan_ports, .. } = ctrl.family
                && fan_ports > 0
            {
                let duties: Vec<(u8, u8)> = (0..fan_ports).map(|p| (p, duty)).collect();
                let pkt = nzxt_fan_packet(&duties, fan_ports);
                if let Err(e) = ctrl.dev.write(&pkt) {
                    ctrl.last_error = Some(e.to_string());
                } else {
                    any_sent = true;
                }
            }
        }
        if any_sent {
            self.last_duty = Some(duty);
        }
    }

    pub fn poll_fan_rpms(&mut self) -> Vec<(String, u16)> {
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        for ctrl in &mut self.controllers {
            if let Family::Nzxt { fan_ports, .. } = ctrl.family
                && fan_ports > 0
                && ctrl.last_error.is_none()
            {
                for _ in 0..8 {
                    match ctrl.dev.read(&mut buf) {
                        Ok(n) if n >= 45 && buf[0] == 0x67 && buf[1] == 0x02 => {
                            ctrl.last_fan_rpms = nzxt_parse_fans(&buf[..n], fan_ports as usize);
                        }
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
                for (i, &(rpm, _)) in ctrl.last_fan_rpms.iter().enumerate() {
                    if rpm > 0 {
                        out.push((format!("Fan {}", i + 1), rpm));
                    }
                }
            }
        }
        out
    }

    pub fn blackout(&mut self) {
        for ctrl in &mut self.controllers {
            if ctrl.last_error.is_some() {
                continue;
            }
            match ctrl.family {
                Family::Nzxt { channels, .. } => {
                    let dark = vec![[0u8; 3]; 40];
                    for ch in 0..channels {
                        for pkt in nzxt_stream_packets(&dark, ch, ColorOrder::Grb) {
                            let _ = ctrl.dev.write(&pkt);
                        }
                    }
                }
                Family::AsusAura => {
                    let dark = vec![[0u8; 3]; 40];
                    for ch in 0..3 {
                        for pkt in aura_direct_packets(ch, &dark, ColorOrder::Rgb) {
                            let _ = ctrl.dev.write(&pkt);
                        }
                    }
                }
            }
        }
    }
}

impl Default for HidOutputManager {
    fn default() -> Self {
        Self::new()
    }
}

fn nzxt_spec(pid: u16) -> Option<(u8, u8)> {
    match pid {
        0x2022 => Some((5, 5)),
        0x2006 | 0x200D | 0x200F | 0x2009 | 0x200E | 0x2010 => Some((2, 3)),
        0x2001 => Some((4, 0)),
        0x2002 => Some((2, 0)),
        0x2020 => Some((6, 3)),
        0x2011 | 0x2019 | 0x201F => Some((0, 3)),
        _ => None,
    }
}

fn init_controller(dev: &hidapi::HidDevice, family: Family) {
    match family {
        Family::Nzxt { fan_ports, .. } => {
            if fan_ports > 0 {
                let mut pkt1 = [0u8; 64];
                pkt1[0] = 0x60;
                pkt1[1] = 0x03;
                let _ = dev.write(&pkt1);

                let mut pkt2 = [0u8; 64];
                pkt2[..8].copy_from_slice(&[0x60, 0x02, 0x01, 0xE8, 3, 0x01, 0xE8, 3]);
                let _ = dev.write(&pkt2);
            }
        }
        Family::AsusAura => {
            let mut pkt_init = [0u8; 65];
            pkt_init[..5].copy_from_slice(&[0xEC, 0x52, 0x53, 0x00, 0x01]);
            let _ = dev.write(&pkt_init);
            for ch in 0..=4u8 {
                let mut pkt_dir = [0u8; 65];
                pkt_dir[..6].copy_from_slice(&[0xEC, 0x35, ch, 0x00, 0x00, 0xFF]);
                let _ = dev.write(&pkt_dir);
            }
        }
    }
}

/// Scans USB HID for supported NZXT and ASUS controllers without failing if none are attached.
pub fn scan_hid_devices() -> Vec<DiscoveredDevice> {
    let mut mgr = HidOutputManager::new();
    mgr.rescan()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nzxt_packets_reorder_grb_and_append_commit() {
        let pkts = nzxt_stream_packets(&[[255, 10, 20]], 2, ColorOrder::Grb);
        assert_eq!(pkts.len(), 2);
        assert_eq!(pkts[0][0..4], [0x22, 0x10, 1 << 2, 0x00]);
        assert_eq!(pkts[0][4..7], [10, 255, 20]);
        assert_eq!(pkts[1][0..3], [0x22, 0xA0, 1 << 2]);
    }

    #[test]
    fn nzxt_fan_packet_sets_mask_and_duties() {
        let pkt = nzxt_fan_packet(&[(0, 45), (3, 80)], 5);
        assert_eq!(pkt[0..3], [0x62, 0x01, 0b01001]);
        assert_eq!(pkt[3], 45);
        assert_eq!(pkt[6], 80);
    }

    #[test]
    fn aura_direct_packets_chunk_twenty_leds_and_apply_last() {
        let colors = vec![[10, 20, 30]; 25];
        let pkts = aura_direct_packets(1, &colors, ColorOrder::Rgb);
        assert_eq!(pkts.len(), 2);
        assert_eq!(pkts[0][0..5], [0xEC, 0x40, 0x01, 0, 20]);
        assert_eq!(pkts[1][0..5], [0xEC, 0x40, 0x81, 20, 5]);
    }
}
