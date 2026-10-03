/**
 * Recomendados: qué modelos le encajan a ESTE equipo, según llmfit.
 *
 * Quién contesta: **llmfit** (`AlexsJones/llmfit`, MIT), una herramienta aparte,
 * no algo que Machinograph pueda calcular por su cuenta. Si no está instalada, esta
 * vista lo dice y enlaza a su repositorio: inventar una lista sin ella sería
 * mentir.
 *
 * LA HONESTIDAD DE LOS NÚMEROS: llmfit ESTIMA los tok/s con el ancho de banda
 * TEÓRICO de la GPU; Machinograph los MIDE de verdad con `llama-bench` (vista
 * Rendimiento). Son dos cosas distintas, así que cada cifra lleva su etiqueta
 * —*estimada* o *medida*— y nunca se enseña una como si fuera la otra.
 *
 * ORDEN DE LA VISTA (a propósito): 1) el perfil del equipo, compacto; 2) los
 * filtros; 3) los resultados. El detalle largo de "cómo leer esto" va en un
 * desplegable, no delante.
 *
 * Y no se llama a llmfit en bucle: tarda ~0,6 s, así que se le pregunta al abrir
 * la vista y cuando cambia un filtro de SU CLI (caso de uso, encaje, capacidad,
 * límite, comando). Los filtros de licencia y tamaño, y los conmutadores "me
 * caben" / "ya lo tengo", se aplican en la interfaz sobre lo que llmfit ya
 * devolvió: no hace falta volver a preguntarle.
 */
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import {
  IconAlertTriangle,
  IconBox,
  IconChevronRight,
  IconDownload,
  IconGauge,
  IconRefresh,
  IconSparkles,
  IconTerminal2,
} from "@tabler/icons-react";
import { useApp, ejecutar, type ResultadoAccion } from "../store";
import { Boton, Card, Etiqueta, Insignia, ThOrden, Vacio, type Dir } from "../components/ui";
import { api, type LlmfitEstado, type LlmfitModelo, type LlmfitSistema } from "../lib/tauri";
import { bLegibles, gb, num } from "../lib/format";
import { estaEnEquipo } from "../lib/modelos";
import { RecomendacionPreferida, puntuacion } from "../components/PerfilModelo";
import { PanelDescarga } from "../components/Descarga";

/**
 * Los filtros que acepta la CLI de llmfit, con la CLAVE corta que espera y la
 * ETIQUETA que él mismo devuelve en `use_case`.
 *
 * Ojo: `--use-case` espera la clave corta (`multimodal`). Pasarle la etiqueta
 * larga NO da error —la IGNORA en silencio y devuelve la lista sin filtrar—,
 * igual que un valor inventado (comprobado con llmfit 1.1.16). Por eso se manda
 * la clave y se enseña la etiqueta.
 */
const CASOS_USO: { clave: string; etiqueta: string }[] = [
  { clave: "multimodal", etiqueta: "Multimodal, vision and text" },
  { clave: "reasoning", etiqueta: "Advanced reasoning, chain-of-thought" },
  { clave: "coding", etiqueta: "Code generation and completion" },
  { clave: "chat", etiqueta: "Instruction following, chat" },
  { clave: "general", etiqueta: "General purpose text generation" },
  { clave: "embedding", etiqueta: "Text embeddings for RAG" },
];

/** `--min-fit` espera la clave en minúsculas; el nivel llega como `Perfect`… */
const ENCAJES: { clave: string; etiqueta: string }[] = [
  { clave: "perfect", etiqueta: "Perfect" },
  { clave: "good", etiqueta: "Good" },
  { clave: "marginal", etiqueta: "Marginal" },
];

/** `--capability` espera estos identificadores, separados por coma si son varios. */
const CAPACIDADES: { clave: string; etiqueta: string }[] = [
  { clave: "vision", etiqueta: "visión" },
  { clave: "tool_use", etiqueta: "uso de herramientas" },
  { clave: "audio", etiqueta: "audio" },
  { clave: "tts", etiqueta: "texto a voz" },
];

/**
 * Opciones del desplegable de límite: un campo de texto llamaba a llmfit por cada
 * tecla. El valor por defecto (40, el mismo que usa el backend cuando no se le
 * dice ninguno) está en `UI_INICIAL` de la tienda, con el resto de filtros.
 */
const LIMITES = [10, 20, 40, 60, 100];

/** El rango de tamaño en parámetros (B). Se aplica en la interfaz, no en llmfit. */
const RANGOS: { clave: string; etiqueta: string }[] = [
  { clave: "", etiqueta: "cualquier tamaño" },
  { clave: "-3", etiqueta: "hasta 3 B" },
  { clave: "3-8", etiqueta: "3–8 B" },
  { clave: "8-20", etiqueta: "8–20 B" },
  { clave: "20-70", etiqueta: "20–70 B" },
  { clave: "70-", etiqueta: "más de 70 B" },
];

/** Cómo se ordena la tabla. Antes era un desplegable; ahora manda la cabecera. */
const COLUMNAS = ["nombre", "params", "caso", "encaje", "quant", "tps", "tamano", "nota", "ajuste"] as const;
type Col = (typeof COLUMNAS)[number];

/** Valida lo que venga del estado guardado (una columna borrada no puede romper la vista). */
const esCol = (v: string): v is Col => (COLUMNAS as readonly string[]).includes(v);

/**
 * Orden de los niveles de encaje de llmfit, de mejor a peor.
 *
 * Un nivel que NO esté aquí (una versión futura de llmfit puede añadir uno) se
 * trata como "sin dato" y se va al final: no se puede afirmar que sea mejor ni
 * peor que los que conocemos.
 */
const RANGO_ENCAJE: Record<string, number> = { Perfect: 4, Good: 3, Marginal: 2, Poor: 1 };

function rangoEncaje(nivel: string | null | undefined): number | null {
  if (nivel == null || nivel === "") return null;
  return RANGO_ENCAJE[nivel] ?? null;
}

/** Numérico con los nulos SIEMPRE al final (no valen como 0). */
function numero(a: number | null, b: number | null, dir: Dir): number {
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  return dir === "asc" ? a - b : b - a;
}

function texto(a: string, b: string, dir: Dir): number {
  const r = a.localeCompare(b);
  return dir === "asc" ? r : -r;
}

/** El valor numérico de una fila en una columna (o `null` si no tiene dato). */
/**
 * El valor de la columna «Ajuste» para un modelo: la puntuación según la
 * preferencia. Se calcula en el orden para poder reordenar la tabla sin volver a
 * pedir nada a llmfit (los componentes ya están en la respuesta).
 */
function valorAjuste(m: LlmfitModelo, preferencia: number): number | null {
  return puntuacion(m, preferencia);
}

function valorNumerico(m: LlmfitModelo, col: Col): number | null {
  switch (col) {
    case "params":
      // `params_b` a 0 significa "no lo dice", no "cero parámetros".
      return m.params_b > 0 ? m.params_b : null;
    case "tps":
      return m.measured_tps ?? m.estimated_tps;
    case "tamano":
      return m.disk_size_gb;
    case "nota":
      return m.score;
    case "encaje":
      return rangoEncaje(m.fit_level);
    default:
      return null;
  }
}

/** El color del nivel de encaje sale del propio valor que da llmfit. */
function tonoEncaje(nivel: string): "ok" | "acento" | "warn" | "neutro" {
  if (nivel === "Perfect") return "ok";
  if (nivel === "Good") return "acento";
  if (nivel === "Marginal" || nivel === "Poor") return "warn";
  return "neutro";
}

/**
 * El nivel de encaje será lo que marque la lectura rápida, así que se usa para el
 * aviso del conmutador "solo los que me caben": caben de verdad los Perfect y los
 * Good; Marginal es al límite y Poor/desconocido no se asegura.
 */
const cabeSeguro = (m: LlmfitModelo): boolean => m.fit_level === "Perfect" || m.fit_level === "Good";

/** La velocidad de un modelo, SIEMPRE con su procedencia. */
function Velocidad({ m }: { m: LlmfitModelo }) {
  const tps = m.measured_tps ?? m.estimated_tps;
  if (tps == null) return <span className="text-fg-faint">—</span>;
  const medida = m.measured_tps != null || m.estimate_confidence === "measured";
  // Una confianza distinta se enseña tal cual: puede ser un valor nuevo de una
  // versión futura de llmfit y no se traduce a la fuerza.
  const conf = m.estimate_confidence;
  const raro = conf != null && conf !== "estimated" && conf !== "measured";
  return (
    <span className="flex items-center gap-1.5 whitespace-nowrap">
      <span className="mono">{medida ? num(tps, 1) : `~${num(tps, 1)}`}</span>
      <Insignia tono={medida ? "ok" : "neutro"}>
        <span
          title={
            medida
              ? "Medida con llama-bench: es una medición real de este equipo."
              : `Estimada por llmfit con el ancho de banda teórico de la GPU (no es una medición). ${
                  m.verify_command ? `Para verificarla, propone: ${m.verify_command}` : ""
                }`
          }
        >
          {medida ? "medida" : raro ? conf : "estimada"}
        </span>
      </Insignia>
    </span>
  );
}

/** Perfil de hardware según llmfit, COMPACTO: una o dos líneas de datos. */
function PerfilCompacto({ s }: { s: LlmfitSistema }) {
  const gpu = [s.gpu_name || "—", s.gpu_vram_gb > 0 ? gb(s.gpu_vram_gb) : null, s.gpu_count > 1 ? `${s.gpu_count} GPUs` : null]
    .filter((x): x is string => x != null)
    .join(" · ");
  return (
    <Card>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
        <span className="flex items-center gap-1.5">
          <IconSparkles size={14} className="text-accent" aria-hidden="true" />
          <Etiqueta>Equipo según llmfit</Etiqueta>
        </span>
        <span className="mono text-fg-muted">
          CPU {s.cpu_name || "—"}
          {s.cpu_cores > 0 ? ` · ${s.cpu_cores} núcleos` : ""}
        </span>
        <span className="text-fg-faint" aria-hidden="true">
          ·
        </span>
        <span className="mono text-fg-muted">RAM {s.available_ram_gb > 0 ? gb(s.available_ram_gb) : "—"}</span>
        <span className="text-fg-faint" aria-hidden="true">
          ·
        </span>
        <span className="mono text-fg-muted">{s.backend || "—"}</span>
        <span className="text-fg-faint" aria-hidden="true">
          ·
        </span>
        <span className="mono text-fg-muted">GPU {gpu}</span>
      </div>
      {/* Con memoria unificada el modelo no "cabe o no cabe" en una VRAM aparte:
          comparte la RAM, y callarlo daría una idea equivocada del encaje. */}
      {s.gpus.some((g) => g.unified_memory) ? (
        <p className="text-fg-faint mt-1 text-xs">
          Alguna GPU usa memoria unificada: comparte la RAM en vez de tener VRAM propia.
        </p>
      ) : null}
      {s.gpus.length > 1 ? (
        <ul className="text-fg-faint mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-xs">
          {s.gpus.map((g, i) => (
            <li key={`${g.name}-${i}`} className="mono truncate" title={g.name}>
              {g.name} · {gb(g.vram_gb)} · {g.backend || "—"}
            </li>
          ))}
        </ul>
      ) : null}
    </Card>
  );
}

/** ¿Cae el modelo dentro del rango elegido? La clave vacía no filtra nada. */
function enRango(paramsB: number, clave: string): boolean {
  if (!clave) return true;
  if (!(paramsB > 0)) return false; // sin dato de tamaño no se puede asegurar que caiga dentro
  if (clave === "-3") return paramsB <= 3;
  if (clave === "70-") return paramsB > 70;
  const [min, max] = clave.split("-").map(Number);
  return paramsB > min && paramsB <= max;
}

export default function Descubrir() {
  const setVista = useApp((st) => st.setVista);
  const medidas = useApp((st) => st.benchmarks);
  const inventario = useApp((st) => st.inventario);
  const cargarInventario = useApp((st) => st.cargarInventario);
  const linea = useApp((st) => st.lineaAccion);
  const enCurso = useApp((st) => st.accionEnCurso);

  const [estado, setEstado] = useState<LlmfitEstado | null>(null);
  const [errorEstado, setErrorEstado] = useState<string | null>(null);
  const [sistema, setSistema] = useState<LlmfitSistema | null>(null);

  const [modelos, setModelos] = useState<LlmfitModelo[] | null>(null);
  const [cargando, setCargando] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /**
   * Filtros y orden: viven en la tienda, no aquí.
   *
   * Al cambiar de sección la vista se desmonta, así que un `useState` local se
   * perdería y al volver aparecería todo en blanco. Lo que NO se guarda es lo que
   * es resultado de una lectura (`modelos`, `error`, el detalle desplegado y el
   * último mensaje de una descarga): eso se vuelve a pedir o no tiene sentido
   * conservarlo.
   */
  const ui = useApp((st) => st.ui.descubrir);
  const setUi = useApp((st) => st.setUi);

  // Filtros que SÍ se le mandan a llmfit (su CLI).
  const { casoUso, encajeMinimo, capacidad, conComando, limite } = ui;
  // Filtros y orden que se aplican en la interfaz sobre lo ya devuelto.
  const { licencia, rango, soloCaben, soloTengo } = ui;
  const col: Col = esCol(ui.col) ? ui.col : "nota";

  /** Claves (`proveedor|nombre`) de las filas con el detalle desplegado. */
  const [expandidos, setExpandidos] = useState<Set<string>>(new Set());
  /** Último resultado por modelo (el evento `ai:action` es global y no dice de cuál era). */
  const [resultados, setResultados] = useState<Record<string, ResultadoAccion>>({});

  // Para cruzar "ya lo tengo" hace falta el inventario; se pide si no está.
  useEffect(() => {
    if (inventario == null) void cargarInventario();
  }, [inventario, cargarInventario]);

  // ── Estado de llmfit y perfil de hardware: una vez, al abrir la vista ──────
  useEffect(() => {
    let vivo = true;
    api.llmfit
      .estado()
      .then((e) => {
        if (!vivo) return;
        setEstado(e);
        setErrorEstado(null);
        if (e.instalado)
          return api.llmfit.sistema().then((s) => {
            if (vivo) setSistema(s);
          });
      })
      .catch((e) => {
        if (vivo) setErrorEstado(String(e));
      });
    return () => {
      vivo = false;
    };
  }, []);

  // ── Recomendaciones: al abrir (si hay llmfit) y al cambiar un filtro ───────
  const consultar = useCallback(() => {
    let vivo = true;
    setCargando(true);
    api.llmfit
      .recomendar({
        limit: limite,
        ...(casoUso ? { useCase: casoUso } : {}),
        ...(encajeMinimo ? { minFit: encajeMinimo } : {}),
        ...(capacidad ? { capability: capacidad } : {}),
        ...(conComando ? { conComando: true } : {}),
      })
      .then((r) => {
        if (!vivo) return;
        setModelos(r.modelos);
        setSistema(r.sistema);
        setError(null);
      })
      .catch((e) => {
        if (vivo) setError(String(e));
      })
      .finally(() => {
        if (vivo) setCargando(false);
      });
    return () => {
      vivo = false;
    };
  }, [limite, casoUso, encajeMinimo, capacidad, conComando]);

  useEffect(() => {
    if (estado?.instalado !== true) return;
    return consultar();
  }, [estado?.instalado, consultar]);

  const lista = useMemo(() => modelos ?? [], [modelos]);

  const licencias = useMemo(
    () => [...new Set(lista.map((m) => m.license).filter((x): x is string => x != null))].sort(),
    [lista],
  );

  /** Ordena dejando SIEMPRE al final lo que no tiene dato (no vale como 0). */
  const visibles = useMemo(() => {
    const filtrados = lista.filter((m) => {
      if (licencia && m.license !== licencia) return false;
      if (!enRango(m.params_b, rango)) return false;
      if (soloCaben && !cabeSeguro(m)) return false;
      if (soloTengo && !estaEnEquipo(m, inventario ?? []).enEquipo) return false;
      return true;
    });
    const porNombre = (a: LlmfitModelo, b: LlmfitModelo) => texto(a.name, b.name, "asc");
    const cmp = (a: LlmfitModelo, b: LlmfitModelo): number => {
      if (col === "nombre") {
        return texto(a.name, b.name, ui.dir) || texto(a.provider ?? "", b.provider ?? "", ui.dir);
      }
      // El caso de uso y la categoría son el MISMO dato en dos sitios de llmfit:
      // se ordena por lo que se enseña en la celda, que es esta misma regla.
      if (col === "caso") {
        return (
          texto(a.use_case ?? a.category ?? "", b.use_case ?? b.category ?? "", ui.dir) || porNombre(a, b)
        );
      }
      if (col === "quant") {
        return texto(a.best_quant ?? "", b.best_quant ?? "", ui.dir) || porNombre(a, b);
      }
      // La columna «Ajuste» depende del deslizador, así que se calcula con él
      // dentro (no con `valorNumerico`, que no lo conoce).
      if (col === "ajuste") {
        return (
          numero(valorAjuste(a, ui.preferencia), valorAjuste(b, ui.preferencia), ui.dir) || porNombre(a, b)
        );
      }
      return numero(valorNumerico(a, col), valorNumerico(b, col), ui.dir) || porNombre(a, b);
    };
    return [...filtrados].sort(cmp);
  }, [lista, licencia, rango, soloCaben, soloTengo, col, ui.dir, inventario, ui.preferencia]);

  const hayFiltroCli = casoUso !== "" || encajeMinimo !== "" || capacidad !== "";
  const hayFiltroLocal = licencia !== "" || rango !== "" || soloCaben || soloTengo;
  const enDisco = useMemo(
    () => lista.filter((m) => estaEnEquipo(m, inventario ?? []).enEquipo).length,
    [lista, inventario],
  );

  const alternar = (clave: string) =>
    setExpandidos((prev) => {
      const n = new Set(prev);
      if (n.has(clave)) n.delete(clave);
      else n.add(clave);
      return n;
    });

  /**
   * Descarga el GGUF con llmfit, por el GESTOR de descargas.
   *
   * Por qué no la acción que vuelca líneas: una descarga son varios GB y media
   * hora, y así el progreso va a su panel (barra, velocidad medida, tiempo que
   * queda y un botón de cancelar) en vez de a un texto que se llena y se pierde.
   * El backend rechaza una segunda descarga a la vez, por este botón o por
   * cualquier otro camino.
   *
   * Si `best_quant` es `null`, NO se inventa una cuantización: se llama sin
   * `quant` y llmfit elige la que mejor encaja.
   */
  const descargar = async (m: LlmfitModelo) => {
    try {
      const mensaje = await api.descarga.iniciar(m.name, m.best_quant ?? undefined);
      setResultados((prev) => ({ ...prev, [m.name]: { ok: true, mensaje } }));
    } catch (e) {
      setResultados((prev) => ({ ...prev, [m.name]: { ok: false, mensaje: String(e) } }));
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {/* ── llmfit instalado (o no) ─────────────────────────────────────── */}
      {errorEstado ? (
        <Vacio titulo="No se pudo comprobar si llmfit está instalado">{errorEstado}</Vacio>
      ) : estado == null ? (
        <Vacio titulo="Comprobando llmfit…" />
      ) : !estado.instalado ? (
        <Card className="border-warn/40">
          <div className="flex items-start gap-2">
            <IconAlertTriangle size={16} className="text-warn mt-0.5 shrink-0" aria-hidden="true" />
            <div className="flex min-w-0 flex-col gap-1">
              <p className="text-sm" role="status">
                <strong className="text-warn">llmfit no está instalado</strong>, así que no hay recomendaciones
                que enseñar. Es una herramienta aparte (MIT): Machinograph no puede calcular esto por su cuenta.
              </p>
              <p className="text-fg-muted text-xs">
                Instálala y vuelve a esta vista:{" "}
                <a
                  href="https://github.com/AlexsJones/llmfit"
                  target="_blank"
                  rel="noreferrer"
                  className="text-accent underline"
                >
                  github.com/AlexsJones/llmfit
                </a>
              </p>
            </div>
          </div>
        </Card>
      ) : (
        <>
          {/* ── 1) Perfil y cómo leer los números ────────────────────────── */}
          <div className="flex flex-wrap items-center gap-2">
            <Insignia tono="ok">
              llmfit {estado.version ?? ""} {estado.binario ? `· ${estado.binario}` : ""}
            </Insignia>
          </div>
          {sistema ? <PerfilCompacto s={sistema} /> : null}

          {/* El panel de descarga va ARRIBA de todo: si hay algo bajando, es lo
              primero que se quiere ver (y poder cortar). Cuando no hay nada, no
              se pinta. */}
          <PanelDescarga />

          {/* ── 2) Lo que mejor le sienta al equipo, según lo que priorices ── */}
          <RecomendacionPreferida
            modelo={visibles.find((m) => valorAjuste(m, ui.preferencia) != null) ?? null}
            preferencia={ui.preferencia}
            onPreferencia={(v) => setUi("descubrir", { preferencia: v })}
            total={visibles.length}
          />

          <div className="text-fg-muted text-xs">
            <p>
              Los <strong className="text-fg">tok/s</strong> van marcados como <em>estimada</em> (llmfit, con el
              ancho de banda teórico de la GPU) o <em>medida</em> (Machinograph con{" "}
              <code className="mono">llama-bench</code>): no son comparables entre sí.
            </p>
            {/* El detalle largo va plegado: delante solo lo que hace falta para leer la tabla. */}
            <details className="mt-1">
              <summary className="text-accent cursor-pointer">Cómo leer los números</summary>
              <div className="text-fg-muted mt-1 flex flex-col items-start gap-2">
                <span>
                  La lista la calcula <strong className="text-fg">llmfit</strong> (herramienta aparte, MIT), que
                  perfila tu hardware y dice qué modelos le encajan; Machinograph solo la enseña. La{" "}
                  <strong className="text-fg">estimación</strong> proyecta los tok/s con el ancho de banda
                  <em> teórico</em> de la GPU; la <strong className="text-fg">medición</strong> es real, de este
                  equipo. La <strong className="text-fg">nota</strong> es de llmfit y sale de sus cuatro componentes
                  (calidad, velocidad, encaje y contexto), que están en el detalle de cada fila.{" "}
                  {medidas.length > 0
                    ? `Machinograph tiene ${medidas.length} medida(s) propias guardadas.`
                    : "Machinograph todavía no tiene medidas propias guardadas."}
                </span>
                <Boton onClick={() => setVista("rendimiento")}>
                  <IconGauge size={12} className="mr-1 inline" aria-hidden="true" />
                  Ir a Rendimiento
                </Boton>
              </div>
            </details>
          </div>

          {/* ── 2) Filtros ───────────────────────────────────────────────── */}
          <section className="flex flex-col gap-3">
            <Etiqueta>Filtros</Etiqueta>
            <Card>
              <div className="flex flex-wrap items-end gap-3">
                <label className="text-fg-muted flex flex-col gap-1 text-xs">
                  Caso de uso
                  <select
                    value={casoUso}
                    onChange={(e) => setUi("descubrir", { casoUso: e.target.value })}
                    className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
                  >
                    <option value="">todos</option>
                    {CASOS_USO.map((c) => (
                      <option key={c.clave} value={c.clave}>
                        {c.etiqueta}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="text-fg-muted flex flex-col gap-1 text-xs">
                  Encaje mínimo
                  <select
                    value={encajeMinimo}
                    onChange={(e) => setUi("descubrir", { encajeMinimo: e.target.value })}
                    className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
                  >
                    <option value="">cualquiera</option>
                    {ENCAJES.map((c) => (
                      <option key={c.clave} value={c.clave}>
                        {c.etiqueta}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="text-fg-muted flex flex-col gap-1 text-xs">
                  Capacidad
                  <select
                    value={capacidad}
                    onChange={(e) => setUi("descubrir", { capacidad: e.target.value })}
                    className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
                  >
                    <option value="">cualquiera</option>
                    {CAPACIDADES.map((c) => (
                      <option key={c.clave} value={c.clave}>
                        {c.etiqueta}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="text-fg-muted flex flex-col gap-1 text-xs">
                  Licencia
                  <select
                    value={licencia}
                    onChange={(e) => setUi("descubrir", { licencia: e.target.value })}
                    className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
                  >
                    <option value="">todas</option>
                    {licencias.map((l) => (
                      <option key={l} value={l}>
                        {l}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="text-fg-muted flex flex-col gap-1 text-xs">
                  Tamaño (params)
                  <select
                    value={rango}
                    onChange={(e) => setUi("descubrir", { rango: e.target.value })}
                    className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
                  >
                    {RANGOS.map((r) => (
                      <option key={r.clave} value={r.clave}>
                        {r.etiqueta}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="text-fg-muted flex flex-col gap-1 text-xs">
                  Máximo de resultados
                  <select
                    value={String(limite)}
                    onChange={(e) => setUi("descubrir", { limite: Number(e.target.value) })}
                    className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
                  >
                    {LIMITES.map((n) => (
                      <option key={n} value={n}>
                        {n}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="text-fg-muted flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={conComando}
                    onChange={(e) => setUi("descubrir", { conComando: e.target.checked })}
                  />
                  Incluir comando de llama.cpp
                </label>

                {/* Con los filtros guardados entre vistas hace falta una salida
                    clara: si no, al volver parece que llmfit devuelve poco. */}
                {hayFiltroCli || hayFiltroLocal ? (
                  <Boton
                    onClick={() =>
                      setUi("descubrir", {
                        casoUso: "",
                        encajeMinimo: "",
                        capacidad: "",
                        licencia: "",
                        rango: "",
                        soloCaben: false,
                        soloTengo: false,
                      })
                    }
                  >
                    Limpiar filtros
                  </Boton>
                ) : null}

                <Boton className="ml-auto" disabled={cargando} onClick={consultar}>
                  <IconRefresh size={12} className="mr-1 inline" aria-hidden="true" />
                  {cargando ? "Consultando…" : "Reconsultar"}
                </Boton>
              </div>

              {/* Los dos conmutadores que cruzan con TU equipo. */}
              <div className="border-line-soft mt-3 flex flex-wrap items-center gap-4 border-t pt-2">
                <label className="text-fg-muted flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={soloCaben}
                    onChange={(e) => setUi("descubrir", { soloCaben: e.target.checked })}
                  />
                  Solo los que me caben
                  <span className="text-fg-faint" title="Caben = llmfit los puntúa Perfect o Good; Marginal es al límite y no se asegura.">
                    (?)
                  </span>
                </label>
                <label className="text-fg-muted flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={soloTengo}
                    onChange={(e) => setUi("descubrir", { soloTengo: e.target.checked })}
                  />
                  Solo los que ya tengo
                  <span className="text-fg-faint" title="Coincidencia aproximada por nombre y, cuando se conoce, por tamaño, cruzando con tu inventario.">
                    (?)
                  </span>
                </label>
              </div>

              <p className="text-fg-faint mt-2 text-xs">
                Caso de uso, encaje, capacidad y límite van a la CLI de llmfit (por eso cada cambio reconsulta, ~0,6 s).
                Licencia, tamaño y los dos conmutadores se aplican aquí sobre lo ya devuelto. La clave del caso de uso
                que se manda es la corta (<code className="mono">multimodal</code>, <code className="mono">coding</code>…):
                con la etiqueta larga, llmfit no da error pero <strong className="text-fg-muted">ignora el filtro</strong>.
              </p>
            </Card>
          </section>

          {/* ── 3) Resultados ────────────────────────────────────────────── */}
          <section className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <Etiqueta>
                {modelos == null ? "Modelos recomendados" : `Modelos recomendados (${visibles.length})`}
              </Etiqueta>
              {cargando ? <Insignia tono="acento">consultando…</Insignia> : null}
              {hayFiltroLocal && modelos != null ? (
                <span className="text-fg-faint text-xs">de {lista.length} que dio llmfit</span>
              ) : null}
              {enDisco > 0 ? <Insignia tono="ok">{enDisco} ya en tu equipo</Insignia> : null}
            </div>

            {/* Tres estados distintos: un fallo NO es una lista vacía. */}
            {error ? (
              <Card className="border-bad/40">
                <p className="text-bad text-sm">llmfit no pudo dar recomendaciones.</p>
                <p className="text-bad mt-1 text-sm" role="alert">
                  {error}
                </p>
              </Card>
            ) : modelos == null ? (
              <Vacio titulo={cargando ? "Consultando a llmfit…" : "Sin consultar"}>
                {cargando ? null : "Pulsa «Reconsultar» para pedir recomendaciones."}
              </Vacio>
            ) : visibles.length === 0 ? (
              <Card>
                <p className="text-fg-muted text-sm">
                  {hayFiltroCli || hayFiltroLocal
                    ? "Ningún modelo encaja con estos filtros. Prueba a quitar alguno."
                    : "llmfit no ha devuelto ningún modelo para este equipo."}
                </p>
              </Card>
            ) : (
              <Card className="overflow-x-auto p-0">
                <table className="w-full min-w-[1040px] text-left text-xs">
                  <caption className="text-fg-faint px-4 pt-3 text-left text-xs">
                    Pulsa el nombre de una columna para ordenar por ella (y otra vez para invertir el sentido);
                    lo que no tiene dato va siempre al final, porque no vale como cero. El nivel de{" "}
                    <strong className="text-fg-muted">encaje</strong> lo dice llmfit. Los detalles secundarios
                    (nota, notas suyas, comandos) están en cada fila, en «Detalles».
                  </caption>
                  <thead className="text-fg-faint border-line-soft border-b">
                    <tr>
                      <ThOrden col="nombre" actual={col} dir={ui.dir} onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}>
                        Modelo
                      </ThOrden>
                      <ThOrden
                        col="params"
                        actual={col}
                        dir={ui.dir}
                        primero="desc"
                        onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}
                      >
                        Params
                      </ThOrden>
                      <ThOrden col="caso" actual={col} dir={ui.dir} onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}>
                        Caso de uso
                      </ThOrden>
                      <ThOrden
                        col="encaje"
                        actual={col}
                        dir={ui.dir}
                        primero="desc"
                        onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}
                      >
                        Encaje
                      </ThOrden>
                      <ThOrden col="quant" actual={col} dir={ui.dir} onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}>
                        Cuantización
                      </ThOrden>
                      <ThOrden
                        col="tps"
                        actual={col}
                        dir={ui.dir}
                        primero="desc"
                        onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}
                      >
                        tok/s
                      </ThOrden>
                      <ThOrden
                        col="tamano"
                        actual={col}
                        dir={ui.dir}
                        primero="desc"
                        alineado="der"
                        onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}
                      >
                        Disco
                      </ThOrden>
                      {/* «Ajuste» es la puntuación según el deslizador de arriba:
                          velocidad y capacidad pesadas por lo que hayas pedido, y
                          multiplicadas por el encaje (un modelo que no cabe no puede
                          ganar por rápido que sea). Se ordena por ella por defecto. */}
                      <ThOrden
                        col="ajuste"
                        actual={col}
                        dir={ui.dir}
                        primero="desc"
                        alineado="der"
                        onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}
                        titulo="Puntuación con el ajuste elegido arriba: velocidad y capacidad pesadas según el deslizador, multiplicadas por el encaje. Sale de los componentes de llmfit, no de una estimación nuestra."
                      >
                        Ajuste
                      </ThOrden>
                      <ThOrden
                        col="nota"
                        actual={col}
                        dir={ui.dir}
                        primero="desc"
                        alineado="der"
                        onOrdenar={(c, d) => setUi("descubrir", { col: c, dir: d })}
                      >
                        Nota
                      </ThOrden>
                      <th scope="col" className="px-4 py-2 text-right font-medium">
                        Acciones
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {visibles.map((m, i) => {
                      const cap = m.capabilities ?? [];
                      const multimodelo = m.category === "Multimodal" || cap.some((c) => /vision/i.test(c));
                      const equipo = estaEnEquipo(m, inventario ?? []);
                      // Clave estable para el desplegable: por proveedor+nombre, no por
                      // índice (el índice cambia al reordenar y el detalle "saltaría").
                      const clave = `${m.provider}|${m.name}`;
                      const idDetalle = `llmfit-det-${clave.replace(/[^a-zA-Z0-9]+/g, "-")}`;
                      const abierto = expandidos.has(clave);
                      const res = resultados[m.name];
                      return (
                        <Fragment key={`${m.name}-${i}`}>
                          <tr className="border-line-soft hover:bg-raised border-b align-top last:border-0">
                            <td className="max-w-[300px] px-4 py-2">
                              <div className="mono truncate" title={m.name}>
                                {m.name || "—"}
                              </div>
                              <div className="mt-1 flex flex-wrap items-center gap-1.5">
                                {m.provider ? <span className="text-fg-faint">{m.provider}</span> : null}
                                {m.license ? <Insignia tono="neutro">{m.license}</Insignia> : null}
                                {m.is_moe ? <Insignia tono="neutro">MoE</Insignia> : null}
                                {multimodelo ? <Insignia tono="acento">multimodal</Insignia> : null}
                                {/* `installed` es el dato de llmfit; la coincidencia con
                                    el inventario es la nuestra. Se distinguen. */}
                                {equipo.porLlmfit ? <Insignia tono="ok">en disco (llmfit)</Insignia> : null}
                                {!equipo.porLlmfit && equipo.fichero ? (
                                  <Insignia tono="acento">
                                    <span title={`Coincide con tu inventario: ${equipo.fichero.ruta}`}>
                                      parece que ya lo tienes
                                    </span>
                                  </Insignia>
                                ) : null}
                              </div>
                            </td>
                            <td className="mono px-4 py-2">
                              <span title={m.params_b > 0 ? `${num(m.params_b, 2)} B` : undefined}>
                                {m.parameter_count || (m.params_b > 0 ? `${num(m.params_b, 2)} B` : "—")}
                              </span>
                            </td>
                            <td className="px-4 py-2">
                              {/* La etiqueta de llmfit es larga ("Code generation and
                                  completion"): se trunca con su `title` para que la fila
                                  no se estire a cuatro líneas y la tabla se lea de un
                                  vistazo. La categoría va en el `title`. */}
                              <div
                                className="max-w-[180px] truncate"
                                title={m.category && m.category !== m.use_case ? `Categoría de llmfit: ${m.category}` : undefined}
                              >
                                {m.use_case || m.category || "—"}
                              </div>
                            </td>
                            <td className="px-4 py-2">
                              {m.fit_level ? <Insignia tono={tonoEncaje(m.fit_level)}>{m.fit_level}</Insignia> : "—"}
                            </td>
                            <td className="mono px-4 py-2">{m.best_quant ?? "—"}</td>
                            <td className="px-4 py-2">
                              <Velocidad m={m} />
                            </td>
                            <td className="mono px-4 py-2 text-right whitespace-nowrap">
                              {m.disk_size_gb != null ? gb(m.disk_size_gb) : "—"}
                            </td>
                            <td className="mono px-4 py-2 text-right whitespace-nowrap">
                              {(() => {
                                const aj = valorAjuste(m, ui.preferencia);
                                return aj == null ? (
                                  <span className="text-fg-faint">—</span>
                                ) : (
                                  <span title="Velocidad y capacidad pesadas según el deslizador, por el encaje">
                                    {num(aj, 1)}
                                  </span>
                                );
                              })()}
                            </td>
                            <td className="mono px-4 py-2 text-right whitespace-nowrap">
                              {m.score != null ? num(m.score, 1) : "—"}
                            </td>
                            <td className="px-4 py-2">
                              <div className="flex flex-col items-end gap-1">
                                <div className="flex items-center gap-1.5">
                                  <Boton
                                    aria-expanded={abierto}
                                    aria-controls={idDetalle}
                                    onClick={() => alternar(clave)}
                                  >
                                    <IconChevronRight
                                      size={12}
                                      className={`mr-1 inline transition-transform ${abierto ? "rotate-90" : ""}`}
                                      aria-hidden="true"
                                    />
                                    Detalles
                                  </Boton>
                                  {/* Si ya lo tienes, NO se ofrece descargar
                                      (serían varios GB para nada): se lleva a Modelos. */}
                                  {equipo.enEquipo ? (
                                    <Boton variante="acento" onClick={() => setVista("disco")}>
                                      <IconBox size={12} className="mr-1 inline" aria-hidden="true" />
                                      Ir a Modelos
                                    </Boton>
                                  ) : (
                                    <Boton
                                      variante="acento"
                                      disabled={!!enCurso}
                                      title={
                                        m.best_quant
                                          ? `Descarga varios GB con llmfit (cuantización ${m.best_quant}).`
                                          : "Descarga varios GB con llmfit; sin cuantización fijada, la elige él."
                                      }
                                      onClick={() => void descargar(m)}
                                    >
                                      <IconDownload size={12} className="mr-1 inline" aria-hidden="true" />
                                      Descargar
                                    </Boton>
                                  )}
                                </div>
                                {!equipo.enEquipo && !m.best_quant ? (
                                  <span className="text-fg-faint text-[11px]">llmfit elegirá la cuantización</span>
                                ) : null}
                                {res ? (
                                  <span
                                    className={`max-w-[240px] text-right text-[11px] ${res.ok ? "text-fg-muted" : "text-bad"}`}
                                    role={res.ok ? undefined : "alert"}
                                  >
                                    {res.mensaje}
                                  </span>
                                ) : null}
                              </div>
                            </td>
                          </tr>
                          {abierto ? (
                            <tr id={idDetalle} className="border-line-soft border-b last:border-0">
                              <td colSpan={9} className="bg-raised/40 px-4 py-3">
                                <div className="grid gap-3 lg:grid-cols-2">
                                  <div className="flex flex-col gap-1 text-xs">
                                    <span className="text-fg-faint">Componentes de la nota (0–100, de llmfit)</span>
                                    <span className="mono text-fg-muted">
                                      {m.score_components
                                        ? `calidad ${num(m.score_components.quality, 1)} · velocidad ${num(m.score_components.speed, 1)} · encaje ${num(m.score_components.fit, 1)} · contexto ${num(m.score_components.context, 1)}`
                                        : "—"}
                                    </span>
                                    <span className="text-fg-faint mt-1">Memoria y contexto (de llmfit)</span>
                                    <span className="mono text-fg-muted">
                                      {m.memory_required_gb != null ? `${gb(m.memory_required_gb)} necesarios` : "memoria —"}
                                      {m.utilization_pct != null ? ` · ${num(m.utilization_pct, 1)} % de uso` : ""}
                                      {m.effective_context_length != null
                                        ? ` · ctx ${m.effective_context_length}`
                                        : m.context_length != null
                                          ? ` · ctx ${m.context_length}`
                                          : ""}
                                    </span>
                                    {cap.length > 0 ? (
                                      <>
                                        <span className="text-fg-faint mt-1">Capacidades</span>
                                        <span className="flex flex-wrap gap-1">
                                          {cap.map((c) => (
                                            <Insignia key={c} tono="neutro">
                                              {c}
                                            </Insignia>
                                          ))}
                                        </span>
                                      </>
                                    ) : null}
                                  </div>
                                  <div className="flex min-w-0 flex-col gap-1 text-xs">
                                    {m.notes.length > 0 ? (
                                      <>
                                        <span className="text-fg-faint">Notas de llmfit</span>
                                        <ul className="text-fg-muted flex flex-col gap-0.5">
                                          {m.notes.map((n, j) => (
                                            <li key={j}>{n}</li>
                                          ))}
                                        </ul>
                                      </>
                                    ) : null}
                                    {m.verify_command ? (
                                      <>
                                        <span className="text-fg-faint mt-1">Para verificar su estimación</span>
                                        <code className="mono text-fg-muted break-all">{m.verify_command}</code>
                                      </>
                                    ) : null}
                                    {m.llamacpp_command ? (
                                      <>
                                        <span className="text-fg-faint mt-1">Comando de llama.cpp</span>
                                        <code className="mono text-fg-muted break-all">{m.llamacpp_command}</code>
                                      </>
                                    ) : null}
                                    {equipo.fichero ? (
                                      <>
                                        <span className="text-fg-faint mt-1">Coincidencia en tu inventario</span>
                                        <code className="mono text-fg-muted break-all" title={equipo.fichero.ruta}>
                                          {equipo.fichero.nombre} · {equipo.fichero.familia} ·{" "}
                                          {bLegibles(equipo.fichero.tamano_bytes, 1)}
                                        </code>
                                      </>
                                    ) : null}
                                  </div>
                                </div>
                              </td>
                            </tr>
                          ) : null}
                        </Fragment>
                      );
                    })}
                  </tbody>
                </table>
              </Card>
            )}

            {visibles.length > 0 ? (
              <p className="text-fg-faint text-xs">
                Descargar baja un GGUF de Hugging Face con llmfit: son <strong className="text-fg-muted">varios GB</strong>.
                Si <code className="mono">best_quant</code> viene vacío, se llama sin cuantización y la elige llmfit.
                Los marcados como «en tu equipo» ya están: para ellos, la estimación se puede sustituir por una medida
                real con <code className="mono">llama-bench</code> desde Rendimiento.
              </p>
            ) : null}
          </section>

          {/* ── Salida en vivo de la descarga ─────────────────────────────── */}
          <section className="flex flex-col gap-3">
            <div className="flex items-center gap-2">
              <IconTerminal2 size={15} className="text-accent" aria-hidden="true" />
              <Etiqueta>{enCurso ? `Salida · ${enCurso}` : "Salida de acciones"}</Etiqueta>
              {enCurso ? <Insignia tono="acento">en curso</Insignia> : null}
            </div>
            <Card className="p-0">
              <pre className="mono max-h-64 min-h-24 overflow-auto p-3 text-xs leading-relaxed">
                {linea.length > 0 ? linea.join("\n") : "Sin salida todavía."}
              </pre>
            </Card>
          </section>
        </>
      )}
    </div>
  );
}
