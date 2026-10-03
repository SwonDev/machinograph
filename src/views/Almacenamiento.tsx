/**
 * Almacenamiento: ¿qué ocupa el disco y qué puedo borrar?
 *
 * Es el analizador de disco (lo que en Kudu es el "disk analyzer"): se elige una
 * carpeta, se ve lo que ocupa CADA hijo con su tamaño recursivo —como `du
 * --max-depth=1`—, se puede bajar a una carpeta, ordenar por tamaño, buscar por
 * nombre y borrar lo seleccionado.
 *
 * Cuatro decisiones que están tomadas a conciencia:
 *
 * 1. **Un nivel por consulta.** El backend recorre el árbol entero para dar el
 *    tamaño de cada hijo, pero solo devuelve los hijos DIRECTOS: bajar a una
 *    carpeta es volver a preguntar. Así la respuesta no depende de lo profundo
 *    que sea el árbol y el disco no se trae entero de una vez.
 * 2. **Borrar por defecto va a la PAPELERA.** Un fichero personal (un vídeo de
 *    una descarga, una carpeta de un proyecto) no se recupera de un borrado
 *    definitivo, así que el modo por defecto es la papelera y el definitivo es
 *    una elección explícita que se dice en la confirmación.
 * 3. **Lo que no se ha medido se dice.** Si el backend agota su presupuesto
 *    (`truncado`) o no puede leer una carpeta (`omitidos`), se enseña: un total
 *    incompleto presentado como completo es peor que no darlo.
 * 4. **Búsqueda con Enter, no por letra.** Cada búsqueda recorre el disco; buscar
 *    al teclear lanzaría un recorrido por pulsación.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  IconAlertTriangle, IconArrowUp, IconFolder, IconFolderOpen, IconHome, IconSearch, IconTrash,
} from "@tabler/icons-react";
import { clsx } from "clsx";
import { cargandoDe, errorDe, ejecutar, useApp, type ResultadoAccion } from "../store";
import { api, type ArbolAlmacen, type Coincidencia, type CrecimientoHijo, type Duplicado, type EnlaceRoto, type FicheroGrande, type HistorialDisco, type Montaje } from "../lib/tauri";
import { Barra, Boton, Card, Etiqueta, Insignia, Kpi, Th, ThOrden, Vacio } from "../components/ui";
import type { Dir } from "../components/ui";
import { bLegibles, fechaHora, hace, num, pct } from "../lib/format";

/** Columnas ordenables de la tabla de contenido. */
const COLUMNAS = ["nombre", "bytes", "ficheros", "dirs", "modificado"] as const;
type Col = (typeof COLUMNAS)[number];
const esCol = (v: string): v is Col => (COLUMNAS as readonly string[]).includes(v);

/** Compara por número, con los nulos SIEMPRE al final (no valen como 0). */
function numero(a: number | null, b: number | null, dir: Dir): number {
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  return dir === "asc" ? a - b : b - a;
}

function texto(a: string, b: string, dir: Dir): number {
  const r = a.localeCompare(b, "es", { numeric: true, sensitivity: "base" });
  return dir === "asc" ? r : -r;
}

/** El padre de una ruta absoluta, o `/` si ya está en la raíz. */
function padreDe(ruta: string): string {
  const partes = ruta.replace(/\/+$/, "").split("/");
  partes.pop();
  return partes.join("/") || "/";
}

/** El nombre que se enseña para una ruta (el último trozo). */
const nombreDe = (ruta: string) => ruta.replace(/\/+$/, "").split("/").pop() || ruta;

/* ── La tabla de contenido de una carpeta ─────────────────────────────────── */

function TablaContenido({
  arbol,
  col,
  dir,
  onOrdenar,
  onBajar,
  seleccion,
  onSel,
}: {
  arbol: ArbolAlmacen;
  col: Col;
  dir: Dir;
  onOrdenar: (c: string, d: Dir) => void;
  onBajar: (ruta: string) => void;
  seleccion: Set<string>;
  onSel: (ruta: string, marcado: boolean) => void;
}) {
  const ordenados = useMemo(() => {
    const cmp = (a: (typeof arbol.hijos)[number], b: (typeof arbol.hijos)[number]): number => {
      switch (col) {
        case "nombre":
          return texto(a.nombre, b.nombre, dir);
        case "bytes":
          return numero(a.bytes, b.bytes, dir);
        case "ficheros":
          return numero(a.ficheros, b.ficheros, dir);
        case "dirs":
          return numero(a.dirs, b.dirs, dir);
        case "modificado":
          return numero(a.modificado, b.modificado, dir);
      }
    };
    return [...arbol.hijos].sort(cmp);
  }, [arbol.hijos, col, dir]);

  if (arbol.hijos.length === 0) {
    return (
      <Vacio titulo="Esta carpeta no tiene nada dentro">
        Está vacía o solo contiene enlaces simbólicos (los enlaces no se siguen
        al medir: un enlace a `/` haría que esta carpeta "ocupara" el disco entero).
      </Vacio>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse text-xs">
        <caption className="sr-only">
          Contenido de la carpeta analizada, con el tamaño recursivo de cada hijo
        </caption>
        <thead>
          <tr className="border-line-soft border-b">
            <Th className="w-8">
              <span className="sr-only">Selección</span>
            </Th>
            <ThOrden col="nombre" actual={col} dir={dir} onOrdenar={onOrdenar}>
              Nombre
            </ThOrden>
            <ThOrden
              col="bytes"
              actual={col}
              dir={dir}
              primero="desc"
              alineado="der"
              onOrdenar={onOrdenar}
              titulo="Lo que ocupa TODO lo que hay dentro, no solo la entrada de la carpeta"
            >
              Tamaño
            </ThOrden>
            <ThOrden col="ficheros" actual={col} dir={dir} primero="desc" alineado="der" onOrdenar={onOrdenar}>
              Ficheros
            </ThOrden>
            <ThOrden col="dirs" actual={col} dir={dir} primero="desc" alineado="der" onOrdenar={onOrdenar}>
              Carpetas
            </ThOrden>
            <ThOrden col="modificado" actual={col} dir={dir} primero="desc" alineado="der" onOrdenar={onOrdenar}>
              Modificado
            </ThOrden>
          </tr>
        </thead>
        <tbody>
          {ordenados.map((h) => (
            <tr key={h.ruta} className="border-line-soft hover:bg-raised border-b last:border-0">
              <td className="px-4 py-1.5">
                <input
                  type="checkbox"
                  checked={seleccion.has(h.ruta)}
                  onChange={(e) => onSel(h.ruta, e.target.checked)}
                  aria-label={`Seleccionar ${h.nombre}`}
                />
              </td>
              <td className="max-w-[320px] px-4 py-1.5">
                {h.es_dir ? (
                  <button
                    type="button"
                    onClick={() => onBajar(h.ruta)}
                    className="hover:text-accent flex items-center gap-1.5 text-left"
                    title={`Abrir ${h.ruta}`}
                  >
                    <IconFolder size={14} aria-hidden="true" className="text-accent-dim shrink-0" />
                    <span className="truncate">{h.nombre}</span>
                  </button>
                ) : (
                  <span className="text-fg-muted flex items-center gap-1.5" title={h.ruta}>
                    <span className="w-[14px] shrink-0" aria-hidden="true" />
                    <span className="truncate">{h.nombre}</span>
                  </span>
                )}
              </td>
              <td className="mono px-4 py-1.5 text-right whitespace-nowrap">{bLegibles(h.bytes, 1)}</td>
              <td className="mono text-fg-muted px-4 py-1.5 text-right">{num(h.ficheros)}</td>
              <td className="mono text-fg-muted px-4 py-1.5 text-right">{num(h.dirs)}</td>
              <td className="text-fg-faint px-4 py-1.5 text-right whitespace-nowrap">{hace(h.modificado)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/* ── La vista ─────────────────────────────────────────────────────────────── */

/**
 * Las cuatro lentes sobre la MISMA carpeta.
 *
 * Van en la misma sección porque contestan a la misma pregunta («¿qué ocupa y qué
 * sobra aquí?») y porque comparten el sitio: la carpeta que has elegido, la
 * selección y el borrado. Cada una dice en una línea qué hace y qué se lleva por
 * delante, que es lo que hay que saber ANTES de pulsar.
 */
const HERRAMIENTAS: { id: string; nombre: string; ayuda: string }[] = [
  {
    id: "contenido",
    nombre: "Contenido",
    ayuda: "Lo que ocupa cada hijo directo de la carpeta, ordenado por tamaño.",
  },
  {
    id: "duplicados",
    nombre: "Repetidos",
    ayuda:
      "Ficheros con el MISMO contenido (no el mismo nombre). De cada grupo se puede borrar todo menos una copia. Lee los ficheros, así que tarda.",
  },
  {
    id: "vacias",
    nombre: "Vacías",
    ayuda: "Carpetas que no tienen ningún fichero dentro, ni en su subárbol.",
  },
  {
    id: "enlaces",
    nombre: "Enlaces rotos",
    ayuda: "Enlaces simbólicos que apuntan a algo que ya no está. Se quita el enlace, no su destino.",
  },
];

/**
 * Lista de rutas con casilla y un botón para marcarlas todas.
 *
 * Es el mismo patrón en las tres herramientas nuevas: una tabla o una lista, una
 * casilla por fila, y arriba el botón de marcar. La selección vive en la vista
 * porque es la misma para las cuatro lentes y alimenta el mismo bloque de borrado.
 */
function Seleccionable({
  rutas,
  sel,
  onSel,
  etiqueta,
  columnas,
  filas,
  vacio,
  seleccionarTexto = "Marcar todas",
  onSeleccionarTodas,
}: {
  rutas: string[];
  sel: Set<string>;
  onSel: (ruta: string, marcado: boolean) => void;
  etiqueta: string;
  columnas: string[];
  filas: { clave: string; celdas: React.ReactNode[] }[];
  vacio: React.ReactNode;
  seleccionarTexto?: string;
  onSeleccionarTodas: () => void;
}) {
  if (rutas.length === 0) return <Vacio titulo="No hay nada que enseñar">{vacio}</Vacio>;
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <Insignia tono="neutro">
          {num(rutas.length)} {etiqueta}
        </Insignia>
        <Boton onClick={onSeleccionarTodas} disabled={rutas.length === 0}>
          {seleccionarTexto}
        </Boton>
        {sel.size > 0 ? <span className="text-fg-faint text-xs">{num(sel.size)} marcadas</span> : null}
      </div>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-xs">
          <caption className="sr-only">{etiqueta}</caption>
          <thead>
            <tr className="border-line-soft border-b">
              <Th className="w-8">
                <span className="sr-only">Selección</span>
              </Th>
              {columnas.map((c, i) => (
                <Th key={c} alineado={i === 0 ? "izq" : "der"}>
                  {c}
                </Th>
              ))}
            </tr>
          </thead>
          <tbody>
            {filas.map((f) => (
              <tr key={f.clave} className="border-line-soft hover:bg-raised border-b last:border-0">
                <td className="px-4 py-1.5">
                  <input
                    type="checkbox"
                    checked={sel.has(f.clave)}
                    onChange={(e) => onSel(f.clave, e.target.checked)}
                    aria-label={`Seleccionar ${f.clave}`}
                  />
                </td>
                {f.celdas.map((c, i) => (
                  <td
                    key={i}
                    className={clsx("max-w-[520px] px-4 py-1.5", i === 0 ? "text-left" : "text-right")}
                  >
                    {c}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/* ── Cómo ha cambiado la carpeta (histórico y comparación) ─────────────────── */

/** Una fila de la lista de lo que ha crecido (o bajado): delta, nombre y % . */
function FilaCrecimiento({ h }: { h: CrecimientoHijo }) {
  const sube = h.delta >= 0;
  return (
    <li className="flex flex-wrap items-baseline gap-2 text-xs">
      <span className={clsx("mono w-20 text-right", sube ? "text-warn" : "text-ok")}>
        {sube ? "+" : "-"}
        {bLegibles(Math.abs(h.delta), 1)}
      </span>
      <span className="truncate" title={h.ruta}>
        {h.nombre}
      </span>
      {h.nuevo ? <Insignia tono="acento">nuevo</Insignia> : null}
      {h.desaparecido ? <Insignia tono="neutro">desaparecido</Insignia> : null}
      <span className="text-fg-faint ml-auto whitespace-nowrap">
        {/* Sin base no hay porcentaje: se dice, no se inventa un 100 %. */}
        {h.pct == null ? "sin base para el %" : `${h.pct >= 0 ? "+" : ""}${num(h.pct, 1)} %`}
      </span>
    </li>
  );
}

/**
 * «Cómo ha cambiado»: la última comparación disponible de la carpeta que se está
 * mirando, con la fecha de cada medida.
 *
 * Tres reglas que vienen del diseño: cada cifra dice de cuándo es; si una medida
 * es parcial se dice (compararla como completa daría un crecimiento falso); y con
 * una sola medida NO se inventa un «+0 B», se dice que todavía no hay con qué
 * comparar.
 */
function BloqueCambios({ h, error }: { h: HistorialDisco | null; error: string | null }) {
  if (h == null) {
    return (
      <Card className="flex flex-col gap-2">
        <Etiqueta>Cómo ha cambiado</Etiqueta>
        <p className={error ? "text-bad text-xs" : "text-fg-muted text-xs"}>
          {error ? `No se pudo leer el histórico de esta carpeta: ${error}` : "Leyendo el histórico de esta carpeta…"}
        </p>
      </Card>
    );
  }
  const c = h.crecimiento;
  const primera = h.instantaneas[0];
  const parcialDe = (i: (typeof h.instantaneas)[number]) => {
    const partes: string[] = [];
    if (i.truncado) partes.push("el recorrido se cortó por presupuesto");
    if (i.excluidos.length > 0) partes.push(`${i.excluidos.length} exclusión(es) dejaron algo fuera`);
    return partes.length > 0 ? partes.join("; ") : null;
  };

  const crecidos = c ? c.hijos.filter((x) => x.delta > 0) : [];
  const bajados = c ? c.hijos.filter((x) => x.delta < 0) : [];

  return (
    <Card className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>Cómo ha cambiado</Etiqueta>
        <Insignia tono="neutro">
          un punto por día · se conservan {h.retencion_dias} días
        </Insignia>
      </div>

      {h.instantaneas.length === 0 ? (
        <p className="text-fg-muted text-xs">
          No hay ninguna medida guardada de esta carpeta todavía. Se guarda una al analizarla
          {h.activo
            ? " y, con la medida diaria encendida, también una vez al día."
            : ". La medida diaria automática está apagada en Ajustes, pero analizar sí deja su medida."}
        </p>
      ) : c == null ? (
        <p className="text-fg-muted text-xs">
          Primera medida: {fechaHora(primera.ts)} ({hace(primera.ts)}) · {bLegibles(primera.bytes, 1)}
          {parcialDe(primera) ? ` (medida parcial: ${parcialDe(primera)})` : ""}. A partir de mañana
          podrás comparar: hace falta una medida de otro día para saber qué ha cambiado.
        </p>
      ) : (
        <>
          <div>
            <p className="text-sm">
              Desde el {fechaHora(c.antes_ts)} ({hace(c.antes_ts)}):{" "}
              <span className={clsx("mono font-medium", c.delta_bytes >= 0 ? "text-warn" : "text-ok")}>
                {c.delta_bytes >= 0 ? "+" : "-"}
                {bLegibles(Math.abs(c.delta_bytes), 1)}
              </span>
              {c.antes_bytes > 0 ? (
                <span className="text-fg-muted">
                  {" "}
                  ({c.delta_bytes >= 0 ? "+" : ""}
                  {num((c.delta_bytes / c.antes_bytes) * 100, 1)} %)
                </span>
              ) : null}
            </p>
            <p className="text-fg-faint text-xs">
              Comparación entre la medida del {fechaHora(c.antes_ts)} ({bLegibles(c.antes_bytes, 1)})
              y la del {fechaHora(c.ahora_ts)} ({bLegibles(c.ahora_bytes, 1)}); la última es de{" "}
              {hace(c.ahora_ts)}.
            </p>
          </div>

          {c.parcial ? (
            <p className="text-warn flex items-start gap-1.5 text-xs" role="status">
              <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
              <span>
                Comparación entre medidas PARCIALES ({c.motivo}): la diferencia puede no ser la del
                crecimiento real de la carpeta.
              </span>
            </p>
          ) : null}

          {crecidos.length > 0 ? (
            <div>
              <span className="label">Lo que más ha crecido</span>
              <ul className="mt-1 flex flex-col gap-0.5">
                {crecidos.slice(0, 8).map((x) => (
                  <FilaCrecimiento key={x.ruta} h={x} />
                ))}
              </ul>
              {crecidos.length > 8 ? (
                <p className="text-fg-faint mt-1 text-xs">
                  … y {num(crecidos.length - 8)} más que también han crecido.
                </p>
              ) : null}
            </div>
          ) : null}

          {bajados.length > 0 ? (
            <div>
              <span className="label">Lo que ha bajado</span>
              <ul className="mt-1 flex flex-col gap-0.5">
                {bajados.slice(0, 8).map((x) => (
                  <FilaCrecimiento key={x.ruta} h={x} />
                ))}
              </ul>
              {bajados.length > 8 ? (
                <p className="text-fg-faint mt-1 text-xs">
                  … y {num(bajados.length - 8)} más que también han bajado.
                </p>
              ) : null}
            </div>
          ) : null}

          {crecidos.length === 0 && bajados.length === 0 ? (
            <p className="text-fg-muted text-xs">
              Ningún hijo directo cambió de tamaño respecto a la medida anterior.
            </p>
          ) : null}

          {c.hijos_parcial ? (
            <p className="text-fg-faint text-xs" role="note">
              Alguna de las dos medidas no guardó todos los hijos (faltan al menos{" "}
              {num(c.hijos_faltan)}): puede aparecer como nuevo o desaparecido algo que solo cambió
              de puesto en la lista de los más grandes.
            </p>
          ) : null}
        </>
      )}
    </Card>
  );
}

export default function Almacenamiento() {
  const ui = useApp((st) => st.ui.almacen);
  const setUi = useApp((st) => st.setUi);
  const setVista = useApp((st) => st.setVista);
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("almacen"));
  const error = useApp(errorDe("almacen"));
  const enCurso = useApp((st) => st.accionEnCurso);

  const [arbol, setArbol] = useState<ArbolAlmacen | null>(null);
  const [grandes, setGrandes] = useState<FicheroGrande[]>([]);
  // La lista de los más grandes es OTRO recorrido del árbol (el del nivel no
  // guarda tamaños por fichero), así que tarda casi lo mismo que el primero. Se
  // marca aparte para que el árbol ya se pueda mirar y usar mientras se busca,
  // en vez de dejar toda la pantalla en "midiendo".
  const [cargandoGrandes, setCargandoGrandes] = useState(false);
  const [montajes, setMontajes] = useState<Montaje[]>([]);
  // El histórico de la carpeta que se está mirando: se pide DESPUÉS del árbol,
  // porque es el árbol el que guarda la medida y comparar sin la de ahora mismo
  // dejaría fuera el análisis que acabas de hacer.
  const [historial, setHistorial] = useState<HistorialDisco | null>(null);
  const [historialError, setHistorialError] = useState<string | null>(null);
  const [resultados, setResultados] = useState<Coincidencia[] | null>(null);
  const [borrador, setBorrador] = useState(ui.raiz);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [modo, setModo] = useState<"papelera" | "definitivo">("papelera");
  const [confirmando, setConfirmando] = useState(false);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);
  // Las tres herramientas caras van bajo demanda: se piden al pulsar, no al abrir.
  const [duplicados, setDuplicados] = useState<Duplicado[] | null>(null);
  const [vacias, setVacias] = useState<string[] | null>(null);
  const [enlaces, setEnlaces] = useState<EnlaceRoto[] | null>(null);

  const col: Col = esCol(ui.col) ? ui.col : "bytes";

  // Cada análisis lleva su número: si se cambia de carpeta antes de que termine el
  // anterior, la respuesta VIEJA no puede pisar a la nueva (ni apagar su aviso).
  const peticion = useRef(0);

  const analizar = useCallback(async () => {
    const mio = ++peticion.current;
    setCargando("almacen", true);
    setConfirmando(false);
    // El histórico de la carpeta anterior no vale para esta: se vacía hasta que
    // llegue el suyo (si no, se enseñaría la comparación de OTRA carpeta).
    setHistorial(null);
    setHistorialError(null);
    try {
      const a = await api.almacen.arbol(ui.raiz || undefined);
      if (peticion.current !== mio) return;
      setArbol(a);
      setSel(new Set());
      limpiarError("almacen");
      // El árbol ya está: se suelta el aviso de "midiendo" y se van a por las
      // otras dos lecturas, que son independientes entre sí (si una falla, el
      // árbol no se pierde) y de las que solo la de los más grandes es cara.
      setCargando("almacen", false);
      setGrandes([]);
      setCargandoGrandes(true);
      const [g, m, h] = await Promise.allSettled([
        api.almacen.grandes(ui.raiz || undefined, 50),
        api.almacen.montajes(),
        api.almacen.historial(ui.raiz || undefined),
      ]);
      if (peticion.current !== mio) return;
      if (g.status === "fulfilled") setGrandes(g.value);
      if (m.status === "fulfilled") setMontajes(m.value);
      if (h.status === "fulfilled") {
        setHistorial(h.value);
        setHistorialError(null);
      } else {
        setHistorial(null);
        setHistorialError(String(h.reason));
      }
      setCargandoGrandes(false);
    } catch (e) {
      if (peticion.current === mio) {
        setError("almacen", String(e));
        setArbol(null);
      }
    } finally {
      if (peticion.current === mio) {
        setCargando("almacen", false);
        setCargandoGrandes(false);
      }
    }
  }, [ui.raiz, setCargando, setError, limpiarError]);

  useEffect(() => {
    void analizar();
  }, [analizar]);

  const irA = (ruta: string) => {
    setUi("almacen", { raiz: ruta });
    setBorrador(ruta);
    setResultados(null);
    setResultado(null);
  };

  const buscarAhora = useCallback(async () => {
    const q = ui.q.trim();
    if (!q) {
      setResultados(null);
      return;
    }
    setCargando("almacen", true);
    try {
      setResultados(await api.almacen.buscar(ui.raiz || undefined, q, 200));
      limpiarError("almacen");
    } catch (err) {
      setError("almacen", String(err));
    } finally {
      setCargando("almacen", false);
    }
  }, [ui.q, ui.raiz, setCargando, setError, limpiarError]);

  /**
   * Pide la herramienta que se está mirando. NO se pide sola al abrir: la de
   * repetidos lee los ficheros y en una carpeta grande tarda minutos, así que se
   * lanza cuando el usuario la pide y sabe lo que va a costar.
   */
  const cargarHerramienta = async () => {
    setCargando("almacen", true);
    try {
      const raiz = ui.raiz || undefined;
      if (ui.herramienta === "duplicados") setDuplicados(await api.almacen.duplicados(raiz));
      else if (ui.herramienta === "vacias") setVacias(await api.almacen.vacias(raiz));
      else if (ui.herramienta === "enlaces") setEnlaces(await api.almacen.enlaces(raiz));
      limpiarError("almacen");
    } catch (e) {
      setError("almacen", String(e));
    } finally {
      setCargando("almacen", false);
    }
  };

  const borrarSel = async () => {
    const rutas = [...sel];
    const r = await ejecutar("almacen:borrar", { rutas, definitivo: modo === "definitivo" });
    setResultado(r);
    setConfirmando(false);
    if (r.ok) {
      setSel(new Set());
      await analizar();
      // Si se borró desde los resultados de una búsqueda, esa lista también está
      // desfasada: se vuelve a pedir, que si no seguiría enseñando lo borrado.
      if (resultados) await buscarAhora();
      // Y si se borró desde una herramienta (repetidos, vacías, enlaces), lo mismo.
      if (ui.herramienta !== "contenido") await cargarHerramienta();
    }
  };

  const bytesSel = useMemo(() => {
    // El total sale de TODAS las listas que pueden llevar casilla: el nivel que se
    // está viendo, los resultados de la búsqueda y los grupos de repetidos. Se
    // de-duplica por ruta porque un fichero puede estar en más de una (si la
    // búsqueda encuentra un hijo directo de la carpeta que ya se ve), y sin eso se
    // contaría dos veces.
    const porRuta = new Map<string, number>();
    for (const h of arbol?.hijos ?? []) porRuta.set(h.ruta, h.bytes);
    for (const c of resultados ?? []) {
      if (c.bytes != null && !porRuta.has(c.ruta)) porRuta.set(c.ruta, c.bytes);
    }
    for (const d of duplicados ?? []) {
      for (const r of d.rutas) if (!porRuta.has(r)) porRuta.set(r, d.bytes);
    }
    // Carpetas vacías y enlaces rotos no ocupan nada por sí mismos (lo que se
    // libera es el hueco, no bytes de contenido): se cuentan como 0 y el mensaje
    // dice cuántos elementos son.
    for (const v of vacias ?? []) if (!porRuta.has(v)) porRuta.set(v, 0);
    for (const e of enlaces ?? []) if (!porRuta.has(e.ruta)) porRuta.set(e.ruta, 0);
    let total = 0;
    for (const r of sel) total += porRuta.get(r) ?? 0;
    return total;
  }, [arbol, resultados, duplicados, vacias, enlaces, sel]);

  if (arbol == null) {
    return (
      <Vacio titulo={error ? "No se pudo analizar la carpeta" : "Analizando el disco…"}>
        {error ??
          "Se está recorriendo la carpeta para medir lo que ocupa cada cosa. Un home grande tarda unos segundos."}
      </Vacio>
    );
  }

  const raizActual = arbol.ruta;

  return (
    <div className="flex flex-col gap-4">
      {/* ── Elegir carpeta y buscar ─────────────────────────────────────────── */}
      <Card className="flex flex-col gap-3">
        <div className="flex flex-wrap items-end gap-2">
          <form
            className="flex min-w-[280px] flex-1 flex-col gap-1"
            onSubmit={(e) => {
              e.preventDefault();
              irA(borrador.trim() || ui.raiz);
            }}
          >
            <label htmlFor="raiz-almacen" className="label">
              Carpeta que analizar
            </label>
            <div className="flex gap-2">
              <input
                id="raiz-almacen"
                className="mono bg-raised border-line min-w-0 flex-1 rounded-md border px-2 py-1 text-xs"
                value={borrador}
                onChange={(e) => setBorrador(e.target.value)}
                placeholder="Vacío = tu carpeta personal"
                spellCheck={false}
              />
              <Boton type="submit" variante="acento" disabled={cargando}>
                Analizar
              </Boton>
            </div>
          </form>
          <div className="flex items-center gap-2">
            <Boton onClick={() => irA(padreDe(raizActual))} disabled={cargando} title="Subir a la carpeta que la contiene">
              <IconArrowUp size={13} aria-hidden="true" /> Subir
            </Boton>
            <Boton onClick={() => irA("")} disabled={cargando} title="Volver a tu carpeta personal">
              <IconHome size={13} aria-hidden="true" /> Inicio
            </Boton>
          </div>
        </div>

        {/* El buscador por nombre solo tiene sentido sobre el contenido: en las
            otras lentes ya se está buscando otra cosa. */}
        {ui.herramienta === "contenido" ? (
        <form
          className="flex items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void buscarAhora();
          }}
        >
          <div className="flex min-w-[240px] flex-1 flex-col gap-1">
            <label htmlFor="buscar-almacen" className="label">
              Buscar por nombre dentro de esta carpeta
            </label>
            <input
              id="buscar-almacen"
              className="bg-raised border-line rounded-md border px-2 py-1 text-xs"
              value={ui.q}
              onChange={(e) => setUi("almacen", { q: e.target.value })}
              placeholder="p. ej. .gguf, node_modules, .iso"
            />
          </div>
          <Boton type="submit" disabled={cargando}>
            <IconSearch size={13} aria-hidden="true" /> Buscar
          </Boton>
          {resultados ? (
            <Boton
              onClick={() => {
                setUi("almacen", { q: "" });
                setResultados(null);
              }}
            >
              Quitar búsqueda
            </Boton>
          ) : null}
        </form>
        ) : null}

        {resultado ? (
          <p className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")} role={resultado.ok ? "status" : "alert"}>
            {resultado.mensaje}
          </p>
        ) : null}
      </Card>

      {/* ── Cifras de la carpeta ────────────────────────────────────────────── */}
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {/* Con exclusiones actuando, el total deja claro que es lo MEDIDO: si no,
            parecería que la carpeta ocupa menos de lo que ocupa. */}
        <Kpi
          etiqueta={arbol.excluidos.length > 0 ? "Ocupa lo medido" : "Ocupa en total"}
          valor={bLegibles(arbol.bytes, 1)}
          pie={<span className="mono">{raizActual}</span>}
        />
        <Kpi etiqueta="Ficheros" valor={num(arbol.ficheros)} />
        <Kpi etiqueta="Carpetas" valor={num(arbol.dirs)} />
        <Kpi
          etiqueta="Tardó"
          valor={num(arbol.ms / 1000, 1)}
          unidad="s"
          pie={`${num(arbol.entradas)} entradas leídas`}
        />
      </section>

      {/* ── Avisos de la medición: lo que no se pudo medir del todo ────────── */}
      {arbol.truncado || arbol.omitidos > 0 || arbol.resto_n > 0 || arbol.excluidos.length > 0 ? (
        <Card className="flex flex-col gap-1">
          <Etiqueta>De esta medición</Etiqueta>
          {arbol.excluidos.length > 0 ? (
            <p className="text-fg-faint flex flex-wrap items-center gap-1.5 text-xs" role="note">
              <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
              <span>
                {arbol.excluidos.length === 1
                  ? "1 exclusión ha dejado fuera parte de este análisis: "
                  : `${num(arbol.excluidos.length)} exclusiones han dejado fuera parte de este análisis: `}
                {arbol.excluidos.join(" · ")}
              </span>
              <Boton onClick={() => setVista("ajustes")}>Ver exclusiones</Boton>
            </p>
          ) : null}
          {arbol.truncado ? (
            <p className="text-warn flex items-start gap-1.5 text-xs" role="status">
              <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
              El recorrido se ha cortado por presupuesto: el TOTAL puede quedarse corto. Analiza
              una carpeta más concreta para verla entera.
            </p>
          ) : null}
          {arbol.resto_n > 0 ? (
            <p className="text-fg-muted text-xs">
              Se enseñan los {num(arbol.hijos.length)} hijos más grandes; hay {num(arbol.resto_n)} más
              que suman {bLegibles(arbol.resto_bytes, 1)}.
            </p>
          ) : null}
          {arbol.omitidos > 0 ? (
            <p className="text-fg-muted text-xs">
              {num(arbol.omitidos)} entradas no se pudieron leer (permisos): no están contadas.
            </p>
          ) : null}
        </Card>
      ) : null}

      {/* ── Cómo ha cambiado (histórico de esta carpeta) ────────────────────── */}
      <BloqueCambios h={historial} error={historialError} />

      {/* ── Qué mirar: cuatro lentes sobre la MISMA carpeta ─────────────────── */}
      <Card className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Etiqueta>Qué mirar</Etiqueta>
          {HERRAMIENTAS.map((h) => (
            <Boton
              key={h.id}
              variante={ui.herramienta === h.id ? "acento" : "normal"}
              aria-pressed={ui.herramienta === h.id}
              title={h.ayuda}
              onClick={() => {
                setUi("almacen", { herramienta: h.id });
                setSel(new Set());
                setConfirmando(false);
                setResultado(null);
              }}
            >
              {h.nombre}
            </Boton>
          ))}
        </div>
        <p className="text-fg-muted text-xs">{HERRAMIENTAS.find((h) => h.id === ui.herramienta)?.ayuda}</p>
        {ui.herramienta !== "contenido" ? (
          <div className="flex flex-wrap items-center gap-2">
            <Boton onClick={() => void cargarHerramienta()} disabled={cargando || !!enCurso}>
              <IconSearch size={13} aria-hidden="true" /> Buscar
            </Boton>
            {ui.herramienta === "duplicados" ? (
              <span className="text-fg-faint text-xs">
                Se miran los ficheros de más de 1 MB y se LEE su contenido: en una carpeta grande tarda.
              </span>
            ) : null}
          </div>
        ) : null}
      </Card>

      {/* ── Repetidos ──────────────────────────────────────────────────────── */}
      {ui.herramienta === "duplicados" ? (
        <Card className="flex flex-col gap-2">
          <Etiqueta>Ficheros repetidos por contenido</Etiqueta>
          {duplicados == null ? (
            <Vacio titulo="Sin buscar todavía">
              Pulsa «Buscar»: se agrupan los ficheros idénticos y se dice lo que se liberaría
              dejando una copia de cada grupo.
            </Vacio>
          ) : (
            <>
              <p className="text-fg-muted text-xs">
                {num(duplicados.length)} grupos · se pueden liberar{" "}
                {bLegibles(
                  duplicados.reduce((a, d) => a + d.desperdicio, 0),
                  1,
                )}
                . De cada grupo se puede borrar todo menos una copia.
              </p>
              <Seleccionable
                rutas={duplicados.flatMap((d) => d.rutas)}
                sel={sel}
                onSel={(ruta, marcado) => {
                  const s = new Set(sel);
                  if (marcado) s.add(ruta);
                  else s.delete(ruta);
                  setSel(s);
                }}
                etiqueta="ficheros repetidos"
                columnas={["Fichero", "Tamaño", "Grupo"]}
                seleccionarTexto="Marcar los repetidos (dejar una copia)"
                onSeleccionarTodas={() => {
                  const s = new Set<string>();
                  // Se deja SIEMPRE la primera copia de cada grupo: marcar todas
                  // sería borrar también la última copia de un fichero.
                  for (const d of duplicados) for (const r of d.rutas.slice(1)) s.add(r);
                  setSel(s);
                }}
                filas={duplicados.flatMap((d) =>
                  d.rutas.map((r, i) => ({
                    clave: r,
                    celdas: [
                      <span className="mono block truncate" title={r}>
                        {r}
                      </span>,
                      bLegibles(d.bytes, 1),
                      i === 0 ? (
                        <Insignia tono="ok">la que se conserva</Insignia>
                      ) : (
                        <span className="text-fg-faint">
                          copia {i + 1} de {d.rutas.length}
                        </span>
                      ),
                    ],
                  })),
                )}
                vacio="No se han encontrado ficheros repetidos por encima de 1 MB."
              />
            </>
          )}
        </Card>
      ) : null}

      {/* ── Vacías ─────────────────────────────────────────────────────────── */}
      {ui.herramienta === "vacias" ? (
        <Card className="flex flex-col gap-2">
          <Etiqueta>Carpetas sin ningún fichero</Etiqueta>
          {vacias == null ? (
            <Vacio titulo="Sin buscar todavía">Pulsa «Buscar» para recorrer la carpeta.</Vacio>
          ) : (
            <Seleccionable
              rutas={vacias}
              sel={sel}
              onSel={(ruta, marcado) => {
                const s = new Set(sel);
                if (marcado) s.add(ruta);
                else s.delete(ruta);
                setSel(s);
              }}
              etiqueta="carpetas vacías"
              columnas={["Carpeta"]}
              onSeleccionarTodas={() => setSel(new Set(vacias))}
              filas={vacias.map((r) => ({
                clave: r,
                celdas: [
                  <span className="mono block truncate" title={r}>
                    {r}
                  </span>,
                ],
              }))}
              vacio="No hay ninguna carpeta vacía en este árbol."
            />
          )}
        </Card>
      ) : null}

      {/* ── Enlaces rotos ──────────────────────────────────────────────────── */}
      {ui.herramienta === "enlaces" ? (
        <Card className="flex flex-col gap-2">
          <Etiqueta>Enlaces que no llevan a ninguna parte</Etiqueta>
          {enlaces == null ? (
            <Vacio titulo="Sin buscar todavía">Pulsa «Buscar» para recorrer la carpeta.</Vacio>
          ) : (
            <Seleccionable
              rutas={enlaces.map((e) => e.ruta)}
              sel={sel}
              onSel={(ruta, marcado) => {
                const s = new Set(sel);
                if (marcado) s.add(ruta);
                else s.delete(ruta);
                setSel(s);
              }}
              etiqueta="enlaces rotos"
              columnas={["Enlace", "Apunta a"]}
              onSeleccionarTodas={() => setSel(new Set(enlaces.map((e) => e.ruta)))}
              filas={enlaces.map((e) => ({
                clave: e.ruta,
                celdas: [
                  <span className="mono block truncate" title={e.ruta}>
                    {e.ruta}
                  </span>,
                  <span className="mono text-fg-faint block truncate" title={e.destino}>
                    {e.destino}
                  </span>,
                ],
              }))}
              vacio="No hay ningún enlace roto en este árbol."
            />
          )}
        </Card>
      ) : null}

      {/* ── Contenido de la carpeta (o resultados de la búsqueda) ──────────── */}
      {ui.herramienta === "contenido" ? (
      <Card className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Etiqueta>{resultados ? `Resultados para «${ui.q.trim()}»` : `Contenido de ${nombreDe(raizActual)}`}</Etiqueta>
          <Insignia tono="neutro">
            <span className="mono">{raizActual}</span>
          </Insignia>
          {cargando ? <span className="text-fg-faint text-xs">midiendo…</span> : null}
        </div>

        {resultados ? (
          resultados.length === 0 ? (
            <Vacio titulo="Ninguna coincidencia">
              No hay nada con ese nombre dentro de esta carpeta (los enlaces simbólicos no se
              recorren).
            </Vacio>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full border-collapse text-xs">
                <caption className="sr-only">Coincidencias de la búsqueda por nombre</caption>
                <thead>
                  <tr className="border-line-soft border-b">
                    <Th>Nombre</Th>
                    <Th alineado="der">Tamaño</Th>
                    <Th alineado="der">Modificado</Th>
                    <Th>
                      <span className="sr-only">Acciones</span>
                    </Th>
                  </tr>
                </thead>
                <tbody>
                  {resultados.map((c) => (
                    <tr key={c.ruta} className="border-line-soft hover:bg-raised border-b last:border-0">
                      <td className="max-w-[420px] px-4 py-1.5" title={c.ruta}>
                        <span className="flex items-center gap-1.5">
                          {c.es_dir ? (
                            <IconFolder size={14} aria-hidden="true" className="text-accent-dim shrink-0" />
                          ) : (
                            <span className="w-[14px] shrink-0" aria-hidden="true" />
                          )}
                          <span className="truncate">{c.nombre}</span>
                        </span>
                      </td>
                      <td className="mono px-4 py-1.5 text-right whitespace-nowrap">
                        {/* Una carpeta no se mide al buscar: "—", nunca un 0. */}
                        {c.bytes == null ? "—" : bLegibles(c.bytes, 1)}
                      </td>
                      <td className="text-fg-faint px-4 py-1.5 text-right whitespace-nowrap">
                        {hace(c.modificado)}
                      </td>
                      <td className="px-4 py-1.5 text-right">
                        {c.es_dir ? (
                          <Boton onClick={() => irA(c.ruta)} aria-label={`Abrir la carpeta ${c.nombre}`}>
                            <IconFolderOpen size={13} aria-hidden="true" /> Abrir
                          </Boton>
                        ) : (
                          <input
                            type="checkbox"
                            checked={sel.has(c.ruta)}
                            onChange={(e) => {
                              const s = new Set(sel);
                              if (e.target.checked) s.add(c.ruta);
                              else s.delete(c.ruta);
                              setSel(s);
                            }}
                            aria-label={`Seleccionar ${c.nombre}`}
                          />
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )
        ) : (
          <TablaContenido
            arbol={arbol}
            col={col}
            dir={ui.dir}
            onOrdenar={(c, d) => setUi("almacen", { col: c, dir: d })}
            onBajar={irA}
            seleccion={sel}
            onSel={(ruta, marcado) => {
              const s = new Set(sel);
              if (marcado) s.add(ruta);
              else s.delete(ruta);
              setSel(s);
            }}
          />
        )}
      </Card>
      ) : null}

      {/* ── Borrar lo seleccionado (acción destructiva, aparte y con confirmación) ── */}
      {sel.size > 0 ? (
        <Card className="border-bad/40 flex flex-col gap-3">
          <Etiqueta>Borrar lo seleccionado</Etiqueta>
          <p className="text-fg-muted text-xs">
            {num(sel.size)} elementos, {bLegibles(bytesSel, 1)}.
          </p>

          <fieldset className="flex flex-col gap-2">
            <legend className="sr-only">Cómo borrar</legend>
            <label className="flex items-start gap-2 text-xs">
              <input
                type="radio"
                name="modo-borrado"
                checked={modo === "papelera"}
                onChange={() => setModo("papelera")}
                className="mt-0.5"
              />
              <span>
                <span className="text-fg">A la papelera</span>{" "}
                <span className="text-fg-faint">
                  — se puede recuperar desde el gestor de archivos. El espacio NO se libera hasta
                  vaciar la papelera.
                </span>
              </span>
            </label>
            <label className="flex items-start gap-2 text-xs">
              <input
                type="radio"
                name="modo-borrado"
                checked={modo === "definitivo"}
                onChange={() => setModo("definitivo")}
                className="mt-0.5"
              />
              <span>
                <span className="text-bad">Borrado definitivo</span>{" "}
                <span className="text-fg-faint">
                  — libera el espacio ahora y NO se puede deshacer.
                </span>
              </span>
            </label>
          </fieldset>

          {confirmando ? (
            <div className="flex flex-wrap items-center gap-2" role="alert">
              <span className="text-fg-muted text-xs">
                {modo === "definitivo"
                  ? `Se borrarán DEFINITIVAMENTE ${num(sel.size)} elementos (${bLegibles(bytesSel, 1)}). No se puede deshacer.`
                  : `Se moverán a la PAPELERA ${num(sel.size)} elementos (${bLegibles(bytesSel, 1)}).`}
              </span>
              <Boton variante="peligro" disabled={!!enCurso} onClick={() => void borrarSel()}>
                {modo === "definitivo" ? "Sí, borrar de verdad" : "Sí, a la papelera"}
              </Boton>
              <Boton onClick={() => setConfirmando(false)}>No</Boton>
            </div>
          ) : (
            <div className="flex flex-wrap items-center gap-2">
              <Boton variante="peligro" disabled={!!enCurso} onClick={() => setConfirmando(true)}>
                <IconTrash size={13} aria-hidden="true" /> Borrar selección
              </Boton>
              <Boton onClick={() => setSel(new Set())}>Quitar selección</Boton>
            </div>
          )}
        </Card>
      ) : null}

      {/* ── Dónde está el espacio: los discos ──────────────────────────────── */}
      <Card className="flex flex-col gap-2">
        <Etiqueta>Uso por disco</Etiqueta>
        {montajes.length === 0 ? (
          <p className="text-fg-muted text-xs">No se pudo leer el uso de los discos (`df`).</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-xs">
              <caption className="sr-only">Uso de cada punto de montaje real</caption>
              <thead>
                <tr className="border-line-soft border-b">
                  <Th>Punto</Th>
                  <Th>Dispositivo</Th>
                  <Th alineado="der">Usado</Th>
                  <Th alineado="der">Libre</Th>
                  <Th alineado="der">Total</Th>
                  <Th className="w-32">Uso</Th>
                </tr>
              </thead>
              <tbody>
                {montajes.map((m) => (
                  <tr
                    key={m.punto}
                    className="border-line-soft border-b last:border-0"
                    // El sistema de ficheros y si es extraíble van en el título: la
                    // tabla se queda con lo que se compara de un vistazo, y el
                    // detalle sigue estando a mano.
                    title={`${m.tipo}${m.extraible ? " · unidad extraíble" : ""}`}
                  >
                    <td className="mono px-4 py-1.5">{m.punto}</td>
                    <td className="mono text-fg-faint px-4 py-1.5">
                      {m.dispositivo || m.tipo}
                      {m.extraible ? " (extraíble)" : ""}
                    </td>
                    <td className="mono px-4 py-1.5 text-right whitespace-nowrap">{bLegibles(m.usado, 1)}</td>
                    <td className="mono px-4 py-1.5 text-right whitespace-nowrap">{bLegibles(m.libre, 1)}</td>
                    <td className="mono text-fg-muted px-4 py-1.5 text-right whitespace-nowrap">
                      {bLegibles(m.total, 1)}
                    </td>
                    <td className="px-4 py-1.5">
                      <div className="flex items-center gap-2">
                        <Barra
                          valor={m.uso_pct}
                          nivel={m.uso_pct >= 90 ? "bad" : m.uso_pct >= 75 ? "warn" : "ok"}
                        />
                        <span className="mono text-fg-faint w-9 text-right">{pct(m.uso_pct)}</span>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      {/* ── Los ficheros más grandes de esta carpeta ───────────────────────── */}
      <Card className="flex flex-col gap-2">
        <Etiqueta>Los ficheros más grandes de {nombreDe(raizActual)}</Etiqueta>
        {cargandoGrandes ? (
          <p className="text-fg-muted text-xs">
            Buscando los más grandes… (es un segundo recorrido del árbol, no bloquea lo de arriba)
          </p>
        ) : grandes.length === 0 ? (
          <p className="text-fg-muted text-xs">No hay ficheros que enseñar (o no se pudieron leer).</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-xs">
              <caption className="sr-only">Los cincuenta ficheros más grandes del árbol analizado</caption>
              <thead>
                <tr className="border-line-soft border-b">
                  <Th alineado="der">Tamaño</Th>
                  <Th>Fichero</Th>
                  <Th alineado="der">Modificado</Th>
                </tr>
              </thead>
              <tbody>
                {grandes.map((f) => (
                  <tr key={f.ruta} className="border-line-soft border-b last:border-0">
                    <td className="mono px-4 py-1.5 text-right whitespace-nowrap">{bLegibles(f.bytes, 1)}</td>
                    <td className="mono text-fg-muted max-w-[560px] truncate px-4 py-1.5" title={f.ruta}>
                      {f.ruta}
                    </td>
                    <td className="text-fg-faint px-4 py-1.5 text-right whitespace-nowrap">{hace(f.modificado)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
