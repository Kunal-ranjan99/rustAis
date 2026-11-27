const path = require('path');
const fs = require('fs');
const envPath = [path.resolve(__dirname, '.env'), path.resolve(__dirname, '../.env')].find((p) => fs.existsSync(p));
require('dotenv').config(envPath ? { path: envPath } : undefined);
const express = require('express');

const bodyParser = require('body-parser');
const cors = require('cors');
const authRoutes = require('./routes/authRoutes'); // Import the auth routes
const locationRoutes = require('./routes/locationRoutes');
const schoolRoutes = require('./routes/schoolRoutes'); // Import the new route
const guardianRoutes = require('./routes/guardianRoutes'); 

const app = express();

app.use(cors());
app.use(bodyParser.json());
app.use(bodyParser.urlencoded({ extended: true }));

app.get('/health', (req, res) => {
    res.json({ status: 'ok', service: 'vltd-mobile-api', port: process.env.PORT || 3019 });
});

// Use the auth routes
app.use('/api/auth', authRoutes);
app.use('/api', locationRoutes);
// Use the new school admin login route
app.use('/api/school', schoolRoutes);
app.use('/api/guardian', guardianRoutes); 

const PORT = process.env.PORT || 3019;
app.listen(PORT, () => {
    console.log(`Server is running on port ${PORT}.`);
});
