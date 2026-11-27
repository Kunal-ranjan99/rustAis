// src/routes/locationRoutes.js

const express = require('express');
const router = express.Router();
const locationController = require('../controllers/locationController');
const verifyToken = require('../middleware/verifyToken'); // Assuming you want this route to be protected

router.get('/locations/:imei', verifyToken, locationController.getLocationsForImei);

module.exports = router;
