// src/config/db.config.js
const path = require('path');
const fs = require('fs');

const envCandidates = [
  path.resolve(__dirname, '../.env'),
  path.resolve(__dirname, '../../.env'),
];
const envPath = envCandidates.find((p) => fs.existsSync(p));
if (envPath) {
  const dotenvResult = require('dotenv').config({ path: envPath });
  if (dotenvResult.error) {
    console.error('DOTENV LOAD ERROR:', dotenvResult.error);
  }
} else {
  require('dotenv').config();
}

const mysql = require('mysql2');

console.log('--- DB CONFIG CHECK ---');
console.log('HOST:', process.env.DB_HOST || '127.0.0.1');
console.log('USER:', process.env.DB_USER);
console.log('DB:', process.env.DB_DATABASE);
console.log('RUST_API_URL:', process.env.RUST_API_URL || 'http://43.205.66.193:8080');
console.log('------------------------');

const dbConn = mysql.createPool({
  connectionLimit: 10,
  host: process.env.DB_HOST || '127.0.0.1',
  user: process.env.DB_USER,
  password: process.env.DB_PASSWORD,
  database: process.env.DB_DATABASE,
});

module.exports = dbConn;
