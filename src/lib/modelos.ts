/**
 * Cosas comunes a los modelos del inventario y a los recomendados.
 *
 * Existe para que la etiqueta y el color de un TIPO sean los mismos en Modelos,
 * en Inventario y en Recomendados, y para que el cruce "esto recomendado, ¿ya lo
 * tengo?" se calcule UNA vez con la misma regla en todas partes.
 */
import type { FitRow, LlmfitModelo, ModeloInventario, TipoModelo } from "./tauri";
import type { Tono } from "../components/ui";

/** Orden de los tipos al agrupar: de lo más habitual en un equipo a lo más raro. */
export const TIPOS_MODELO: TipoModelo[] = [
  "texto",
  "vision",
  "audio",
  "imagen",
  "video",
  "embedding",
  "adaptador",
  "vae",
  "codificador",
  "control",
  "otro",
];

/** Cómo se lee cada tipo en la interfaz. La clave cruda solo se enseña si es desconocida. */
export const NOMBRE_TIPO: Record<string, string> = {
  texto: "texto",
  vision: "visión",
  audio: "audio",
  imagen: "imagen",
  video: "vídeo",
  embedding: "embedding",
  adaptador: "adaptador",
  vae: "VAE",
  codificador: "codificador",
  control: "control",
  otro: "otro",
};

/**
 * Un tipo desconocido (una versión futura del backend, o una raíz de la variable
 * `MACHINOGRAPH_MODEL_DIRS`) se enseña TAL CUAL: nunca se traduce a la fuerza ni se
 * esconde, que sería afirmar algo que no sabemos.
 */
export const etiquetaTipo = (tipo: string): string => NOMBRE_TIPO[tipo] ?? tipo;

/** El distintivo del tipo lleva color, pero con contención: el acento marca lo "multimedia". */
export const tonoTipo = (tipo: string): Tono => {
  if (tipo === "vision" || tipo === "imagen" || tipo === "video") return "acento";
  if (tipo === "audio") return "ok";
  return "neutro";
};

/* ── Encaje automático (tabla `fits`) ─────────────────────────────────────── */

/**
 * Cómo se lee cada nivel de encaje.
 *
 * `Error` no se traduce a "sin dato" ni a "no cabe": es un cálculo que se intentó
 * y no salió, y su motivo va en `detalle`. Y "cabe" se dice con matiz: `Gpu` es
 * todo en la GPU; `Mixto` funciona, pero con parte de las capas en CPU y mucho
 * más lento. Confundir los dos haría prometer una velocidad que no es.
 */
export const TEXTO_ENCAJE: Record<string, string> = {
  Gpu: "cabe en la GPU",
  Mixto: "mixto",
  NoCabe: "no cabe",
  Error: "no se pudo calcular",
};

/** Un valor de encaje desconocido (backend futuro) se enseña tal cual. */
export const etiquetaEncaje = (encaje: string): string => TEXTO_ENCAJE[encaje] ?? encaje;

/** El color del encaje sale del propio valor; nunca va solo, siempre con texto. */
export const tonoEncaje = (encaje: string): Tono => {
  if (encaje === "Gpu") return "ok";
  if (encaje === "Mixto") return "warn";
  if (encaje === "NoCabe" || encaje === "Error") return "bad";
  return "neutro";
};

/**
 * A partir de aquí, un encaje guardado se marca como VIEJO.
 *
 * El backend recalcula cada 10 minutos, así que pasados 15 el dato no puede ser
 * de la última vuelta: o el bucle se atascó o el modelo dejó de ser encajable.
 * Se sigue enseñando —es lo último que se sabe— pero diciendo que es viejo.
 */
export const ENCAJE_VIEJO_SEG = 900;

/** El encaje de un modelo concreto, buscando por ruta completa. */
export function encajeDe(fits: FitRow[] | null, ruta: string): FitRow | null {
  return (fits ?? []).find((f) => f.modelo === ruta) ?? null;
}

/** Cuánto contexto enseña la cabecera de encaje. `0` solo pasa en un error. */
export function textoTopeCtx(f: FitRow): string {
  if (f.encaje === "Error" || f.ctx_max <= 0) return "—";
  return `${f.ctx_max} tok`;
}

/* ── Cruce inventario ↔ recomendados ───────────────────────────────────────── */

/**
 * Palabras que NO identifican a un modelo (formato, sufijos comerciales y
 * cuantizaciones): si contaran, dos modelos distintos parecerían el mismo.
 */
const RUIDO = new Set([
  "gguf", "safetensors", "onnx", "model", "models", "instruct", "chat",
  "hf", "it", "base", "preview", "beta", "ggml", "unsloth", "quant", "the", "and",
]);

/** Parte un nombre en señales útiles: minúsculas, sin ruta, sin extensión ni ruido. */
function señales(nombre: string): string[] {
  const base = nombre.split(/[\\/]/).pop() ?? nombre;
  return base
    .toLowerCase()
    .replace(/\.[a-z0-9]{1,5}$/, "")
    .split(/[^a-z0-9]+/)
    .filter(
      (t) =>
        t.length >= 2 &&
        !RUIDO.has(t) &&
        !/^q\d/.test(t) &&
        !/^iq\d/.test(t) &&
        !/^f\d+$/.test(t),
    );
}

/**
 * ¿Este recomendado de llmfit ya está en tu inventario? Devuelve el fichero que
 * coincide, o `null`.
 *
 * Es a propósito una COINCIDENCIA, no una certeza: llmfit da un nombre de modelo
 * (a veces con el autor delante) y el inventario da un nombre de FICHERO con su
 * cuantización. Por eso se compara por señales del nombre y, cuando se conoce el
 * tamaño, se usa para corroborar. La interfaz lo dice así ("parece que ya lo
 * tienes"), nunca lo afirma como un hecho.
 */
export function coincideConInventario(
  modelo: LlmfitModelo,
  inventario: ModeloInventario[],
): ModeloInventario | null {
  const objetivo = señales(modelo.name);
  if (objetivo.length === 0) return null;
  const bytesEsperados = modelo.disk_size_gb != null ? modelo.disk_size_gb * 1e9 : null;

  let mejor: { fichero: ModeloInventario; puntos: number } | null = null;
  for (const fichero of inventario) {
    const señalesFichero = new Set(señales(fichero.nombre));
    const compartidas = objetivo.filter((t) => señalesFichero.has(t)).length;
    if (compartidas === 0) continue;
    const cobertura = compartidas / objetivo.length;
    // El tamaño corrobora: si cuadra, basta con la mitad de las señales; si no se
    // conoce, se exige un nombre claramente igual (¾) para no dar falsos positivos.
    const tamanoCuadra =
      bytesEsperados != null &&
      fichero.tamano_bytes > 0 &&
      Math.abs(fichero.tamano_bytes - bytesEsperados) /
        Math.max(fichero.tamano_bytes, bytesEsperados) <
        0.4;
    const acepta = tamanoCuadra ? cobertura >= 0.5 : cobertura >= 0.75;
    if (!acepta) continue;
    const puntos = cobertura + (tamanoCuadra ? 1 : 0);
    if (!mejor || puntos > mejor.puntos) mejor = { fichero, puntos };
  }
  return mejor?.fichero ?? null;
}

/**
 * ¿Se puede decir que el recomendado ya está en el equipo?
 *
 * `installed` es el dato de LLMFIT (su propia comprobación, que hace por su
 * cuenta); la coincidencia con NUESTRO inventario es otra vía. Se acepta
 * cualquiera de las dos, y la interfaz distingue de dónde sale cada una.
 */
export function estaEnEquipo(
  modelo: LlmfitModelo,
  inventario: ModeloInventario[],
): { enEquipo: boolean; fichero: ModeloInventario | null; porLlmfit: boolean } {
  const fichero = coincideConInventario(modelo, inventario);
  return { enEquipo: modelo.installed || fichero != null, fichero, porLlmfit: modelo.installed };
}
