// src/controllers/schoolController.js
const db = require('../config/db.config');
const { fetchLatestParsed } = require('../utils/rustGateway');

exports.getSchoolBuses = async (req, res) => {
    try {
        // Extract school_id from JWT token
        const schoolId = req.user.schoolId;

        // Fetch bus IMEIs and route data from the database
        const routeData = await getRouteDataFromDb(schoolId);

        // Extract just the IMEIs for the request to the external service
        const imeis = routeData.map(route => route.bus_imei);

        // Fetch device details from external service
        const deviceDetails = await getDeviceDetailsFromExternalService(imeis);

        // Enrich device details with route data from your database
        const enrichedDeviceDetails = enrichDeviceDetailsWithRouteData(deviceDetails, routeData);
        
        res.json(enrichedDeviceDetails);
    } catch (error) {
        console.error('Error fetching bus details:', error);
        res.status(500).send({ message: 'Failed to fetch bus details' });
    }
};

const getRouteDataFromDb = async (schoolId) => {
    const [rows] = await db.promise().query('SELECT * FROM route_data WHERE sch_id = ?', [schoolId]);
    return rows;
};

const getDeviceDetailsFromExternalService = async (imeis) => {
    return fetchLatestParsed(imeis);
};

const enrichDeviceDetailsWithRouteData = (deviceDetails, routeData) => {
    const deviceDetailsMap = new Map(deviceDetails.map(detail => [detail.imei, detail]));

    let entriesWithDetails = [];
    let entriesWithoutDetails = [];

    routeData.forEach(route => {
        const detail = deviceDetailsMap.get(route.bus_imei);
        const deviceDetailAvailable = detail && !detail.deviceDetailNotFound;

        const entry = {
            imei: route.bus_imei,
            routeName: route.route_name,
            deviceDetails: deviceDetailAvailable ? {
                firmware: detail.firmware,
                alert_id: detail.alert_id,
                packet_status: detail.packet_status,
                vehicle_number: detail.vehicle_number,
                // Include other device-specific details as needed
            } : 'Details Not Found',
            locationDetails: deviceDetailAvailable ? {
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
                // Include other location-specific details as needed
            } : 'Details Not Found'
        };

        if (deviceDetailAvailable) {
            entriesWithDetails.push(entry);
        } else {
            entriesWithoutDetails.push(entry);
        }
    });

    // Concatenating entries with details first, followed by entries without details
    const sortedEntries = [...entriesWithDetails, ...entriesWithoutDetails];

    return sortedEntries;
};



