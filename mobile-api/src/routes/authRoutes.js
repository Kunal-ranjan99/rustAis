// src/routes/authRoutes.js
const express = require('express');
const router = express.Router();
const verifyToken = require('../middleware/verifyToken');
const authController = require('../controllers/authController');
const deviceController = require('../controllers/deviceController'); 

router.post('/login', authController.login);

// Add this new route
router.post('/device-details', verifyToken, deviceController.getDeviceDetails);

module.exports = router;