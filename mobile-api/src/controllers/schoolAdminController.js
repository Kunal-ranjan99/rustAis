// src/controllers/schoolAdminController.js

const db = require('../config/db.config').promise();
const jwt = require('jsonwebtoken');

exports.schoolAdminLogin = async (req, res) => {
    const { superviser_mobile, superviser_pwd } = req.body;

    if (!superviser_mobile || !superviser_pwd) {
        return res.status(400).send({ message: "Mobile and password are required." });
    }

    try {
        const [admins] = await db.query("SELECT * FROM school_data WHERE superviser_mobile = ?", [superviser_mobile]);
        const admin = admins[0];

        if (!admin) {
            return res.status(404).send({ message: "Admin not found." });
        }

        if (admin.superviser_pwd !== superviser_pwd) {
            return res.status(401).send({ message: "Invalid password." });
        }

        // Generate JWT
        const token = jwt.sign(
            { schoolId: admin.school_id, superviserMobile: admin.superviser_mobile },
            process.env.JWT_SECRET,
            { expiresIn: '720h' }
        );

        res.send({ token, schoolId: admin.school_id });
    } catch (error) {
        console.error('Login error:', error);
        res.status(500).send({ message: "An error occurred during login." });
    }
};
