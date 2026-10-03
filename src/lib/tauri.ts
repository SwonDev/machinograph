/**
 * Puente con el backend Rust.
 *
 * Los TIPOS de aquí son un espejo EXACTO de `src-tauri/src/types.rs` y
 * `src-tauri/src/db.rs`: si cambia un campo allí, cambia aquí. No hay
 * generación automática, así que es lo primero que hay que mirar si el panel
 * muestra "—" en algo.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/* ── Tipos (espejo de types.rs) ───────────────────────────────────────────── */

export interface Mem {
  total_mb: number;
  used_mb: number;
  free_mb: number;
  avail_mb: number;
  pct: number;
}

export interface System {
  cpu_pct: number;
  load1: number;
  load5: number;
  load15: number;
  cores: number;
  mem: Mem;
  swap: Mem;
}

export interface DiskUsage {
  total_gb: number;
  used_gb: number;
  free_gb: number;
  pct: number;
  /**
   * Punto de montaje que se está midiendo. Va al final, en el mismo orden que
   * `types.rs`, y se enseña: en Bazzite `/` es la imagen de solo lectura del
   * sistema atómico, así que lo que interesa es el sistema de ficheros del home
   * (`/var/home`) — y en un equipo con varios discos, "474 GB" sin decir de dónde
   * salen no significa nada.
   */
  mount: string;
}

export interface Gpu {
  id: number;
  name: string;
  driver: string;
  /**
   * `null` cuando no hay GPU o no se pudo leer: NO es 0 °C. Antes se declaraba
   * `number` y el backend mandaba ceros, así que una máquina sin `amd-smi`
   * enseñaba "0 °C / 0 W" como si fueran lecturas.
   */
  temp_c: number | null;
  mem_temp_c: number | null;
  power_w: number | null;
  mem_used_mb: number;
  mem_total_mb: number;
  mem_pct: number;
  util: number;
  clock_mhz: number;
  fan_rpm: number;
  fan_pct: number;
  throttle: string | null;
  /**
   * Solo se conoce el nombre, la VRAM y el driver (pasa en macOS y Windows, donde
   * el uso y la temperatura no se pueden leer sin privilegios). La vista enseña «—»
   * en esos huecos en vez de un `0 %` que afirmaría que la tarjeta está parada.
   */
  parcial: boolean;
}

export interface Mode {
  w: number;
  h: number;
  hz: number;
  flags: string;
}

export interface DisplayOutput {
  name: string;
  status: string;
  connected: boolean;
  primary: boolean;
  w: number;
  h: number;
  hz: number;
  offset_x: number;
  offset_y: number;
  modes: Mode[];
  current_flags: string;
}

export interface ServerModel {
  id: string;
  label: string;
  /**
   * Lo que publique el motor, no una constante nuestra: en llama-swap viene de
   * `status.value` de `/v1/models` y vale `unloaded` | `loading` | `loaded`. Otro
   * motor puede decir otra cosa, y la cadena vacía significa "no lo publica"
   * (NADA de "descargado"): por eso se traduce con `estadoModelo` de format.ts.
   */
  state: string;
  quant: string | null;
  size_mb: number | null;
}

export interface Server {
  id: string;
  name: string;
  kind: string;
  port: number;
  state: string; // active | stopped
  process_active: boolean;
  /** Siempre `null`: el backend no publica la versión del motor. */
  version: string | null;
  pid: number | null;
  proc_uptime_secs: number | null;
  models: ServerModel[];
  error: string | null;
}

export interface AiProc {
  pid: number;
  name: string;
  cmd: string;
  cpu_pct: number;
  mem_mb: number;
  uptime_secs: number;
  tag: string;
}

/* ── Tipos (espejo de inventario.rs) ──────────────────────────────────────── */

/**
 * Los tipos que distingue el inventario. NO son "texto" y ya está: un `mmproj`
 * es un proyector de visión, una lora es un adaptador, y en ComfyUI hay
 * checkpoints, vae, controlnet, codificadores de texto y embeddings. El tipo se
 * DEDUCE de la ubicación y del nombre del fichero —no se abre para adivinarlo—,
 * así que es fiable pero no es una inspección del contenido.
 */
export type TipoModelo =
  | "texto"
  | "vision"
  | "imagen"
  | "video"
  | "audio"
  | "embedding"
  | "adaptador"
  | "vae"
  | "codificador"
  | "control"
  | "otro";

/** Un modelo de cualquier familia que haya en disco (inventario.rs: `Modelo`). */
export interface ModeloInventario {
  ruta: string;
  nombre: string;
  tipo: TipoModelo;
  /** "GGUF" | "safetensors" | "ONNX" | "PyTorch" | "checkpoint" … */
  formato: string;
  /**
   * La cuantización del fichero ("Q8_0", "PQ2_0", "IQ4_XS", "BF16"…), o `null`.
   *
   * `null` NO es "desconocida": es que NO se puede deducir. Solo se rellena para
   * los `.gguf`, donde la cuantización va en el nombre por convención; en un
   * `.safetensors` no hay nada que leer, así que inventarse un valor sería mentir.
   * Los proyectores de visión (`mmproj`) SÍ la traen: son ficheros como cualquier
   * otro.
   */
  quant: string | null;
  tamano_bytes: number;
  /** De dónde sale el fichero: "llama.cpp" | "LM Studio" | "piper" | "Coqui TTS" | "ComfyUI"… */
  familia: string;
  /** Quién lo usa, si consta: "llama-swap / llama-server" | "LM Studio" | "ComfyUI"… */
  motor: string | null;
  /** Epoch en SEGUNDOS (o `null` si no se pudo leer). */
  modificado: number | null;
}

/** Totales de un tipo concreto (inventario.rs: `Resumen`). */
export interface ResumenInventario {
  tipo: string;
  ficheros: number;
  bytes: number;
}

/** Totales del inventario, que ya vienen dentro de la foto (inventario.rs: `Totales`). */
export interface TotalesInventario {
  ficheros: number;
  bytes: number;
  por_tipo: ResumenInventario[];
}

/** Respuesta de `inventario:listar`. */
export interface InventarioRespuesta {
  modelos: ModeloInventario[];
  resumen: ResumenInventario[];
}

export interface Snapshot {
  ts: number;
  uptime_secs: number;
  boot: number;
  system: System;
  gpu: Gpu[];
  display: DisplayOutput[];
  servers: Server[];
  ai_procs: AiProc[];
  disk: DiskUsage;
  /**
   * Todo lo que el equipo expone por `hwmon` (temperaturas, ventiladores,
   * voltajes), más los caudales de disco y red, la potencia de la CPU y la
   * frecuencia por núcleo. Viene DENTRO de la foto porque es una lectura de sysfs
   * sin privilegios: así la sección Hardware está al día sin pedir nada aparte.
   */
  hardware: HardwareSensores;
  note: string;
  /** El sistema operativo: `linux`, `macos`, `windows` u `otro`. */
  so: string;
  /** El nombre del sistema para leerlo ("Linux", "macOS", "Windows"). */
  so_nombre: string;
  /**
   * Motivo por el que la base de datos no se pudo abrir, o `null` si todo va
   * bien. Es un campo aparte de `note` porque `note` solo se pinta en la vista
   * de Pantalla, y esto tiene que verse desde cualquier sección: sin base de
   * datos no hay histórico ni ajustes, y antes el proceso moría sin decir nada.
   */
  db_error: string | null;
  /**
   * Recuento REAL de modelos en disco, de cualquier familia y tipo. Sustituye al
   * recuento viejo, que solo miraba `~/models` y decía un número falso (6
   * ficheros / 29 GB cuando en el equipo hay 20 y 45 GB).
   *
   * La LISTA de modelos no viaja aquí: sale de `inventario:listar`, que es la
   * única fuente (antes había una copia en la foto, y las dos podían discrepar).
   */
  inventario: TotalesInventario;
}





/* ── Tipos (espejo de memoria.rs) ─────────────────────────────────────────── */

/**
 * Un modelo servido AHORA, con lo que se sabe de su memoria.
 *
 * `pesos_gb` es el tamaño del fichero, leído del disco. `contexto`, `kv_quant` y
 * `ngl` salen de la línea de comandos con la que el proxy lo arrancó: es la
 * configuración REAL con la que está sirviendo, no la que diga un fichero.
 */
export interface ModeloCargado {
  id: string;
  nombre: string;
  ruta: string;
  pesos_gb: number | null;
  contexto: number | null;
  ngl: number | null;
  /** La cuantización de la caché KV (`q4_0`…), si va cuantizada. */
  kv_quant: string | null;
  flash_attention: boolean;
  /** Segundos que le quedan antes de que el proxy lo descargue solo. */
  ttl_s: number | null;
  /** Las banderas reconocidas, para enseñarlas tal cual. */
  banderas: string[];
  /** La línea de comandos completa: es la prueba de todo lo de arriba. */
  cmd: string;
}

/**
 * El reparto de la memoria de la GPU.
 *
 * `resto_gb` NO es una estimación: es VRAM en uso menos la suma de los pesos, los
 * dos medidos. Lo que hay dentro (caché KV, sobrecarga del motor y el resto de
 * programas) no lo publica ningún motor, así que va en un solo bloque y se dice.
 */
export interface MemoriaGpu {
  modelos: ModeloCargado[];
  pesos_gb: number;
  vram_usada_gb: number | null;
  vram_total_gb: number | null;
  resto_gb: number | null;
}

/* ── Tipos (espejo de descarga.rs) ─────────────────────────────────────────── */

/** En qué punto está una descarga. */
export type FaseDescarga =
  | "preparando"
  | "descargando"
  | "cerrando"
  | "terminada"
  | "cancelada"
  | "fallida";

/**
 * Una descarga de modelo, en curso o la última que hubo.
 *
 * `b_s` y `eta_s` los MIDE Machinograph comparando dos lecturas del contador de bytes:
 * `llmfit download` no publica ni la velocidad ni el tiempo que queda, así que no
 * se copian de ningún sitio. Hasta que hay dos lecturas valen `null`.
 */
export interface Descarga {
  fase: FaseDescarga;
  modelo: string;
  /** La última línea que dijo llmfit, tal cual. */
  linea: string;
  pct: number | null;
  descargado_gb: number | null;
  total_gb: number | null;
  b_s: number | null;
  eta_s: number | null;
  carpeta: string | null;
  error: string | null;
}

/* ── Tipos (espejo de entorno.rs) ─────────────────────────────────────────── */

/** Una interfaz de red con su IPv4, para poder elegir en cuál escuchar. */
export interface InterfazRed {
  nombre: string;
  ip: string;
}

/** De dónde saca la puerta su dirección para que la alcancen otros equipos. */
export interface EntornoRed {
  /** La IPv4 con la que este equipo sale a la red, o `null` si no hay salida. */
  ip: string | null;
  interfaces: InterfazRed[];
}

/** El arranque automático del escritorio (un `.desktop` en ~/.config/autostart). */
export interface ArranqueAuto {
  activado: boolean;
  /** El fichero o la clave donde vive (no siempre es un fichero: en Windows es una clave del registro). */
  fichero: string;
  /** El comando que lanzaría, leído del `Exec=` del fichero. */
  comando: string | null;
  /**
   * Motivo por el que NO se pudo comprobar el estado. Cuando viene, `activado` no
   * significa nada: la interfaz tiene que decir «sin comprobar» y el motivo, no
   * «desactivado» (que sería afirmar algo que no se sabe).
   */
  error?: string | null;
}

/** Una carpeta que el inventario recorre de verdad. */
export interface CarpetaModelos {
  ruta: string;
  familia: string;
  existe: boolean;
}

/* ── Tipos (espejo de sensores.rs) ─────────────────────────────────────────── */

/** De qué clase es una medida de `hwmon`. Decide la unidad y cómo se pinta. */
export type ClaseSensor =
  | "temperatura"
  | "ventilador"
  | "voltaje"
  | "potencia"
  | "corriente"
  | "energia";

/** Una medida de un sensor, con de dónde sale. */
export interface Sensor {
  /** El chip que lo publica, como lo llama sysfs (`nct6683`, `k10temp`…). */
  chip: string;
  /** El nombre legible del chip, cuando se conoce. */
  chip_legible: string;
  /** El nombre que da el chip ("CPU Fan", "Tctl", "+12V") o su identificador. */
  etiqueta: string;
  clase: ClaseSensor;
  valor: number;
  unidad: string;
  /** Umbral de aviso, solo si está POR ENCIMA de la lectura (si no, engañaría). */
  max: number | null;
  /** Umbral crítico, con la misma regla. */
  critico: number | null;
  /** La ruta sysfs de la que sale: permite comprobarlo con `cat`. */
  fuente: string;
}

export interface GrupoSensores {
  chip: string;
  chip_legible: string;
  /** El driver del kernel que lo publica (`nct6687`, `nct6775`…). */
  driver: string | null;
  items: Sensor[];
  /** Sensores desconectados que no se enseñan (leen 0 exactos). */
  descartados: number;
  /** Todos sus ventiladores a 0 mientras otro chip sí ve ventiladores girando. */
  ventiladores_a_cero: boolean;
}

export interface TempDisco {
  nombre: string;
  temp_c: number | null;
  /** "hwmon (nvme)", "smartctl"… De dónde sale el número. */
  fuente: string;
}

export interface CaudalDisco {
  nombre: string;
  leer_b_s: number;
  escribir_b_s: number;
}

export interface CaudalRed {
  nombre: string;
  rx_b_s: number;
  tx_b_s: number;
  activa: boolean;
}

export interface FrecuenciaCpu {
  actual_mhz: number | null;
  media_mhz: number | null;
  min_mhz: number | null;
  max_mhz: number | null;
  nucleos: number;
}

/** Todo lo del equipo que no cuentan `system.rs` ni `gpu.rs`. */
export interface HardwareSensores {
  grupos: GrupoSensores[];
  descartados: number;
  /** Potencia del paquete de CPU. `null` en la primera lectura (no hay delta). */
  cpu_potencia_w: number | null;
  /**
   * De dónde sale `cpu_potencia_w`, o `null` si este sistema no publica ningún
   * contador de energía de la CPU.
   *
   * Existe porque `cpu_potencia_w === null` significa DOS cosas: «es la primera
   * lectura, espera» y «esta máquina no tiene contador» (macOS y Windows: el SMC
   * pide root y ACPI/WMI, elevación). Sin este campo la tarjeta decía «se mide en
   * la siguiente lectura» para siempre, que es falso.
   */
  cpu_potencia_fuente: string | null;
  cpu_frecuencia: FrecuenciaCpu;
  discos_temp: TempDisco[];
  discos_caudal: CaudalDisco[];
  red: CaudalRed[];
}

/* ── Tipos (espejo de gateway.rs / db.rs: uso) ─────────────────────────────── */

/**
 * Cuánto se ha servido en un periodo.
 *
 * Todas las cifras pueden ser 0 y eso es un dato; lo que NO se hace es inventar.
 * `con_tokens` dice cuántas peticiones publicaron uso: si es menor que
 * `peticiones`, los tokens suman solo de unas y la interfaz tiene que decirlo.
 */
export interface UsoResumen {
  peticiones: number;
  con_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  con_ttft: number;
  ttft_medio_ms: number | null;
  tok_s: number | null;
}

/** Un día de actividad (para la gráfica). El día es LOCAL, no UTC. */
export interface UsoDia {
  dia: string;
  peticiones: number;
  prompt_tokens: number;
  completion_tokens: number;
}

export interface UsoModelo {
  modelo: string;
  peticiones: number;
  prompt_tokens: number;
  completion_tokens: number;
}

/** Una petición registrada por la puerta de enlace. */
export interface UsoFila {
  ts: number;
  modelo: string;
  ruta: string;
  metodo: string;
  estado: number;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  cached_tokens: number | null;
  ttft_ms: number | null;
  generacion_ms: number | null;
  duracion_ms: number;
  bytes_entrada: number;
  bytes_salida: number;
  /** "local" (loopback) o "red". */
  origen: string;
  /** El User-Agent tal cual: dice QUÉ herramienta llamó sin inventar un catálogo. */
  cliente: string;
}

/**
 * La puerta de enlace: lo que va a valer (configuración guardada), no lo que hay
 * en marcha ahora mismo. `error` es el fallo del último intento de arranque, que
 * se enseña en vez de decir "activa" y que el puerto no conteste.
 */
export interface GatewayEstado {
  activa: boolean;
  direccion: string;
  /** El puerto CONFIGURADO (lo que edita Ajustes; se aplica al reiniciar). */
  puerto: number;
  /**
   * El puerto REAL en el que está escuchando la puerta, o `null` si no hay
   * constancia de que escuche. Puede NO ser el configurado: si el configurado
   * estaba ocupado, la puerta prueba los siguientes y arranca en el primero libre.
   */
  puerto_escuchando: number | null;
  /**
   * El aviso de que el puerto configurado estaba ocupado y se escucha en otro
   * (`null` si no pasó). Se enseña junto a la URL: mandar a los clientes al puerto
   * configurado cuando la puerta escucha en otro sería indicarles donde no hay nada.
   */
  aviso_puerto: string | null;
  destino: string;
  requiere_clave: boolean;
  clave: string;
  url: string;
  error: string | null;
}

export interface UsoRespuesta {
  periodo: "hoy" | "todo";
  desde: number;
  resumen: UsoResumen;
  por_modelo: UsoModelo[];
  diario: UsoDia[];
  recientes: UsoFila[];
  retencion_dias: number;
  config: GatewayEstado;
}

/* ── Tipos (espejo de diagnostico.rs) ─────────────────────────────────────── */

/**
 * El estado de una comprobación, tal cual lo serializa el backend
 * (`#[serde(rename_all = "lowercase")]` sobre `diagnostico::Estado`).
 *
 * `Desconocido` NO es un problema ni un aprobado: es "no se ha podido
 * comprobar", y la interfaz lo enseña como tal en vez de contarlo como bueno.
 */
export type EstadoDiagnostico = "ok" | "aviso" | "problema" | "desconocido";

/** Una comprobación de salud del entorno (`diagnostico::Comprobacion`). */
export interface Comprobacion {
  id: string;
  titulo: string;
  estado: EstadoDiagnostico;
  /** Qué se ha encontrado, en una frase ya redactada por el backend. */
  detalle: string;
  /** Qué hacer si hay algo que hacer; `null` cuando no hay nada que arreglar. */
  como_arreglarlo: string | null;
}

/* ── Tipos (espejo de salud.rs: autorreparación) ──────────────────────────── */

/**
 * El estado de una comprobación de autorreparación
 * (`#[serde(rename_all = "snake_case")]` sobre `salud::EstadoSalud`).
 *
 * `reparado` no es un aprobado: dice que ALGO ESTABA ROTO y se ha arreglado, y por
 * eso siempre trae en `detalle` qué se hizo (con las rutas). `no_se_pudo` es un
 * problema real sin arreglar, nunca un «bien» falso.
 */
export type EstadoSalud = "correcto" | "reparado" | "no_se_pudo";

/** Una fila de la autorreparación: una cosa que la app se mira a sí misma. */
export interface ComprobacionSalud {
  id: string;
  titulo: string;
  estado: EstadoSalud;
  /** Qué se encontró y, si se reparó, qué se hizo exactamente (con rutas). */
  detalle: string;
  /** Cuando no se pudo arreglar: qué haría falta. */
  como_arreglarlo: string | null;
}

/** El resultado de una pasada de autorreparación (`salud::Revision`). */
export interface RevisionSalud {
  /** El resumen de una línea: «todo correcto» o «N reparadas / M sin arreglar». */
  resumen: string;
  reparadas: number;
  sin_arreglar: number;
  comprobaciones: ComprobacionSalud[];
}

/* ── Tipos (espejo de conexiones.rs) ──────────────────────────────────────── */

/**
 * Un cliente de IA que hay en este equipo (`conexiones::Cliente`).
 *
 * Frontera: Machinograph solo escribe donde el formato está COMPROBADO (hoy, solo
 * gentle-shell), y siempre con copia de seguridad y verificación. En los demás
 * no se toca nada: se enseña lo detectado y `como_lo_tiene` son las líneas
 * REALES de su fichero.
 */
export interface ClienteConexion {
  id: string;
  nombre: string;
  /** Ruta de su configuración, exista o no. */
  config: string;
  existe: boolean;
  /** Parece que ya apunta a un endpoint de esta máquina. */
  apunta_local: boolean;
  /** Líneas del fichero que mencionan un endpoint local, tal cual están. */
  como_lo_tiene: string[];
  /**
   * Se puede proponer Y escribir en él (formato comprobado). Es una sola
   * condición a propósito: de un cliente cuyo formato no se conoce no se puede
   * ni proponer el bloque.
   */
  admite_escritura: boolean;
  /**
   * Los modelos que SU configuración ya declara, leídos de su fichero.
   *
   * Sirve para que lo que el formulario propone POR DEFECTO sea «lo que ya
   * tienes + lo que publica el servidor»: proponer solo lo del servidor hacía
   * que aplicar sin tocar nada quitara de la configuración los modelos que el
   * servidor no anuncia (pasó de verdad, con uno de este equipo).
   */
  modelos_declarados: string[];
  nota: string;
}

/**
 * Lo que se escribiría, para revisarlo antes (`conexiones::Propuesta`).
 *
 * `contenido` es el fichero COMPLETO como quedaría, no un trozo suelto: así se
 * revisa exactamente lo que se va a escribir y no hay que decidir dónde
 * encajarlo. Lo que se enseña aquí es byte a byte lo que escribe `aplicar`.
 */
export interface PropuestaConexion {
  cliente: string;
  destino: string;
  formato: string;
  contenido: string;
  resumen: string;
  /**
   * PATRÓN del nombre de la copia de seguridad (lleva la fecha y la hora), no
   * una ruta que exista ya: se crea al aplicar.
   */
  copia_patron: string | null;
}

/** El resultado de aplicar de verdad (`conexiones::Aplicado`). */
export interface AplicadoConexion {
  cliente: string;
  destino: string;
  /** Copia del fichero tal y como estaba antes de tocarlo. */
  copia: string;
  /** Comprobado DESPUÉS de escribir, releyendo el fichero del cliente. */
  apunta_local: boolean;
  resumen: string;
}

/** Argumentos de `conexiones:propuesta` y `conexiones:aplicar`. */
export interface ArgsPropuesta {
  cliente: string;
  id: string;
  nombre: string;
  endpoint: string;
  api: string;
  modelos: string[];
}

/* ── Tipos (espejo de db.rs) ──────────────────────────────────────────────── */

export interface ServerRow {
  id: string;
  name: string;
  kind: string;
  port: number;
  cmd: string | null;
  enabled: boolean;
}

export interface ActionRow {
  ts: number;
  kind: string;
  detail: string;
  ok: boolean;
  message: string;
}

export interface UpdateRow {
  ts: number;
  component: string;
  cmd: string;
  code: number | null;
  ok: boolean;
  output: string;
}

/**
 * Un ajuste que el backend LEE de verdad (ver `db::AJUSTES`).
 *
 * `valor` es el efectivo ahora mismo: lo guardado si es válido y si no el de
 * por defecto. Se devuelven los dos, y `guardado`, porque la interfaz tiene que
 * poder decir "está así porque tú lo pusiste" y no "está así de fábrica".
 */
export interface Ajuste {
  valor: number;
  por_defecto: number;
  min: number;
  max: number;
  descripcion: string;
  guardado: boolean;
  /** Un ajuste de sí/no (0 o 1): la interfaz lo pinta como casilla, no como número. */
  booleano: boolean;
}

/** Los ajustes conocidos, por clave (`snapshot_interval_ms`, `metric_retention_hours`). */
export type Ajustes = Record<string, Ajuste>;

export interface MetricRow {
  ts: number;
  cpu: number;
  mem: number;
  disk: number;
  gpu_mem_used: number | null;
  gpu_mem_total: number | null;
  gpu_temp: number | null;
  gpu_power: number | null;
}

/**
 * Un runtime de llama.cpp detectado en el equipo (espejo de `perf::Runtime`).
 *
 * Ojo: esto NO dice qué modelos sabe leer cada uno. Solo hay nombre y ruta, así
 * que la interfaz no puede afirmar cuál es el fork y cuál el oficial: lo único
 * deducible es lo que diga el nombre del directorio.
 */
export interface RuntimeLlama {
  /** Nombre corto para la interfaz: el del directorio, no una etiqueta nuestra. */
  nombre: string;
  dir: string;
  /** Ruta al ejecutable `llama-fit-params`, o `null` si este runtime no lo trae. */
  fit: string | null;
  /** Ruta al ejecutable `llama-bench`, o `null` si este runtime no lo trae. */
  bench: string | null;
}

/** Una medida de `llama-bench` guardada en SQLite (espejo de `db::BenchRow`). */
export interface BenchRow {
  ts: number;
  modelo: string;
  runtime: string;
  /** `"prefill"` o `"decode"` (el segundo se enseña como "generación"). */
  tipo: string;
  n_prompt: number;
  n_gen: number;
  tok_s: number;
  desviacion: number;
  build: string;
  gpu: string;
}

/* ── Tipos (espejo de provision.rs) ───────────────────────────────────────── */

/**
 * En qué punto está una herramienta que la app usa.
 *
 * `roto` NO es «no está»: el fichero está pero no arranca (truncado, sin sus
 * bibliotecas). Es justo el caso que se vuelve a bajar solo.
 */
export type EstadoHerramienta =
  | "listo"
  | "falta"
  | "descargando"
  | "roto"
  | "noinstalable";

/** Una herramienta del catálogo de Machinograph (espejo de `provision::EstadoHerramienta`). */
export interface HerramientaProvision {
  id: string;
  nombre: string;
  /** Para qué la usa la app. Sin esto, una lista de binarios no dice nada. */
  para_que: string;
  imprescindible: boolean;
  /** `false` = no se puede instalar sola: se enseña motivo y comando. */
  instalable: boolean;
  estado: EstadoHerramienta;
  /** De dónde sale: la URL del proyecto o el paquete del sistema. */
  origen: string;
  ruta: string | null;
  version: string | null;
  motivo_manual: string | null;
  comando_manual: string | null;
  detalle: string | null;
}

/** Fase de una instalación (espejo de `provision::Fase`). */
export type FaseProvision =
  | "preparando"
  | "descargando"
  | "verificando"
  | "extrayendo"
  | "terminada"
  | "cancelada"
  | "fallida";

/** Lo que se está instalando (espejo de `provision::Progreso`). */
export interface ProgresoProvision {
  herramienta: string;
  fase: FaseProvision;
  linea: string;
  pct: number | null;
  bajado_bytes: number;
  total_bytes: number;
  /** Medidos por la app, no publicados por nadie. */
  b_s: number | null;
  eta_s: number | null;
  error: string | null;
}

/** El aviso de `ai:provision` (espejo de `provision::Aviso`). */
export interface AvisoProvision {
  en_curso: ProgresoProvision | null;
  ultimo: ProgresoProvision | null;
}

/** Respuesta de `provision:estado`. */
export interface ProvisionEstado {
  herramientas: HerramientaProvision[];
  auto_provision: boolean;
  en_curso: ProgresoProvision | null;
}

/* ── Tipos (espejo de llmfit.rs) ──────────────────────────────────────────── */

/**
 * Una GPU tal como la ve llmfit.
 *
 * `unified_memory` no es un detalle: en un equipo con memoria unificada (Apple
 * Silicon, por ejemplo) el modelo no "cabe o no cabe" en una VRAM aparte,
 * comparte la RAM. Por eso se enseña en vez de suponer VRAM dedicada.
 */
export interface LlmfitGpu {
  name: string;
  vram_gb: number;
  backend: string;
  unified_memory: boolean;
}

/** Perfil de hardware que detecta llmfit por su cuenta (no es lectura de Machinograph). */
export interface LlmfitSistema {
  cpu_name: string;
  cpu_cores: number;
  available_ram_gb: number;
  backend: string;
  gpu_name: string;
  gpu_vram_gb: number;
  gpu_count: number;
  gpus: LlmfitGpu[];
}

/** Las cuatro notas (0–100) que llmfit da a cada modelo. */
export interface LlmfitComponentes {
  quality: number;
  speed: number;
  fit: number;
  context: number;
}

/**
 * Un modelo recomendado por llmfit.
 *
 * Todo lo que puede faltar es opcional a propósito: llmfit es una herramienta de
 * FUERA y su formato puede cambiar entre versiones, así que un campo que no
 * venga deja el hueco vacío y nunca se rellena con algo inventado.
 */
export interface LlmfitModelo {
  name: string;
  provider: string;
  params_b: number;
  parameter_count: string;
  use_case: string;
  category: string;
  /** `Perfect` | `Good` | `Marginal` | `Poor`, tal cual lo dice llmfit. */
  fit_level: string;
  run_mode: string;
  runtime: string;
  best_quant: string | null;
  /**
   * ESTIMADA por llmfit con el ancho de banda TEÓRICO de la GPU. No es una
   * medida. Lo medido lo pone Machinograph con `llama-bench` (vista Rendimiento), y
   * nunca se enseña una cifra como si fuera la otra.
   */
  estimated_tps: number | null;
  measured_tps: number | null;
  disk_size_gb: number | null;
  memory_required_gb: number | null;
  utilization_pct: number | null;
  context_length: number | null;
  effective_context_length: number | null;
  score: number | null;
  score_components: LlmfitComponentes | null;
  license: string | null;
  capabilities: string[];
  is_moe: boolean;
  /** llmfit cree que ya lo tienes en disco: es SU dato, no una comprobación nuestra. */
  installed: boolean;
  /** `estimated` | `measured`: de dónde sale la velocidad que enseña llmfit. */
  estimate_confidence: string | null;
  /** El `llama-bench` que propone llmfit para VERIFICAR su propia estimación. */
  verify_command: string | null;
  llamacpp_command: string | null;
  notes: string[];
}

/** Estado de la integración con llmfit (`llmfit:estado`). */
export interface LlmfitEstado {
  instalado: boolean;
  binario: string | null;
  version: string | null;
  /** El backend lo lleva en la estructura pero lo deja siempre en `null`. */
  sistema: LlmfitSistema | null;
}

/** Respuesta de `llmfit:recomendar`. */
export interface LlmfitRecomendaciones {
  sistema: LlmfitSistema;
  modelos: LlmfitModelo[];
}

/** Argumentos de `llmfit:recomendar`. Son los filtros de su CLI. */
export interface ArgsRecomendar {
  /** Sin valor, el backend usa su límite por defecto (40). */
  limit?: number;
  /** CLAVE corta del caso de uso (`coding`, `reasoning`…), no la etiqueta larga. */
  useCase?: string;
  /** `perfect` | `good` | `marginal`. */
  minFit?: string;
  /** `vision`, `tool_use`, `audio`, `tts`… varias separadas por coma. */
  capability?: string;
  /** Añade `--output-llamacpp`; de ahí sale `llamacpp_command`. */
  conComando?: boolean;
}

/** Argumentos de `llmfit:plan`. */
export interface ArgsPlan {
  /** Nombre del modelo COMO LO CONOCE LLMFIT (no la ruta del fichero). */
  modelo: string;
  /** Sin valor, el backend usa 32768. */
  context?: number;
  /** Cuantización concreta a suponer (p. ej. `Q4_K_M`); opcional. */
  quant?: string;
}

/**
 * Lo que hace falta para mover un modelo (llmfit.rs: `Recursos`).
 *
 * Los tres son OPCIONALES y no es comodidad: llmfit manda `vram_gb: null` en la
 * vía «solo CPU», y eso significa «no necesita VRAM», que NO es 0. Por eso la
 * interfaz enseña «—» y no un cero.
 */
export interface LlmfitRecursos {
  vram_gb: number | null;
  ram_gb: number | null;
  cpu_cores: number | null;
}

/** Una forma de ejecutar el modelo (llmfit.rs: `Via`). */
export interface LlmfitVia {
  /** `gpu` | `cpu_offload` | `cpu_only`. */
  path: string;
  feasible: boolean;
  /** `Perfect` | `Good` | `Marginal` | `TooLight`… tal cual lo dice llmfit. */
  fit_level: string | null;
  /** ESTIMADO por llmfit: no es una medición. Nunca se enseña como tal. */
  estimated_tps: number | null;
  minimum: LlmfitRecursos | null;
  recommended: LlmfitRecursos | null;
  notes: string[];
}

/** El plan de hardware de un modelo (llmfit.rs: `Plan`). Es una ESTIMACIÓN. */
export interface LlmfitPlan {
  /** El aviso de llmfit sobre sus propios números, cuando lo manda. */
  estimate_notice: string | null;
  model_name: string;
  provider: string;
  context: number;
  quantization: string | null;
  kv_quant: string | null;
  disk_size_gb: number | null;
  minimum: LlmfitRecursos | null;
  recommended: LlmfitRecursos | null;
  run_paths: LlmfitVia[];
}

/** Un escalón de la escalera de concurrencia (llmfit.rs: `Escalon`). */
export interface LlmfitEscalon {
  requested_context: number;
  effective_context: number;
  /** Lo que ocupa la caché KV de UNA sesión a ese contexto. */
  per_session_kv_gb: number;
  /** Cuántas sesiones caben a la vez. `0` = no cabe ni una. */
  max_sessions: number;
}

/** El cálculo de capacidad de memoria (llmfit.rs: `EstimacionConcurrencia`). */
export interface LlmfitEstimacionConcurrencia {
  kv_budget_gb: number;
  kv_quant: string | null;
  pool_gb: number | null;
  weights_resident_gb: number | null;
  /** Contexto máximo NATIVO del modelo: el techo real de la escalera. */
  native_context: number | null;
  /** Cuantización de los PESOS residentes (no la de la caché, que es `kv_quant`). */
  quant: string | null;
  /** Memoria por sesión de las capas recurrentes; solo en modelos híbridos. */
  per_session_recurrent_gb: number | null;
  ladder: LlmfitEscalon[];
}

/** Cuántas sesiones aguanta el equipo (llmfit.rs: `Concurrencia`). */
export interface LlmfitConcurrencia {
  model: string | null;
  run_mode: string | null;
  fit_level: string | null;
  max_context_for_target: number | null;
  estimate: LlmfitEstimacionConcurrencia | null;
}

/* ── Tipos (espejo de db.rs: el encaje automático) ────────────────────────── */

/**
 * El nivel de encaje de un modelo, tal cual lo guarda el backend
 * (`format!("{:?}")` de `perf::Encaje`).
 *
 * `Error` NO es "sin dato": es un cálculo que se intentó y no salió, con el
 * motivo en `detalle`. Y `ctx_max` solo vale 0 en ese caso, así que "0 contexto"
 * nunca se enseña como si fuera un encaje.
 */
export type EncajeFit = "Gpu" | "Mixto" | "NoCabe" | "Error";

/** Una fila de la tabla `fits` (`fits:listar`, y el payload de `ai:fit`). */
export interface FitRow {
  /** Ruta COMPLETA del `.gguf`: es la clave con la que se cruza el inventario. */
  modelo: string;
  /** Nombre del runtime de llama.cpp que lo calculó ("vulkan", "bin"…). */
  runtime: string;
  /** Contexto que entra. `0` solo cuando `encaje` es `Error`. */
  ctx_max: number;
  /** `-1` = todas las capas en la GPU. */
  ngl: number;
  encaje: EncajeFit;
  /** Contexto que se pidió, si se pidió uno (`null` = solo se pidió el máximo). */
  pedido: number | null;
  /** Frase ya redactada por el backend: se enseña literal, sin reescribirla. */
  detalle: string;
  /** Epoch en SEGUNDOS: la antigüedad del cálculo se enseña siempre. */
  ts: number;
}

/* ── Tipos (espejo de gpu.rs: el reloj de memoria) ────────────────────────── */

/** Un nivel de reloj de memoria de la tabla `pp_dpm_mclk`. */
export interface NivelMclk {
  idx: number;
  mhz: number;
  activo: boolean;
}

/**
 * Estado del reloj de memoria (MCLK).
 *
 * Esta tarjeta tiene un fallo de Display Core en amdgpu: el MCLK se queda
 * clavado en el mínimo (96 MHz) y no sube aunque la GPU esté al 100 %. Es
 * SILENCIOSO —no da error— y hace que los modelos vayan ~15 veces más lentos,
 * así que la única forma de enterarse es mirarlo y avisar.
 */
export interface EstadoMclk {
  niveles: NivelMclk[];
  activo_idx: number;
  activo_mhz: number;
  max_idx: number;
  max_mhz: number;
  gpu_busy: number;
  mem_busy: number;
  /** `true` con la GPU trabajando y el reloj en el mínimo: la firma del fallo. */
  degradado: boolean;
  /** Frase ya redactada por el backend: se enseña literal, sin reescribirla. */
  veredicto: string;
}

/* ── Tipos (espejo de almacen.rs) ─────────────────────────────────────────── */

/**
 * Un hijo directo de la carpeta analizada, con su tamaño RECURSIVO.
 *
 * `bytes` de una carpeta es lo que ocupa TODO lo que hay dentro, no el tamaño de
 * su entrada de directorio: es la misma cuenta que hace `du`, que es la que el
 * usuario espera al preguntar "¿qué ocupa esto?".
 */
export interface NodoAlmacen {
  ruta: string;
  nombre: string;
  bytes: number;
  ficheros: number;
  /** Carpetas contenidas (recursivo), sin contarse a sí misma. */
  dirs: number;
  es_dir: boolean;
  modificado: number | null;
}

/** El resultado de analizar UNA carpeta (un nivel, como `du --max-depth=1`). */
export interface ArbolAlmacen {
  ruta: string;
  bytes: number;
  ficheros: number;
  dirs: number;
  /** Hijos directos, de mayor a menor tamaño. */
  hijos: NodoAlmacen[];
  /** Hijos que NO se enseñan (se conservan los más grandes) y lo que suman. */
  resto_n: number;
  resto_bytes: number;
  /** Se agotó el presupuesto: el total puede quedarse corto. Hay que decirlo. */
  truncado: boolean;
  /** Entradas sin poder leer (permisos): no es lo mismo que "no hay nada". */
  omitidos: number;
  entradas: number;
  ms: number;
  /**
   * Patrones de exclusión que han dejado algo FUERA de esta medición (solo los
   * que han actuado de verdad). Se enseña: un total que encoge sin explicación
   * parece un fallo. Vacío = el análisis lo ha medido todo.
   */
  excluidos: string[];
}

/**
 * Un hijo directo guardado en una medida de disco, tal cual está en la base.
 *
 * Solo se guardan los hijos que el analizador DEVOLVIÓ (los más grandes, según
 * su `max_hijos`); `Instantanea.resto_n` dice si dejó alguno fuera.
 */
export interface HijoInstantanea {
  ruta: string;
  nombre: string;
  bytes: number;
  ficheros: number;
  dirs: number;
}

/**
 * Una medida del uso de disco de una carpeta en un momento concreto.
 *
 * Los tres campos de calidad (`truncado`, `excluidos`, `resto_n`) permiten decir
 * POR QUÉ una comparación no es de fiar, en vez de un «parcial» sin explicación.
 */
export interface Instantanea {
  /** Marca de tiempo UNIX en segundos: de CUÁNDO es la medida. */
  ts: number;
  ruta: string;
  bytes: number;
  ficheros: number;
  dirs: number;
  /** El recorrido se agotó por presupuesto: el total puede quedarse corto. */
  truncado: boolean;
  /** Patrones de exclusión que dejaron algo fuera de esta medida. */
  excluidos: string[];
  /** Hijos que el analizador no listó (no afecta al total, sí a la lista de hijos). */
  resto_n: number;
  resto_bytes: number;
  hijos: HijoInstantanea[];
}

/** Un hijo en la comparación. `pct` es `null` cuando antes estaba a cero. */
export interface CrecimientoHijo {
  ruta: string;
  nombre: string;
  antes: number;
  ahora: number;
  delta: number;
  pct: number | null;
  nuevo: boolean;
  desaparecido: boolean;
}

/** Lo que ha cambiado una carpeta entre dos medidas (espejo de `historial::Crecimiento`). */
export interface Crecimiento {
  ruta: string;
  antes_ts: number;
  ahora_ts: number;
  /** Diferencia real entre las dos medidas, en segundos. */
  segundos: number;
  antes_bytes: number;
  ahora_bytes: number;
  delta_bytes: number;
  antes_ficheros: number;
  ahora_ficheros: number;
  delta_ficheros: number;
  /** El delta TOTAL puede no ser el real: alguna medida quedó incompleta. */
  parcial: boolean;
  motivo: string | null;
  /** No se guardaron todos los hijos de alguna medida: la lista puede tener altas o bajas falsas. */
  hijos_parcial: boolean;
  hijos_faltan: number;
  /** De mayor a menor crecimiento; incluye los que BAJAN (delta negativo). */
  hijos: CrecimientoHijo[];
}

/** Lo que devuelve `almacen:historial`: las medidas y la comparación disponible. */
export interface HistorialDisco {
  ruta: string;
  /** Si la medida DIARIA automática está encendida (analizar a mano guarda igual). */
  activo: boolean;
  /** Crecimiento semanal (GB) a partir del cual Inicio avisa. */
  umbral_gb: number;
  retencion_dias: number;
  /** Los días pedidos para la comparación (7 para «la última semana`), o `null` si son las dos últimas. */
  dias: number | null;
  instantaneas: Instantanea[];
  /** `null` cuando hay menos de dos medidas: no hay con qué comparar. */
  crecimiento: Crecimiento | null;
}

/** Uso de un punto de montaje real (los pseudo-sistemas no se listan). */
export interface Montaje {
  punto: string;
  /** El dispositivo (`/dev/nvme1n1p3`, `C:`). Puede venir vacío: entonces se enseña el tipo. */
  dispositivo: string;
  /** El sistema de ficheros (`btrfs`, `ext4`, `apfs`, `NTFS`). */
  tipo: string;
  total: number;
  usado: number;
  libre: number;
  uso_pct: number;
  /** Un USB. Borrar en uno que se va a desconectar es distinto de borrar en el disco del sistema. */
  extraible: boolean;
}

/** Un grupo de ficheros con el MISMO contenido (espejo de `almacen::Duplicado`). */
export interface Duplicado {
  bytes: number;
  rutas: string[];
  /** Lo que se liberaría dejando UNA copia de cada grupo. */
  desperdicio: number;
}

/** Un enlace simbólico que apunta a algo que ya no está. */
export interface EnlaceRoto {
  ruta: string;
  destino: string;
}

/** Cuánto hay en la papelera del sistema. `null` si no se puede contar. */
export interface PapeleraEstado {
  elementos: number;
  bytes: number;
  ruta: string;
}

/**
 * De dónde salen las actualizaciones: la herramienta DE ESTE SISTEMA.
 *
 * Machinograph no tiene canal de versiones propio: ejecuta la comprobación de
 * rpm-ostree/flatpak/brew/winget/softwareupdate y traduce su salida. El comando que
 * aplica es el de esa herramienta, no uno nuestro.
 */
export interface FuenteActualizacion {
  id: string;
  nombre: string;
  disponible: boolean;
  actualizaciones: string[];
  comando_comprobar: string;
  comando_aplicar: string;
  requiere_root: boolean;
  /** Aplicar deja el cambio para el siguiente arranque (rpm-ostree). */
  requiere_reinicio: boolean;
  /** Letra pequeña de la propia herramienta (se enseña tal cual). */
  nota: string | null;
  error: string | null;
}

/** La limpieza programada (`programar:leer` / `programar:guardar`). */
export interface Programacion {
  activa: boolean;
  hora: number;
  minuto: number;
  /** Categorías del catálogo. Vacío = todas. */
  categorias: string[];
  /** El día local (`AAAA-MM-DD`) de la última ejecución. */
  ultima: string | null;
}

/** Cómo poner la limpieza en el planificador del sistema (systemd/launchd/tareas). */
export interface RecetaProgramacion {
  titulo: string;
  destino: string;
  contenido: string;
  instrucciones: string;
}

/**
 * Una copia de seguridad de un fichero que la app ha tocado (su `.bak-`).
 *
 * `existe` se comprueba AL LEER: una copia que alguien borró por fuera no se puede
 * restaurar, y la interfaz tiene que poder decirlo en vez de ofrecer un botón que
 * fallará.
 */
export interface CopiaRow {
  id: number;
  ts: number;
  ruta_original: string;
  ruta_copia: string;
  bytes: number;
  motivo: string;
  existe: boolean;
}

export interface FicheroGrande {
  ruta: string;
  nombre: string;
  bytes: number;
  modificado: number | null;
}

export interface Coincidencia {
  ruta: string;
  nombre: string;
  /** `null` en una carpeta: no se mide su subárbol al buscar (no es un 0). */
  bytes: number | null;
  es_dir: boolean;
  modificado: number | null;
}

/* ── Tipos (espejo de limpieza.rs) ────────────────────────────────────────── */

/** Una categoría del catálogo de limpieza. */
export interface CategoriaLimpieza {
  id: string;
  nombre: string;
}

/**
 * Un objetivo de limpieza ya medido.
 *
 * `bytes` es lo que se LIBERARÍA de verdad (solo lo que supera la antigüedad
 * mínima); lo que es demasiado nuevo se cuenta aparte, en `recientes`.
 */
export interface ObjetivoLimpieza {
  id: string;
  categoria: string;
  subcategoria: string;
  descripcion: string;
  rutas: string[];
  bytes: number;
  elementos: number;
  recientes: number;
  min_dias: number;
  /** Necesita root: no se limpia desde aquí, se enseña el comando. */
  root: boolean;
  /** Limpieza nativa recomendada (pnpm, docker, uv, journalctl…) o comando de root. */
  comando: string | null;
  sin_permiso: boolean;
  /** El presupuesto se agotó midiendo esto: el tamaño es incompleto. */
  parcial: boolean;
  /**
   * Es una HUELLA de tu actividad (historial, recientes, portapapeles): no se
   * regenera sola, así que no se borra con un «marcar todo» ni sin pedirla por su
   * nombre.
   */
  traza: boolean;
  /**
   * Es un REINICIO de caché de rendimiento (shaders de GPU, Steam…): borrarla no
   * rompe nada, pero la primera vez todo va más lento. No se marca sola (ni con
   * «marcar todo»), ni entra en la limpieza automática: se limpia solo si la
   * seleccionas a mano.
   */
  reinicio_cache: boolean;
}

export interface EscaneoLimpieza {
  objetivos: ObjetivoLimpieza[];
  bytes: number;
  elementos: number;
  ms: number;
  truncado: boolean;
  /**
   * Objetivos que NO se han medido por una exclusión del usuario, con el patrón
   * que los excluyó. Se enseña: un total más bajo sin explicación parece un fallo.
   */
  excluidos: string[];
}

/* ── Tipos (espejo de bases.rs) ───────────────────────────────────────────── */

/**
 * Cómo salió una base.
 *
 * `bloqueada` NO es «0 recuperable»: es «no se pudo medir porque otra aplicación
 * la tiene abierta», y la interfaz tiene que poder decir la diferencia.
 */
export type EstadoBaseSqlite = "ok" | "bloqueada" | "sin_permiso" | "error";

/**
 * Una base SQLite del catálogo de Kudu ya medida.
 *
 * Cada cifra dice de dónde sale: `bytes` es `PRAGMA page_count × page_size`,
 * `recuperable` es `PRAGMA freelist_count × page_size` (lo que devolvería un
 * `VACUUM`) y `disco` es el tamaño del fichero más su `-wal`, medido con `stat`.
 * Los `null` son «no se pudo medir», nunca un cero de mentira.
 */
export interface BaseSqlite {
  app: string;
  ruta: string;
  /** El perfil dentro de la carpeta (`Default`, `xxxx.default-release`), si la hay. */
  perfil: string | null;
  bytes: number | null;
  paginas: number | null;
  pagina_bytes: number | null;
  libres: number | null;
  recuperable: number | null;
  /** Fichero + `-wal` en disco (`stat`), que no tiene por qué coincidir. */
  disco: number | null;
  /** Tamaño del `-wal` si existe: ese fichero lo gestiona SQLite y no se toca. */
  wal_bytes: number;
  auto_vacuum: string | null;
  journal: string | null;
  estado: EstadoBaseSqlite;
  /** Motivo cuando el estado no es `ok` (bloqueada, sin permiso, error). */
  nota: string | null;
  /** El comando equivalente para hacerlo a mano (nota secundaria: la app lo hace sola). */
  comando: string;
}

export interface ListadoBases {
  bases: BaseSqlite[];
  total: number;
  /** Cuántas se pudieron medir: solo estas suman en `bytes_recuperables`. */
  medidas: number;
  bloqueadas: number;
  sin_permiso: number;
  errores: number;
  bytes_recuperables: number;
  bytes_ocupados: number;
  wal_bytes: number;
  ms: number;
  /** El presupuesto se agotó: faltan bases por mirar y el total se queda corto. */
  truncado: boolean;
  sistema: string;
  /** Objetivos de Kudu que no se pudieron traducir en este sistema. */
  sin_traducir: string[];
  nota: string | null;
}

/** Lo que se ha hecho con una base al compactarla, con la medida ANTES y DESPUÉS. */
export interface BaseCompactada {
  app: string;
  ruta: string;
  ok: boolean;
  estado: EstadoBaseSqlite;
  motivo: string | null;
  /** Los procesos que la tenían abierta, si estaba bloqueada: «chrome (pid 12)». */
  bloqueantes: string[];
  /** Lo recuperado DE VERDAD en disco (`stat` antes − después, `-wal` incluido). */
  liberado: number | null;
  recuperable_antes: number | null;
  recuperable_despues: number | null;
  bytes_antes: number | null;
  bytes_despues: number | null;
  disco_antes: number | null;
  disco_despues: number | null;
  ms: number;
}

export interface CompactacionBases {
  resultados: BaseCompactada[];
  liberado: number;
  compactadas: number;
  bloqueadas: number;
  fallos: number;
  ms: number;
  mensaje: string;
}

/* ── Tipos (espejo de exclusiones.rs) ─────────────────────────────────────── */

/** Una exclusión vigente: el texto del usuario y a qué se resuelve. */
export interface ExclusionVigente {
  patron: string;
  /** «${HOME}/VMs → /home/usuario/VMs», para no tener que adivinar qué significa. */
  descripcion: string;
  /** La ruta absoluta resuelta, o `null` si es un comodín o un nombre suelto. */
  resuelta: string | null;
}

export interface Exclusiones {
  guardadas: { patron: string; ts: number }[];
  vigentes: ExclusionVigente[];
}

/* ── Tipos (espejo de seguridad.rs) ───────────────────────────────────────── */

/**
 * El veredicto de un indicador de compromiso.
 *
 * `desconocido` NO es un problema: es «no se pudo comprobar» (falta una
 * herramienta, o hace falta root). Se pinta distinto a propósito, porque decir
 * «bien» cuando no se ha mirado sería mentir.
 */
export type VeredictoSeguridad = "ok" | "aviso" | "problema" | "desconocido";

/** Un indicador de compromiso, con su prueba y (si toca) su remedio. */
export interface HallazgoSeguridad {
  id: string;
  titulo: string;
  veredicto: VeredictoSeguridad;
  /** Qué se ha encontrado, con la prueba concreta (la línea, el permiso, la ruta). */
  detalle: string;
  remedio: string | null;
  /** De dónde sale: el fichero o el comando que se ha mirado. */
  fuente: string;
}

export interface RevisionSeguridad {
  hallazgos: HallazgoSeguridad[];
  /** El peor de todos, para poder resumirlo en una línea. */
  resumen: VeredictoSeguridad;
}

/* ── Tipos (espejo de arranque.rs) ────────────────────────────────────────── */

/** Un programa que arranca con la sesión (XDG Autostart). */
export interface EntradaArranque {
  id: string;
  nombre: string;
  exec: string;
  comentario: string | null;
  ruta: string;
  /** "usuario" (~/.config/autostart) o "sistema" (/etc/xdg/autostart). */
  origen: string;
  activo: boolean;
  oculta: boolean;
}

/** Acciones que acepta `action:run` (ver src-tauri/src/actions.rs). */
export type Accion =
  | "display:reapply"
  | "display:apply"
  | "display:toggle"
  | "process:kill"
  | "server:start"
  | "server:stop"
  | "update:run"
  // Las dos caras de medir: `perf:fit` calcula el contexto que cabe y
  // `perf:bench` mide tokens/s reales. Medir es CARO, así que solo se lanzan a
  // petición del usuario.
  | "perf:fit"
  | "perf:bench"
  // Medir SIRVIENDO: `llmfit:medir` lanza peticiones al servidor que está en
  // marcha (llama-swap), así que mide lo que de verdad se sirve, con su proxy y
  // su configuración. `perf:bench` mide EN AISLADO (llama-bench, proceso
  // aparte). Los números de una y otra NO son comparables, y la interfaz lo
  // dice al lado de cada botón. `llmfit:medir` guarda su fila en
  // `benchmarks:recent` con runtime `llmfit (llamacpp)`.
  | "llmfit:medir"
  // Cargar/descargar modelos: son endpoints de la API de llama-swap, así que
  // solo tienen sentido para ese motor y no se ofrecen para los demás.
  | "modelo:cargar"
  | "modelo:descargar"
  | "modelo:descargar-todos"
  // Las dos caras del reloj de memoria clavado. En la interfaz van en ESTE
  // orden: primero el ciclo de pantalla (suave, sin root, no pierde la VRAM) y
  // solo después el reinicio del motor gráfico (root, pierde la VRAM).
  | "gpu:arreglar"
  | "gpu:reiniciar"
  // Gestión del inventario nuevo. `modelo:borrar` NO borra: mueve a la papelera
  // del escritorio (se recupera). `llmfit:descargar` baja el GGUF de Hugging
  // Face y emite `ai:update-line` mientras corre.
  | "modelo:borrar"
  | "modelo:abrir-carpeta"
  | "llmfit:descargar"
  // Almacenamiento y optimización. `almacen:borrar` lleva a la papelera por
  // defecto y solo borra de verdad con `definitivo: true`; `limpieza:limpiar`
  // borra SIEMPRE de verdad (mover cachés a la papelera no libera espacio) y por
  // eso la interfaz enseña la lista completa antes de confirmar.
  | "almacen:borrar"
  | "limpieza:limpiar"
  | "arranque:activar"
  // La papelera y el Centro de recuperación. Restaurar una copia hace SU propia
  // copia antes de pisar el original, así que tampoco es irreversible.
  | "papelera:vaciar"
  | "copias:restaurar"
  | "copias:borrar";

/* ── Comandos ─────────────────────────────────────────────────────────────── */

export const api = {
  /** Foto completa del sistema (bloquea hasta que termina: ~200-600 ms). */
  snapshot: () => invoke<Snapshot>("snapshot:now"),

  /**
   * Corrige el tamaño de la ventana cuando el viewport CSS se queda corto.
   *
   * Existe porque el backend NO sabe cuántos píxeles CSS tiene la webview: en
   * X11 `scale_factor` dice 1 mientras WebKit pinta a `Xft.dpi/96` (1,45 en este
   * equipo), así que la ventana salía con 882x551 CSS en vez de 1280x800. La
   * interfaz sí lo sabe, así que lo mide y lo manda. El backend no toca nada si
   * el viewport ya llega al mínimo del diseño (960x640).
   */
  ajustarVentana: (anchoCss: number, altoCss: number) =>
    invoke<string>("ventana:ajustar", { args: { ancho_css: anchoCss, alto_css: altoCss } }),

  /** El uso de un periodo. `modelo` vacío = todos. */
  uso: (periodo: "hoy" | "todo", modelo = "") =>
    invoke<UsoRespuesta>("uso:resumen", { args: { periodo, modelo } }),

  memoria: () => invoke<MemoriaGpu>("memoria:cargados"),

  descarga: {
    estado: () => invoke<Descarga>("descarga:estado"),
    iniciar: (modelo: string, quant?: string) =>
      invoke<string>("descarga:iniciar", { args: { modelo, quant: quant ?? null } }),
    cancelar: () => invoke<string>("descarga:cancelar"),
  },

  /**
   * Lo que Machinograph necesita para funcionar entera (llmfit, llama.cpp) y lo que no
   * se puede instalar sola (amd-smi, que viene con ROCm y necesita root).
   */
  provision: {
    estado: () => invoke<ProvisionEstado>("provision:estado"),
    /** Lanza la instalación de lo que falte. El progreso llega por `ai:provision`. */
    instalar: (herramientas: string[] = [], forzar = false) =>
      invoke<string>("provision:instalar", { args: { herramientas, forzar } }),
    /** «Comprobar ahora»: repara solo lo roto o lo que falte. */
    reparar: () => invoke<string>("provision:reparar"),
    cancelar: () => invoke<string>("provision:cancelar"),
    /** Lee o cambia la instalación automática (por defecto, activada). */
    auto: (activo?: boolean) =>
      invoke<boolean>("provision:auto", { args: activo == null ? {} : { activo } }),
  },

  entorno: {
    red: () => invoke<EntornoRed>("entorno:red"),
    arranque: () => invoke<ArranqueAuto>("entorno:arranque"),
    /** `activar` es obligatorio: sin él no se toca el arranque del equipo. */
    setArranque: (activar: boolean) =>
      invoke<string>("entorno:arranque-configurar", { args: { activar } }),
    carpetas: () => invoke<{ carpetas: CarpetaModelos[] }>("entorno:carpetas"),
  },

  gateway: {
    estado: () => invoke<GatewayEstado>("gateway:estado"),
    /**
     * Guarda la configuración de la puerta. Se envían solo los campos que cambian
     * (los `undefined` no viajan), y el backend valida puerto, dirección y destino
     * antes de escribir nada.
     */
    configurar: (cambios: {
      activa?: boolean;
      direccion?: string;
      puerto?: number;
      destino?: string;
      requiere_clave?: boolean;
    }) => invoke<string>("gateway:configurar", { args: cambios }),
    regenerarClave: () => invoke<string>("gateway:regenerar-clave"),
  },

  /**
   * Autorreparación: lo que la app se mira y se arregla a sí misma (el puerto de
   * la puerta, la base del histórico, su propia entrada de arranque y los ficheros
   * de configuración que ella escribió).
   *
   * Con `reparar` a `false` solo informa; con `true`, arregla (la base rota se
   * aparta sin borrarla, el fichero se restaura desde su copia, el arranque se
   * reescribe). Corre solo al arrancar; el botón «Reparar ahora» es la versión a
   * mano.
   */
  salud: {
    revisar: (reparar = false) => invoke<RevisionSalud>("salud:revisar", { args: { reparar } }),
  },

  /** Ejecuta una acción. El backend emite `ai:update-line` mientras corre. */
  accion: (kind: Accion, args: Record<string, unknown> = {}) =>
    invoke<string>("action:run", { aj: { kind, args } }),

  servers: {
    list: () => invoke<ServerRow[]>("servers:list"),
    add: (r: Omit<ServerRow, "cmd" | "enabled"> & { cmd?: string; enabled?: boolean }) =>
      invoke<string>("servers:add", {
        args: {
          id: r.id, name: r.name, kind: r.kind, port: r.port,
          cmd: r.cmd ?? "", enabled: r.enabled ?? true,
        },
      }),
    update: (id: string, cmd: string, enabled: boolean) =>
      invoke<string>("servers:update", { args: { id, cmd, enabled } }),
    remove: (id: string) => invoke<string>("servers:remove", { args: { id } }),
  },

  settings: {
    get: () => invoke<Ajustes>("settings:get"),
    /**
     * El valor va como TEXTO (`"5000"`), que es lo que guarda SQLite. El backend
     * valida clave, número y rango: si algo no cuadra, la promesa se rechaza con
     * el motivo ya redactado y hay que enseñarlo tal cual, sin envolverlo.
     */
    set: (key: string, value: string) =>
      invoke<string>("settings:set", { args: { key, value } }),
  },

  metrics: (since = 0) => invoke<MetricRow[]>("metrics:recent", { args: { since } }),
  acciones: (limit = 100) => invoke<ActionRow[]>("actions:recent", { args: { limit } }),
  updates: (limit = 100) => invoke<UpdateRow[]>("updates:recent", { args: { limit } }),

  /**
   * Inventario de modelos de TODO tipo (llama.cpp, LM Studio, piper, Coqui TTS,
   * ComfyUI…). Es una lectura del sistema de ficheros: se llama al abrir la vista
   * y al cambiar algo (borrar), NUNCA en bucle, porque recorre carpetas enteras.
   */
  inventario: () => invoke<InventarioRespuesta>("inventario:listar"),

  /**
   * Almacenamiento: analizador de disco.
   *
   * Todas recorren el disco de verdad (segundos), así que se llaman al analizar
   * una carpeta o al pulsar buscar, NUNCA en bucle ni con cada foto. Sin `raiz`
   * el backend usa el home.
   */
  almacen: {
    arbol: (raiz?: string, maxHijos = 400) =>
      invoke<ArbolAlmacen>("almacen:arbol", { args: { raiz: raiz ?? null, max_hijos: maxHijos } }),
    grandes: (raiz?: string, limite = 50) =>
      invoke<FicheroGrande[]>("almacen:grandes", { args: { raiz: raiz ?? null, limite } }),
    buscar: (raiz: string | undefined, consulta: string, limite = 200) =>
      invoke<Coincidencia[]>("almacen:buscar", { args: { raiz: raiz ?? null, consulta, limite } }),
    montajes: () => invoke<Montaje[]>("almacen:montajes"),
    /**
     * Ficheros repetidos por CONTENIDO. Es la lectura más cara de la app (hay que
     * leer los ficheros), así que va bajo demanda y con un mínimo de tamaño: los
     * miles de ficheros pequeños que se repiten solos no liberan nada y llenarían
     * la lista de ruido.
     */
    duplicados: (raiz?: string, minBytes = 1024 * 1024, limite = 200) =>
      invoke<Duplicado[]>("almacen:duplicados", { args: { raiz: raiz ?? null, min_bytes: minBytes, limite } }),
    /** Carpetas que no tienen ningún fichero (ni en su subárbol). */
    vacias: (raiz?: string, limite = 500) =>
      invoke<string[]>("almacen:vacias", { args: { raiz: raiz ?? null, limite } }),
    /**
     * Enlaces simbólicos rotos. En Windows devuelve lista vacía: los accesos
     * directos son `.lnk` y comprobarlos necesita la API del shell.
     */
    enlaces: (raiz?: string, limite = 500) =>
      invoke<EnlaceRoto[]>("almacen:enlaces", { args: { raiz: raiz ?? null, limite } }),
    /**
     * Las medidas de disco guardadas de una carpeta y, si hay al menos dos, el
     * crecimiento. Sin `dias` compara las DOS últimas; con `dias`, contra la
     * medida más reciente de hace al menos esos días (Inicio usa 7).
     */
    historial: (raiz?: string, dias?: number, limite = 30) =>
      invoke<HistorialDisco>("almacen:historial", {
        args: { raiz: raiz ?? null, dias: dias ?? null, limite },
      }),
  },

  /** La papelera del sistema: cuánto ocupa y vaciarla. */
  papelera: {
    estado: () => invoke<PapeleraEstado | null>("papelera:estado"),
  },

  /** Las copias de seguridad de los ficheros que la app ha tocado. */
  copias: {
    listar: () => invoke<CopiaRow[]>("copias:listar"),
  },

  /**
   * Qué está desactualizado, según la herramienta de este sistema.
   *
   * Ejecuta las comprobaciones de fuera (rpm-ostree, flatpak, brew, winget…), así
   * que es BAJO DEMANDA y con su límite: cada una puede tardar y algunas consultan
   * su repositorio.
   */
  actualizar: {
    comprobar: () => invoke<FuenteActualizacion[]>("actualizar:comprobar"),
  },

  /** La limpieza programada: mide y avisa, nunca borra sola. */
  programar: {
    leer: () => invoke<Programacion>("programar:leer"),
    guardar: (p: { activa: boolean; hora: number; minuto: number; categorias: string[] }) =>
      invoke<string>("programar:guardar", { args: p }),
    /** Recetas para el planificador del sistema (para copiar, no se escriben solas). */
    recetas: () => invoke<RecetaProgramacion[]>("programar:recetas"),
  },

  /** Optimización: catálogo de basura y programas que arrancan solos. */
  limpieza: {
    categorias: () => invoke<CategoriaLimpieza[]>("limpieza:categorias"),
    escanear: (categorias: string[] = []) =>
      invoke<EscaneoLimpieza>("limpieza:escanear", { args: { categorias } }),
  },

  /**
   * Las bases SQLite de las aplicaciones (catálogo `databases.json` de Kudu).
   *
   * `listar` solo MIDE (PRAGMA, en solo lectura). `compactar` es la acción
   * explícita que hace el `VACUUM`: sin lista, compacta las que tengan algo que
   * recuperar; de las que otra aplicación tenga abiertas informa con el proceso,
   * sin tocarlas.
   */
  bases: {
    listar: () => invoke<ListadoBases>("bases:listar"),
    compactar: (bases: { app: string; ruta: string }[] = []) =>
      invoke<CompactacionBases>("bases:compactar", { args: { bases } }),
  },

  /**
   * Indicadores de compromiso: qué se ejecuta solo en este equipo y qué hay en
   * los sitios donde se esconde la persistencia.
   *
   * NO es un antivirus y NO sale nada de aquí: es local y sin firmas. Lo que sí
   * hace es dar la PRUEBA de cada hallazgo (la línea, el permiso, la ruta).
   */
  seguridad: {
    revisar: () => invoke<RevisionSeguridad>("seguridad:revisar"),
  },

  /**
   * Exclusiones del usuario: lo que no se mide ni se borra.
   *
   * Las respetan el analizador de disco, la limpieza y el borrado, y cada sitio
   * dice QUÉ exclusión dejó algo fuera (`comprobar` es para poder explicarlo).
   */
  exclusiones: {
    listar: () => invoke<Exclusiones>("exclusiones:listar"),
    anadir: (patron: string) => invoke<string>("exclusiones:anadir", { args: { patron } }),
    quitar: (patron: string) => invoke<string>("exclusiones:quitar", { args: { patron } }),
    comprobar: (ruta: string) =>
      invoke<{ excluida: boolean; patron: string | null }>("exclusiones:comprobar", { args: { ruta } }),
  },

  arranque: {
    listar: () => invoke<EntradaArranque[]>("arranque:listar"),
  },

  perf: {
    /** Runtimes de llama.cpp instalados. No lleva argumentos. */
    tools: () => invoke<RuntimeLlama[]>("perf:tools"),
  },

  /** Medidas de `llama-bench` guardadas, más recientes primero. */
  benchmarks: (limit = 100) => invoke<BenchRow[]>("benchmarks:recent", { args: { limit } }),

  /**
   * Encajes YA calculados, con su fecha (`fits:listar`).
   *
   * Esto solo LEE la tabla: el cálculo lo hace el backend solo (al arrancar y
   * cada 10 min), así que abrir la vista no lanza ninguna medida. Un modelo sin
   * fila aquí es "todavía no se ha calculado", que no es lo mismo que "no cabe".
   */
  fits: () => invoke<FitRow[]>("fits:listar"),

  /**
   * Últimas líneas del log del motor de llama-swap (`GET /logs`).
   *
   * Lectura BAJO DEMANDA: se pide al abrir el panel y cuando se pulsa refrescar.
   * No se llama en bucle ni en cada foto (el log crece con cada petición servida).
   */
  swapLogs: (port = 8080, lineas = 300) =>
    invoke<string[]>("swap:logs", { args: { port, lineas } }),

  /**
   * Reloj de memoria de la GPU amdgpu.
   *
   * `null` significa que NO hay ninguna GPU amdgpu: eso no es un error y la
   * vista lo dice tal cual en vez de pintar un fallo. Es una lectura de sysfs,
   * así que se puede repetir con cada foto sin coste apreciable.
   */
  gpu: {
    mclk: () => invoke<EstadoMclk | null>("gpu:mclk"),
  },

  /** Integración con llmfit (`AlexsJones/llmfit`), la herramienta aparte. */
  llmfit: {
    /** Si está instalado, su versión y su binario. No lanza llmfit. */
    estado: () => invoke<LlmfitEstado>("llmfit:estado"),
    /** Perfil de hardware que detecta llmfit. */
    sistema: () => invoke<LlmfitSistema>("llmfit:sistema"),
    /**
     * Modelos que le encajan a este equipo.
     *
     * Tarda ~0,6 s, así que SOLO se llama al abrir la vista y cuando el usuario
     * cambia un filtro: nunca en bucle ni por cada foto.
     */
    recomendar: (args: ArgsRecomendar = {}) =>
      invoke<LlmfitRecomendaciones>("llmfit:recomendar", { args }),

    /**
     * Plan de hardware de UN modelo, ESTIMADO por llmfit (~0,5 s).
     *
     * No es una medición: son números calculados a partir de los pesos y de las
     * heurísticas de llmfit. Se pide a mano, al pulsar el botón.
     */
    plan: (args: ArgsPlan) => invoke<LlmfitPlan>("llmfit:plan", { args }),

    /**
     * Cuántas sesiones simultáneas aguanta el equipo con ese modelo. También
     * es un CÁLCULO de llmfit (memoria), no una medida bajo carga.
     */
    concurrencia: (modelo: string) =>
      invoke<LlmfitConcurrencia>("llmfit:concurrencia", { args: { modelo } }),
  },

  /**
   * Diagnóstico del entorno: GPU, MCLK, llmfit, motor, disco, espacio…
   *
   * Es una lectura LOCAL de este equipo (sin red y sin GPU), así que se puede
   * repetir cuando el usuario lo pida. Cada comprobación llega con su estado y,
   * si hay algo que hacer, con el remedio.
   */
  diagnostico: {
    comprobar: () => invoke<Comprobacion[]>("diagnostico:comprobar"),
  },

  /**
   * Otros clientes de IA de este equipo y quién apunta al motor local.
   *
   * Esto solo DETECTA y genera TEXTO: Machinograph no toca sus ficheros. Si el usuario
   * quiere cambiar su configuración, la pega él.
   */
  conexiones: {
    clientes: () => invoke<ClienteConexion[]>("conexiones:clientes"),
    propuesta: (args: ArgsPropuesta) =>
      invoke<PropuestaConexion>("conexiones:propuesta", { args }),
    /**
     * Escribe el proveedor en la configuración del cliente. Es la única llamada
     * de la app que MODIFICA un fichero de otro programa, así que el backend
     * hace copia de seguridad con fecha, escritura atómica, conserva los
     * permisos del original y verifica releyendo (restaurando si no cuadra).
     */
    aplicar: (args: ArgsPropuesta) =>
      invoke<AplicadoConexion>("conexiones:aplicar", { args }),
  },
};

/* ── Eventos ──────────────────────────────────────────────────────────────── */

/**
 * `ai:snapshot` llega con la foto nueva cada `snapshot_interval_ms` (2 s por
 * defecto): el bucle del backend relee ese ajuste en cada vuelta.
 */
export const onSnapshot = (fn: (s: Snapshot) => void): Promise<UnlistenFn> =>
  listen<Snapshot>("ai:snapshot", (e) => fn(e.payload));

/** `ai:update-line` es una línea de salida de una acción en curso. */
export const onUpdateLine = (fn: (linea: string) => void): Promise<UnlistenFn> =>
  listen<string>("ai:update-line", (e) => fn(e.payload));

/** `ai:action` avisa de que una acción ha terminado (ok o error). */
export const onAction = (fn: (detalle: string) => void): Promise<UnlistenFn> =>
  listen<string>("ai:action", (e) => fn(e.payload));

/**
 * `ai:fit` llega con UN encaje recién calculado (mismo objeto que una fila de
 * `fits:listar`).
 *
 * Lo emite un cálculo a mano (`perf:fit`); el bucle automático del backend
 * escribe en la base sin emitir, así que al abrir la vista la tabla se pide
 * entera y este evento solo refresca lo que acaba de cambiar.
 */
/**
 * Progreso de una descarga. El backend lo emite con cada línea de llmfit, así que
 * llega varias veces por segundo: la interfaz tiene que repintar solo lo que
 * cambia, no volver a pedir nada.
 */
export const onDescarga = (fn: (d: Descarga) => void) => listen<Descarga>("ai:descarga", (e) => fn(e.payload));

/**
 * Progreso de una instalación de herramienta. Llega con cada aviso (descarga,
 * verificación, extracción y cierre), así que la tarjeta se repinta sin pedir nada.
 */
export const onProvision = (fn: (a: AvisoProvision) => void) =>
  listen<AvisoProvision>("ai:provision", (e) => fn(e.payload));

export const onFit = (fn: (f: FitRow) => void): Promise<UnlistenFn> =>
  listen<FitRow>("ai:fit", (e) => fn(e.payload));
