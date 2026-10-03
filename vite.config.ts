import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Configuracion estandar de Tauri 2 + Vite.
// OJO: NO existe el paquete "@tauri-apps/vite-plugin" (404 en npm); lo habia
// puesto el modelo anterior. Tauri no necesita plugin de Vite: basta con fijar
// el puerto y decirle a Vite que no vigile src-tauri (que lo recompila cargo).
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      // src-tauri lo vigila cargo; si Vite tambien, se reinicia en bucle.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "es2022",
    sourcemap: false,
  },
});
