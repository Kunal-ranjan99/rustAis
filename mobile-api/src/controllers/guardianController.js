// src/controllers/guardianController.js
const db = require("../config/db.config");
const jwt = require("jsonwebtoken");
const { fetchLatestParsed } = require("../utils/rustGateway");

exports.guardianLogin = async (req, res) => {
  const { st_mobile, st_pwd } = req.body;

  if (!st_mobile || !st_pwd) {
    return res
      .status(400)
      .send({ message: "Mobile and password are required." });
  }

  try {
    const [students] = await db
      .promise()
      .query("SELECT * FROM student_data WHERE st_mobile = ? AND st_pwd = ?", [
        st_mobile,
        st_pwd,
      ]);
    const student = students[0];

    if (!student) {
      return res.status(404).send({ message: "Student not found." });
    }

    // Generate JWT, include bus_imei in the token
    const token = jwt.sign(
      {
        studentId: student.student_id,
        st_mobile: student.st_mobile,
        bus_imei: student.bus_imei,
      },
      process.env.JWT_SECRET,
      { expiresIn: "720h" },
    );

    res.send({ token, bus_imei: student.bus_imei });
  } catch (error) {
    console.error("Login error:", error);
    res.status(500).send({ message: "An error occurred during login." });
  }
};

exports.getGuardianBusDetails = async (req, res) => {
  try {
    const { bus_imei } = req.user; // Extract bus_imei from JWT token

    const routeData = await getRouteDataFromDbByImei(bus_imei);
    const deviceDetails = await getDeviceDetailsFromExternalService([bus_imei]);
    const enrichedDeviceDetails = enrichDeviceDetailsWithRouteData(
      deviceDetails,
      routeData,
    );

    res.json(enrichedDeviceDetails);
  } catch (error) {
    console.error("Error fetching guardian bus details:", error);
    res.status(500).send({ message: "Failed to fetch guardian bus details" });
  }
};

const getRouteDataFromDbByImei = async (bus_imei) => {
  const [rows] = await db
    .promise()
    .query("SELECT * FROM route_data WHERE bus_imei = ?", [bus_imei]);
  return rows;
};

const getDeviceDetailsFromExternalService = async (imeis) => {
  return fetchLatestParsed(imeis);
};

const enrichDeviceDetailsWithRouteData = (deviceDetails, routeData) => {
  // Convert deviceDetails array to a Map for quick lookup.
  const deviceDetailsMap = new Map(
    deviceDetails.map((detail) => [detail.imei, detail]),
  );

  // Process each route to create the structured entry.
  const structuredEntries = routeData.map((route) => {
    const detail = deviceDetailsMap.get(route.bus_imei);
    const deviceDetailAvailable = !!(detail && !detail.deviceDetailNotFound);

    const entry = {
      imei: route.bus_imei,
      routeName: route.route_name,
      deviceDetails: deviceDetailAvailable
        ? {
            firmware: detail.firmware,
            alert_id: detail.alert_id,
            packet_status: detail.packet_status,
            vehicle_number: detail.vehicle_number,
            // Potentially add other device-specific details here.
          }
        : "Details Not Found",
      locationDetails: deviceDetailAvailable
        ? {
            lat: detail.lat,
            lng: detail.lng,
            speed: detail.speed,
            heading: detail.heading,
            no_satellites: detail.no_satellites,
            altitude: detail.altitude,
            egnition: detail.egnition,
            emergency: detail.emergency,
            timestamp: detail.timestamp,
            commAdd: detail.commAdd,
            // Potentially add other location-specific details here.
          }
        : "Details Not Found",
    };

    return entry;
  });

  return structuredEntries;
};
