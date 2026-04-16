import path from 'path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [tailwindcss(), react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    exclude: ['e2e/**', 'node_modules/**'],
  },
  server: {
    // Browser connects directly to daemon WebSocket URLs — no proxy needed.
    // Default daemon: ws://localhost:9111/ws
    watch: {
      usePolling: true,
    },
  },
})
