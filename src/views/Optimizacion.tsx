/**
 * Optimización: qué basura se puede tirar sin miedo y qué arranca solo.
 *
 * Dos bloques con dos preguntas distintas, en el orden en que se usan:
 *
 *  1. **Limpieza** — el catálogo de cachés, temporales y registros que se
 *     regeneran solos, medido: cuánto se liberaría de cada uno. Es lo que en Kudu
 *     es el "cleaner", con sus reglas de Linux portadas.
 *  2. **Arranque** — los programas que arrancan con la sesión (XDG Autostart),
 *     con la posibilidad de quitarlos de en medio. Recortar el arranque es la
 *     otra mitad de "optimizar el equipo": menos cosas levantándose a la vez.
 *
 * Tres decisiones que no son de gusto:
 *
 * 1. **Lo que no se puede limpiar desde aquí se dice, con su comando.** Hay
 *    objetivos que necesitan root (/var/cache/dnf, el journal) y otros que se
 *    limpian mejor con su propia herramienta (pnpm, docker, uv). En vez de lanzar
 *    `sudo` a escondidas o de borrar el almacén de pnpm a lo bruto, se enseña el
 *    comando exacto y se copia con un botón.
 * 2. **La limpieza NO va a la papelera, y se dice antes de confirmar.** Mover una
 *    caché de 3 GB a la papelera no libera nada hasta vaciarla, así que la basura
 *    se borra de verdad; por eso el botón enseña cuánto y qué se va a borrar
 *    antes de hacerlo.
 * 3. **La lista se filtra en el cliente.** El escaneo mide TODO el catálogo una
 *    vez; marcar o desmarcar una categoría no vuelve a recorrer el disco.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { IconAlertTriangle, IconPower, IconRefresh, IconTrash } from "@tabler/icons-react";
import { clsx } from "clsx";
import { cargandoDe, errorDe, ejecutar, useApp, type ResultadoAccion } from "../store";
import {
  api,
  type CategoriaLimpieza,
  type EntradaArranque,
  type EscaneoLimpieza,
  type ObjetivoLimpieza,
  type PapeleraEstado,
  type Programacion,
  type RecetaProgramacion,
} from "../lib/tauri";
import { Boton, BotonCopiar, Card, Etiqueta, Insignia, Kpi, Th, Vacio } from "../components/ui";
import { Bases } from "../components/Bases";
import type { Tono } from "../components/ui";
import { bLegibles, num } from "../lib/format";

/** Cómo se enseña un objetivo que no se puede limpiar desde aquí. */
function InsigniasObjetivo({ o }: { o: ObjetivoLimpieza }) {
  const marcas: { texto: string; tono: Tono }[] = [];
  if (o.root) marcas.push({ texto: "necesita root", tono: "warn" });
  if (o.comando && !o.root) marcas.push({ texto: "con su comando", tono: "acento" });
  if (o.sin_permiso) marcas.push({ texto: "sin permiso", tono: "warn" });
  if (o.parcial) marcas.push({ texto: "medición incompleta", tono: "warn" });
  // Un reinicio de caché de rendimiento no es un problema, es un aviso: se puede
  // limpiar, pero la primera vez irá más lento y no se marca solo.
  if (o.reinicio_cache) marcas.push({ texto: "reinicio de caché: no se borra solo", tono: "acento" });
  if (o.recientes > 0) marcas.push({ texto: `${num(o.recientes)} recientes se quedan`, tono: "neutro" });
  if (marcas.length === 0) return null;
  return (
    <span className="flex flex-wrap items-center gap-1">
      {marcas.map((m) => (
        <Insignia key={m.texto} tono={m.tono}>
          {m.texto}
        </Insignia>
      ))}
    </span>
  );
}

/* ── Bloque 1: limpieza ───────────────────────────────────────────────────── */

function BloqueLimpieza() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("limpieza"));
  const error = useApp(errorDe("limpieza"));
  const enCurso = useApp((st) => st.accionEnCurso);
  const ui = useApp((st) => st.ui.optimizacion);
  const setUi = useApp((st) => st.setUi);
  const setVista = useApp((st) => st.setVista);

  const [categorias, setCategorias] = useState<CategoriaLimpieza[]>([]);
  const [escaneo, setEscaneo] = useState<EscaneoLimpieza | null>(null);
  const [confirmando, setConfirmando] = useState(false);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  const escanear = useCallback(async () => {
    setCargando("limpieza", true);
    setConfirmando(false);
    try {
      setEscaneo(await api.limpieza.escanear([]));
      limpiarError("limpieza");
    } catch (e) {
      setError("limpieza", String(e));
      setEscaneo(null);
    } finally {
      setCargando("limpieza", false);
    }
  }, [setCargando, setError, limpiarError]);

  useEffect(() => {
    api.limpieza
      // La categoría de privacidad no se ofrece aquí: sus objetivos son huellas y
      // esta sección limpia basura que se regenera sola. Si se ofreciera, el chip
      // daría una lista vacía sin explicar por qué.
      .categorias()
      .then((c) => setCategorias(c.filter((x) => x.id !== "privacidad")))
      .catch((e) => setError("limpieza", String(e)));
    void escanear();
  }, [escanear, setError]);

  const lista = useMemo(() => {
    // Las HUELLAS (historial, recientes, portapapeles) NO se listan aquí: son de
    // Seguridad, donde se piden una a una y con aviso de que no se recuperan.
    // Aquí, con un «marcar todo lo que ocupa», se borrarían de un clic, y eso es
    // justo lo que hay que impedir. El backend las marca (`traza`) y esta vista
    // las respeta.
    const objs = (escaneo?.objetivos ?? []).filter((o) => !o.traza);
    if (ui.categorias.length === 0) return objs;
    return objs.filter((o) => ui.categorias.includes(o.categoria));
  }, [escaneo, ui.categorias]);

  /** Cuántas huellas hay, para poder decir que están en otra sección y por qué. */
  const huellas = useMemo(
    () => (escaneo?.objetivos ?? []).filter((o) => o.traza && o.bytes > 0),
    [escaneo],
  );

  const seleccionables = useMemo(
    () => lista.filter((o) => !o.root && !o.comando),
    [lista],
  );
  const sel = useMemo(
    () => new Set(seleccionables.filter((o) => ui.seleccion.includes(o.id)).map((o) => o.id)),
    [seleccionables, ui.seleccion],
  );
  const bytesSel = useMemo(
    () => seleccionables.filter((o) => sel.has(o.id)).reduce((a, o) => a + o.bytes, 0),
    [seleccionables, sel],
  );
  const bytesLista = useMemo(() => lista.reduce((a, o) => a + o.bytes, 0), [lista]);

  const alternarCat = (id: string) => {
    const marcadas = ui.categorias.includes(id)
      ? ui.categorias.filter((c) => c !== id)
      : [...ui.categorias, id];
    setUi("optimizacion", { categorias: marcadas });
  };

  const limpiar = async () => {
    const r = await ejecutar("limpieza:limpiar", { ids: [...sel] });
    setResultado(r);
    setConfirmando(false);
    if (r.ok) {
      setUi("optimizacion", { seleccion: [] });
      await escanear();
    }
  };

  if (escaneo == null) {
    return (
      <Card>
        <Etiqueta>{error ? "No se pudo escanear la basura" : "Escaneando cachés y temporales…"}</Etiqueta>
        {error ? (
          <p className="text-fg-muted mt-2 text-xs">{error}</p>
        ) : (
          <p className="text-fg-muted mt-2 text-xs">
            Se está midiendo lo que ocupa cada caché del catálogo. Tarda unos segundos porque
            recorre los ficheros de verdad.
          </p>
        )}
      </Card>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Kpi
          etiqueta="Se puede liberar"
          valor={bLegibles(escaneo.bytes, 1)}
          pie={`${num(escaneo.objetivos.length)} objetivos encontrados`}
        />
        <Kpi etiqueta="Elementos" valor={num(escaneo.elementos)} />
        <Kpi etiqueta="Tardó" valor={num(escaneo.ms / 1000, 1)} unidad="s" />
        <Kpi
          etiqueta="Seleccionado"
          valor={bLegibles(bytesSel, 1)}
          pie={sel.size === 0 ? "nada marcado" : `${num(sel.size)} objetivos`}
        />
      </section>

      <Card className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <Etiqueta>Categorías</Etiqueta>
          <Boton
            onClick={() => setUi("optimizacion", { categorias: [] })}
            variante={ui.categorias.length === 0 ? "acento" : "normal"}
            aria-pressed={ui.categorias.length === 0}
          >
            Todas
          </Boton>
          {categorias.map((c) => {
            const activa = ui.categorias.includes(c.id);
            const cuantos = (escaneo.objetivos ?? []).filter((o) => o.categoria === c.id).length;
            return (
              <Boton
                key={c.id}
                onClick={() => alternarCat(c.id)}
                variante={activa ? "acento" : "normal"}
                aria-pressed={activa}
                title={`${cuantos} objetivos en esta categoría`}
              >
                {c.nombre}
              </Boton>
            );
          })}
          <Boton className="ml-auto" onClick={() => void escanear()} disabled={cargando || !!enCurso}>
            <IconRefresh size={13} aria-hidden="true" /> Volver a escanear
          </Boton>
        </div>

        {escaneo.truncado ? (
          <p className="text-warn flex items-start gap-1.5 text-xs" role="status">
            <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
            El escaneo se ha cortado por presupuesto: algunos objetivos pueden estar medidos a
            medias (se marcan como «medición incompleta»).
          </p>
        ) : null}

        {escaneo.excluidos.length > 0 ? (
          <p className="text-fg-faint flex flex-wrap items-center gap-1.5 text-xs" role="note">
            <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
            <span>
              {num(escaneo.excluidos.length)} objetivo(s) no se han medido por tus exclusiones:{" "}
              {escaneo.excluidos.slice(0, 2).join(" · ")}
              {escaneo.excluidos.length > 2 ? ` y ${num(escaneo.excluidos.length - 2)} más` : ""}.
            </span>
            <Boton onClick={() => setVista("ajustes")}>Ver exclusiones</Boton>
          </p>
        ) : null}

        {huellas.length > 0 ? (
          <p className="text-fg-faint flex flex-wrap items-center gap-1.5 text-xs" role="note">
            <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
            <span>
              Hay {num(huellas.length)} huella(s) de tu actividad ({bLegibles(huellas.reduce((a, o) => a + o.bytes, 0), 1)})
              que NO se limpian desde aquí: son historiales, recientes y portapapeles, no se
              recuperan, y se borran una a una.
            </span>
            <Boton onClick={() => setVista("seguridad")}>Ver Seguridad</Boton>
          </p>
        ) : null}

        {resultado ? (
          <p
            className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")}
            role={resultado.ok ? "status" : "alert"}
          >
            {resultado.mensaje}
          </p>
        ) : null}

        {lista.length === 0 ? (
          <Vacio titulo="No hay nada que limpiar en lo que has marcado">
            Ninguna de las categorías seleccionadas tiene cachés en este equipo (o no se pueden
            leer).
          </Vacio>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-xs">
              <caption className="sr-only">
                Objetivos de limpieza con lo que se liberaría de cada uno
              </caption>
              <thead>
                <tr className="border-line-soft border-b">
                  <Th className="w-8">
                    <span className="sr-only">Selección</span>
                  </Th>
                  <Th>Qué</Th>
                  <Th alineado="der">Se libera</Th>
                  <Th alineado="der">Elementos</Th>
                  <Th>Notas</Th>
                </tr>
              </thead>
              <tbody>
                {lista.map((o) => {
                  const fijo = o.root || !!o.comando;
                  return (
                    <tr key={o.id} className="border-line-soft hover:bg-raised align-top border-b last:border-0">
                      <td className="px-4 py-1.5">
                        <input
                          type="checkbox"
                          disabled={fijo}
                          checked={sel.has(o.id)}
                          onChange={(e) => {
                            const s = new Set(ui.seleccion);
                            if (e.target.checked) s.add(o.id);
                            else s.delete(o.id);
                            setUi("optimizacion", { seleccion: [...s] });
                          }}
                          aria-label={fijo ? `${o.subcategoria} (no se limpia desde aquí)` : `Seleccionar ${o.subcategoria}`}
                        />
                      </td>
                      <td className="max-w-[420px] px-4 py-1.5">
                        <div className="text-fg">{o.subcategoria}</div>
                        <div className="text-fg-faint">{o.descripcion}</div>
                        <div className="mono text-fg-faint mt-0.5 truncate" title={o.rutas.join("\n")}>
                          {o.rutas[0]}
                          {o.rutas.length > 1 ? ` (+${o.rutas.length - 1})` : ""}
                        </div>
                        {o.comando ? (
                          <div className="mt-1 flex items-center gap-2">
                            <code className="mono text-accent bg-raised rounded px-1.5 py-0.5">{o.comando}</code>
                            <BotonCopiar texto={o.comando ?? ""} que={`el comando para ${o.subcategoria}`} />
                        </div>
                      ) : null}
                      </td>
                      <td className="mono px-4 py-1.5 text-right whitespace-nowrap">{bLegibles(o.bytes, 1)}</td>
                      <td className="mono text-fg-muted px-4 py-1.5 text-right">{num(o.elementos)}</td>
                      <td className="px-4 py-1.5">
                        <InsigniasObjetivo o={o} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}

        {/* Acción destructiva: SIEMPRE separada, con color de peligro y en dos pasos. */}
        <div className="border-line-soft border-t pt-3">
          {confirmando ? (
            <div className="flex flex-wrap items-center gap-2" role="alert">
              <span className="text-fg-muted text-xs">
                Se borrarán DEFINITIVAMENTE {num(sel.size)} objetivos ({bLegibles(bytesSel, 1)}). Esto no va a la
                papelera y no se puede deshacer.
              </span>
              <Boton variante="peligro" disabled={!!enCurso} onClick={() => void limpiar()}>
                Sí, borrar de verdad
              </Boton>
              <Boton onClick={() => setConfirmando(false)}>No</Boton>
            </div>
          ) : (
            <div className="flex flex-wrap items-center gap-2">
              <Boton
                variante="peligro"
                disabled={sel.size === 0 || !!enCurso}
                onClick={() => setConfirmando(true)}
              >
                <IconTrash size={13} aria-hidden="true" /> Limpiar seleccionados
              </Boton>
              <Boton
                disabled={seleccionables.length === 0}
                onClick={() =>
                  setUi("optimizacion", {
                    // Los REINICIOS de caché se quedan fuera: borrarlos deja la
                    // próxima partida a tirones mientras se recompilan los shaders,
                    // así que se marcan uno a uno. El backend los excluye también de
                    // la limpieza automática.
                    seleccion: seleccionables
                      .filter((o) => o.bytes > 0 && !o.reinicio_cache)
                      .map((o) => o.id),
                  })
                }
              >
                Marcar todo lo que ocupa
              </Boton>
              <span className="text-fg-faint text-xs">
                {bLegibles(bytesLista, 1)} en esta vista
              </span>
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}

/* ── Bloque 2: arranque ───────────────────────────────────────────────────── */

function BloqueArranque() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("arranque"));
  const error = useApp(errorDe("arranque"));
  const enCurso = useApp((st) => st.accionEnCurso);

  const [entradas, setEntradas] = useState<EntradaArranque[] | null>(null);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  const cargar = useCallback(async () => {
    setCargando("arranque", true);
    try {
      setEntradas(await api.arranque.listar());
      limpiarError("arranque");
    } catch (e) {
      setError("arranque", String(e));
      setEntradas(null);
    } finally {
      setCargando("arranque", false);
    }
  }, [setCargando, setError, limpiarError]);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const activos = (entradas ?? []).filter((e) => e.activo).length;

  if (entradas == null) {
    return (
      <Card>
        <Etiqueta>{error ? "No se pudo leer el arranque" : "Leyendo el arranque…"}</Etiqueta>
        {error ? <p className="text-fg-muted mt-2 text-xs">{error}</p> : null}
      </Card>
    );
  }

  return (
    <Card className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>Programas que arrancan con la sesión</Etiqueta>
        <Insignia tono={activos > 12 ? "warn" : "neutro"}>{num(activos)} activos</Insignia>
        <Boton className="ml-auto" onClick={() => void cargar()} disabled={cargando || !!enCurso}>
          <IconRefresh size={13} aria-hidden="true" /> Volver a leer
        </Boton>
      </div>
      <p className="text-fg-muted text-xs">
        Desactivar no borra nada: una entrada del sistema se tapa con un fichero en tu carpeta
        (`Hidden=true`, el mecanismo de XDG) y se puede volver a activar. El fichero del sistema no
        se toca.
      </p>

      {resultado ? (
        <p className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")} role={resultado.ok ? "status" : "alert"}>
          {resultado.mensaje}
        </p>
      ) : null}

      {entradas.length === 0 ? (
        <Vacio titulo="No hay entradas de arranque">
          No hay ningún fichero `.desktop` en `~/.config/autostart` ni en `/etc/xdg/autostart`.
        </Vacio>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-xs">
            <caption className="sr-only">Entradas de arranque de la sesión y su estado</caption>
            <thead>
              <tr className="border-line-soft border-b">
                <Th>Nombre</Th>
                <Th>Origen</Th>
                <Th>Comando</Th>
                <Th alineado="der">Estado</Th>
              </tr>
            </thead>
            <tbody>
              {entradas.map((e) => (
                <tr key={e.id} className="border-line-soft hover:bg-raised align-top border-b last:border-0">
                  <td className="max-w-[280px] px-4 py-1.5">
                    <div className="text-fg truncate">{e.nombre}</div>
                    {e.comentario ? <div className="text-fg-faint truncate">{e.comentario}</div> : null}
                    <div className="mono text-fg-faint truncate" title={e.ruta}>
                      {e.ruta}
                    </div>
                  </td>
                  <td className="px-4 py-1.5">
                    <Insignia tono={e.origen === "usuario" ? "acento" : "neutro"}>{e.origen}</Insignia>
                  </td>
                  <td className="mono text-fg-muted max-w-[260px] truncate px-4 py-1.5" title={e.exec}>
                    {e.exec || "—"}
                  </td>
                  <td className="px-4 py-1.5 text-right">
                    <Boton
                      variante={e.activo ? "peligro" : "normal"}
                      disabled={!!enCurso}
                      onClick={async () => {
                        const r = await ejecutar("arranque:activar", { id: e.id, activo: !e.activo });
                        setResultado(r);
                        await cargar();
                      }}
                      aria-label={`${e.activo ? "Desactivar" : "Activar"} ${e.nombre} al arrancar`}
                    >
                      <IconPower size={13} aria-hidden="true" />
                      {e.activo ? "Desactivar" : "Activar"}
                    </Boton>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

/* ── Bloque 3: la papelera del sistema ───────────────────────────────────── */

/**
 * Cuánto hay en la papelera y el botón para vaciarla.
 *
 * Es de las cifras que más explican un disco lleno y casi nadie mira: lo que se
 * manda a la papelera **no libera espacio** hasta que se vacía. Va aquí, junto a la
 * limpieza, porque es lo mismo: espacio que se puede recuperar hoy.
 */
function BloquePapelera() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("limpieza"));
  const enCurso = useApp((st) => st.accionEnCurso);

  const [estado, setEstado] = useState<PapeleraEstado | null | undefined>(undefined);
  const [confirmando, setConfirmando] = useState(false);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  const cargar = useCallback(async () => {
    setCargando("limpieza", true);
    try {
      setEstado(await api.papelera.estado());
      limpiarError("limpieza");
    } catch (e) {
      setError("limpieza", String(e));
      setEstado(null);
    } finally {
      setCargando("limpieza", false);
    }
  }, [setCargando, setError, limpiarError]);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const vaciar = async () => {
    const r = await ejecutar("papelera:vaciar");
    setResultado(r);
    setConfirmando(false);
    await cargar();
  };

  if (estado === undefined) {
    return (
      <Card>
        <Etiqueta>Mirando la papelera…</Etiqueta>
      </Card>
    );
  }

  return (
    <Card className="flex flex-col gap-2">
      <Etiqueta>Papelera del sistema</Etiqueta>
      {estado == null ? (
        <p className="text-fg-muted text-xs">Este sistema no deja contar la papelera desde aquí.</p>
      ) : (
        <>
          <p className="text-fg-muted text-xs">
            {num(estado.elementos)} elementos, <span className="mono">{bLegibles(estado.bytes, 1)}</span> en{" "}
            <span className="mono">{estado.ruta}</span>. Recuerda que ese espacio{" "}
            <strong className="text-fg">no se libera</strong> hasta vaciarla: es de lo que más espacio
            recupera de una vez.
          </p>
          {resultado ? (
            <p className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")} role={resultado.ok ? "status" : "alert"}>
              {resultado.mensaje}
            </p>
          ) : null}
          {confirmando ? (
            <div className="flex flex-wrap items-center gap-2" role="alert">
              <span className="text-fg-muted text-xs">
                Se borrarán DEFINITIVAMENTE los {num(estado.elementos)} elementos de la papelera
                ({bLegibles(estado.bytes, 1)}). No se puede deshacer.
              </span>
              <Boton variante="peligro" disabled={!!enCurso} onClick={() => void vaciar()}>
                Sí, vaciar
              </Boton>
              <Boton onClick={() => setConfirmando(false)}>No</Boton>
            </div>
          ) : (
            <div className="flex flex-wrap items-center gap-2">
              <Boton
                variante="peligro"
                disabled={!!enCurso || cargando || estado.elementos === 0}
                onClick={() => setConfirmando(true)}
              >
                <IconTrash size={13} aria-hidden="true" /> Vaciar la papelera
              </Boton>
              <Boton onClick={() => void cargar()} disabled={cargando || !!enCurso}>
                <IconRefresh size={13} aria-hidden="true" /> Volver a contar
              </Boton>
            </div>
          )}
        </>
      )}
    </Card>
  );
}

/* ── Bloque 4: la limpieza programada ────────────────────────────────────── */

/**
 * La limpieza que se hace sola, con dos caminos y los dos a la vista.
 *
 * 1. **Con la app abierta**: cada minuto comprueba si toca y, si toca, MIDE la
 *    basura y lo deja anotado en el registro. **NO borra nada.** Un borrado a las
 *    tres de la mañana que nadie ha mirado es justo lo que este programa no hace
 *    (Kudu lo arregló en su 3.5, después de que pasara).
 * 2. **Con la app cerrada**: se le entrega la tarea al planificador del SISTEMA
 *    (systemd, launchd o el Programador de tareas) llamando al CLI. Las recetas se
 *    enseñan **para copiar**, no se escriben solas: tocar el planificador del
 *    sistema es una decisión del usuario y se activa a mano.
 */
function BloqueProgramacion() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("limpieza"));
  const enCurso = useApp((st) => st.accionEnCurso);

  const [p, setP] = useState<Programacion | null>(null);
  const [categorias, setCategorias] = useState<CategoriaLimpieza[]>([]);
  const [recetas, setRecetas] = useState<RecetaProgramacion[]>([]);
  const [mensaje, setMensaje] = useState<string | null>(null);

  const cargar = useCallback(async () => {
    setCargando("limpieza", true);
    try {
      setP(await api.programar.leer());
      setRecetas(await api.programar.recetas());
      setCategorias(await api.limpieza.categorias());
      limpiarError("limpieza");
    } catch (e) {
      setError("limpieza", String(e));
      setP(null);
    } finally {
      setCargando("limpieza", false);
    }
  }, [setCargando, setError, limpiarError]);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const guardar = async () => {
    if (!p) return;
    try {
      setMensaje(await api.programar.guardar(p));
      setRecetas(await api.programar.recetas());
      limpiarError("limpieza");
    } catch (e) {
      setError("limpieza", String(e));
    }
  };

  if (p == null) {
    return (
      <Card>
        <Etiqueta>{cargando ? "Leyendo la limpieza programada…" : "Limpieza programada"}</Etiqueta>
      </Card>
    );
  }

  const alternarCategoria = (id: string) => {
    const marcadas = p.categorias.includes(id)
      ? p.categorias.filter((c) => c !== id)
      : [...p.categorias, id];
    setP({ ...p, categorias: marcadas });
  };

  return (
    <Card className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>Limpieza programada</Etiqueta>
        <Insignia tono={p.activa ? "ok" : "neutro"}>{p.activa ? "activada" : "desactivada"}</Insignia>
        {p.ultima ? <span className="text-fg-faint text-xs">última vez: {p.ultima}</span> : null}
      </div>
      <p className="text-fg-muted text-xs">
        Cuando toca, <strong className="text-fg">mide</strong> la basura y lo deja anotado en el
        registro de acciones. <strong className="text-fg">No borra nada</strong>: borrar es una
        decisión tuya, y se toma mirando lo que hay.
      </p>

      <div className="flex flex-wrap items-end gap-3">
        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={p.activa}
            onChange={(e) => setP({ ...p, activa: e.target.checked })}
          />
          Activar
        </label>
        <label className="flex flex-col gap-1 text-xs">
          <span className="text-fg-faint">Hora</span>
          <span className="flex items-center gap-1">
            <input
              type="number"
              min={0}
              max={23}
              className="mono bg-raised border-line w-16 rounded-md border px-2 py-1"
              value={p.hora}
              onChange={(e) => setP({ ...p, hora: Math.max(0, Math.min(23, Number(e.target.value) || 0)) })}
              aria-label="Hora de la limpieza programada"
            />
            <span className="text-fg-faint">:</span>
            <input
              type="number"
              min={0}
              max={59}
              className="mono bg-raised border-line w-16 rounded-md border px-2 py-1"
              value={p.minuto}
              onChange={(e) => setP({ ...p, minuto: Math.max(0, Math.min(59, Number(e.target.value) || 0)) })}
              aria-label="Minuto de la limpieza programada"
            />
          </span>
        </label>
        <Boton onClick={() => void guardar()} disabled={cargando || !!enCurso} variante="acento">
          Guardar
        </Boton>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <span className="text-fg-faint text-xs">Categorías:</span>
        <Boton
          variante={p.categorias.length === 0 ? "acento" : "normal"}
          aria-pressed={p.categorias.length === 0}
          onClick={() => setP({ ...p, categorias: [] })}
        >
          Todas
        </Boton>
        {categorias.map((c) => (
          <Boton
            key={c.id}
            variante={p.categorias.includes(c.id) ? "acento" : "normal"}
            aria-pressed={p.categorias.includes(c.id)}
            onClick={() => alternarCategoria(c.id)}
          >
            {c.nombre}
          </Boton>
        ))}
      </div>

      {mensaje ? (
        <p className="text-fg-muted text-xs" role="status">
          {mensaje}
        </p>
      ) : null}

      <details className="border-line-soft rounded-md border p-3">
        <summary className="cursor-pointer text-xs">
          Para que funcione con la app CERRADA: cómo ponerlo en el planificador de este sistema
        </summary>
        <div className="mt-3 flex flex-col gap-3">
          {recetas.map((r) => (
            <div key={r.titulo} className="flex flex-col gap-1">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-fg text-xs font-medium">{r.titulo}</span>
                <span className="mono text-fg-faint text-xs">{r.destino}</span>
                <BotonCopiar className="ml-auto" texto={r.contenido} que={`el contenido de ${r.titulo}`} />
              </div>
              <pre className="mono bg-raised border-line-soft max-h-56 overflow-auto rounded-md border p-2 text-[11px] whitespace-pre-wrap">
                {r.contenido}
              </pre>
              <p className="text-fg-faint text-xs whitespace-pre-wrap">{r.instrucciones}</p>
            </div>
          ))}
        </div>
      </details>
    </Card>
  );
}

export default function Optimizacion() {
  return (
    <div className="flex flex-col gap-4">
      <BloqueLimpieza />
      <Bases />
      <BloquePapelera />
      <BloqueProgramacion />
      <BloqueArranque />
    </div>
  );
}
