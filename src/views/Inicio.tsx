/**
 * Inicio: qué está pasando y qué te toca hacer.
 *
 * Es la portada y responde a UNA pregunta: "¿hay algo que atender?". Por eso el
 * orden es el de la urgencia, no el de las fuentes de datos:
 *
 *  1. AVISOS — lo que va mal y tiene remedio (reloj de memoria clavado, un
 *     servidor en marcha que no contesta, comprobaciones en rojo). Primero.
 *  2. LA MÁQUINA, DE UN VISTAZO — cinco números con enlace al detalle. Aquí NO
 *     se repite lo de Hardware: son cifras de estado, no el análisis.
 *  3. EL MOTOR — quién sirve, qué modelos tiene cargados y qué procesos lo
 *     explican, con las acciones de arranque y parada ahí mismo.
 *  4. EL DISCO — qué ocupa y de qué tipo, con enlace a En disco.
 *
 * Lo que NO hace: comprobar el entorno entero por su cuenta (eso es de
 * Diagnóstico, que recorre el inventario y tarda). Ofrece el botón, y si ya hay
 * un resultado de una comprobación previa, enseña solo lo que está mal.
 */
import { IconAlertTriangle, IconBox, IconCheck, IconServer } from "@tabler/icons-react";
import { useEffect, useState } from "react";
import { useApp, cargandoDe, ejecutar, errorDe } from "../store";
import { Barra, Boton, Card, Datos, Etiqueta, Insignia, Kpi, Vacio } from "../components/ui";
import { AvisoMclk, useMclk } from "../components/RelojMemoria";
import { api, type HistorialDisco } from "../lib/tauri";
import { bLegibles, c, estadoModelo, fechaHora, hace, mb, nivelUso, num, w } from "../lib/format";
import { etiquetaTipo, tonoTipo } from "../lib/modelos";
import { clsx } from "clsx";

/** Una fila de acción rápida: levantar o parar un servidor de los dados de alta. */
function AccionServidor({ id, activo }: { id: string; activo: boolean }) {
  const enCurso = useApp((st) => st.accionEnCurso);
  return activo ? (
    <Boton variante="peligro" disabled={!!enCurso} onClick={() => void ejecutar("server:stop", { id })}>
      Parar
    </Boton>
  ) : (
    <Boton disabled={!!enCurso} onClick={() => void ejecutar("server:start", { id })}>
      Levantar
    </Boton>
  );
}

/**
 * Aviso de crecimiento del hogar: si en la última semana ha subido más del umbral
 * (se configura en Ajustes), se dice con la fecha de la medida de partida y un
 * enlace para verlo en Almacenamiento.
 *
 * Solo sale cuando la comparación es COMPLETA. Un crecimiento medido a medias
 * (presupuesto agotado o exclusiones actuando) no es «el crecimiento del hogar»,
 * y avisar con él sería alarmar con un número que no se sostiene. Y si el
 * histórico no llega a una semana, no hay aviso: no se compara contra lo que no
 * existe.
 */
function AvisoCrecimiento() {
  const setVista = useApp((st) => st.setVista);
  const [dato, setDato] = useState<HistorialDisco | null>(null);

  useEffect(() => {
    let vivo = true;
    api.almacen
      .historial(undefined, 7)
      .then((h) => {
        if (vivo) setDato(h);
      })
      .catch(() => {
        // Un fallo al leer el histórico no es un crecimiento: Inicio no inventa
        // un aviso por él (el error de base ya se cuenta donde toca).
      });
    return () => {
      vivo = false;
    };
  }, []);

  const c = dato?.crecimiento;
  if (!dato || !c || c.parcial || c.delta_bytes <= 0) return null;
  if (c.delta_bytes < dato.umbral_gb * 1024 ** 3) return null;

  return (
    <Card className="border-warn/40">
      <div className="flex flex-wrap items-center gap-2">
        <IconAlertTriangle size={15} className="text-warn shrink-0" aria-hidden="true" />
        <span className="text-sm">
          Tu carpeta personal ha crecido <strong>{bLegibles(c.delta_bytes, 1)}</strong> desde el{" "}
          {fechaHora(c.antes_ts)} ({hace(c.antes_ts)}); el umbral de aviso son {num(dato.umbral_gb)}{" "}
          GB por semana.
        </span>
        <Boton className="ml-auto" onClick={() => setVista("almacenamiento")}>
          Ver almacenamiento
        </Boton>
      </div>
      <p className="text-fg-faint mt-1 text-xs">
        Comparación entre las medidas completas del {fechaHora(c.antes_ts)} y del{" "}
        {fechaHora(c.ahora_ts)}. Si alguna hubiera quedado a medias, no habría aviso.
      </p>
    </Card>
  );
}

export default function Inicio() {
  const s = useApp((st) => st.snapshot);
  const serie = useApp((st) => st.serie);
  const servidores = useApp((st) => st.servidores);
  const diagnostico = useApp((st) => st.diagnostico);
  const cargandoDiag = useApp(cargandoDe("diagnostico"));
  const cargarDiagnostico = useApp((st) => st.cargarDiagnostico);
  const errorFoto = useApp(errorDe("foto"));
  const setVista = useApp((st) => st.setVista);

  // La lectura del reloj va con la foto: aparece y desaparece con el fallo real.
  const { lectura, releer } = useMclk(s?.ts);

  if (!s)
    return (
      <Vacio titulo={errorFoto ? "No se pudo leer la foto del sistema" : "Cargando"}>
        {errorFoto}
      </Vacio>
    );

  const g = s.gpu[0];
  const activos = s.servers.filter((x) => x.state === "active");
  // `error` solo se rellena cuando el proceso SIGUE en marcha pero no contesta
  // en su puerto; un servidor parado es lo normal y no entra aquí.
  const sinRespuesta = s.servers.filter((x) => x.error);
  // `loaded` es lo único que cuenta como cargado; si el motor no publica el
  // estado (cadena vacía) no se puede contar y se dice, no se pone un 0.
  const cargados = s.servers.flatMap((sv) =>
    sv.models.filter((m) => m.state === "loaded").map((m) => ({ ...m, servidor: sv })),
  );

  // Solo lo que ha FALLADO en la última comprobación: las que van bien no son
  // una tarea, y el detalle completo vive en Diagnóstico. Un estado
  // `desconocido` NO entra aquí: no se sabe si va mal, y contarlo como problema
  // sería afirmar algo que no consta.
  const fallos = (diagnostico ?? []).filter((c) => c.estado === "problema" || c.estado === "aviso");

  return (
    <div className="flex flex-col gap-4">
      {/* ── 1. Avisos ─────────────────────────────────────────────────── */}
      <AvisoMclk lectura={lectura} onRecargar={releer} />

      <AvisoCrecimiento />

      {sinRespuesta.length > 0 ? (
        <Card className="border-bad/40">
          <div className="flex items-center gap-2">
            <IconAlertTriangle size={16} className="text-bad shrink-0" aria-hidden="true" />
            <span className="text-sm">
              {sinRespuesta.length === 1
                ? "1 servidor en marcha no responde en su puerto: "
                : `${sinRespuesta.length} servidores en marcha no responden en su puerto: `}
              {sinRespuesta.map((x) => x.name).join(", ")}
            </span>
            <Boton className="ml-auto" onClick={() => setVista("servidores")}>
              Ver servidores
            </Boton>
          </div>
        </Card>
      ) : null}

      {fallos.length > 0 ? (
        <Card>
          <div className="flex items-center gap-2">
            <IconAlertTriangle size={15} className="text-warn shrink-0" aria-hidden="true" />
            <Etiqueta>De la última comprobación</Etiqueta>
            <Boton className="ml-auto" onClick={() => setVista("diagnostico")}>
              Ver diagnóstico
            </Boton>
          </div>
          <ul className="mt-2 flex flex-col gap-1">
            {fallos.map((c) => (
              <li key={c.id} className="flex items-start gap-2 text-xs">
                <Insignia tono={c.estado === "problema" ? "bad" : "warn"}>
                  {c.estado === "problema" ? "falla" : "aviso"}
                </Insignia>
                <div className="min-w-0">
                  <span className="text-fg">{c.titulo}</span>
                  <span className="text-fg-muted"> · {c.detalle}</span>
                </div>
              </li>
            ))}
          </ul>
        </Card>
      ) : (
        // Sin resultado no se dice "todo bien" (no se ha comprobado) NI se deja
        // la portada sin la salida: se ofrece la comprobación, que es lo honesto.
        <Card>
          <div className="flex flex-wrap items-center gap-2">
            {diagnostico ? (
              <span className="text-fg-muted flex items-center gap-2 text-xs">
                <IconCheck size={14} className="text-ok" aria-hidden="true" />
                La última comprobación no encontró nada que arreglar.
              </span>
            ) : (
              <span className="text-fg-muted text-xs">
                El estado del entorno no se ha comprobado todavía.
              </span>
            )}
            <Boton
              className="ml-auto"
              disabled={cargandoDiag}
              onClick={() => void cargarDiagnostico()}
            >
              {cargandoDiag ? "Comprobando…" : "Comprobar ahora"}
            </Boton>
          </div>
        </Card>
      )}

      {/* ── 2. La máquina, de un vistazo ──────────────────────────────── */}
      <section aria-label="Estado de la máquina">
        <div className="mb-2 flex items-center gap-2">
          <Etiqueta>La máquina, de un vistazo</Etiqueta>
          <Boton className="ml-auto" onClick={() => setVista("hardware")}>
            Ver hardware
          </Boton>
        </div>
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-5">
          <Kpi
            etiqueta="CPU"
            valor={num(s.system.cpu_pct)}
            unidad="%"
            uso={s.system.cpu_pct}
            nivel={nivelUso(s.system.cpu_pct)}
            serie={serie.map((p) => p.cpu)}
          />
          <Kpi
            etiqueta="Memoria"
            valor={num(s.system.mem.pct)}
            unidad="%"
            uso={s.system.mem.pct}
            nivel={nivelUso(s.system.mem.pct)}
            serie={serie.map((p) => p.mem)}
          />
          <Kpi
            etiqueta="GPU"
            valor={num(g?.util ?? null)}
            unidad="%"
            uso={g?.util ?? 0}
            nivel={nivelUso(g?.util ?? null)}
            serie={serie.map((p) => p.gpuUtil)}
          />
          <Kpi
            etiqueta="VRAM"
            valor={num(g?.mem_pct ?? null)}
            unidad="%"
            uso={g?.mem_pct ?? 0}
            nivel={nivelUso(g?.mem_pct ?? null)}
            serie={serie.map((p) => p.gpuMemPct)}
          />
          <Kpi
            etiqueta="Disco"
            valor={num(s.disk.pct)}
            unidad="%"
            uso={s.disk.pct}
            nivel={nivelUso(s.disk.pct)}
          />
        </div>
      </section>

      <div className="grid gap-3 lg:grid-cols-2">
        {/* ── 3. El motor ─────────────────────────────────────────────── */}
        <Card>
          <div className="mb-3 flex items-center gap-2">
            <IconServer size={15} className="text-accent" aria-hidden="true" />
            <Etiqueta>El motor</Etiqueta>
            <span className="text-fg-faint ml-auto text-xs">
              {activos.length} de {s.servers.length} activos
            </span>
            <Boton onClick={() => setVista("servidores")}>Ver servidores</Boton>
          </div>

          {s.servers.length === 0 ? (
            <p className="text-fg-muted text-sm">
              Ningún servidor dado de alta. Se añaden en <strong>Ajustes</strong>.
            </p>
          ) : (
            <ul className="flex flex-col gap-2">
              {s.servers.map((sv) => {
                // El servidor dado de alta aporta la acción; el estado vivo viene
                // de la foto. Se emparejan por `kind`, que es lo único que
                // comparten los dos (los ids no salen de la misma tabla).
                const alta = servidores.find((r) => r.kind === sv.kind);
                const publicaEstado = sv.models.some((m) => m.state !== "");
                const cargadosServidor = sv.models.filter((m) => m.state === "loaded").length;
                return (
                  <li key={sv.id} className="border-line-soft flex flex-wrap items-center gap-2 border-b pb-2 last:border-0 last:pb-0">
                    <Insignia tono={sv.state === "active" ? "ok" : "neutro"}>
                      {sv.state === "active" ? "activo" : "parado"}
                    </Insignia>
                    <span className="text-sm">{sv.name}</span>
                    <span className="mono text-fg-faint text-xs">:{sv.port}</span>
                    {sv.models.length > 0 ? (
                      <span className="text-fg-muted text-xs">
                        {publicaEstado
                          ? `${cargadosServidor} de ${sv.models.length} cargados`
                          : "estado sin publicar"}
                      </span>
                    ) : null}
                    {alta ? (
                      <span className="ml-auto">
                        <AccionServidor id={alta.id} activo={sv.state === "active"} />
                      </span>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          )}

          {cargados.length > 0 ? (
            <div className="border-line-soft mt-3 border-t pt-3">
              <Etiqueta>Cargados ahora en memoria</Etiqueta>
              <ul className="mt-1.5 flex flex-col gap-1">
                {cargados.map((m) => (
                  <li key={`${m.servidor.id}-${m.id}`} className="row">
                    <span className="truncate text-sm">{m.label || m.id}</span>
                    {m.quant ? <span className="mono text-fg-faint text-xs">{m.quant}</span> : null}
                    <span className="text-fg-faint ml-auto text-xs">{m.servidor.name}</span>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </Card>

        {/* ── 4. Lo que ocupa y lo que consume ────────────────────────── */}
        <Card>
          <div className="mb-3 flex items-center gap-2">
            <IconBox size={15} className="text-accent" aria-hidden="true" />
            <Etiqueta>En disco</Etiqueta>
            <Boton className="ml-auto" onClick={() => setVista("disco")}>
              Ver en disco
            </Boton>
          </div>
          <div className="text-fg-muted text-xs">
            {s.inventario.ficheros} {s.inventario.ficheros === 1 ? "fichero" : "ficheros"} ·{" "}
            {bLegibles(s.inventario.bytes, 1)}
          </div>
          {s.inventario.por_tipo.length > 0 ? (
            <ul className="mt-2 flex flex-col gap-1">
              {s.inventario.por_tipo.map((r) => (
                <li key={r.tipo} className="flex items-center gap-2 text-xs">
                  <Insignia tono={tonoTipo(r.tipo)}>{etiquetaTipo(r.tipo)}</Insignia>
                  <span className="text-fg-muted mono ml-auto">
                    {r.ficheros} · {bLegibles(r.bytes, 1)}
                  </span>
                </li>
              ))}
            </ul>
          ) : s.inventario.ficheros > 0 ? (
            // Hay ficheros pero el desglose por tipo no ha venido (una foto
            // puede llegar sin él): se calla el desglose en vez de afirmar “no
            // se ha encontrado ningún modelo”, que sería mentira con 20 delante.
            <p className="text-fg-muted mt-2 text-sm">El desglose por tipo no ha llegado en esta foto.</p>
          ) : (
            <p className="text-fg-muted mt-2 text-sm">No se ha encontrado ningún modelo en disco.</p>
          )}

          <div className="border-line-soft mt-3 border-t pt-3">
            <Etiqueta>Procesos de IA ({s.ai_procs.length})</Etiqueta>
            {s.ai_procs.length === 0 ? (
              <p className="text-fg-muted mt-1.5 text-sm">Ninguno detectado.</p>
            ) : (
              <ul className="mt-1.5 flex flex-col gap-1">
                {s.ai_procs.slice(0, 6).map((p) => (
                  <li key={p.pid} className="row">
                    <span className="mono text-fg-faint w-12 text-xs">{p.pid}</span>
                    <span className="truncate text-sm" title={p.cmd}>
                      {p.name}
                    </span>
                    <span className="text-fg-muted mono ml-auto text-xs">{num(p.cpu_pct)}% CPU</span>
                    <span className="text-fg-muted mono w-16 text-right text-xs">{mb(p.mem_mb, 0)}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </Card>
      </div>

      {/* Resumen de la GPU en una línea: el detalle (ventilador, consumo,
          limitación, niveles del reloj) vive en Hardware, que es donde se mira
          cuando se quiere analizar. */}
      {g || s.disk ? (
        <div className={clsx("grid gap-3", "lg:grid-cols-2")}>
          {g ? (
            <Card>
              <Etiqueta>GPU</Etiqueta>
              <div className="mt-2">
                <Datos
                  items={[
                    ["Modelo", g.name],
                    ["Temperatura", c(g.temp_c)],
                    ["Consumo", w(g.power_w)],
                    [
                      "VRAM",
                      <span key="v">
                        {num((g.mem_used_mb ?? 0) / 1024, 1)} de {num((g.mem_total_mb ?? 0) / 1024, 1)} GB (
                        {num(g.mem_pct)} %)
                      </span>,
                    ],
                  ]}
                />
                <div className="mt-2">
                  <Barra valor={g.mem_pct} nivel={nivelUso(g.mem_pct)} />
                </div>
              </div>
            </Card>
          ) : null}
          <Card>
            <Etiqueta>Almacenamiento</Etiqueta>
            <div className="mt-2 flex flex-col gap-2">
              <Barra valor={s.disk.pct} nivel={nivelUso(s.disk.pct)} />
              <Datos
                items={[
                  // El punto de montaje va con los números: "186.6 GB libres" sin
                  // decir de qué sistema de ficheros sale no informa de nada.
                  ["Punto de montaje", s.disk.mount],
                  ["Libre", `${num(s.disk.free_gb)} GB`],
                  ["Total", `${num(s.disk.total_gb)} GB`],
                ]}
              />
            </div>
            <p className="text-fg-faint mt-2 flex items-center gap-1 text-xs">
              Los modelos ocupan su propio espacio: ese desglose está en En disco.
            </p>
          </Card>
        </div>
      ) : null}

      {/* Ficha de una línea: de dónde sale el estado de los modelos. Se dice
          porque `loaded` lo publica el MOTOR, no Machinograph. */}
      <p className="text-fg-faint text-xs">
        {s.servers.some((sv) => sv.models.some((m) => m.state !== ""))
          ? `Estados de modelo tal como los publica el motor (${estadoModelo("loaded")} = en memoria).`
          : "Ningún motor está publicando el estado de sus modelos ahora mismo."}
      </p>
    </div>
  );
}
