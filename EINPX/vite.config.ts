import { resolve } from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Port 1424: ZIM 1420, PFMX 1421, CTHX 1422
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  root: resolve('src/renderer'),
  plugins: [react()],
  resolve: { alias: { '@shared': resolve('src/shared') } },
  clearScreen: false,
  server: {
    port: 1424,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1425 } : undefined
  },
  build: { outDir: resolve('dist'), emptyOutDir: true, target: 'safari15' }
})
