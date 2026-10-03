/**
 * La memoria de la GPU: qué está servido, con qué configuración y cuánto ocupa.
 *
 * POR QUÉ NO HAY UN DESGLOSE "PESOS / CACHÉ KV / SOBRECARGA" como el de otras
 * herramientas: porque **ningún motor de este equipo publica ese reparto**. Se
 * comprobó antes de escribir esto: `/props` de llama-server no da el tamaño de la
 * caché KV, `llama-swap /running` no da la VRAM por modelo, `amd-smi process`
 * responde «No running processes detected» aunque haya un servidor usando la GPU,
 * y el tamaño del KV solo sale en el log de arranque de llama-server, que el proxy
 * no reenvía. Inventar el reparto sería lo contrario de lo que hace este panel.
 *
 * LO QUE SÍ HAY, y es lo que se enseña:
 *  - Los **pesos** de cada modelo: el tamaño del fichero, leído del disco.
 *  - La **configuración con la que se sirve**: contexto, si la caché KV va
 *    cuantizada y cuántas capas están en la GPU. Sale de la línea de comandos con
 *    la que arrancó el motor, que es el dato real (y está a la vista, entera).
 *  - La **VRAM total en uso** de la tarjeta, de sysfs.
 *  - El **resto**, que es la resta de los dos anteriores: ahí va la caché KV, la
 *    sobrecarga del motor y lo que ocupen los demás programas. Es una resta de
 *    medidas, no una estimación, y por eso no se parte en trozos que no se pueden
 *    medir.
 */
import { useCallback, useEffect, useState } from "react";
import { IconCpu, IconRefresh } from "@tabler/icons-react";
import { api, type MemoriaGpu } from "../lib/tauri";
import { Barra, Boton, Card, Datos, Etiqueta, Insignia } from "./ui";
import { gb, num } from "../lib/format";

export function Memoria() {
  const [m, setM] = useState<MemoriaGpu | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(false);

  const cargar = useCallback(async () => {
    setCargando(true);
    try {
      setM(await api.memoria());
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setCargando(false);
    }
  }, []);

  useEffect(() => {
    void cargar();
    // Se refresca cada 30 s: un modelo se descarga solo cuando cumple su `ttl`, y
    // una tarjeta que dijera "cargado" media hora después mentiría.
    const id = setInterval(() => void cargar(), 30_000);
    return () => clearInterval(id);
  }, [cargar]);

  const total = m?.vram_total_gb ?? null;
  const usada = m?.vram_usada_gb ?? null;
  const pctUso = total && usada != null ? (usada / total) * 100 : null;

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconCpu size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Memoria de la GPU</Etiqueta>
        <Insignia tono={(m?.modelos.length ?? 0) > 0 ? "ok" : "neutro"}>
          {m == null ? "leyendo…" : m.modelos.length === 0 ? "nada servido" : `${m.modelos.length} servido${m.modelos.length === 1 ? "" : "s"}`}
        </Insignia>
        <Boton className="ml-auto" disabled={cargando} onClick={() => void cargar()}>
          <IconRefresh size={13} className="mr-1 inline" aria-hidden="true" />
          {cargando ? "Leyendo…" : "Refrescar"}
        </Boton>
      </div>

      {error ? (
        <p className="text-bad text-sm" role="alert">
          No se pudo leer la memoria de los motores: {error}
        </p>
      ) : null}

      {m ? (
        <>
          {/* El reparto, en una barra: pesos (medidos) y resto (la resta). */}
          {usada != null && total != null ? (
            <div className="flex flex-col gap-2">
              <div className="flex items-baseline justify-between text-xs">
                <span className="text-fg-muted">
                  VRAM en uso: <strong className="text-fg">{gb(usada)}</strong> de {gb(total)}
                </span>
                <span className="mono text-fg-faint">{num(pctUso ?? 0, 1)} %</span>
              </div>
              <Barra valor={pctUso ?? 0} />
              <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
                <span className="text-fg-muted">
                  Pesos servidos: <span className="mono text-fg">{gb(m.pesos_gb)}</span>
                </span>
                <span className="text-fg-muted">
                  Caché KV, sobrecarga y demás:{" "}
                  <span className="mono text-fg">{gb(m.resto_gb ?? 0)}</span>
                </span>
              </div>
            </div>
          ) : (
            <p className="text-fg-muted text-sm">
              No se pudo leer la VRAM de la tarjeta. Los modelos servidos se pueden ver igual, más abajo.
            </p>
          )}

          {/* Cada modelo con lo que se sabe de él. */}
          {m.modelos.length === 0 ? (
            <p className="text-fg-muted mt-3 text-sm">
              Ningún modelo servido ahora mismo. Cuando uno se cargue, aquí aparecerá con los pesos que ocupa y la
              configuración con la que se está sirviendo.
            </p>
          ) : (
            <ul className="mt-3 flex flex-col gap-3">
              {m.modelos.map((x) => (
                <li key={x.id} className="border-line-soft border-t pt-3 first:border-0 first:pt-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm">{x.nombre}</span>
                    <span className="mono text-fg-faint text-xs">{x.id}</span>
                    {x.pesos_gb != null ? (
                      <Insignia tono="neutro">
                        <span title="Tamaño del fichero del modelo, leído del disco">
                          {gb(x.pesos_gb)} de pesos
                        </span>
                      </Insignia>
                    ) : null}
                    {x.ttl_s != null && x.ttl_s > 0 ? (
                      <Insignia tono="neutro">
                        <span title="Minutos sin peticiones tras los que el proxy lo descarga solo">
                          se descarga a los {Math.round(x.ttl_s / 60)} min sin uso
                        </span>
                      </Insignia>
                    ) : null}
                  </div>
                  <div className="mt-2">
                    <Datos
                      items={[
                        [
                          "Fichero",
                          <span key="f" className="mono break-all text-xs">
                            {x.ruta || "—"}
                          </span>,
                        ],
                        ["Contexto servido", <span key="c">{x.contexto == null ? "—" : `${num(x.contexto)} tokens`}</span>],
                        [
                          "Caché KV",
                          x.kv_quant ? (
                            <span key="k">
                              cuantizada en <span className="mono">{x.kv_quant}</span>
                              {x.flash_attention ? " (con flash attention)" : ""}
                            </span>
                          ) : (
                            <span key="k" className="text-fg-faint">
                              sin cuantizar: ocupa bastante más
                            </span>
                          ),
                        ],
                        [
                          "Capas en GPU",
                          <span key="n">
                            {x.ngl == null
                              ? "—"
                              : x.ngl < 0 || x.ngl >= 99
                                ? "todas"
                                : `${x.ngl} de todas`}
                          </span>,
                        ],
                      ]}
                    />
                  </div>
                  {/* La línea de comandos ENTERA: es la prueba de todo lo de
                      arriba, y se puede comparar con el `llama-swap.yaml`. */}
                  <details className="mt-2">
                    <summary className="text-accent cursor-pointer text-xs">
                      Con qué se está sirviendo (línea de comandos)
                    </summary>
                    <pre className="mono text-fg-muted mt-1 overflow-x-auto rounded-md p-2 text-[11px] leading-relaxed">
                      {x.cmd}
                    </pre>
                  </details>
                </li>
              ))}
            </ul>
          )}

          <p className="text-fg-faint mt-3 text-xs">
            El reparto entre caché KV y sobrecarga del motor no se puede medir: llama.cpp no lo publica por su API (solo
            lo dice en su log de arranque, que el proxy no reenvía). Por eso van en un solo bloque, sumados a lo que
            ocupen los demás programas. Preferimos decir «no se puede» antes que inventar una proporción.
          </p>
        </>
      ) : null}
    </Card>
  );
}
