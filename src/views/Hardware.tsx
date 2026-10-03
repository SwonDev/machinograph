/**
 * Hardware: la máquina por dentro.
 *
 * Fusión de las dos vistas que antes partían esto en dos (Panel y Sistema):
 * el Panel enseñaba los KPI y la GPU, y Sistema las series de dos horas, el
 * disco y los procesos. Mirar "cómo va la máquina" obligaba a saltar entre las
 * dos, y ninguna de las dos estaba completa.
 *
 * Orden: primero LO QUE PUEDE ESTAR MAL (el reloj de memoria clavado, que hace
 * que todo vaya ~15× más lento sin dar error), después las cifras, luego el
 * detalle de la GPU, las series guardadas, el disco y los procesos.
 *
 * Las series de 2 h salen de SQLite (`metrics:recent`), no de la foto: la foto
 * solo lleva los últimos cuatro minutos en memoria, que es lo que usan las
 * chispas. Se piden al montar y con el botón, no en cada foto.
 */
import { useEffect, useMemo, useState } from "react";
import { IconBolt, IconRefresh, IconTrash } from "@tabler/icons-react";
import { useApp, ejecutar, errorDe } from "../store";
import { api, type AiProc, type MetricRow } from "../lib/tauri";
import { Barra, Boton, Card, Datos, Etiqueta, Insignia, Kpi, Spark, Vacio } from "../components/ui";
import { PanelMclk, useMclk } from "../components/RelojMemoria";
import { Caudal, PotenciaFrecuencia, Temperaturas, Ventiladores, Voltajes } from "../components/Sensores";
import { Memoria } from "../components/Memoria";
import { c, gb, hora, mb, nivelUso, num, pct, w } from "../lib/format";

/**
 * Terminar un proceso de IA: dos pasos, porque no se puede deshacer.
 *
 * Matar un proceso en marcha corta la generación que tenga a medias y la VRAM que
 * ocupa se pierde, así que el mismo clic no puede valer. El aviso dice QUÉ
 * proceso y QUÉ pasa, que es lo que hace falta para decidir.
 */
function TerminarProceso({ p, enCurso }: { p: AiProc; enCurso: string | null }) {
  const [confirmando, setConfirmando] = useState(false);

  if (!confirmando) {
    return (
      <Boton
        variante="peligro"
        disabled={!!enCurso}
        onClick={() => setConfirmando(true)}
        aria-label={`Terminar el proceso ${p.pid} (${p.name}), pedirá confirmación`}
      >
        <IconTrash size={12} className="mr-1 inline" aria-hidden="true" />
        Terminar…
      </Boton>
    );
  }

  return (
    <span className="flex flex-wrap items-center justify-end gap-1.5">
      <span className="text-bad text-[11px]" role="alert">
        ¿Terminar {p.name} (PID {p.pid})? Se corta lo que esté haciendo. No se puede deshacer.
      </span>
      <Boton
        variante="peligro"
        disabled={!!enCurso}
        onClick={() => {
          setConfirmando(false);
          void ejecutar("process:kill", { pid: p.pid });
        }}
        aria-label={`Sí, terminar el proceso ${p.pid}`}
      >
        Sí, terminar
      </Boton>
      <Boton onClick={() => setConfirmando(false)}>No</Boton>
    </span>
  );
}

export default function Hardware() {
  const s = useApp((st) => st.snapshot);
  const serie = useApp((st) => st.serie);
  const enCurso = useApp((st) => st.accionEnCurso);
  // Esta vista también vive de la foto, así que solo le corresponde el error de
  // la foto. El de `metrics:recent` es otro y se guarda aparte, aquí abajo.
  const errorFoto = useApp(errorDe("foto"));
  const [metricas, setMetricas] = useState<MetricRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  const { lectura, releer } = useMclk(s?.ts);

  const cargar = async () => {
    try {
      const desde = Math.floor(Date.now() / 1000) - 2 * 3600; // 2 h atrás
      setMetricas(await api.metrics(desde));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    void cargar();
  }, []);

  const resumen = useMemo(() => {
    if (metricas.length === 0) return null;
    const cpu = metricas.map((m) => m.cpu);
    const mem = metricas.map((m) => m.mem);
    const vram = metricas
      .filter((m) => m.gpu_mem_total)
      .map((m) => ((m.gpu_mem_used ?? 0) / (m.gpu_mem_total || 1)) * 100);
    const max = (v: number[]) => (v.length ? Math.max(...v) : null);
    const media = (v: number[]) => (v.length ? v.reduce((a, b) => a + b, 0) / v.length : null);
    return { cpuMax: max(cpu), cpuMedia: media(cpu), memMax: max(mem), memMedia: media(mem), vramMax: max(vram) };
  }, [metricas]);

  if (!s)
    return (
      <Vacio titulo={errorFoto ? "No se pudo leer la foto del sistema" : "Cargando"}>
        {errorFoto}
      </Vacio>
    );

  const g = s.gpu[0];

  return (
    <div className="flex flex-col gap-4">
      {/* ── Cifras de ahora ───────────────────────────────────────────── */}
      <section aria-label="Métricas actuales" className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-5">
        <Kpi
          etiqueta="CPU"
          valor={num(s.system.cpu_pct)}
          unidad="%"
          uso={s.system.cpu_pct}
          nivel={nivelUso(s.system.cpu_pct)}
          serie={serie.map((p) => p.cpu)}
          pie={`${s.system.cores} núcleos · carga ${num(s.system.load1, 2)}`}
        />
        <Kpi
          etiqueta="Memoria"
          valor={num(s.system.mem.pct)}
          unidad="%"
          uso={s.system.mem.pct}
          nivel={nivelUso(s.system.mem.pct)}
          serie={serie.map((p) => p.mem)}
          pie={`${mb(s.system.mem.used_mb, 0)} de ${mb(s.system.mem.total_mb, 0)}`}
        />
        <Kpi
          etiqueta="GPU"
          valor={num(g?.util ?? null)}
          unidad="%"
          uso={g?.util ?? 0}
          nivel={nivelUso(g?.util ?? null)}
          serie={serie.map((p) => p.gpuUtil)}
          pie={g ? `${g.name} · ${c(g.temp_c)} · ${w(g.power_w)}` : "sin datos"}
        />
        <Kpi
          etiqueta="VRAM"
          valor={num(g?.mem_pct ?? null)}
          unidad="%"
          uso={g?.mem_pct ?? 0}
          nivel={nivelUso(g?.mem_pct ?? null)}
          serie={serie.map((p) => p.gpuMemPct)}
          pie={g ? `${mb(g.mem_used_mb, 0)} de ${mb(g.mem_total_mb, 0)}` : "sin datos"}
        />
        <Kpi
          etiqueta="Disco"
          valor={num(s.disk.pct)}
          unidad="%"
          uso={s.disk.pct}
          nivel={nivelUso(s.disk.pct)}
          pie={
            // Se dice QUÉ punto de montaje se mide: el dato no significa nada sin
            // saber de qué sistema de ficheros sale.
            <>
              {gb(s.disk.free_gb)} libres de {gb(s.disk.total_gb)} ·{" "}
              <span className="mono">{s.disk.mount}</span>
            </>
          }
        />
      </section>

      {/* ── GPU al detalle, con el reloj de memoria ───────────────────── */}
      <Card>
        <div className="mb-3 flex items-center gap-2">
          <IconBolt size={15} className="text-accent" aria-hidden="true" />
          <Etiqueta>GPU</Etiqueta>
        </div>
        {g?.parcial ? (
          // macOS y Windows: el sistema publica el nombre, la VRAM y el driver, y
          // NADA MÁS sin privilegios. Se enseña eso y se dice por qué no hay más,
          // en vez de pintar ceros que se leerían como «la tarjeta está parada».
          <div className="flex flex-col gap-3">
            <Datos
              items={[
                ["Modelo", g.name],
                ["Controlador", g.driver || "—"],
                ["VRAM", g.mem_total_mb > 0 ? gb(g.mem_total_mb / 1024) : "—"],
              ]}
            />
            <p className="text-fg-faint text-xs">
              En {s.so_nombre} el uso, la temperatura y el consumo de la GPU no los publica el
              sistema sin privilegios: eso se lee en Linux (sysfs/amdgpu), y aquí no.
            </p>
          </div>
        ) : g ? (
          <div className="flex flex-col gap-3">
            <Datos
              items={[
                ["Modelo", g.name],
                ["Controlador", g.driver],
                ["Núcleo", `${num(g.clock_mhz)} MHz`],
                [
                  "Temperatura",
                  <span key="t">
                    {c(g.temp_c)}
                    {g.mem_temp_c ? ` · VRAM ${c(g.mem_temp_c)}` : ""}
                  </span>,
                ],
                ["Ventilador", g.fan_rpm > 0 ? `${g.fan_rpm} rpm (${num(g.fan_pct)}%)` : "parado"],
                ["Consumo", w(g.power_w)],
                ["Limitación", g.throttle ?? "ninguna"],
              ]}
            />
            <div className="flex flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-fg-faint">VRAM</span>
                <span className="mono">{pct(g.mem_pct)}</span>
              </div>
              <Barra valor={g.mem_pct} nivel={nivelUso(g.mem_pct)} />
            </div>
          </div>
        ) : (
          <p className="text-fg-muted text-sm">
            No se pudo leer la GPU. Revisa <code className="mono">/sys/class/drm</code> o ROCm.
          </p>
        )}

        {/* ── Reloj de memoria (MCLK) ─────────────────────────────────── */}
        {s.so === "linux" ? (
        <div className="border-line-soft mt-3 border-t pt-3">
          <div className="mb-2 flex items-center gap-2">
            <Etiqueta>Reloj de memoria</Etiqueta>
            <span className="text-fg-faint ml-auto text-xs">
              el fallo que hace lento todo, sin dar error
            </span>
          </div>
          <PanelMclk lectura={lectura} onRecargar={releer} />
        </div>
        ) : null}
      </Card>

      {/* La memoria va JUSTO debajo de la GPU: es la respuesta a "¿por qué tengo
          la VRAM llena?", y esa pregunta se hace mirando la tarjeta. */}
      <Memoria />

      {/* ── Sensores: temperaturas, ventiladores y voltajes ───────────── */}
      {s.so !== "linux" ? (
        // La parte rica de los sensores (ventiladores, voltajes, potencia de la
        // CPU) la publica Linux por `hwmon`. En otro sistema NO existe, y decirlo
        // es la diferencia entre "el programa no lo sabe" y "esta máquina no lo
        // publica".
        <p className="text-fg-faint text-xs">
          En {s.so_nombre} solo se pueden leer temperaturas: los ventiladores, los voltajes y la
          potencia de la CPU los publica Linux por <span className="mono">hwmon</span>, y este
          sistema no tiene ese interfaz.
        </p>
      ) : null}
      <Temperaturas
        grupos={s.hardware.grupos}
        discos={s.hardware.discos_temp}
        descartados={s.hardware.descartados}
      />

      <div className="grid items-start gap-3 lg:grid-cols-2">
        <Ventiladores grupos={s.hardware.grupos} />
        <PotenciaFrecuencia
          potencia={s.hardware.cpu_potencia_w}
          fuente={s.hardware.cpu_potencia_fuente}
          frecuencia={s.hardware.cpu_frecuencia}
          nucleos={s.system.cores}
          cpuPct={s.system.cpu_pct}
        />
      </div>

      <div className="grid items-start gap-3 lg:grid-cols-2">
        <Voltajes grupos={s.hardware.grupos} />
        <Caudal discos={s.hardware.discos_caudal} red={s.hardware.red} />
      </div>

      {/* ── Series guardadas (2 h) ────────────────────────────────────── */}
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Etiqueta>
            Últimas 2 horas · {metricas.length} {metricas.length === 1 ? "muestra" : "muestras"}
          </Etiqueta>
          <Boton className="ml-auto" onClick={() => void cargar()}>
            <IconRefresh size={13} className="mr-1 inline" aria-hidden="true" />
            Refrescar
          </Boton>
        </div>
        {error ? (
          <Card className="border-bad/40">
            <p className="text-bad text-sm">No se pudo leer el histórico de métricas: {error}</p>
          </Card>
        ) : null}
        <div className="grid gap-3 lg:grid-cols-3">
          <Card>
            <Etiqueta>CPU (2 h)</Etiqueta>
            <div className="mt-2">
              <Spark datos={metricas.map((m) => m.cpu)} alto={44} nivel={nivelUso(s.system.cpu_pct)} />
            </div>
            <div className="text-fg-muted mt-2 text-xs">
              {resumen?.cpuMax != null && resumen.cpuMedia != null ? (
                <>
                  Máximo {pct(resumen.cpuMax)} · media {pct(resumen.cpuMedia)}.{" "}
                </>
              ) : null}
              Ahora {pct(s.system.cpu_pct)} · carga {num(s.system.load1, 2)} / {num(s.system.load5, 2)} /{" "}
              {num(s.system.load15, 2)}
            </div>
          </Card>
          <Card>
            <Etiqueta>Memoria (2 h)</Etiqueta>
            <div className="mt-2">
              <Spark datos={metricas.map((m) => m.mem)} alto={44} nivel={nivelUso(s.system.mem.pct)} />
            </div>
            <div className="text-fg-muted mt-2 text-xs">
              {resumen?.memMax != null ? <>Máximo {pct(resumen.memMax)}. </> : null}
              {mb(s.system.mem.used_mb, 0)} de {mb(s.system.mem.total_mb, 0)} · swap{" "}
              {mb(s.system.swap.used_mb, 0)}
            </div>
          </Card>
          <Card>
            <Etiqueta>VRAM (2 h)</Etiqueta>
            <div className="mt-2">
              <Spark
                datos={metricas.map((m) =>
                  m.gpu_mem_total ? ((m.gpu_mem_used ?? 0) / m.gpu_mem_total) * 100 : 0,
                )}
                alto={44}
                nivel={nivelUso(s.gpu[0]?.mem_pct ?? null)}
              />
            </div>
            <div className="text-fg-muted mt-2 text-xs">
              {resumen?.vramMax != null ? <>Máximo {pct(resumen.vramMax)}. </> : null}
              {mb(s.gpu[0]?.mem_used_mb ?? 0, 0)} de {mb(s.gpu[0]?.mem_total_mb ?? 0, 0)}
            </div>
          </Card>
        </div>
      </section>

      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <Etiqueta>Almacenamiento</Etiqueta>
          <div className="mt-2 flex flex-col gap-2">
            <Barra valor={s.disk.pct} nivel={nivelUso(s.disk.pct)} />
            <Datos
              items={[
                // El punto de montaje va con los números: "Total 473.9 GB" sin
                // decir de qué sistema de ficheros sale no informa de nada.
                ["Punto de montaje", s.disk.mount],
                ["Total", `${num(s.disk.total_gb)} GB`],
                ["Usado", `${num(s.disk.used_gb)} GB`],
                ["Libre", `${num(s.disk.free_gb)} GB`],
                ["Uso", pct(s.disk.pct)],
              ]}
            />
          </div>
        </Card>

        <Card>
          <Etiqueta>Procesos de IA ({s.ai_procs.length})</Etiqueta>
          {s.ai_procs.length === 0 ? (
            <p className="text-fg-muted mt-2 text-sm">Ninguno detectado.</p>
          ) : (
            <ul className="mt-2 flex flex-col gap-1">
              {s.ai_procs.map((p) => (
                <li key={p.pid} className="row">
                  <span className="mono text-fg-faint w-14 text-xs">{p.pid}</span>
                  <span className="truncate text-sm">{p.name}</span>
                  {p.tag ? <Insignia tono="neutro">{p.tag}</Insignia> : null}
                  <span className="text-fg-muted mono ml-auto text-xs">
                    {num(p.cpu_pct)}% · {mb(p.mem_mb, 0)}
                  </span>
                  <TerminarProceso p={p} enCurso={enCurso} />
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>

      <Card>
        <Etiqueta>Historial reciente de métricas</Etiqueta>
        <div className="mt-2 max-h-64 overflow-y-auto">
          <table className="w-full text-left text-xs">
            <thead className="text-fg-faint border-line-soft sticky top-0 border-b">
              <tr>
                <th scope="col" className="py-1.5 font-medium">Hora</th>
                <th scope="col" className="py-1.5 font-medium">CPU</th>
                <th scope="col" className="py-1.5 font-medium">Memoria</th>
                <th scope="col" className="py-1.5 font-medium">Disco</th>
                <th scope="col" className="py-1.5 font-medium">VRAM</th>
                <th scope="col" className="py-1.5 font-medium">GPU</th>
              </tr>
            </thead>
            <tbody>
              {metricas.slice(-60).reverse().map((m) => (
                <tr key={m.ts} className="border-line-soft border-b last:border-0">
                  <td className="mono py-1">{hora(m.ts)}</td>
                  <td className="mono py-1">{pct(m.cpu)}</td>
                  <td className="mono py-1">{pct(m.mem)}</td>
                  <td className="mono py-1">{pct(m.disk)}</td>
                  <td className="mono py-1">
                    {m.gpu_mem_total ? pct(((m.gpu_mem_used ?? 0) / m.gpu_mem_total) * 100) : "—"}
                  </td>
                  <td className="mono py-1">{num(m.gpu_temp)} °C / {num(m.gpu_power)} W</td>
                </tr>
              ))}
            </tbody>
          </table>
          {metricas.length === 0 && !error ? (
            <p className="text-fg-muted py-4 text-sm">
              Todavía no hay muestras guardadas. Se guardan solas con cada foto (cada 2 s por defecto).
            </p>
          ) : null}
        </div>
      </Card>
    </div>
  );
}