// src/routes/guardianRoutes.js
const express = require('express');
const router = express.Router();
const guardianController = require('../controllers/guardianController');
const verifyToken = require('../middleware/verifyToken');

router.post('/login', guardianController.guardianLogin);
router.get('/bus-details', verifyToken, guardianController.getGuardianBusDetails); // New route

module.exports = router;
