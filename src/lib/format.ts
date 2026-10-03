/**
 * Formateo de datos para la interfaz.
 *
 * Todo lo que se muestra en el panel pasa por aquí: así los números se ven
 * IGUAL en todas las vistas (mismas unidades, mismos decimales, mismo idioma).
 *
 * REGLA NUMÉRICA ÚNICA (DESIGN §3): punto como separador decimal y SIN
 * separador de millares, en todas las cifras y en todas las vistas. El punto de
 * verdad es `num()`: los demás formateadores de aquí DELEGAN en él en vez de
 * repetir el `.toFixed()`.
 *
 * Motivo: (1) el backend redacta sus propios textos con `{:.1}` (punto) y se
 * enseñan TAL CUAL, así que una coma en la interfaz dejaría en la misma pantalla
 * dos criterios distintos; (2) `27/09, 09:15` usaba la coma como separador de
 * fecha y hora, y con las dos cosas a la vez no se sabe si "262,144" son
 * decimales o millares; (3) estos números se copian a comandos y se comparan con
 * la salida de llama.cpp, que también usa punto.
 */

/**
 * El único sitio del panel que decide cómo se escribe una cifra.
 *
 * `null`/`NaN`/`Infinity` no son un cero: devuelven "—", que en el panel
 * significa "este dato no lo tenemos".
 */
export function num(v: number | null | undefined, decimals = 0): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return v.toFixed(decimals);
}

/** Bytes a la unidad legible, con 1 decimal a partir de GiB. */
export function mb(v: number | null | undefined, decimals = 1): string {
  if (v == null || !Number.isFinite(v)) return "—";
  if (v < 0.1) return "0 MB";
  if (v < 1024) return `${num(v, 0)} MB`;
  const gb = v / 1024;
  if (gb < 1024) return `${num(gb, decimals)} GB`;
  return `${num(gb / 1024, decimals)} TB`;
}

export function gb(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return `${num(v, 1)} GB`;
}

/**
 * Bytes CRUDOS a unidad legible. El inventario nuevo da los tamaños en bytes
 * (`tamano_bytes`), no en MB como el escaneo viejo, así que necesita su propio
 * formateador: se usa la misma base binaria (GiB) que el resto del panel y así
 * "45.1 GB" significa lo mismo en todas las vistas.
 *
 * POR QUÉ BAJA HASTA BYTES: por debajo de 1 MiB esto redondeaba a MB enteros, así
 * que un historial de bash de 927 B —o la copia de un `models.json` de 1,2 KB—
 * salía como «0 MB», que se lee como «aquí no hay nada» y no es verdad. La unidad
 * la elige el tamaño, y solo el cero de verdad se escribe como 0.
 */
export function bLegibles(b: number | null | undefined, decimals = 1): string {
  if (b == null || !Number.isFinite(b)) return "—";
  const gib = b / 1024 ** 3;
  if (gib >= 1) return `${num(gib, decimals)} GB`;
  const mib = b / 1024 ** 2;
  if (mib >= 1) return `${num(mib, 0)} MB`;
  const kib = b / 1024;
  if (kib >= 1) return `${num(kib, 1)} KB`;
  if (b <= 0) return "0 MB";
  return `${num(b, 0)} B`;
}

/**
 * Bytes por SEGUNDO, para caudales (disco y red).
 *
 * POR QUÉ NO VALE `bLegibles`: ese formateador es para TAMAÑOS de fichero y
 * redondea a MB enteros por debajo de 1 GB, así que un disco moviendo 400 kB/s
 * salía como «0 MB/s», indistinguible de un disco parado. Un caudal se lee en la
 * unidad que le toca y con un decimal, que es donde está la información.
 */
export function bPorSegundo(b: number | null | undefined): string {
  if (b == null || !Number.isFinite(b)) return "—";
  const v = Math.abs(b);
  if (v < 1000) return `${num(b, 0)} B/s`;
  if (v < 1000 ** 2) return `${num(b / 1000, v / 1000 < 10 ? 1 : 0)} kB/s`;
  if (v < 1000 ** 3) return `${num(b / 1000 ** 2, v / 1000 ** 2 < 10 ? 1 : 0)} MB/s`;
  return `${num(b / 1000 ** 3, 1)} GB/s`;
}

export function pct(v: number | null | undefined, decimals = 0): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return `${num(v, decimals)}%`;
}

/** Watios y temperaturas: sin decimales, que no aportan nada. */
export const w = (v: number | null | undefined) => (v == null ? "—" : `${Math.round(v)} W`);
export const c = (v: number | null | undefined) => (v == null ? "—" : `${Math.round(v)} °C`);
export const mhz = (v: number | null | undefined) => (v == null ? "—" : `${Math.round(v)} MHz`);

/** Duración en formato corto: 3d 4h, 5h 12m, 47s. */
export function dur(secs: number | null | undefined): string {
  if (secs == null || !Number.isFinite(secs) || secs < 0) return "—";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/**
 * Hace cuánto, en palabras cortas: "hace 4 min", "hace 3 h", "hace 2 d".
 *
 * Existe porque un encaje guardado no vale igual de reciente que de hace media
 * hora: la memoria libre de la GPU cambia sola. Sin la edad, un número viejo se
 * lee como si fuera de ahora.
 */
export function hace(ts: number | null | undefined, ahoraMs = Date.now()): string {
  const seg = segundosDesde(ts, ahoraMs);
  if (seg == null) return "sin fecha";
  if (seg < 45) return "hace unos segundos";
  const min = Math.round(seg / 60);
  if (min < 60) return `hace ${min} min`;
  const h = Math.floor(min / 60);
  if (h < 24) return `hace ${h} h`;
  return `hace ${Math.floor(h / 24)} d`;
}

/** Segundos transcurridos desde un `ts` en epoch SEGUNDOS (`null` si no hay). */
export function segundosDesde(ts: number | null | undefined, ahoraMs = Date.now()): number | null {
  if (ts == null || !Number.isFinite(ts) || ts <= 0) return null;
  return Math.max(0, Math.round(ahoraMs / 1000 - ts));
}

/** Hora local corta, para la columna de tiempo de los registros. */
export function hora(ts: number | null | undefined): string {
  if (!ts) return "—";
  const d = new Date(ts > 1e12 ? ts : ts * 1000);
  return d.toLocaleTimeString("es-ES", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/**
 * Fecha y hora cortas, en hora local.
 *
 * Sin coma entre las dos: la regla numérica del panel (DESIGN §3) dice que la
 * coma no se usa como separador en ninguna cifra, y el `toLocaleString("es-ES")`
 * la metía justo ahí ("27/09, 09:15"). Se compone a mano para que no dependa del
 * idioma del sistema y salga igual en cualquier equipo.
 */
export function fechaHora(ts: number | null | undefined): string {
  if (!ts) return "—";
  const d = new Date(ts > 1e12 ? ts : ts * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getDate())}/${p(d.getMonth() + 1)} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Semáforo de un porcentaje: los umbrales son los mismos en todo el panel. */
export function nivelUso(p: number | null | undefined): "ok" | "warn" | "bad" | "idle" {
  if (p == null || !Number.isFinite(p)) return "idle";
  if (p >= 90) return "bad";
  if (p >= 75) return "warn";
  return "ok";
}

export const claseNivel: Record<string, string> = {
  ok: "text-ok",
  warn: "text-warn",
  bad: "text-bad",
  idle: "text-fg-faint",
};

/**
 * Estado de un modelo de servidor, traducido para leerlo.
 *
 * El valor lo pone el motor: en llama-swap es `unloaded`/`loading`/`loaded`, pero
 * otro motor puede publicar otra cosa. La regla que importa: la CADENA VACÍA
 * significa "el servidor no lo dice", así que devuelve `null` (sin distintivo) y
 * nunca "descargado", que sería afirmar algo que no sabemos. Un valor
 * desconocido se enseña tal cual, sin inventarse traducción.
 */
export function estadoModelo(estado: string): string | null {
  if (estado === "loaded") return "cargado";
  if (estado === "unloaded") return "descargado";
  if (estado === "loading") return "cargando";
  if (estado === "") return null;
  return estado;
}
