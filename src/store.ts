/**
 * Estado de la aplicación (zustand).
 *
 * Decisión: el backend YA empuja la foto completa por el evento
 * `ai:snapshot`, así que aquí no se hace sondeo ni se duplica el trabajo: se
 * guarda la última foto, un histórico corto (para las chispas de los KPI) y las
 * listas que vienen de SQLite (que sí se piden a mano).
 *
 * El histórico: las chispas necesitan una serie, y el backend guarda métricas en
 * SQLite con la retención que diga el ajuste `metric_retention_hours` (2 h por
 * defecto). Para no pedirlas en cada vuelta, la serie viva se construye en
 * memoria (últimos N puntos del evento) y las vistas largas (Log/System) sí leen
 * `metrics:recent`.
 */
import { create } from "zustand";
import type {
  ActionRow,
  BenchRow,
  ClienteConexion,
  Comprobacion,
  FitRow,
  ModeloInventario,
  RuntimeLlama,
  ServerRow,
  Snapshot,
  UpdateRow,
} from "./lib/tauri";
import type { Dir } from "./components/ui";
import { api } from "./lib/tauri";

export type Vista =
  | "inicio"
  // ── Modelos: qué puedo tener, qué tengo y cuánto rinde ──────────────────
  | "descubrir"
  | "disco"
  | "rendimiento"
  // ── Motor: quién sirve los modelos y quién los usa ──────────────────────
  | "servidores"
  | "uso"
  | "conexiones"
  // ── Equipo: la máquina por dentro ───────────────────────────────────────
  | "hardware"
  | "pantalla"
  // Almacenamiento y optimización son DOS preguntas distintas y no se mezclan:
  // una es "¿qué ocupa el disco y qué borro?", la otra "¿qué basura puedo tirar
  // sin miedo y qué arranca solo?". Unirlas obligaría a decidir, al ver un
  // fichero grande, si era basura o dato.
  | "almacenamiento"
  | "optimizacion"
  // Seguridad es otra pregunta más, y no se mezcla con las anteriores: no es "qué
  // ocupa" ni "qué basura tiro", es "qué se ejecuta sin que yo lo vea y en qué
  // sitios se escondería algo". La respuesta se da CON PRUEBAS, no con un color.
  | "seguridad"
  | "diagnostico"
  | "mantenimiento"
  // ── Configuración ───────────────────────────────────────────────────────
  | "ajustes";

/**
 * Los grupos de la barra lateral.
 *
 * POR QUÉ AGRUPADA: antes eran doce secciones planas y varias se solapaban
 * (Modelos e Inventario leían la MISMA fuente; Panel y Sistema partían el
 * hardware; Diagnóstico repetía avisos del Panel). Con cuatro grupos, cada
 * sección responde a una pregunta distinta y se ve a qué grupo pertenece.
 *
 * El orden del array ES el orden en la pantalla: `Vista` no lo fija.
 */
export const GRUPOS: { id: string; nombre: string | null; vistas: Vista[] }[] = [
  { id: "raiz", nombre: null, vistas: ["inicio"] },
  { id: "modelos", nombre: "Modelos", vistas: ["descubrir", "disco", "rendimiento"] },
  { id: "motor", nombre: "Motor", vistas: ["servidores", "uso", "conexiones"] },
  {
    id: "equipo",
    nombre: "Equipo",
    vistas: ["hardware", "pantalla", "almacenamiento", "optimizacion", "seguridad", "diagnostico", "mantenimiento"],
  },
  { id: "config", nombre: null, vistas: ["ajustes"] },
];

/**
 * Puntos que se guardan para las chispas: 120 × el intervalo de la foto, que es
 * un ajuste (`snapshot_interval_ms`, 2 s por defecto). Con el valor de fábrica
 * son 4 minutos de serie; si se sube el intervalo, la serie cubre más tiempo.
 */
const MAX_PUNTOS = 120;
const MAX_LINEAS_LOG = 400;

export interface PuntoMetrica {
  ts: number;
  cpu: number;
  mem: number;
  gpuUtil: number;
  gpuMemPct: number;
}

/**
 * De qué es un error. La lectura del backend son varias llamadas independientes
 * (la foto, la tabla de servidores, el histórico), y antes se guardaba un único
 * texto sin dueño: cada vista lo suyo lo volvía a prefijar y acababan saliendo
 * cadenas del tipo "No se pudo leer el inventario de modelos: No se pudo leer la
 * lista de servidores: …", donde la culpa se le echaba a la vista equivocada.
 */
export type OrigenError =
  | "foto"
  | "servidores"
  | "acciones"
  | "actualizaciones"
  | "accion"
  | "eventos"
  // Las dos lecturas de Rendimiento son independientes (los runtimes instalados
  // y el histórico de medidas) y fallan por su cuenta: si compartieran origen,
  // un fallo en una haría callar el aviso de la otra.
  | "runtimes"
  | "benchmarks"
  // El inventario de modelos de TODO tipo es una lectura APARTE (recorre
  // carpetas del sistema de ficheros): tiene su propio origen para que su fallo
  // no se confunda con el de la foto, que es otra cosa.
  | "inventario"
  // Los encajes guardados se leen de SQLite (`fits:listar`): otra lectura, otro
  // fallo. Si falla, la vista dice "no se pudo leer", que no es "no hay encaje".
  | "fits"
  // El diagnóstico recorre el entorno (GPU, MCLK, disco, inventario…): es su
  // propia lectura, y su fallo NO es "el equipo está mal", es "no se pudo
  // comprobar". Origen aparte para no confundir las dos cosas.
  | "diagnostico"
  // Detectar los otros clientes de IA lee SUS ficheros de configuración. Si
  // falla, "no se pudo detectar" no es "no hay ningún cliente".
  | "clientes"
  // El analizador de disco recorre el sistema de ficheros y el limpiador recorre
  // las cachés: son lecturas pesadas y con su propio fallo. Que falle el análisis
  // no es "el disco está vacío", y que falle el escaneo no es "no hay basura".
  | "almacen"
  | "limpieza"
  // Leer las entradas de arranque toca ficheros de la sesión: si falla, "no se
  // pudo leer la lista" no es "no arranca nada".
  | "arranque"
  // El Centro de recuperación lee la tabla de copias y los ficheros que hay al
  // lado de los originales: su fallo es suyo, no de la foto.
  | "copias"
  // La revisión de seguridad lee ficheros de arranque, crontab y llaves SSH: si
  // falla, "no se pudo comprobar" no es "está todo bien" (y pintarlo de verde
  // sería exactamente el error que esta sección existe para evitar).
  | "seguridad"
  // Comprobar actualizaciones ejecuta las herramientas de fuera: si falla, "no se
  // pudo comprobar" no es "está todo al día".
  | "actualizar";

export interface ErrorApp {
  origen: OrigenError;
  /** Motivo tal como lo redacta el backend: se enseña LITERAL, sin prefijos. */
  mensaje: string;
}

/* ── Estado de interfaz por vista (sobrevive a cambiar de sección) ─────────── */

/**
 * Por qué esto vive en la tienda y no en cada vista: al cambiar de sección el
 * componente se DESMONTA, así que un `useState` local se pierde y, al volver,
 * los filtros, la búsqueda y el orden aparecían en blanco. Guardarlos aquí (que
 * no se desmonta) es lo que hace que la vista esté donde la dejaste.
 *
 * Solo se guarda lo que se introduce a mano: filtros, búsqueda y orden. Lo que es
 * resultado de una lectura (la lista, el último mensaje de una acción) no.
 */
export interface UiDisco {
  q: string;
  tipo: string;
  familia: string;
  motor: string;
  /** Columna de ordenación (clave de la tabla). */
  col: string;
  dir: Dir;
}

/** Filtros de Descubrir: los de llmfit (los tres primeros) y los locales. */
export interface UiRecomendados {
  /** Los que van a la CLI de llmfit. */
  casoUso: string;
  encajeMinimo: string;
  capacidad: string;
  conComando: boolean;
  /** 40 es el límite por defecto del backend cuando no se le manda ninguno. */
  limite: number;
  /**
   * Peso entre velocidad y capacidad, de 0 (lo más rápido) a 100 (lo más capaz).
   *
   * Va en la tienda y no en un `useState` porque al salir de Descubrir y volver
   * el deslizador se pondría a 50 otra vez, y el orden de la tabla cambiaría sin
   * que nadie lo tocara.
   */
  preferencia: number;
  /** Los que se aplican aquí sobre lo ya devuelto. */
  licencia: string;
  rango: string;
  soloCaben: boolean;
  soloTengo: boolean;
  col: string;
  dir: Dir;
}

export interface UiLog {
  resultado: "todos" | "ok" | "fallo";
  tipo: string;
}

/**
 * Estado del analizador de disco que se recuerda al cambiar de sección.
 *
 * La carpeta analizada va aquí y no en un `useState`: salir a mirar otra cosa y
 * volver te devolvía al home, perdiendo el sitio donde estabas mirando (que es
 * justo lo que se estaba investigando).
 */
export interface UiAlmacen {
  /** Carpeta analizada (vacío = el home, que decide el backend). */
  raiz: string;
  /** Columna y sentido de la tabla de contenido. */
  col: string;
  dir: Dir;
  /** Búsqueda por nombre dentro de la carpeta analizada. */
  q: string;
  /**
   * Qué herramienta se está mirando: el contenido de la carpeta, los ficheros
   * repetidos, las carpetas vacías o los enlaces rotos. Va en la tienda para que
   * al volver a la sección sigas mirando lo mismo.
   */
  herramienta: string;
}

/** Filtros de Optimización: qué categorías se escanean y qué se ha marcado. */
export interface UiLimpieza {
  /** Categorías marcadas (vacío = todas). */
  categorias: string[];
  /** Objetivos marcados para limpiar. */
  seleccion: string[];
}

/**
 * Lo que se marca en Seguridad: HUELLAS concretas, una a una.
 *
 * No hay «marcar todo» a propósito. Estas se borran de verdad (no van a la
 * papelera, porque una huella que quieres borrar no puede quedarse en la basura) y
 * no se regeneran, así que cada una se pide por su nombre.
 */
export interface UiSeguridad {
  /** Ids de las huellas marcadas para borrar. */
  seleccion: string[];
}

export interface Uis {
  disco: UiDisco;
  descubrir: UiRecomendados;
  log: UiLog;
  almacen: UiAlmacen;
  optimizacion: UiLimpieza;
  seguridad: UiSeguridad;
}

export const UI_INICIAL: Uis = {
  // El tamaño de mayor a menor es lo primero que se mira en un inventario.
  disco: { q: "", tipo: "", familia: "", motor: "", col: "tamano", dir: "desc" },
  descubrir: {
    casoUso: "",
    encajeMinimo: "",
    capacidad: "",
    conComando: false,
    limite: 40,
    preferencia: 50,
    licencia: "",
    rango: "",
    soloCaben: false,
    soloTengo: false,
    // Se abre ordenada por «Ajuste», que es la puntuación según el deslizador:
    // así lo primero que se ve es «esto es lo mejor para tu equipo con lo que
    // has pedido», y no la nota de llmfit a secas.
    col: "ajuste",
    dir: "desc",
  },
  log: { resultado: "todos", tipo: "todos" },
  // De fábrica se empieza por el home y por tamaño, que es lo que se quiere ver
  // al abrir un analizador de disco.
  almacen: { raiz: "", col: "bytes", dir: "desc", q: "", herramienta: "contenido" },
  // Sin categorías marcadas = todas (el backend lo interpreta así).
  optimizacion: { categorias: [], seleccion: [] },
  // Aquí no hay categorías: lo marcado son HUELLAS concretas, y por eso se
  // seleccionan una a una y no con un «todo».
  seguridad: { seleccion: [] },
};

export interface Estado {
  vista: Vista;
  snapshot: Snapshot | null;
  /** Histórico en memoria para las chispas (más nuevo al final). */
  serie: PuntoMetrica[];
  servidores: ServerRow[];
  acciones: ActionRow[];
  actualizaciones: UpdateRow[];
  /** Runtimes de llama.cpp detectados (para el selector y las fichas). */
  runtimes: RuntimeLlama[];
  /** Medidas de `llama-bench` guardadas, más recientes primero. */
  benchmarks: BenchRow[];
  /**
   * Inventario real de modelos en disco (todas las familias y tipos).
   *
   * `null` = todavía NO se ha pedido (no es "vacío"): la vista Modelos distingue
   * "aún no ha llegado" de "el equipo no tiene ningún modelo", que son cosas
   * distintas y no se pueden pintar igual.
   */
  inventario: ModeloInventario[] | null;
  /**
   * Encajes guardados, uno por modelo (`fits:listar`).
   *
   * `null` = todavía NO se ha pedido, igual que el inventario: sin esa
   * distinción, una tabla vacía afirmaría "no hay encaje de nada" cuando lo que
   * pasa es que aún no se ha leído.
   */
  fits: FitRow[] | null;
  /**
   * Resultado de la última comprobación de salud (`diagnostico:comprobar`).
   *
   * `null` = todavía NO se ha comprobado. La vista distingue ese caso de "se
   * comprobó y no hay nada": lo primero pide pulsar, lo segundo es un resultado.
   * La comprobación es BAJO DEMANDA (al abrir la sección y cuando el usuario
   * pulsa): no hay bucle.
   */
  diagnostico: Comprobacion[] | null;
  /**
   * Otros clientes de IA detectados en el equipo (`conexiones:clientes`).
   *
   * `null` = todavía NO se ha detectado. Es una lectura de ficheros AJENOS: los
   * datos se enseñan como EVIDENCIA y Machinograph no escribe nada en ellos.
   */
  clientes: ClienteConexion[] | null;
  /** Filtros, búsqueda y orden de cada vista (ver `Uis`). */
  ui: Uis;
  /**
   * Qué lectura está en curso AHORA, por origen.
   *
   * Existe porque una lectura que tarda (recorrer el inventario, leer la tabla de
   * encajes) no puede pintarse como "vacío": hay que poder decir "leyendo…" y
   * apagar los botones que la disparan.
   */
  cargando: Partial<Record<OrigenError, boolean>>;
  /** Líneas de salida de la acción en curso (panel de terminal). */
  lineaAccion: string[];
  accionEnCurso: string | null;
  /**
   * Errores vivos POR ORIGEN, no un único mensaje: cada vista lee el suyo con
   * `errorDe`. Con un solo hueco, el último error en llegar borraba a los demás
   * y una vista acababa callando un fallo real (o atribuyéndose uno ajeno).
   */
  errores: Partial<Record<OrigenError, string>>;
  /** El último error de CUALQUIER origen: solo para la cabecera global de App. */
  ultimoError: ErrorApp | null;

  setVista: (v: Vista) => void;
  aplicarSnapshot: (s: Snapshot) => void;
  anadirLinea: (l: string) => void;
  setAccion: (nombre: string | null) => void;
  setError: (origen: OrigenError, mensaje: string) => void;
  limpiarError: (origen: OrigenError) => void;
  setCargando: (origen: OrigenError, si: boolean) => void;
  /** Cambia filtros/orden de UNA vista sin tocar los de las demás. */
  setUi: <K extends keyof Uis>(vista: K, parche: Partial<Uis[K]>) => void;
  cargarServidores: () => Promise<void>;
  cargarAcciones: () => Promise<void>;
  cargarActualizaciones: () => Promise<void>;
  cargarRuntimes: () => Promise<void>;
  cargarBenchmarks: () => Promise<void>;
  /** Relee el inventario de modelos (se llama al abrir Modelos y tras borrar). */
  cargarInventario: () => Promise<void>;
  /** Relee los encajes guardados. La vista los pide al abrir; no hay sondeo. */
  cargarFits: () => Promise<void>;
  /**
   * Comprueba la salud del entorno. Se llama al abrir Diagnóstico (una vez) y
   * cada vez que el usuario pulsa "Comprobar ahora"; nunca en bucle.
   */
  cargarDiagnostico: () => Promise<void>;
  /** Detecta los otros clientes de IA y quién apunta al motor local. */
  cargarClientes: () => Promise<void>;
  /** Un encaje recién calculado (`ai:fit`) reemplaza al de ESE modelo. */
  aplicarFit: (f: FitRow) => void;
  /** Las dos listas a la vez, para el arranque y tras una acción. */
  cargarHistorial: () => Promise<void>;
}

/**
 * Selector de zustand para el mensaje del error de UN origen concreto (o `null`
 * si esa lectura no ha fallado): así una vista nunca culpa a la lectura
 * equivocada ni repite un mensaje que no le toca.
 */
export const errorDe =
  (origen: OrigenError) =>
  (st: Estado): string | null =>
    st.errores[origen] ?? null;

/** Selector de "esta lectura está en curso" para UN origen. */
export const cargandoDe =
  (origen: OrigenError) =>
  (st: Estado): boolean =>
    st.cargando[origen] === true;

export const useApp = create<Estado>((set, get) => ({
  vista: "inicio",
  snapshot: null,
  serie: [],
  servidores: [],
  acciones: [],
  actualizaciones: [],
  runtimes: [],
  benchmarks: [],
  inventario: null,
  fits: null,
  diagnostico: null,
  clientes: null,
  ui: UI_INICIAL,
  cargando: {},
  lineaAccion: [],
  accionEnCurso: null,
  errores: {},
  ultimoError: null,

  setVista: (vista) => set({ vista }),

  aplicarSnapshot: (s) =>
    set((st) => {
      const punto: PuntoMetrica = {
        ts: s.ts,
        cpu: s.system.cpu_pct,
        mem: s.system.mem.pct,
        gpuUtil: s.gpu[0]?.util ?? 0,
        gpuMemPct: s.gpu[0]?.mem_pct ?? 0,
      };
      const serie = [...st.serie, punto];
      if (serie.length > MAX_PUNTOS) serie.splice(0, serie.length - MAX_PUNTOS);
      // Una foto nueva es una lectura BUENA de la misma fuente: el error de la
      // lectura anterior ya no describe lo que hay y se retira (si no, el aviso
      // se quedaría pegado aunque el backend ya estuviera contestando).
      const errores = { ...st.errores };
      delete errores.foto;
      return {
        snapshot: s,
        serie,
        errores,
        ultimoError: st.ultimoError?.origen === "foto" ? null : st.ultimoError,
      };
    }),

  anadirLinea: (linea) =>
    set((st) => {
      const lineaAccion = [...st.lineaAccion, linea];
      if (lineaAccion.length > MAX_LINEAS_LOG)
        lineaAccion.splice(0, lineaAccion.length - MAX_LINEAS_LOG);
      return { lineaAccion };
    }),

  setAccion: (accionEnCurso) =>
    set({ accionEnCurso, ...(accionEnCurso ? { lineaAccion: [] } : {}) }),

  setError: (origen, mensaje) =>
    set((st) => ({
      // Se anota bajo su origen y NO se toca lo que haya de otros: si además se
      // queda como "el último", es solo para que la cabecera global lo enseñe.
      errores: { ...st.errores, [origen]: mensaje },
      ultimoError: { origen, mensaje },
    })),

  /**
   * Retira el error de UN origen (una lectura que ya va bien). Los demás siguen
   * vivos: si no, el éxito de una llamada tapaba el fallo de otra y el usuario
   * perdía el aviso sin haberlo visto.
   */
  limpiarError: (origen) =>
    set((st) => {
      const errores = { ...st.errores };
      delete errores[origen];
      return {
        errores,
        ultimoError: st.ultimoError?.origen === origen ? null : st.ultimoError,
      };
    }),

  setCargando: (origen, si) =>
    set((st) => ({ cargando: { ...st.cargando, [origen]: si } })),

  /**
   * Los filtros de una vista se cambian con un parche: así `setUi("models", {q})`
   * no borra el orden ni los demás filtros de esa misma vista.
   */
  setUi: (vista, parche) =>
    set((st) => {
      const ui = { ...st.ui, [vista]: { ...st.ui[vista], ...parche } };
      return { ui } as Pick<Estado, "ui">;
    }),

  cargarServidores: async () => {
    get().setCargando("servidores", true);
    try {
      set({ servidores: await api.servers.list() });
      get().limpiarError("servidores");
    } catch (e) {
      get().setError("servidores", String(e));
    } finally {
      get().setCargando("servidores", false);
    }
  },

  cargarAcciones: async () => {
    try {
      set({ acciones: await api.acciones(200) });
      get().limpiarError("acciones");
    } catch (e) {
      get().setError("acciones", String(e));
    }
  },

  cargarActualizaciones: async () => {
    try {
      set({ actualizaciones: await api.updates(200) });
      get().limpiarError("actualizaciones");
    } catch (e) {
      get().setError("actualizaciones", String(e));
    }
  },

  cargarRuntimes: async () => {
    get().setCargando("runtimes", true);
    try {
      set({ runtimes: await api.perf.tools() });
      get().limpiarError("runtimes");
    } catch (e) {
      get().setError("runtimes", String(e));
    } finally {
      get().setCargando("runtimes", false);
    }
  },

  cargarBenchmarks: async () => {
    get().setCargando("benchmarks", true);
    try {
      set({ benchmarks: await api.benchmarks(200) });
      get().limpiarError("benchmarks");
    } catch (e) {
      get().setError("benchmarks", String(e));
    } finally {
      get().setCargando("benchmarks", false);
    }
  },

  cargarInventario: async () => {
    get().setCargando("inventario", true);
    try {
      // Solo se guardan los modelos: el `resumen` que devuelve el backend se
      // recalcula en la vista a partir de esta MISMA lista, y así los totales
      // que se enseñan y las filas que se ven no pueden discrepar entre sí.
      const r = await api.inventario();
      set({ inventario: r.modelos });
      get().limpiarError("inventario");
    } catch (e) {
      get().setError("inventario", String(e));
    } finally {
      get().setCargando("inventario", false);
    }
  },

  cargarFits: async () => {
    get().setCargando("fits", true);
    try {
      set({ fits: await api.fits() });
      get().limpiarError("fits");
    } catch (e) {
      get().setError("fits", String(e));
    } finally {
      get().setCargando("fits", false);
    }
  },

  aplicarFit: (f) => {
    // Sin la lista completa, guardar solo esta fila la haría pasar por completa:
    // se pide entera. Con la lista cargada, el encaje NUEVO de ese modelo
    // reemplaza al viejo y los demás no se tocan.
    if (get().fits == null) {
      void get().cargarFits();
      return;
    }
    set((st) => ({ fits: [f, ...(st.fits ?? []).filter((x) => x.modelo !== f.modelo)] }));
  },

  /**
   * Comprobación de salud BAJO DEMANDA. Se pide al abrir la sección y cada vez
   * que se pulsa el botón; no hay sondeo, porque cada vuelta recorre el entorno
   * entero (incluido el inventario de modelos).
   */
  cargarDiagnostico: async () => {
    get().setCargando("diagnostico", true);
    try {
      set({ diagnostico: await api.diagnostico.comprobar() });
      get().limpiarError("diagnostico");
    } catch (e) {
      get().setError("diagnostico", String(e));
    } finally {
      get().setCargando("diagnostico", false);
    }
  },

  cargarClientes: async () => {
    get().setCargando("clientes", true);
    try {
      set({ clientes: await api.conexiones.clientes() });
      get().limpiarError("clientes");
    } catch (e) {
      get().setError("clientes", String(e));
    } finally {
      get().setCargando("clientes", false);
    }
  },

  cargarHistorial: async () => {
    const { cargarAcciones, cargarActualizaciones } = get();
    // En paralelo y con error propio cada una: así un fallo en las acciones no
    // hace que la vista de actualizaciones afirme que también le falló a ella.
    await Promise.all([cargarAcciones(), cargarActualizaciones()]);
  },
}));

/**
 * Lo que devuelve una acción, tal cual lo redacta el backend.
 *
 * `mensaje` es el texto que hay que enseñar: describe QUÉ pasó. Se devuelve
 * porque algunas vistas (Rendimiento) tienen que enseñar el resultado de la
 * ÚLTIMA vez que se lanzó algo para UN modelo concreto, y el evento `ai:action`
 * es global y no dice de qué modelo era.
 *
 * Cuidado con un matiz: "no cabe" NO es un error. Si pides un contexto mayor que
 * el que cabe, el backend contesta `Ok("NO cabe con lo pedido: …")`, así que aquí
 * llega con `ok: true` y hay que enseñarlo como un dato, no como un fallo.
 */
export interface ResultadoAccion {
  ok: boolean;
  mensaje: string;
}

/** Ejecuta una acción del backend dejando rastro en el estado. */
export async function ejecutar(
  kind: Parameters<typeof api.accion>[0],
  args: Record<string, unknown> = {},
): Promise<ResultadoAccion> {
  const { setAccion, setError, limpiarError, cargarHistorial } = useApp.getState();
  setAccion(kind);
  try {
    const mensaje = await api.accion(kind, args);
    // Solo se retira el error de la última ACCIÓN: si lo que falló fue una
    // lectura, ese motivo sigue siendo válido y no debe desaparecer.
    limpiarError("accion");
    return { ok: true, mensaje };
  } catch (e) {
    const mensaje = String(e);
    // El `kind` sí es información nuestra, no un prefijo de vista: dice qué
    // acción concreta rechazó el backend.
    setError("accion", `${kind}: ${mensaje}`);
    return { ok: false, mensaje };
  } finally {
    setAccion(null);
    void cargarHistorial();
  }
}
