// src/controllers/deviceController.js

const db = require("../config/db.config");
const { fetchLatestRaw } = require("../utils/rustGateway");

exports.getDeviceDetails = async (req, res) => {
  try {
    // Extract user's mobile number from JWT token (set by verifyToken middleware)
    const mobile = req.user.mobile;

    // Fetch IMEIs from the database
    const imeis = await getImeisFromDb(mobile);

    // Fetch device details from the Rust gateway external service
    const deviceDetails = await getDeviceDetailsFromExternalService(imeis);

    res.json(deviceDetails);
  } catch (error) {
    console.error("Error fetching device details:", error);
    res.status(500).send({ message: "Failed to fetch device details" });
  }
};

const getImeisFromDb = async (mobile) => {
  const [rows] = await db
    .promise()
    .query("SELECT imei FROM device_inventory WHERE owner_mobile = ?", [
      mobile,
    ]);
  return rows.map((row) => row.imei);
};

const getDeviceDetailsFromExternalService = async (imeis) => {
  return fetchLatestRaw(imeis);
};
