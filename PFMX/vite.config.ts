import { resolve } from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri oczekuje stałego portu. PFMX ma własny (1421), żeby nie kolidował z ZIM (1420).
export default defineConfig({
  root: resolve('src/renderer'),
  plugins: [react()],
  resolve: { alias: { '@shared': resolve('src/shared') } },
  clearScreen: false,
  server: { port: 1421, strictPort: true },
  build: { outDir: resolve('dist'), emptyOutDir: true, target: 'safari15' }
})
