/**
 * Servidores: ver el estado real y actuar.
 *
 * Dos listas distintas y a propósito:
 *  - ARRIBA, el estado VIVO (de la foto: puerto, PID y los modelos con su estado).
 *  - ABAJO, los que están dados de alta en SQLite (los que se pueden arrancar o
 *    parar), con las acciones.
 * La salida de la acción en curso se muestra en el panel de la derecha, línea a
 * línea, porque un `update:run` puede tardar minutos.
 *
 * Cargar y descargar modelos SOLO se ofrece en las tarjetas de llama-swap: esas
 * acciones llaman a SU API (`/api/models/unload/...`), así que en cualquier otro
 * motor fallarían. No se enseña un botón que no puede funcionar.
 *
 * La versión del motor NO se enseña: `Server.version` es siempre nula porque el
 * backend no la publica, y una fila fija de "—" no informa de nada.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { clsx } from "clsx";
import {
  IconAlertCircle,
  IconAlertTriangle,
  IconArrowDownToArc,
  IconArrowUpFromArc,
  IconFileText,
  IconPlayerPlay,
  IconPlayerStop,
  IconRefresh,
  IconTerminal2,
} from "@tabler/icons-react";
import { useApp, errorDe, ejecutar, type ResultadoAccion } from "../store";
import { Boton, Card, Datos, Etiqueta, Insignia, Vacio } from "../components/ui";
import { api } from "../lib/tauri";
import { dur, estadoModelo, mb } from "../lib/format";

/** Las tres acciones que solo existen en llama-swap, para no repetir el tipo. */
type AccionModelo = "modelo:cargar" | "modelo:descargar" | "modelo:descargar-todos";

/* ── El log del motor ─────────────────────────────────────────────────────── */

type NivelLog = "error" | "aviso" | "info";

/**
 * El nivel de una línea del log.
 *
 * llama-swap prefija cada línea con su nivel (`[INFO]`, `[WARN]`, `[ERROR]`), así
 * que se lee de ahí y no se adivina por palabras del mensaje. Un nivel que no se
 * conozca (versión futura) se trata como `info`: no se pinta en rojo algo que no
 * se sabe si es un problema.
 */
function nivelDeLinea(linea: string): NivelLog {
  const m = /^\s*\[([A-Za-z]+)\]/.exec(linea);
  const nivel = m?.[1]?.toUpperCase() ?? "";
  if (nivel === "ERROR" || nivel === "ERR") return "error";
  if (nivel === "WARN" || nivel === "WARNING") return "aviso";
  return "info";
}

const TONO_LOG: Record<NivelLog, string> = {
  error: "text-bad",
  aviso: "text-warn",
  info: "",
};

/**
 * Visor del log del motor, BAJO DEMANDA.
 *
 * Por qué así: el log crece con cada petición que atiende llama-swap, así que
 * pedirlo en cada foto sería tirar ancho de banda para nada. Se pide al abrir el
 * panel y cuando se pulsa refrescar. El refresco automático cada 5 s es OPCIONAL
 * (viene apagado) y solo corre con el panel abierto, y se dice en la etiqueta.
 */
function PanelLog({ puerto, activo, nombre }: { puerto: number; activo: boolean; nombre: string }) {
  const id = `log-motor-${puerto}`;
  const [abierto, setAbierto] = useState(false);
  const [lineas, setLineas] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [leyendo, setLeyendo] = useState(false);
  const [auto, setAuto] = useState(false);
  /** Evita solapar dos peticiones si el refresco coincide con un clic. */
  const pidiendo = useRef(false);

  const leer = useCallback(async () => {
    if (pidiendo.current) return;
    pidiendo.current = true;
    setLeyendo(true);
    try {
      setLineas(await api.swapLogs(puerto, 300));
      setError(null);
    } catch (e) {
      // Un fallo de lectura NO es "el log está vacío": se enseña el motivo.
      setError(String(e));
    } finally {
      pidiendo.current = false;
      setLeyendo(false);
    }
  }, [puerto]);

  // Se pide la primera vez que se abre (y nunca más solo por abrir: para eso
  // está el botón de refrescar).
  useEffect(() => {
    if (abierto && lineas == null && error == null) void leer();
  }, [abierto, lineas, error, leer]);

  // Refresco automático: solo con el panel abierto, el servidor en marcha y la
  // casilla marcada. Al cerrar el panel, el intervalo se apaga con él.
  useEffect(() => {
    if (!abierto || !auto || !activo) return;
    const id = setInterval(() => void leer(), 5000);
    return () => clearInterval(id);
  }, [abierto, auto, activo, leer]);

  const cuenta = useMemo(() => {
    const l = lineas ?? [];
    return {
      error: l.filter((x) => nivelDeLinea(x) === "error").length,
      aviso: l.filter((x) => nivelDeLinea(x) === "aviso").length,
      total: l.length,
    };
  }, [lineas]);

  const motivoParado = activo ? undefined : "El servidor está parado: no hay log que pedirle.";

  return (
    <div className="border-line-soft mt-3 flex flex-col gap-2 border-t pt-3">
      <div className="flex flex-wrap items-center gap-2">
        <Boton
          aria-expanded={abierto}
          aria-controls={id}
          onClick={() => setAbierto((v) => !v)}
          aria-label={`${abierto ? "Ocultar" : "Ver"} el log del motor de ${nombre}`}
        >
          <IconFileText size={12} className="mr-1 inline" aria-hidden="true" />
          {abierto ? "Ocultar log del motor" : "Ver log del motor…"}
        </Boton>
        <span className="text-fg-faint text-xs">
          Últimas 300 líneas de llama-swap. Se piden al abrir y al refrescar: no van en bucle.
        </span>
      </div>

      {abierto ? (
        <div id={id} className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-3">
            <Boton
              disabled={leyendo || !activo}
              title={motivoParado}
              onClick={() => void leer()}
              aria-label={`Refrescar el log del motor de ${nombre}`}
            >
              <IconRefresh size={12} className="mr-1 inline" aria-hidden="true" />
              {leyendo ? "Leyendo…" : "Refrescar"}
            </Boton>
            <label className="text-fg-muted flex items-center gap-2 text-xs">
              <input
                type="checkbox"
                checked={auto}
                disabled={!activo}
                onChange={(e) => setAuto(e.target.checked)}
              />
              Refrescar solo cada 5 s
              <span
                className="text-fg-faint"
                title="Solo mientras este panel esté desplegado. Si lo cierras, deja de pedirse."
              >
                (?)
              </span>
            </label>
            {lineas && lineas.length > 0 ? (
              <span className="text-fg-faint ml-auto flex items-center gap-1.5 text-xs">
                {cuenta.total} líneas
                {cuenta.error > 0 ? <Insignia tono="bad">{cuenta.error} con error</Insignia> : null}
                {cuenta.aviso > 0 ? <Insignia tono="warn">{cuenta.aviso} de aviso</Insignia> : null}
              </span>
            ) : null}
          </div>

          {error ? (
            <Vacio titulo="No se pudo leer el log del motor">{error}</Vacio>
          ) : lineas == null ? (
            <p className="text-fg-muted text-xs">Leyendo el log…</p>
          ) : lineas.length === 0 ? (
            <p className="text-fg-muted text-xs">
              El motor ha devuelto el log vacío: no hay ninguna línea que enseñar.
            </p>
          ) : (
            <ul className="bg-bg mono max-h-64 overflow-auto rounded-md border border-line-soft p-2 text-[11px] leading-relaxed">
              {lineas.map((l, i) => {
                const nivel = nivelDeLinea(l);
                // Se enseña el nivel con texto (el `[ERROR]` que ya trae la línea)
                // Y con icono Y con color: nunca solo con color.
                const t = /^(\s*\[[A-Za-z]+\])([\s\S]*)$/.exec(l);
                return (
                  <li key={i} className={clsx("flex items-start gap-1.5", TONO_LOG[nivel])}>
                    {nivel === "error" ? (
                      <IconAlertTriangle size={12} className="mt-0.5 shrink-0" aria-hidden="true" />
                    ) : nivel === "aviso" ? (
                      <IconAlertCircle size={12} className="mt-0.5 shrink-0" aria-hidden="true" />
                    ) : null}
                    <span className="whitespace-pre-wrap break-all">
                      {t ? (
                        <>
                          <span className="font-semibold">{t[1]}</span>
                          {t[2]}
                        </>
                      ) : (
                        l
                      )}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}


export default function Servers() {
  const s = useApp((st) => st.snapshot);
  const filas = useApp((st) => st.servidores);
  const cargar = useApp((st) => st.cargarServidores);
  const linea = useApp((st) => st.lineaAccion);
  const enCurso = useApp((st) => st.accionEnCurso);
  // Dos lecturas distintas, dos errores distintos: el estado vivo sale de la
  // foto y la lista de abajo de `servers:list`. Antes se enseñaba el mismo texto
  // en las dos y cada sección se atribuía el fallo de la otra.
  const errorVivo = useApp(errorDe("foto"));
  const errorFilas = useApp(errorDe("servidores"));

  /**
   * Último resultado por botón (clave: `servidor:modelo` o `servidor:todos`).
   *
   * El evento `ai:action` es global y no dice de qué modelo era, así que el
   * mensaje se guarda junto al botón que lo lanzó: si no, "descargado" podría
   * aparecer bajo el modelo equivocado.
   */
  const [resultados, setResultados] = useState<Record<string, ResultadoAccion>>({});

  useEffect(() => { void cargar(); }, [cargar]);

  const lanzar = async (clave: string, kind: AccionModelo, args: Record<string, unknown>) => {
    const r = await ejecutar(kind, args);
    setResultados((prev) => ({ ...prev, [clave]: r }));
  };

  const vivos = s?.servers ?? [];

  return (
    <div className="flex flex-col gap-4">
      <section className="flex flex-col gap-3">
        <Etiqueta>Estado en vivo</Etiqueta>
        {!s ? (
          <Vacio titulo={errorVivo ? "No se pudo leer el estado en vivo" : "Cargando"}>
            {errorVivo}
          </Vacio>
        ) : vivos.length === 0 ? (
          <Card><p className="text-fg-muted text-sm">Ningún servidor configurado todavía.</p></Card>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {vivos.map((sv) => {
              // Solo llama-swap publica estos endpoints; los demás motores no.
              const esSwap = sv.kind === "llama-swap";
              const activo = sv.state === "active";
              const resTodos = resultados[`${sv.id}:todos`];
              // El motivo se repite en el `title` de los botones deshabilitados:
              // un botón apagado sin explicación no dice nada.
              const motivoParado = activo ? undefined : "El servidor está parado: no hay API a la que pedírselo.";
              return (
                <Card key={sv.id}>
                  <div className="flex items-center gap-2">
                    <Insignia tono={activo ? "ok" : "neutro"}>
                      {activo ? "activo" : "parado"}
                    </Insignia>
                    <span className="text-sm font-medium">{sv.name}</span>
                    <span className="mono text-fg-faint ml-auto text-xs">:{sv.port}</span>
                  </div>
                  <div className="mt-3">
                    <Datos
                      items={[
                        ["Tipo", sv.kind],
                        ["PID", sv.pid ?? "—"],
                        ["En marcha", dur(sv.proc_uptime_secs)],
                      ]}
                    />
                  </div>
                  {/* `error` solo se rellena con el proceso VIVO que no contesta: un servidor
                      parado es lo normal y no se pinta en rojo. */}
                  {sv.error ? (
                    <p className="text-bad mt-2 text-xs">
                      El proceso está en marcha pero no contesta en el puerto: {sv.error}
                    </p>
                  ) : null}

                  {sv.models.length > 0 ? (
                    <div className="mt-3 flex flex-col gap-2">
                      <Etiqueta>Modelos</Etiqueta>
                      {sv.models.map((m) => {
                        // El estado lo pone el motor. Con cadena vacía NO significa
                        // "descargado", significa que no lo dice, así que ahí no se
                        // pinta distintivo (lo traduce `estadoModelo`).
                        const estado = estadoModelo(m.state);
                        const clave = `${sv.id}:${m.id}`;
                        const res = resultados[clave];
                        return (
                          <div
                            key={m.id}
                            className="border-line-soft flex flex-col gap-1.5 border-t pt-1.5 first:border-t-0 first:pt-0"
                          >
                            <div className="flex items-center gap-2 text-xs">
                              {estado ? (
                                <Insignia
                                  tono={
                                    m.state === "loaded" ? "acento" : m.state === "loading" ? "warn" : "neutro"
                                  }
                                >
                                  {estado}
                                </Insignia>
                              ) : null}
                              <span className="truncate">{m.label}</span>
                              <span className="text-fg-faint mono ml-auto">
                                {m.quant ?? ""} {m.size_mb ? mb(m.size_mb, 1) : ""}
                              </span>
                            </div>

                            {/* Cargar/descargar van POR MODELO y solo en llama-swap.
                                Los `aria-label` EMPIEZAN por el texto visible
                                ("Cargar", "Descargar") a propósito: así quien use
                                control por voz puede decir lo que lee en pantalla
                                (WCAG 2.5.3) y a la vez sabe de qué modelo es. */}
                            {esSwap ? (
                              <div className="flex flex-wrap items-center gap-1.5">
                                <Boton
                                  variante="acento"
                                  disabled={!activo || !!enCurso}
                                  title={motivoParado}
                                  onClick={() =>
                                    void lanzar(clave, "modelo:cargar", { id: m.id, port: sv.port })
                                  }
                                  aria-label={`Cargar ${m.label} en la VRAM`}
                                >
                                  <IconArrowDownToArc size={12} className="mr-1 inline" aria-hidden="true" />
                                  Cargar
                                </Boton>
                                <Boton
                                  disabled={!activo || !!enCurso}
                                  title={motivoParado}
                                  onClick={() =>
                                    void lanzar(clave, "modelo:descargar", { id: m.id, port: sv.port })
                                  }
                                  aria-label={`Descargar ${m.label} de la VRAM`}
                                >
                                  <IconArrowUpFromArc size={12} className="mr-1 inline" aria-hidden="true" />
                                  Descargar
                                </Boton>
                                {res ? (
                                  <span
                                    className={clsx("text-xs", res.ok ? "text-fg-muted" : "text-bad")}
                                    role={res.ok ? undefined : "alert"}
                                  >
                                    {res.mensaje}
                                  </span>
                                ) : null}
                              </div>
                            ) : null}
                          </div>
                        );
                      })}

                      {esSwap ? (
                        <p className="text-fg-faint text-xs">
                          <strong className="text-fg-muted">Cargar</strong> trae el modelo a la VRAM y{" "}
                          <strong className="text-fg-muted">tarda</strong> (llama-swap no tiene un «cargar»: carga
                          bajo demanda, así que se le manda una petición mínima y él trae el modelo entero).{" "}
                          <strong className="text-fg-muted">Descargar</strong> lo saca de la VRAM sin parar el
                          servidor.
                        </p>
                      ) : null}
                    </div>
                  ) : null}

                  {/* Liberar la VRAM va en la TARJETA DEL SERVIDOR, no por modelo:
                      es una acción de todo el servidor y su mensaje dice qué se
                      ha liberado (o que no había nada). */}
                  {esSwap ? (
                    <div className="border-line-soft mt-3 flex flex-col gap-2 border-t pt-3">
                      <div className="flex flex-wrap items-center gap-2">
                        <Boton
                          disabled={!activo || !!enCurso}
                          title={motivoParado}
                          onClick={() =>
                            void lanzar(`${sv.id}:todos`, "modelo:descargar-todos", { port: sv.port })
                          }
                          aria-label={`Liberar VRAM (todos) de ${sv.name}`}
                        >
                          <IconArrowUpFromArc size={12} className="mr-1 inline" aria-hidden="true" />
                          Liberar VRAM (todos)
                        </Boton>
                        <span className="text-fg-faint text-xs">
                          Saca de la VRAM todos los modelos cargados de golpe, sin parar el servidor.
                        </span>
                      </div>
                      {resTodos ? (
                        <p
                          className={clsx("text-xs", resTodos.ok ? "text-fg-muted" : "text-bad")}
                          role={resTodos.ok ? undefined : "alert"}
                        >
                          {resTodos.mensaje}
                        </p>
                      ) : null}
                    </div>
                  ) : null}

                  {/* El log del motor: bajo demanda (panel desplegable), con su
                      botón de refrescar. Solo para llama-swap, que es el único
                      que publica `GET /logs`. */}
                  {esSwap ? <PanelLog puerto={sv.port} activo={activo} nombre={sv.name} /> : null}
                </Card>
              );
            })}
          </div>
        )}
      </section>

      <section className="flex flex-col gap-3">
        <Etiqueta>Configurados · acciones</Etiqueta>

        {/* La lista vacía NO siempre es "no hay servidores dados de alta": si la
            lectura falló, la tabla puede estar llena y no saberlo. Se distinguen. */}
        {filas.length === 0 && errorFilas ? (
          <Vacio titulo="No se pudo leer la lista de servidores">{errorFilas}</Vacio>
        ) : (
          <>
            {filas.length > 0 && errorFilas ? (
              <Card className="border-bad/40">
                <p className="text-bad text-sm">
                  Falló la última lectura de la lista; esto es lo último que se pudo leer.
                </p>
                <p className="text-bad mt-1 text-sm" role="alert">{errorFilas}</p>
              </Card>
            ) : null}

            <Card>
              {filas.length === 0 ? (
                <p className="text-fg-muted text-sm">
                  La tabla <code className="mono">servers</code> está vacía. Añade uno en Ajustes.
                </p>
              ) : (
                <ul className="flex flex-col gap-1">
                  {filas.map((r) => (
                    <li key={r.id} className="row">
                      <Insignia tono={r.enabled ? "ok" : "neutro"}>
                        {r.enabled ? "habilitado" : "deshabilitado"}
                      </Insignia>
                      <span className="text-sm">{r.name}</span>
                      <span className="text-fg-faint mono text-xs">:{r.port}</span>
                      <span className="ml-auto flex gap-1.5">
                        <Boton
                          variante="acento"
                          disabled={!!enCurso}
                          onClick={() => void ejecutar("server:start", { id: r.id })}
                        >
                          <IconPlayerPlay size={12} className="mr-1 inline" aria-hidden="true" />
                          Arrancar
                        </Boton>
                        <Boton
                          disabled={!!enCurso}
                          onClick={() => void ejecutar("server:stop", { id: r.id })}
                        >
                          <IconPlayerStop size={12} className="mr-1 inline" aria-hidden="true" />
                          Parar
                        </Boton>
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </Card>
          </>
        )}
      </section>

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
    </div>
  );
}
