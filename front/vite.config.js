import { defineConfig } from 'vite';

export default defineConfig({
    root: '.',
    publicDir: 'public',
    server: {
        port: 5173,
        host: '0.0.0.0',
        allowedHosts: ['c76d.abrdns.com'],
        hmr: {
            host: 'c76d.abrdns.com',
            clientPort: 80
        },
        watch: {
            ignored: ['../**', '**/target/**']
        },
        proxy: {
            '/api': {
                target: 'http://localhost:3000',
                changeOrigin: true,
                rewrite: (path) => path.replace(/^\/api/, '')
            },
            '/tasks': {
                target: 'http://localhost:3000',
                changeOrigin: true
            },
            '/health': {
                target: 'http://localhost:3000',
                changeOrigin: true
            }
        }
    },
    build: {
        outDir: 'dist',
        assetsDir: 'assets'
    },
    // Enable SPA fallback for client-side routing
    appType: 'spa'
});
