/**
 * Pantalla: salidas, modos y la reactivación (el problema que motivó la app: el
 * monitor que se queda en 60 Hz o no despierta tras suspender).
 *
 * La acción `display:reapply` es la que arregla el caso típico, así que está
 * como botón principal y arriba, no escondida en un menú.
 *
 * Dos cosas que antes estaban mal y se ven aquí:
 *   * había un botón "Conmutar salida" que llamaba a `display:toggle` SIN decir
 *     qué salida, así que el backend solo podía contestar con un error. Ahora el
 *     conmutador está en la tarjeta de cada salida, que es donde tiene sentido.
 *   * los modos se listaban pero no se podían aplicar: ahora cada modo es un
 *     botón, y el que está en uso se marca como tal.
 *
 * El backend elige la herramienta según la sesión (`kscreen-doctor` en KDE
 * Wayland, `xrandr` en X11) y lo dice en `snapshot.note`.
 */
import { IconRefresh, IconDeviceDesktop, IconPower } from "@tabler/icons-react";
import { useApp, ejecutar, errorDe } from "../store";
import { Boton, Card, Etiqueta, Insignia, Vacio } from "../components/ui";
import { hora, num } from "../lib/format";

export default function Pantalla() {
  const s = useApp((st) => st.snapshot);
  const enCurso = useApp((st) => st.accionEnCurso);
  // Las salidas salen de la foto: el error que le toca es el de la foto.
  const error = useApp(errorDe("foto"));
  const salidas = s?.display ?? [];

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Boton
          variante="acento"
          disabled={!!enCurso}
          onClick={() => void ejecutar("display:reapply")}
        >
          <IconRefresh size={13} className="mr-1 inline" aria-hidden="true" />
          Reaplicar configuración de pantalla
        </Boton>
        {s ? (
          <span className="text-fg-faint ml-auto text-xs">Última lectura: {hora(s.ts)}</span>
        ) : null}
      </div>

      {!s ? (
        <Vacio titulo={error ? "No se pudo leer el estado de las pantallas" : "Cargando"}>
          {error}
        </Vacio>
      ) : salidas.length === 0 ? (
        <Vacio titulo="No se detectan salidas">
          {/* El motivo real lo da el backend en `note`: en Wayland, xrandr vería
              la pantalla falsa de XWayland, así que si no hay backend soportado
              es mejor decir por qué que enseñar una lista vacía sin explicación. */}
          {s.note || "El backend de pantalla no ha devuelto ninguna salida conectada."}
        </Vacio>
      ) : (
        <>
          <div className="grid gap-3 lg:grid-cols-2">
            {salidas.map((o) => {
              const modos = o.modes
                .slice()
                .sort((a, b) => b.w * b.h - a.w * a.h || b.hz - a.hz);
              // Una salida está ACTIVA si tiene un modo en uso. Los dos backends
              // rellenan w/h solo con el modo marcado con `*`, así que w=0
              // significa "conectada pero apagada" (o desconectada), sin
              // depender de que `status` signifique lo mismo en ambos.
              const activa = o.w > 0 && o.h > 0;
              return (
                <Card key={o.name}>
                  <div className="flex items-center gap-2">
                    <IconDeviceDesktop size={15} className="text-accent" aria-hidden="true" />
                    <span className="text-sm font-medium">{o.name}</span>
                    {o.primary ? <Insignia tono="acento">principal</Insignia> : null}
                    <Insignia tono={activa ? "ok" : "neutro"}>
                      {activa ? "activa" : o.connected ? "apagada" : "desconectada"}
                    </Insignia>
                    <Boton
                      className="ml-auto"
                      disabled={!!enCurso}
                      aria-label={`${activa ? "Desactivar" : "Activar"} la salida ${o.name}`}
                      onClick={() =>
                        void ejecutar("display:toggle", { output: o.name, on: !activa })
                      }
                    >
                      <IconPower size={13} className="mr-1 inline" aria-hidden="true" />
                      {activa ? "Desactivar" : "Activar"}
                    </Boton>
                  </div>

                  {activa ? (
                    <>
                      <div className="mt-3 flex items-baseline gap-2">
                        <span className="metric">
                          {o.w}×{o.h}
                        </span>
                        <span className="text-fg-muted mono text-sm">@ {num(o.hz, 2)} Hz</span>
                      </div>
                      <div className="text-fg-faint mt-1 text-xs">
                        Posición {o.offset_x},{o.offset_y} · {o.modes.length} modos disponibles
                      </div>

                      <div className="mt-3">
                        <Etiqueta>Aplicar un modo</Etiqueta>
                        <div className="mt-1 flex max-h-40 flex-wrap gap-1 overflow-y-auto">
                          {modos.map((m, i) => {
                            // El backend marca el modo en uso con `*` en sus
                            // indicadores (`*` en xrandr, `*!` en kscreen-doctor).
                            const actual = m.flags.includes("*");
                            return (
                              <button
                                key={`${m.w}x${m.h}@${m.hz}-${i}`}
                                type="button"
                                disabled={!!enCurso || actual}
                                aria-pressed={actual}
                                aria-label={`Aplicar ${m.w}×${m.h} a ${num(m.hz, 0)} hercios a ${o.name}`}
                                onClick={() =>
                                  void ejecutar("display:apply", {
                                    output: o.name,
                                    w: m.w,
                                    h: m.h,
                                    hz: m.hz,
                                  })
                                }
                                className={
                                  "mono rounded border px-1.5 py-0.5 text-[11px] transition-colors " +
                                  (actual
                                    ? "border-accent/50 bg-accent-soft text-accent"
                                    : "border-line-soft text-fg-muted hover:bg-raised disabled:opacity-40")
                                }
                              >
                                {m.w}×{m.h}@{num(m.hz, 0)}
                                {actual ? " ·" : ""}
                              </button>
                            );
                          })}
                        </div>
                      </div>
                    </>
                  ) : (
                    <p className="text-fg-muted mt-3 text-sm">
                      {o.connected
                        ? "La salida está conectada pero apagada: actívala para ver y elegir sus modos."
                        : "El cable no está conectado."}
                    </p>
                  )}
                </Card>
              );
            })}
          </div>

          {s.note ? <p className="text-fg-faint text-xs">{s.note}</p> : null}
        </>
      )}
    </div>
  );
}
