use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub id: u64,
    pub received_at: String,
    pub peer_addr: String,
    pub valid: bool,
    pub validation_error: Option<String>,
    pub checksum_valid: Option<bool>,
    pub imei: String,
    pub packet_status: String, // "L" (Live) or "H" (History)
    pub vendor_id: Option<String>,
    pub firmware: Option<String>,
    pub packet_type: Option<String>, // NR, EA, TA, HP, IN, IF, etc.
    pub alert_id: Option<String>,
    pub vehicle_reg: Option<String>,
    pub gps_fix: Option<bool>,
    pub date: Option<String>,
    pub time: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub speed_kmh: Option<f64>,
    pub heading: Option<f64>,
    pub satellites: Option<u8>,
    pub altitude: Option<f64>,
    pub ignition: Option<bool>,
    pub main_power: Option<bool>,
    pub battery_voltage: Option<f64>,
    pub emergency: Option<bool>,
    pub raw_packet: String,
}

/// Attempts to decode an ASCII hex-encoded stream back into a UTF-8 string
/// if it contains an AIS-140 `$PVT` header.
pub fn try_hex_to_string(input: &str) -> Option<String> {
    let clean: String = input.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() >= 8 && clean.len() % 2 == 0 {
        let bytes: Option<Vec<u8>> = (0..clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).ok())
            .collect();
        if let Some(b) = bytes {
            let s = String::from_utf8_lossy(&b).to_string();
            if s.contains("$PVT") {
                return Some(s);
            }
        }
    }
    None
}

/// Validates and parses an AIS-140 `$PVT` packet into a structured `TelemetryRecord`.
///
/// Standard AIS-140 `$PVT` comma-separated layout:
/// 0: Header ($PVT)
/// 1: Vendor ID
/// 2: Firmware Version
/// 3: Packet Type (NR, EA, TA, HP, IN, IF, etc.)
/// 4: Alert ID
/// 5: Packet Status ('L' = Live, 'H' = History)
/// 6: IMEI (15 digits)
/// 7: Vehicle Registration Number
/// 8: GPS Fix ('1' = valid, '0' = invalid)
/// 9: Date (DDMMYYYY)
/// 10: Time (HHMMSS)
/// 11: Latitude (DD.dddddd or DDMM.mmmm)
/// 12: Latitude Dir (N/S)
/// 13: Longitude (DDD.dddddd or DDDMM.mmmm)
/// 14: Longitude Dir (E/W)
/// 15: Speed (km/h)
/// 16: Heading (degrees 0-360)
/// 17: No. of Satellites
/// 18: Altitude (meters)
/// 22: Ignition ('1'/'0')
/// 23: Main Power Status ('1'/'0')
/// 24: Main Input Voltage
/// 25: Internal Battery Voltage
/// 26: Emergency Status ('1'/'0')
pub fn parse_ais140_packet(id: u64, peer_addr: &str, raw_packet: &str) -> TelemetryRecord {
    let received_at = Utc::now().to_rfc3339();
    let trimmed = raw_packet.trim();

    let (body, checksum_valid) = verify_optional_checksum(trimmed);
    let parts: Vec<&str> = body.split(',').collect();

    if parts.len() < 7 {
        return invalid_record(
            id,
            received_at,
            peer_addr,
            trimmed,
            checksum_valid,
            format!("Too few fields: expected >= 7, got {}", parts.len()),
        );
    }

    if !parts[0].trim().starts_with("$PVT") {
        return invalid_record(
            id,
            received_at,
            peer_addr,
            trimmed,
            checksum_valid,
            format!("Invalid header: {}", parts[0]),
        );
    }

    let packet_status = parts[5].trim().to_uppercase();
    let imei = parts[6].trim().to_string();

    let is_valid_imei = imei.len() == 15 && imei.chars().all(|c| c.is_ascii_digit());
    if !is_valid_imei {
        return invalid_record(
            id,
            received_at,
            peer_addr,
            trimmed,
            checksum_valid,
            format!("Invalid IMEI '{}' (must be 15 numeric digits)", imei),
        );
    }

    if packet_status != "L" && packet_status != "H" {
        return invalid_record(
            id,
            received_at,
            peer_addr,
            trimmed,
            checksum_valid,
            format!("Invalid packet status '{}' (expected L or H)", packet_status),
        );
    }

    let lat = parse_coord(parts.get(11).copied(), parts.get(12).copied(), false);
    let lon = parse_coord(parts.get(13).copied(), parts.get(14).copied(), true);

    // Validate coordinate bounds if present
    let (lat, lon) = match (lat, lon) {
        (Some(la), Some(lo)) if (-90.0..=90.0).contains(&la) && (-180.0..=180.0).contains(&lo) => {
            (Some(la), Some(lo))
        }
        _ => (None, None),
    };

    let gps_fix = parts.get(8).and_then(|v| match v.trim() {
        "1" | "A" => Some(true),
        "0" | "V" => Some(false),
        _ => None,
    });

    TelemetryRecord {
        id,
        received_at,
        peer_addr: peer_addr.to_string(),
        valid: true,
        validation_error: None,
        checksum_valid,
        imei,
        packet_status,
        vendor_id: non_empty(parts.get(1)),
        firmware: non_empty(parts.get(2)),
        packet_type: non_empty(parts.get(3)),
        alert_id: non_empty(parts.get(4)),
        vehicle_reg: non_empty(parts.get(7)),
        gps_fix,
        date: non_empty(parts.get(9)),
        time: non_empty(parts.get(10)),
        lat,
        lon,
        speed_kmh: parse_f64(parts.get(15)),
        heading: parse_f64(parts.get(16)),
        satellites: parts.get(17).and_then(|v| v.trim().parse::<u8>().ok()),
        altitude: parse_f64(parts.get(18)),
        ignition: parse_flag(parts.get(22)),
        main_power: parse_flag(parts.get(23)),
        battery_voltage: parse_f64(parts.get(25)).or_else(|| parse_f64(parts.get(24))),
        emergency: parse_flag(parts.get(26)),
        raw_packet: trimmed.to_string(),
    }
}

fn invalid_record(
    id: u64,
    received_at: String,
    peer_addr: &str,
    raw_packet: &str,
    checksum_valid: Option<bool>,
    err: String,
) -> TelemetryRecord {
    TelemetryRecord {
        id,
        received_at,
        peer_addr: peer_addr.to_string(),
        valid: false,
        validation_error: Some(err),
        checksum_valid,
        imei: "UNKNOWN".to_string(),
        packet_status: "?".to_string(),
        vendor_id: None,
        firmware: None,
        packet_type: None,
        alert_id: None,
        vehicle_reg: None,
        gps_fix: None,
        date: None,
        time: None,
        lat: None,
        lon: None,
        speed_kmh: None,
        heading: None,
        satellites: None,
        altitude: None,
        ignition: None,
        main_power: None,
        battery_voltage: None,
        emergency: None,
        raw_packet: raw_packet.to_string(),
    }
}

/// Checks optional `*HH` NMEA XOR checksum (between `$` and `*`).
/// Returns `(body_before_star, Option<bool>)`.
fn verify_optional_checksum(packet: &str) -> (&str, Option<bool>) {
    if let Some(star_idx) = packet.rfind('*') {
        let body = &packet[..star_idx];
        let chk_str = packet[star_idx + 1..].trim();
        if chk_str.len() == 2 {
            if let Ok(expected) = u8::from_str_radix(chk_str, 16) {
                let payload = body.strip_prefix('$').unwrap_or(body);
                let computed = payload.bytes().fold(0u8, |acc, b| acc ^ b);
                return (body, Some(computed == expected));
            }
        }
        return (body, None);
    }
    (packet, None)
}

/// Parses decimal degrees (`28.6139`) OR NMEA `DDMM.mmmm` (`2836.8340`) with N/S/E/W direction.
fn parse_coord(val: Option<&str>, dir: Option<&str>, is_lon: bool) -> Option<f64> {
    let raw_str = val?.trim();
    if raw_str.is_empty() {
        return None;
    }
    let raw: f64 = raw_str.parse().ok()?;
    if raw == 0.0 {
        return None;
    }

    // Heuristic: values > 180 (lon) or > 90 (lat) are in NMEA DDMM.mmmm format
    let max_deg = if is_lon { 180.0 } else { 90.0 };
    let mut deg = if raw.abs() > max_deg {
        let d = (raw / 100.0).trunc();
        let m = raw - (d * 100.0);
        d + (m / 60.0)
    } else {
        raw
    };

    if let Some(d_str) = dir {
        match d_str.trim().to_uppercase().as_str() {
            "S" | "W" => deg = -deg.abs(),
            "N" | "E" => deg = deg.abs(),
            _ => {}
        }
    }

    Some((deg * 1_000_000.0).round() / 1_000_000.0)
}

fn non_empty(val: Option<&&str>) -> Option<String> {
    val.map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn parse_f64(val: Option<&&str>) -> Option<f64> {
    val.and_then(|s| s.trim().parse::<f64>().ok())
}

fn parse_flag(val: Option<&&str>) -> Option<bool> {
    val.and_then(|s| match s.trim() {
        "1" | "true" | "ON" => Some(true),
        "0" | "false" | "OFF" => Some(false),
        _ => None,
    })
}
