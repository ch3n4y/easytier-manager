import {defineConfig} from 'vite'
import react from '@vitejs/plugin-react'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  // Tauri owns the terminal; don't let Vite wipe its output.
  clearScreen: false,
  server: {
    // `tauri android dev` reaches the dev server over the LAN, so it has to
    // listen on every interface rather than loopback only. Production builds
    // do not use this server at all.
    host: '0.0.0.0',
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
})
