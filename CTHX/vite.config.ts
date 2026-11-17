import { resolve } from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Port 1422: ZIM używa 1420, PFMX 1421
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  root: resolve('src/renderer'),
  plugins: [react()],
  resolve: { alias: { '@shared': resolve('src/shared') } },
  clearScreen: false,
  server: {
    port: 1422,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1423 } : undefined
  },
  build: { outDir: resolve('dist'), emptyOutDir: true, target: 'safari15' }
})
