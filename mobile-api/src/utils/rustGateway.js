// src/utils/rustGateway.js
const axios = require("axios");

const RUST_API_URL = (
  process.env.RUST_API_URL || "http://43.205.66.193:8080"
).replace(/\/+$/, "");

function parseCoord(val, dir, isLon) {
  if (!val) return null;
  const raw = parseFloat(String(val).trim());
  if (Number.isNaN(raw) || raw === 0) return null;
  const maxDeg = isLon ? 180 : 90;
  let deg =
    Math.abs(raw) > maxDeg
      ? Math.trunc(raw / 100) + (raw - Math.trunc(raw / 100) * 100) / 60
      : raw;
  const d = String(dir || "").trim().toUpperCase();
  if (d === "S" || d === "W") deg = -Math.abs(deg);
  else if (d === "N" || d === "E") deg = Math.abs(deg);
  return Number(deg.toFixed(6));
}

/**
 * Parses a raw AIS-140 `$PVT,...` packet string into the structured object
 * expected by guardianController & schoolController, while preserving `packet`.
 */
function parsePvtPacket(imei, rawPacket) {
  if (!rawPacket || typeof rawPacket !== "string") {
    return { imei, packet: null, deviceDetailNotFound: true };
  }
  const clean = rawPacket.split("*")[0].trim();
  const p = clean.split(",");
  if (p.length < 7) {
    return { imei, packet: rawPacket, deviceDetailNotFound: true };
  }

  return {
    imei,
    packet: rawPacket,
    firmware: (p[2] || "").trim(),
    packet_type: (p[3] || "").trim(),
    alert_id: (p[4] || "").trim(),
    packet_status: (p[5] || "").trim(),
    vehicle_number: (p[7] || "").trim(),
    gps_fix: (p[8] || "").trim(),
    date: (p[9] || "").trim(),
    time: (p[10] || "").trim(),
    lat: parseCoord(p[11], p[12], false),
    lng: parseCoord(p[13], p[14], true),
    speed: p[15] !== undefined && p[15] !== "" ? parseFloat(p[15]) : 0,
    heading: p[16] !== undefined && p[16] !== "" ? parseFloat(p[16]) : 0,
    no_satellites: p[17] !== undefined && p[17] !== "" ? parseInt(p[17], 10) : 0,
    altitude: p[18] !== undefined && p[18] !== "" ? parseFloat(p[18]) : 0,
    egnition: (p[22] || "0").trim(),
    emergency: (p[26] || "0").trim(),
    timestamp: `${(p[9] || "").trim()} ${(p[10] || "").trim()}`.trim(),
    commAdd: "",
  };
}

/**
 * Fetches raw `[{ imei, packet }]` from Rust gateway `/devices/latest`.
 */
async function fetchLatestRaw(imeis) {
  if (!imeis || imeis.length === 0) return [];
  const imeiQuery = imeis.map((i) => String(i).trim()).filter(Boolean).join(",");
  if (!imeiQuery) return [];

  const response = await axios.get(`${RUST_API_URL}/devices/latest`, {
    params: { imei: imeiQuery },
    timeout: 5000,
  });
  return response.data;
}

/**
 * Fetches latest packets from Rust gateway and returns parsed telemetry fields
 * (used by guardianController & schoolController).
 */
async function fetchLatestParsed(imeis) {
  const rows = await fetchLatestRaw(imeis);
  return rows.map((row) => parsePvtPacket(row.imei, row.packet));
}

module.exports = {
  RUST_API_URL,
  fetchLatestRaw,
  fetchLatestParsed,
  parsePvtPacket,
};
