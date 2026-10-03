import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, headers: { 'Content-Security-Policy': "frame-src 'none'; object-src 'none'; base-uri 'self'" }, watch: { ignored: ['**/.tools/**', '**/src-tauri/**', '**/artifacts/**', '**/release/**'] } },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: { target: 'es2022' },
});
