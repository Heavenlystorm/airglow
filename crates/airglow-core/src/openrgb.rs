//! OpenRGB SDK TCP client (discovery, controller metadata parsing, and UpdateLEDs streaming).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::config::ColorOrder;

const ORGB_MAGIC: &[u8; 4] = b"ORGB";
const PKT_REQUEST_CONTROLLER_COUNT: u32 = 0;
const PKT_REQUEST_CONTROLLER_DATA: u32 = 1;
const PKT_REQUEST_PROTOCOL_VERSION: u32 = 40;
const PKT_SET_CLIENT_NAME: u32 = 50;
const PKT_UPDATE_LEDS: u32 = 1050;
const PKT_SET_CUSTOM_MODE: u32 = 1100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRgbControllerInfo {
    pub index: u32,
    pub name: String,
    pub led_count: usize,
}

#[derive(Debug, Clone)]
pub struct OpenRgbStatus {
    pub connected: bool,
    pub controller_count: u32,
    pub message: String,
    pub controllers: Vec<OpenRgbControllerInfo>,
}

pub fn encode_packet(device_idx: u32, pkt_type: u32, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16 + payload.len());
    buf.extend_from_slice(ORGB_MAGIC);
    buf.extend_from_slice(&device_idx.to_le_bytes());
    buf.extend_from_slice(&pkt_type.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

pub fn encode_update_leds(colors: &[[u8; 3]], order: ColorOrder) -> Vec<u8> {
    let count = colors.len().min(u16::MAX as usize);
    let body_len = 2 + count * 4;
    let total_size = 4 + body_len as u32;
    let mut buf = Vec::with_capacity(total_size as usize);
    buf.extend_from_slice(&total_size.to_le_bytes());
    buf.extend_from_slice(&(count as u16).to_le_bytes());
    for &rgb in &colors[..count] {
        let [r, g, b] = order.reorder(rgb);
        let packed: u32 = (r as u32) | ((g as u32) << 8) | ((b as u32) << 16);
        buf.extend_from_slice(&packed.to_le_bytes());
    }
    buf
}

pub struct OpenRgbClient {
    stream: Option<TcpStream>,
    protocol: u32,
    controllers: Vec<OpenRgbControllerInfo>,
    last_sent: Vec<Vec<[u8; 3]>>,
    last_keepalive: Instant,
}

impl OpenRgbClient {
    pub fn new() -> Self {
        Self {
            stream: None,
            protocol: 3,
            controllers: Vec::new(),
            last_sent: Vec::new(),
            last_keepalive: Instant::now(),
        }
    }

    pub fn disconnect(&mut self) {
        self.stream = None;
        self.controllers.clear();
        self.last_sent.clear();
    }

    pub fn ensure_connected(&mut self, host: &str, port: u16) -> OpenRgbStatus {
        if self.stream.is_some() {
            return OpenRgbStatus {
                connected: true,
                controller_count: self.controllers.len() as u32,
                message: format!("Connected ({} controllers)", self.controllers.len()),
                controllers: self.controllers.clone(),
            };
        }

        let addr_str = format!("{host}:{port}");
        let Ok(addr) = addr_str.parse::<SocketAddr>() else {
            return OpenRgbStatus {
                connected: false,
                controller_count: 0,
                message: format!("Invalid address {addr_str}"),
                controllers: Vec::new(),
            };
        };

        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(220)) else {
            return OpenRgbStatus {
                connected: false,
                controller_count: 0,
                message: format!("OpenRGB SDK offline at {addr_str}"),
                controllers: Vec::new(),
            };
        };

        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));

        // Negotiate protocol version (ask for 3 or 5; protocol 0 servers may not answer).
        let ver_req = encode_packet(0, PKT_REQUEST_PROTOCOL_VERSION, &3u32.to_le_bytes());
        let mut protocol = 0u32;
        if stream.write_all(&ver_req).is_ok()
            && let Some((_, pkt_type, body)) = read_packet(&mut stream)
            && pkt_type == PKT_REQUEST_PROTOCOL_VERSION
            && body.len() >= 4
        {
            protocol = u32::from_le_bytes([body[0], body[1], body[2], body[3]]).min(3);
        }

        let _ = stream.write_all(&encode_packet(0, PKT_SET_CLIENT_NAME, b"Airglow\0"));
        if stream
            .write_all(&encode_packet(0, PKT_REQUEST_CONTROLLER_COUNT, &[]))
            .is_err()
        {
            return OpenRgbStatus {
                connected: false,
                controller_count: 0,
                message: "OpenRGB handshake failed".to_string(),
                controllers: Vec::new(),
            };
        }

        let count = match read_packet(&mut stream) {
            Some((_, PKT_REQUEST_CONTROLLER_COUNT, body)) if body.len() >= 4 => {
                u32::from_le_bytes([body[0], body[1], body[2], body[3]])
            }
            _ => 0,
        };

        let mut controllers = Vec::new();
        for idx in 0..count.min(32) {
            let req_body = protocol.to_le_bytes();
            if stream
                .write_all(&encode_packet(idx, PKT_REQUEST_CONTROLLER_DATA, &req_body))
                .is_err()
            {
                break;
            }
            if let Some((_, PKT_REQUEST_CONTROLLER_DATA, body)) = read_packet(&mut stream) {
                let name = parse_controller_name(&body).unwrap_or_else(|| format!("Controller {}", idx + 1));
                let _ = stream.write_all(&encode_packet(idx, PKT_SET_CUSTOM_MODE, &[]));
                controllers.push(OpenRgbControllerInfo {
                    index: idx,
                    name,
                    led_count: 0,
                });
            }
        }

        self.protocol = protocol;
        self.controllers = controllers.clone();
        self.last_sent = vec![Vec::new(); controllers.len()];
        self.stream = Some(stream);

        OpenRgbStatus {
            connected: true,
            controller_count: count,
            message: format!("Connected ({count} controllers)"),
            controllers,
        }
    }

    pub fn push_colors(&mut self, colors: &[[u8; 3]], order: ColorOrder) {
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        if self.controllers.is_empty() || colors.is_empty() {
            return;
        }
        let now = Instant::now();
        let force = now.duration_since(self.last_keepalive) >= Duration::from_secs(5);
        let mut any_failed = false;

        for (i, ctrl) in self.controllers.iter().enumerate() {
            if !force && self.last_sent.get(i).is_some_and(|prev| prev == colors) {
                continue;
            }
            let payload = encode_update_leds(colors, order);
            let pkt = encode_packet(ctrl.index, PKT_UPDATE_LEDS, &payload);
            if stream.write_all(&pkt).is_err() {
                any_failed = true;
                break;
            }
            if let Some(slot) = self.last_sent.get_mut(i) {
                *slot = colors.to_vec();
            }
        }
        if force {
            self.last_keepalive = now;
        }
        if any_failed {
            self.disconnect();
        }
    }
}

impl Default for OpenRgbClient {
    fn default() -> Self {
        Self::new()
    }
}

fn read_packet(stream: &mut TcpStream) -> Option<(u32, u32, Vec<u8>)> {
    let mut hdr = [0u8; 16];
    stream.read_exact(&mut hdr).ok()?;
    if &hdr[0..4] != ORGB_MAGIC {
        return None;
    }
    let dev_idx = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
    let pkt_type = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    let len = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]) as usize;
    if len > 4 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut body).ok()?;
    }
    Some((dev_idx, pkt_type, body))
}

fn parse_controller_name(data: &[u8]) -> Option<String> {
    // Layout: u32 data_size, i32 type, u16 name_len, [u8; name_len]
    if data.len() < 10 {
        return None;
    }
    let name_len = u16::from_le_bytes([data[8], data[9]]) as usize;
    let end = 10usize.checked_add(name_len)?;
    if end > data.len() {
        return None;
    }
    let raw = &data[10..end];
    let trimmed = raw.split(|&b| b == 0).next().unwrap_or(raw);
    let s = String::from_utf8_lossy(trimmed).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

pub fn probe_server(host: &str, port: u16) -> OpenRgbStatus {
    let mut client = OpenRgbClient::new();
    client.ensure_connected(host, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_orgb_header_little_endian() {
        let pkt = encode_packet(1, 50, b"AB");
        assert_eq!(&pkt[0..4], b"ORGB");
        assert_eq!(u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]), 1);
        assert_eq!(u32::from_le_bytes([pkt[8], pkt[9], pkt[10], pkt[11]]), 50);
        assert_eq!(u32::from_le_bytes([pkt[12], pkt[13], pkt[14], pkt[15]]), 2);
        assert_eq!(&pkt[16..], b"AB");
    }

    #[test]
    fn encodes_update_leds_with_color_order() {
        let body = encode_update_leds(&[[10, 20, 30]], ColorOrder::Grb);
        // 4 bytes total_size + 2 bytes count + 4 bytes color = 10
        assert_eq!(body.len(), 10);
        assert_eq!(u32::from_le_bytes([body[0], body[1], body[2], body[3]]), 10);
        assert_eq!(u16::from_le_bytes([body[4], body[5]]), 1);
        // GRB -> [20, 10, 30, 0]
        assert_eq!(&body[6..10], &[20, 10, 30, 0]);
    }
}
