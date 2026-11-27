module.exports = {
  apps: [
    {
      name: 'vltd-mobile-api',
      script: 'src/app.js',
      instances: 1,
      exec_mode: 'fork',
      node_args: '--max-old-space-size=256',
      max_memory_restart: '300M',
      env: {
        NODE_ENV: 'production',
      },
    },
  ],
};
