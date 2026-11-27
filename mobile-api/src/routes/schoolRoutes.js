// src/routes/schoolRoutes.js

const express = require('express');
const router = require('express').Router();
const verifyToken = require('../middleware/verifyToken');
const schoolAdminController = require('../controllers/schoolAdminController');
const schoolController = require('../controllers/schoolController');

router.post('/school-admin/login', schoolAdminController.schoolAdminLogin);
router.get('/school-buses', verifyToken, schoolController.getSchoolBuses);

module.exports = router;
