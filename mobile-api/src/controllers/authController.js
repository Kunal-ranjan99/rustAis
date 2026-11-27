// src/controllers/authController.js
const db = require('../config/db.config').promise();
const jwt = require('jsonwebtoken');

exports.login = async (req, res) => {
    const { mobile, password } = req.body;

    if (!mobile || !password) {
        return res.status(400).send({ message: "Mobile and password are required." });
    }

    try {
        // Fetch user by mobile
        const [users] = await db.query("SELECT * FROM users WHERE mobile = ?", [mobile]);
        const user = users[0];

        if (!user) {
            return res.status(404).send({ message: "User not found." });
        }

        // Password verification
        if (user.password !== password) {
            return res.status(401).send({ message: "Invalid password." });
        }

        // Generate JWT
        const token = jwt.sign(
            { userId: user.id, mobile: user.mobile },
            process.env.JWT_SECRET, // The secret key for signing the token
            { expiresIn: '720h' } // Token expiration time
        );

        // Fetch IMEIs associated with the mobile number from device_inventory table
        const [deviceInventory] = await db.query("SELECT imei FROM device_inventory WHERE owner_mobile = ?", [user.mobile]);
        const imeis = deviceInventory.map(device => device.imei);

        // Send response with token, mobile, and list of IMEIs
        res.send({ token, mobile: user.mobile, imeis });
    } catch (error) {
        console.error('Login error:', error);
        res.status(500).send({ message: "An error occurred during login." });
    }
};
