import { resolve } from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri oczekuje stałego portu; na telefonie dev serwer musi słuchać na adresie hosta (TAURI_DEV_HOST)
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  root: resolve('src/renderer'),
  plugins: [react()],
  resolve: { alias: { '@shared': resolve('src/shared') } },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined
  },
  build: { outDir: resolve('dist'), emptyOutDir: true, target: 'safari15' }
})
