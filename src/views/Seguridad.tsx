/**
 * Seguridad: dos preguntas que no se mezclan, y ninguna se contesta con un color.
 *
 * 1. **Qué se ejecuta sin que lo veas.** Los sitios donde vive la persistencia de
 *    verdad en un equipo de escritorio: el gancho de bibliotecas, el cron, los
 *    servicios de usuario, lo que arranca con la sesión, los ficheros de arranque
 *    del shell y las llaves SSH que entran sin contraseña. NO es un antivirus y la
 *    propia pantalla lo dice: no mira dentro de los binarios, no tiene firmas y no
 *    baja reglas de internet. A cambio, CADA hallazgo lleva su PRUEBA (la línea, el
 *    permiso, la ruta): sin eso, un aviso de seguridad no se puede comprobar y
 *    acaba siendo ruido que nadie mira.
 *
 * 2. **Qué huellas dejas.** Historiales, recientes y portapapeles: cosas que no se
 *    regeneran solas. Por eso NO hay «marcar todo» y el borrado va en dos pasos con
 *    el aviso de que no se recuperan — van a la basura del sistema las cachés, no
 *    esto. La marca `traza` del backend es la misma que usa el CLI para no
 *    llevárselas por delante con un `--aplicar` a secas.
 *
 * Lo que NO se enseña aquí, a propósito: un semáforo verde grande. Un «todo bien»
 * que no dice QUÉ se ha mirado es peor que no decir nada.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { IconAlertTriangle, IconRefresh, IconShieldCheck, IconTrash } from "@tabler/icons-react";
import { clsx } from "clsx";
import { cargandoDe, errorDe, ejecutar, useApp, type ResultadoAccion } from "../store";
import { api, type EscaneoLimpieza, type HallazgoSeguridad, type RevisionSeguridad, type VeredictoSeguridad } from "../lib/tauri";
import { Boton, Card, Etiqueta, Insignia, Th, Vacio } from "../components/ui";
import type { Tono } from "../components/ui";
import { bLegibles, num } from "../lib/format";

/** Cómo se enseña cada veredicto. `desconocido` NO se pinta como `ok`. */
const VEREDICTOS: Record<VeredictoSeguridad, { texto: string; tono: Tono }> = {
  ok: { texto: "bien", tono: "ok" },
  aviso: { texto: "aviso", tono: "warn" },
  problema: { texto: "problema", tono: "bad" },
  desconocido: { texto: "sin comprobar", tono: "neutro" },
};

/** El orden en que se miran: primero lo que está mal, y lo comprobado antes que lo que no. */
const GRAVEDAD: Record<VeredictoSeguridad, number> = {
  problema: 3,
  aviso: 2,
  desconocido: 1,
  ok: 0,
};

/* ── Bloque 1: qué se ejecuta solo ────────────────────────────────────────── */

function BloqueIndicadores() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("seguridad"));
  const error = useApp(errorDe("seguridad"));

  const [rev, setRev] = useState<RevisionSeguridad | null>(null);

  const revisar = useCallback(async () => {
    setCargando("seguridad", true);
    try {
      setRev(await api.seguridad.revisar());
      limpiarError("seguridad");
    } catch (e) {
      setError("seguridad", String(e));
      setRev(null);
    } finally {
      setCargando("seguridad", false);
    }
  }, [setCargando, setError, limpiarError]);

  useEffect(() => {
    void revisar();
  }, [revisar]);

  // Lo que está mal, primero. El orden del backend es el de la comprobación (que
  // es el que tiene sentido al leerlo entero), pero al mirar por encima lo que
  // importa es qué ha salido mal.
  const hallazgos = useMemo(() => {
    const h = rev?.hallazgos ?? [];
    return [...h].sort((a, b) => GRAVEDAD[b.veredicto] - GRAVEDAD[a.veredicto]);
  }, [rev]);

  const cuantos = useMemo(() => {
    const c: Record<VeredictoSeguridad, number> = { problema: 0, aviso: 0, desconocido: 0, ok: 0 };
    for (const h of rev?.hallazgos ?? []) c[h.veredicto] += 1;
    return c;
  }, [rev]);

  return (
    <Card className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>Qué se ejecuta sin que lo veas</Etiqueta>
        <Boton className="ml-auto" onClick={() => void revisar()} disabled={cargando}>
          <IconRefresh size={13} aria-hidden="true" /> Volver a comprobar
        </Boton>
      </div>

      {/* El alcance, dicho en la propia pantalla. Si esto no estuviera, alguien
          podría creer que un «bien» significa que el equipo está limpio. */}
      <p className="text-fg-faint text-xs">
        Mira los sitios donde vive la persistencia (gancho de bibliotecas, cron, servicios de
        usuario, arranque de la sesión, arranque del shell y llaves SSH). No es un antivirus: no
        mira dentro de los binarios ni usa firmas, y no baja reglas de internet. Todo es local.
      </p>

      {error ? (
        <Vacio titulo="No se pudo comprobar">{error}</Vacio>
      ) : cargando && !rev ? (
        <Vacio titulo="Comprobando…" />
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            {rev && rev.resumen !== "ok" ? (
              <Insignia tono={VEREDICTOS[rev.resumen].tono}>
                <IconAlertTriangle size={12} aria-hidden="true" className="mr-1" />
                {rev.resumen === "problema"
                  ? "hay algo que no lo pone nadie sin querer"
                  : VEREDICTOS[rev.resumen].texto}
              </Insignia>
            ) : (
              <Insignia tono="ok">
                <IconShieldCheck size={12} aria-hidden="true" className="mr-1" />
                nada raro en lo que se mira
              </Insignia>
            )}
            <span className="text-fg-faint text-xs">
              {cuantos.problema} problema(s), {cuantos.aviso} aviso(s), {cuantos.desconocido} sin
              comprobar
            </span>
          </div>

          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-xs">
              <caption className="sr-only">
                Comprobaciones de seguridad, lo que se ha encontrado y qué hacer
              </caption>
              <thead>
                <tr className="border-line-soft border-b">
                  <Th className="w-28">Estado</Th>
                  <Th>Qué se ha mirado</Th>
                  <Th>Qué se ha encontrado</Th>
                  <Th>Qué hacer</Th>
                </tr>
              </thead>
              <tbody>
                {hallazgos.map((h) => (
                  <FilaHallazgo key={h.id} h={h} />
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </Card>
  );
}

function FilaHallazgo({ h }: { h: HallazgoSeguridad }) {
  const v = VEREDICTOS[h.veredicto];
  const setVista = useApp((st) => st.setVista);
  // Si el remedio de un hallazgo está EN otra sección, se salta a ella en vez de
  // repetir allí lo que ya hay: la comprobación del arranque es un aviso sobre la
  // lista que Optimización ya enseña, con sus interruptores.
  const salto = h.id === "autostart" ? { texto: "Ver Optimización", vista: "optimizacion" as const } : null;
  return (
    <tr className="border-line-soft hover:bg-raised align-top border-b last:border-0">
      <td className="px-4 py-1.5">
        <Insignia tono={v.tono}>{v.texto}</Insignia>
      </td>
      <td className="px-4 py-1.5">
        <div className="text-fg">{h.titulo}</div>
        {/* La FUENTE siempre visible: es la mitad de la prueba. Sin saber de dónde
            sale un hallazgo, no se puede ni comprobar ni descartar. */}
        <div className="mono text-fg-faint mt-0.5 break-all">{h.fuente}</div>
      </td>
      <td className="text-fg-muted max-w-[560px] px-4 py-1.5">{h.detalle}</td>
      <td className="text-fg-muted max-w-[380px] px-4 py-1.5">
        {h.remedio ?? "—"}
        {salto ? (
          <Boton className="mt-1.5 block" onClick={() => setVista(salto.vista)}>
            {salto.texto}
          </Boton>
        ) : null}
      </td>
    </tr>
  );
}

/* ── Bloque 2: huellas de tu actividad ────────────────────────────────────── */

function BloqueHuellas() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("limpieza"));
  const error = useApp(errorDe("limpieza"));
  const enCurso = useApp((st) => st.accionEnCurso);
  const ui = useApp((st) => st.ui.seguridad);
  const setUi = useApp((st) => st.setUi);

  const [escaneo, setEscaneo] = useState<EscaneoLimpieza | null>(null);
  const [confirmando, setConfirmando] = useState(false);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  const escanear = useCallback(async () => {
    setCargando("limpieza", true);
    setConfirmando(false);
    try {
      setEscaneo(await api.limpieza.escanear(["privacidad"]));
      limpiarError("limpieza");
    } catch (e) {
      setError("limpieza", String(e));
      setEscaneo(null);
    } finally {
      setCargando("limpieza", false);
    }
  }, [setCargando, setError, limpiarError]);

  useEffect(() => {
    void escanear();
  }, [escanear]);

  // Solo se pueden borrar las que ocupa algo: marcar una de 0 bytes y que el
  // backend la ignore dejaría el recuento de la confirmación mintiendo.
  const lista = useMemo(() => (escaneo?.objetivos ?? []).filter((o) => o.bytes > 0), [escaneo]);
  const sel = useMemo(
    () => new Set(lista.filter((o) => ui.seleccion.includes(o.id)).map((o) => o.id)),
    [lista, ui.seleccion],
  );
  const bytesSel = useMemo(
    () => lista.filter((o) => sel.has(o.id)).reduce((a, o) => a + o.bytes, 0),
    [lista, sel],
  );
  const bytesLista = useMemo(() => lista.reduce((a, o) => a + o.bytes, 0), [lista]);

  const borrar = async () => {
    const r = await ejecutar("limpieza:limpiar", { ids: [...sel] });
    setResultado(r);
    setConfirmando(false);
    if (r.ok) {
      setUi("seguridad", { seleccion: [] });
      await escanear();
    }
  };

  return (
    <Card className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>Huellas de tu actividad</Etiqueta>
        <Boton className="ml-auto" onClick={() => void escanear()} disabled={cargando || !!enCurso}>
          <IconRefresh size={13} aria-hidden="true" /> Volver a escanear
        </Boton>
      </div>

      <p className="text-warn flex items-start gap-1.5 text-xs" role="note">
        <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
        Esto no son cachés: son historiales, recientes y portapapeles. Se borran de verdad (no van a
        la papelera) y NO se pueden recuperar. Por eso no hay «marcar todo»: cada una se pide por su
        nombre.
      </p>

      {error ? (
        <Vacio titulo="No se pudieron medir las huellas">{error}</Vacio>
      ) : escaneo && lista.length === 0 ? (
        <Vacio titulo="No hay huellas de las que se miran aquí">
          Historiales de shell, documentos recientes, portapapeles y registros de actividad: en este
          equipo no hay ninguno con contenido.
        </Vacio>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-xs">
            <caption className="sr-only">Huellas de actividad que se pueden borrar</caption>
            <thead>
              <tr className="border-line-soft border-b">
                <Th className="w-8">
                  <span className="sr-only">Selección</span>
                </Th>
                <Th>Qué</Th>
                <Th alineado="der">Ocupa</Th>
                <Th alineado="der">Elementos</Th>
                <Th className="w-24">Antigüedad</Th>
              </tr>
            </thead>
            <tbody>
              {lista.map((o) => (
                <tr key={o.id} className="border-line-soft hover:bg-raised align-top border-b last:border-0">
                  <td className="px-4 py-1.5">
                    <input
                      type="checkbox"
                      checked={sel.has(o.id)}
                      onChange={(e) => {
                        const s = new Set(ui.seleccion);
                        if (e.target.checked) s.add(o.id);
                        else s.delete(o.id);
                        setUi("seguridad", { seleccion: [...s] });
                      }}
                      aria-label={`Seleccionar ${o.subcategoria}`}
                    />
                  </td>
                  <td className="max-w-[520px] px-4 py-1.5">
                    <div className="text-fg">{o.subcategoria}</div>
                    <div className="text-fg-faint">{o.descripcion}</div>
                    <div className="mono text-fg-faint mt-0.5 truncate" title={o.rutas.join("\n")}>
                      {o.rutas[0]}
                      {o.rutas.length > 1 ? ` (+${o.rutas.length - 1})` : ""}
                    </div>
                  </td>
                  <td className="mono px-4 py-1.5 text-right whitespace-nowrap">{bLegibles(o.bytes, 1)}</td>
                  <td className="mono text-fg-muted px-4 py-1.5 text-right">{num(o.elementos)}</td>
                  <td className="text-fg-faint px-4 py-1.5 whitespace-nowrap">
                    {o.min_dias === 0 ? "cualquiera" : `más de ${num(o.min_dias)} días`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {resultado ? (
        <p
          className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")}
          role={resultado.ok ? "status" : "alert"}
        >
          {resultado.mensaje}
        </p>
      ) : null}

      <div className="border-line-soft border-t pt-3">
        {confirmando ? (
          <div className="flex flex-wrap items-center gap-2" role="alert">
            <span className="text-fg-muted text-xs">
              Se borrarán DEFINITIVAMENTE {num(sel.size)} huella(s) ({bLegibles(bytesSel, 1)}). No se
              pueden recuperar: no van a la papelera.
            </span>
            <Boton variante="peligro" disabled={!!enCurso} onClick={() => void borrar()}>
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
              <IconTrash size={13} aria-hidden="true" /> Borrar las marcadas
            </Boton>
            <span className="text-fg-faint text-xs">{bLegibles(bytesLista, 1)} en total</span>
          </div>
        )}
      </div>
    </Card>
  );
}

export default function Seguridad() {
  return (
    <div className="flex flex-col gap-4">
      <BloqueIndicadores />
      <BloqueHuellas />
    </div>
  );
}
