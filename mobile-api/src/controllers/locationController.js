// src/controllers/locationControllers.js

const db = require('../config/db.config');

exports.getLocationsForImei = async (req, res) => {
    const imei = req.params.imei;
    const today = new Date();
    today.setHours(0, 0, 0, 0); // Set to midnight

    // Convert 'today' to Unix timestamp (seconds since the Unix Epoch)
    const startOfDayTimestamp = Math.floor(today.getTime() / 1000);

    try {
        const query = `
            SELECT vehicle_number, lat, lng, speed, heading, egnition, datatimeall as dtime
            FROM lod_data_seprate
            WHERE imei = ? AND datatimeall >= ?
            ORDER BY datatimeall ASC
        `;
        const [rows] = await db.promise().query(query, [imei, startOfDayTimestamp]);
        res.json(rows);
    } catch (error) {
        console.error('Error fetching location data:', error);
        res.status(500).send({ message: 'Failed to fetch location data' });
    }
};
