/**
 * Uso: qué se ha servido, cuántos tokens y a qué velocidad.
 *
 * DE DÓNDE SALEN ESTOS NÚMEROS, porque es lo que los hace creíbles: la puerta de
 * enlace de Machinograph (`gateway.rs`) se pone delante del motor, reenvía las
 * peticiones TAL CUAL y apunta lo que el propio motor publica de cada respuesta
 * (`usage` en formato OpenAI o Anthropic, `timings` en llama.cpp). No se estima
 * nada: lo que no viene queda en «—».
 *
 * Y por eso la vista empieza por si la puerta está encendida: sin ella no hay
 * nada que medir, y decir "0 tokens" sería mentira —lo que pasa es que nadie ha
 * pasado por aquí—. Ese es el motivo de que el primer bloque sea la puerta y no
 * una gráfica.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { IconPlugConnected, IconRefresh } from "@tabler/icons-react";
import { clsx } from "clsx";
import { api, type GatewayEstado, type UsoRespuesta } from "../lib/tauri";
import { Boton, Card, Datos, Etiqueta, Insignia, Kpi, Vacio, useReloj } from "../components/ui";
import { hace, num } from "../lib/format";

/**
 * La gráfica de actividad: una columna por día, la más alta relativa al máximo.
 *
 * La altura se calcula en PÍXELES y no en porcentaje, y eso es una corrección
 * medida en la app real: con `height: X%` dentro de una columna de altura
 * automática el porcentaje no tiene contra qué resolverse, y las barras salían
 * invisibles con los números flotando en el aire. El alto del área es una
 * constante (72px), así que el reparto se hace antes de pintar.
 *
 * SVG no: son 14 rectángulos y una etiqueta cada uno. Meter una librería de
 * gráficas para esto no se sostiene.
 */
const ALTO_GRAFICA = 72;

function Actividad({ dias }: { dias: UsoRespuesta["diario"] }) {
  const max = Math.max(1, ...dias.map((d) => d.peticiones));
  if (dias.length === 0) {
    return (
      <p className="text-fg-muted text-sm">
        Todavía no hay actividad registrada en los últimos 14 días.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      <div
        className="flex items-end gap-1"
        style={{ height: ALTO_GRAFICA }}
        role="img"
        aria-label={`Peticiones por día de los últimos ${dias.length} días. El máximo es ${max}.`}
      >
        {dias.map((d) => {
          const alto = d.peticiones > 0 ? Math.max(3, Math.round((d.peticiones / max) * ALTO_GRAFICA)) : 2;
          return (
            <div key={d.dia} className="flex min-w-0 flex-1 items-end" style={{ height: ALTO_GRAFICA, maxWidth: 40 }}>
              <div
                className={clsx("w-full rounded-sm", d.peticiones > 0 ? "bg-accent" : "bg-raised")}
                style={{ height: alto }}
                title={`${d.dia}: ${d.peticiones} peticiones · ${num(d.prompt_tokens)} de entrada, ${num(d.completion_tokens)} de salida`}
              />
            </div>
          );
        })}
      </div>
      <div className="flex gap-1">
        {dias.map((d) => (
          <span key={d.dia} className="text-fg-faint mono min-w-0 flex-1 text-center text-[10px]" style={{ maxWidth: 40 }}>
            {d.dia.slice(8, 10)}
          </span>
        ))}
      </div>
      <p className="text-fg-faint text-xs">
        Peticiones por día (día local).{" "}
        {dias.length === 1
          ? // Con un solo día, una columna de ancho completo parecía una barra de
            // progreso y no una gráfica: se dice que aún no hay serie que mirar.
            `Todavía hay un solo día con actividad (${dias[0].dia}); el máximo es ${num(max)}.`
          : `${dias.length} días. El máximo del periodo es ${num(max)}.`}
      </p>
    </div>
  );
}

/**
 * El bloque de la puerta de enlace: encenderla, su dirección y su clave.
 *
 * Va PRIMERO en la vista y no al final porque sin ella esta pantalla no tiene
 * datos: es la que cuenta. Si está apagada se dice qué hacer, en vez de enseñar
 * tres ceros que se leerían como "no se ha usado nada".
 */
function PuertaDeEnlace({
  estado,
  onCambio,
}: {
  estado: GatewayEstado;
  onCambio: () => void;
}) {
  const [ocupado, setOcupado] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [puerto, setPuerto] = useState(String(estado.puerto));
  const [destino, setDestino] = useState(estado.destino);

  useEffect(() => {
    setPuerto(String(estado.puerto));
    setDestino(estado.destino);
  }, [estado.puerto, estado.destino]);

  const lanzar = async (nombre: string, fn: () => Promise<string>) => {
    setOcupado(nombre);
    setError(null);
    try {
      setAviso(await fn());
      onCambio();
    } catch (e) {
      setError(String(e));
    } finally {
      setOcupado(null);
    }
  };

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconPlugConnected size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Puerta de enlace</Etiqueta>
        <Insignia tono={estado.activa ? "ok" : "neutro"}>{estado.activa ? "encendida" : "apagada"}</Insignia>
        {estado.error ? <Insignia tono="bad">no arrancó</Insignia> : null}
        <span className="text-fg-faint ml-auto text-xs">
          es la que cuenta el uso: sin ella, esta pantalla no tiene datos
        </span>
      </div>

      {/* El error de arranque, con su motivo literal: el caso típico es el puerto
          ocupado por otro programa, y sin decirlo parecería que la puerta va. */}
      {estado.error ? (
        <p className="text-bad mb-2 text-xs" role="alert">
          La puerta no pudo arrancar: {estado.error}
        </p>
      ) : null}

      {/* El puerto configurado estaba ocupado y la puerta arrancó en el siguiente:
          se dice AQUÍ, junto a la URL, porque es la dirección que tienen que usar
          los clientes. Enseñar la configurada los mandaría a donde no hay nada. */}
      {estado.aviso_puerto ? (
        <p className="text-warn mb-2 text-xs" role="status">
          {estado.aviso_puerto}
        </p>
      ) : null}

      <Datos
        items={[
          [
            "Escucha en",
            <span key="e" className="flex items-center gap-2">
              <span className="mono">
                {estado.direccion}:{estado.puerto_escuchando ?? estado.puerto}
              </span>
              {estado.activa ? <Insignia tono="acento">{estado.url}</Insignia> : null}
            </span>,
          ],
          ["Reenvía a", <span key="d" className="mono">{estado.destino}</span>],
          [
            "Clave",
            <span key="c" className="flex items-center gap-2">
              <span className="mono">{estado.requiere_clave ? estado.clave || "(sin generar)" : "no se pide"}</span>
              {estado.requiere_clave ? (
                <Boton
                  disabled={ocupado != null}
                  onClick={() => void lanzar("clave", () => api.gateway.regenerarClave())}
                >
                  Regenerar
                </Boton>
              ) : null}
            </span>,
          ],
        ]}
      />

      <p className="text-fg-muted mt-3 text-xs">
        Ponla delante de tu motor y apunta los clientes a ella en vez de al motor:{" "}
        <span className="mono">{estado.url}</span>. Reenvía las peticiones tal cual y devuelve la
        respuesta del motor byte a byte; lo único que hace de más es contar tokens, medir tiempos y
        exigir la clave si se le pide.
      </p>

      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-xs">
          <span className="text-fg-faint">Puerto</span>
          <input
            value={puerto}
            onChange={(e) => setPuerto(e.target.value)}
            inputMode="numeric"
            aria-label="Puerto de la puerta de enlace"
            className="border-line bg-raised mono w-24 rounded-md border px-2 py-1 text-xs"
          />
        </label>
        <label className="flex min-w-[240px] flex-1 flex-col gap-1 text-xs">
          <span className="text-fg-faint">Motor al que reenvía</span>
          <input
            value={destino}
            onChange={(e) => setDestino(e.target.value)}
            spellCheck={false}
            aria-label="Motor al que reenvía la puerta"
            className="border-line bg-raised mono rounded-md border px-2 py-1 text-xs"
          />
        </label>
        <Boton
          disabled={ocupado != null || (puerto === String(estado.puerto) && destino === estado.destino)}
          onClick={() =>
            void lanzar("guardar", () =>
              api.gateway.configurar({ puerto: Number(puerto), destino }),
            )
          }
        >
          Guardar destino
        </Boton>
        <Boton
          variante={estado.activa ? "normal" : "acento"}
          disabled={ocupado != null}
          onClick={() => void lanzar("estado", () => api.gateway.configurar({ activa: !estado.activa }))}
        >
          {estado.activa ? "Apagar" : "Encender"}
        </Boton>
        <Boton
          disabled={ocupado != null}
          onClick={() =>
            void lanzar("clave_req", () => api.gateway.configurar({ requiere_clave: !estado.requiere_clave }))
          }
          title="Exigir la clave a quien llame. Necesario si escuchas en 0.0.0.0."
        >
          {estado.requiere_clave ? "Dejar de exigir clave" : "Exigir clave"}
        </Boton>
      </div>

      {aviso ? <p className="text-fg-muted mt-2 text-xs">{aviso}</p> : null}
      {error ? (
        <p className="text-bad mt-2 text-xs" role="alert">
          {error}
        </p>
      ) : null}
      <p className="text-fg-faint mt-2 text-xs">
        Los cambios se aplican al reiniciar la app (la puerta se abre al arrancar).
      </p>
    </Card>
  );
}

export default function Uso() {
  const [periodo, setPeriodo] = useState<"hoy" | "todo">("hoy");
  const [modelo, setModelo] = useState("");
  const [datos, setDatos] = useState<UsoRespuesta | null>(null);
  const [cargando, setCargando] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const ahora = useReloj(30_000);

  const cargar = useCallback(async () => {
    setCargando(true);
    try {
      setDatos(await api.uso(periodo, modelo));
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setCargando(false);
    }
  }, [periodo, modelo]);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const r = datos?.resumen;
  const modelos = datos?.por_modelo ?? [];

  // Cuántas peticiones del periodo NO publicaron tokens: si hay alguna, los
  // totales suman solo de las demás y hay que decirlo, o el número engaña.
  const sinTokens = useMemo(
    () => (r ? Math.max(0, r.peticiones - r.con_tokens) : 0),
    [r],
  );

  return (
    <div className="flex flex-col gap-4">
      {datos ? <PuertaDeEnlace estado={datos.config} onCambio={() => void cargar()} /> : null}

      {/* Filtros del periodo y del modelo */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="border-line flex rounded-md border p-0.5" role="group" aria-label="Periodo">
          {(["hoy", "todo"] as const).map((p) => (
            <Boton
              key={p}
              variante={periodo === p ? "acento" : "normal"}
              aria-pressed={periodo === p}
              onClick={() => setPeriodo(p)}
              className="border-0"
            >
              {p === "hoy" ? "Hoy" : `Todo (${datos?.retencion_dias ?? 90} días)`}
            </Boton>
          ))}
        </div>
        <label className="text-fg-muted flex items-center gap-2 text-xs">
          Modelo
          <select
            value={modelo}
            onChange={(e) => setModelo(e.target.value)}
            aria-label="Filtrar por modelo"
            className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
          >
            <option value="">todos</option>
            {modelos.map((m) => (
              <option key={m.modelo} value={m.modelo}>
                {m.modelo} ({m.peticiones})
              </option>
            ))}
          </select>
        </label>
        <Boton className="ml-auto" disabled={cargando} onClick={() => void cargar()}>
          <IconRefresh size={13} className="mr-1 inline" aria-hidden="true" />
          {cargando ? "Leyendo…" : "Refrescar"}
        </Boton>
      </div>

      {error ? (
        <Card className="border-bad/40">
          <p className="text-bad text-sm" role="alert">
            No se pudo leer el uso: {error}
          </p>
        </Card>
      ) : null}

      {!datos && !error ? <Vacio titulo="Leyendo el uso…" /> : null}

      {datos && r ? (
        <>
          {/* ── Las cifras del periodo ─────────────────────────────────── */}
          <section aria-label="Cifras del periodo" className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-6">
            <Kpi
              etiqueta="Peticiones"
              valor={num(r.peticiones)}
              pie={sinTokens > 0 ? `${sinTokens} sin datos del motor` : "todas con datos"}
            />
            <Kpi
              etiqueta="Tokens de entrada"
              valor={r.con_tokens > 0 ? num(r.prompt_tokens) : "—"}
              pie="prompt, incluida la parte de caché"
            />
            <Kpi
              etiqueta="Entrada en caché"
              valor={r.con_tokens > 0 ? num(r.cached_tokens) : "—"}
              uso={r.prompt_tokens > 0 ? (r.cached_tokens / r.prompt_tokens) * 100 : 0}
              pie={
                r.prompt_tokens > 0
                  ? `${num((r.cached_tokens / r.prompt_tokens) * 100, 1)} % de la entrada`
                  : "sin entrada que medir"
              }
            />
            <Kpi
              etiqueta="Tokens de salida"
              valor={r.con_tokens > 0 ? num(r.completion_tokens) : "—"}
              pie="lo que se generó"
            />
            {/* Las dos cifras de velocidad van juntas y separadas del resto: son
                las que dicen si la máquina está sana. */}
            <Kpi
              etiqueta="Velocidad"
              valor={r.tok_s != null ? num(r.tok_s, 1) : "—"}
              unidad={r.tok_s != null ? "tok/s" : undefined}
              pie={r.tok_s != null ? "generación: tokens / tiempo de generar" : "el motor no publica tiempos"}
            />
            <Kpi
              etiqueta="Primer token"
              valor={r.ttft_medio_ms != null ? num(r.ttft_medio_ms, 0) : "—"}
              unidad={r.ttft_medio_ms != null ? "ms" : undefined}
              pie={
                r.con_ttft > 0 && r.con_ttft < r.peticiones
                  ? `media de ${r.con_ttft} de ${r.peticiones}`
                  : "media del periodo"
              }
            />
          </section>

          {r.peticiones === 0 ? (
            <Card>
              <p className="text-fg-muted text-sm">
                No hay ninguna petición registrada en este periodo. Si acabas de encender la puerta,
                apunta los clientes a <span className="mono">{datos.config.url}</span> en vez de al
                motor y lo que pase por ahí aparecerá aquí.
              </p>
            </Card>
          ) : null}

          {/* ── Actividad ──────────────────────────────────────────────── */}
          <Card>
            <Etiqueta>Actividad de los últimos 14 días</Etiqueta>
            <div className="mt-3">
              <Actividad dias={datos.diario} />
            </div>
          </Card>

          <div className="grid gap-3 lg:grid-cols-2">
            {/* ── Por modelo ───────────────────────────────────────────── */}
            <Card className="p-0">
              <div className="border-line-soft border-b px-4 py-3">
                <Etiqueta>Por modelo</Etiqueta>
              </div>
              {modelos.length === 0 ? (
                <p className="text-fg-muted p-4 text-sm">Ningún modelo con uso en este periodo.</p>
              ) : (
                <table className="w-full text-left text-xs">
                  <thead className="text-fg-faint border-line-soft border-b">
                    <tr>
                      <th scope="col" className="px-4 py-2 font-medium">Modelo</th>
                      <th scope="col" className="px-4 py-2 text-right font-medium">Peticiones</th>
                      <th scope="col" className="px-4 py-2 text-right font-medium">Entrada</th>
                      <th scope="col" className="px-4 py-2 text-right font-medium">Salida</th>
                    </tr>
                  </thead>
                  <tbody>
                    {modelos.map((m) => (
                      <tr key={m.modelo} className="border-line-soft border-b last:border-0">
                        <td className="mono truncate px-4 py-1.5" title={m.modelo}>
                          {m.modelo}
                        </td>
                        <td className="mono px-4 py-1.5 text-right">{num(m.peticiones)}</td>
                        <td className="mono px-4 py-1.5 text-right">{num(m.prompt_tokens)}</td>
                        <td className="mono px-4 py-1.5 text-right">{num(m.completion_tokens)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </Card>

            {/* ── Últimas peticiones ───────────────────────────────────── */}
            <Card className="p-0">
              <div className="border-line-soft flex items-center gap-2 border-b px-4 py-3">
                <Etiqueta>Últimas peticiones</Etiqueta>
                <span className="text-fg-faint ml-auto text-xs">{datos.recientes.length}</span>
              </div>
              {datos.recientes.length === 0 ? (
                <p className="text-fg-muted p-4 text-sm">Ninguna todavía.</p>
              ) : (
                <div className="max-h-80 overflow-y-auto">
                  <table className="w-full text-left text-xs">
                    <thead className="text-fg-faint border-line-soft sticky top-0 border-b">
                      <tr>
                        <th scope="col" className="px-4 py-2 font-medium">Cuándo</th>
                        <th scope="col" className="px-4 py-2 font-medium">Modelo</th>
                        <th scope="col" className="px-4 py-2 text-right font-medium">Entrada</th>
                        <th scope="col" className="px-4 py-2 text-right font-medium">Salida</th>
                        <th scope="col" className="px-4 py-2 text-right font-medium">Primer token</th>
                      </tr>
                    </thead>
                    <tbody>
                      {datos.recientes.map((f, i) => (
                        <tr key={`${f.ts}-${i}`} className="border-line-soft border-b last:border-0">
                          <td className="mono text-fg-faint px-4 py-1.5 whitespace-nowrap" title={f.cliente}>
                            {hace(f.ts, ahora)}
                          </td>
                          <td className="mono max-w-[160px] truncate px-4 py-1.5" title={`${f.modelo} · ${f.ruta}`}>
                            {f.modelo || f.ruta}
                          </td>
                          <td className="mono px-4 py-1.5 text-right">
                            {f.prompt_tokens == null ? (
                              <span className="text-fg-faint">—</span>
                            ) : (
                              num(f.prompt_tokens)
                            )}
                          </td>
                          <td className="mono px-4 py-1.5 text-right">
                            {f.completion_tokens == null ? (
                              <span className="text-fg-faint">—</span>
                            ) : (
                              num(f.completion_tokens)
                            )}
                          </td>
                          <td className="mono px-4 py-1.5 text-right">
                            {f.ttft_ms == null ? (
                              <span className="text-fg-faint">—</span>
                            ) : f.ttft_ms < 1000 ? (
                              `${num(f.ttft_ms)} ms`
                            ) : (
                              `${num(f.ttft_ms / 1000, 1)} s`
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Card>
          </div>

          {/* ── De dónde sale cada cifra ──────────────────────────────── */}
          <Card>
            <Etiqueta>De dónde sale cada cifra</Etiqueta>
            <ul className="text-fg-muted mt-2 flex flex-col gap-1 text-xs">
              <li>
                <strong className="text-fg">Tokens y peticiones</strong>: los publica el MOTOR en
                cada respuesta. La puerta los lee al pasar y los guarda con la petición; no los
                estima ni los cuenta por su cuenta. Por eso una petición sin datos del motor sale
                como «—» y no como 0.
              </li>
              <li>
                <strong className="text-fg">Entrada en caché</strong>: los tokens del prompt que el
                motor sacó de su caché, que es lo que hace rápida la segunda pregunta de un mismo
                contexto.
              </li>
              <li>
                <strong className="text-fg">Velocidad</strong>: tokens de salida entre el tiempo que
                el motor tardó en generarlos. Cuando el motor no lo dice se despeja del reloj de la
                puerta, y entonces es una medición propia, no una estimación.
              </li>
              <li>
                <strong className="text-fg">Primer token</strong>: medido por la puerta, de la
                petición al primer byte de la respuesta. Incluye cargar el modelo si estaba
                descargado: un 6 s en frío y 0,4 s en caliente son la misma máquina con el mismo
                modelo.
              </li>
              <li>
                <strong className="text-fg">Histórico</strong>: se guardan {datos.retencion_dias}{" "}
                días y después se borra lo viejo. Lo que no pasa por la puerta no se cuenta: si un
                cliente sigue apuntando al motor directamente, aquí no aparece.
              </li>
            </ul>
          </Card>
        </>
      ) : null}
    </div>
  );
}
