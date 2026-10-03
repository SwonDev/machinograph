/**
 * Ajustes: los servidores que están en SQLite, los AJUSTES que el backend lee
 * de verdad y el contexto de la instalación en solo lectura.
 *
 * Tres decisiones que vienen del backend, no del gusto:
 *  - El id se compone como `tipo:puerto` porque actions.rs deriva el tipo del
 *    PREFIJO del id (`id.split(':')`) para buscar procesos por patrón. Un id que
 *    no empiece por el tipo arranca un servidor que luego no se puede parar.
 *  - Solo se ofrecen los ajustes que devuelve `settings:get` (el intervalo de la
 *    foto, la retención de métricas y los del histórico de disco). El rango, el
 *    valor por defecto y la descripción vienen del backend: aquí no se inventa
 *    ninguno. Los que son de sí/no se pintan como casilla (`booleano`).
 *  - `settings:set` valida y rechaza con un mensaje ya redactado en español, así
 *    que ese texto se enseña TAL CUAL: reescribirlo perdería el detalle real.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { IconDeviceFloppy, IconTrash } from "@tabler/icons-react";
import { useApp, errorDe } from "../store";
import { api, type Ajustes, type Exclusiones, type GatewayEstado } from "../lib/tauri";
import { Boton, Card, Datos, Etiqueta, Insignia, Vacio } from "../components/ui";
import { ArranqueAutomatico, CarpetasModelos, PuertaEnRed } from "../components/Entorno";
import { Provision } from "../components/Provision";
import { fechaHora } from "../lib/format";

/** Tipos que el backend reconoce de serie: los de KNOWN_SERVERS (db.rs). */
const TIPOS_BACKEND = [
  "llama-swap",
  "llama-cpp",
  "ollama",
  "lmstudio",
  "exllama",
  "vllm",
  "tgwebui",
  "comfyui",
];

/**
 * Etiqueta y unidad de los ajustes conocidos. Los VALORES (efectivo, rango,
 * defecto) los manda el backend; aquí solo se pone cómo se llama cada clave en
 * la interfaz, porque `snapshot_interval_ms` no es algo que se enseñe tal cual.
 */
const META_AJUSTE: Record<string, { etiqueta: string; unidad: string; paso: number }> = {
  snapshot_interval_ms: { etiqueta: "Intervalo de la foto", unidad: "ms", paso: 250 },
  metric_retention_hours: { etiqueta: "Retención de métricas", unidad: "h", paso: 1 },
  historial_activo: { etiqueta: "Histórico de disco", unidad: "", paso: 1 },
  historial_umbral_gb: { etiqueta: "Aviso de crecimiento del hogar", unidad: "GB/semana", paso: 1 },
};

interface Borrador {
  cmd: string;
  enabled: boolean;
}

interface Aviso {
  tono: "ok" | "bad";
  texto: string;
}

const metaDe = (clave: string) =>
  META_AJUSTE[clave] ?? { etiqueta: clave, unidad: "", paso: 1 };

/* ── Exclusiones: lo que NO se mide ni se borra ───────────────────────────── */

/**
 * Las exclusiones no son un ajuste con número: van en su propia tarjeta porque el
 * backend las guarda aparte (`exclusiones` en SQLite) y porque hay que poder ver
 * QUÉ significan (`${HOME}/VMs` no se entiende solo).
 *
 * Se enseña el EFECTO, no solo la lista: lo excluido no se mide ni se borra, así
 * que un total más bajo en el analizador tiene que poder explicarse desde aquí.
 */
function BloqueExclusiones() {
  const [datos, setDatos] = useState<Exclusiones | null>(null);
  const [patron, setPatron] = useState("");
  const [aviso, setAviso] = useState<Aviso | null>(null);
  const [ocupado, setOcupado] = useState(false);

  const cargar = useCallback(async () => {
    try {
      setDatos(await api.exclusiones.listar());
    } catch (e) {
      setAviso({ tono: "bad", texto: String(e) });
    }
  }, []);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const accion = async (fn: () => Promise<string>) => {
    setOcupado(true);
    try {
      const m = await fn();
      setAviso({ tono: "ok", texto: m });
      setPatron("");
      await cargar();
    } catch (e) {
      setAviso({ tono: "bad", texto: String(e) });
    } finally {
      setOcupado(false);
    }
  };

  const anadir = () => void accion(() => api.exclusiones.anadir(patron.trim()));

  return (
    <section className="flex flex-col gap-3">
      <Etiqueta>Exclusiones ({datos?.vigentes.length ?? 0})</Etiqueta>
      <Card className="flex flex-col gap-3">
        <p className="text-fg-muted text-xs">
          Lo que excluyas <strong>no se mide ni se borra</strong>: vale para el analizador de disco
          (tamaños, repetidos, carpetas vacías, enlaces y búsqueda), para la limpieza y para el
          borrado. Se escribe una <strong>carpeta</strong> (<code className="mono">{"${HOME}/VMs"}</code>
          ), un <strong>patrón</strong> (<code className="mono">*.iso</code>) o un{" "}
          <strong>nombre</strong> (<code className="mono">node_modules</code>, que vale para
          cualquier carpeta con ese nombre).
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <input
            className="mono bg-raised border-line min-w-64 flex-1 rounded-md border px-2 py-1 text-xs"
            placeholder="${HOME}/VMs  ·  *.iso  ·  node_modules"
            aria-label="Carpeta o patrón que excluir"
            value={patron}
            onChange={(e) => setPatron(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && patron.trim() && !ocupado) anadir();
            }}
          />
          <Boton disabled={!patron.trim() || ocupado} onClick={anadir}>
            Excluir
          </Boton>
        </div>

        {aviso ? (
          <p
            className={aviso.tono === "ok" ? "text-fg-muted text-xs" : "text-bad text-xs"}
            role={aviso.tono === "ok" ? "status" : "alert"}
          >
            {aviso.texto}
          </p>
        ) : null}

        {datos && datos.vigentes.length === 0 ? (
          <Vacio titulo="No hay ninguna exclusión">
            De fábrica no hay ninguna: el analizador promete enseñar todo lo que ocupa, y traer
            carpetas ocultas haría que sus totales no cuadraran con los del gestor de archivos sin
            que se supiera por qué. Las raíces del sistema ya están protegidas aparte.
          </Vacio>
        ) : (
          <ul className="flex flex-col gap-1.5">
            {datos?.vigentes.map((v) => (
              <li
                key={v.patron}
                className="border-line-soft flex flex-wrap items-center gap-2 rounded-md border px-2 py-1.5"
              >
                <code className="mono text-accent">{v.patron}</code>
                {v.resuelta ? (
                  <span className="mono text-fg-faint text-xs">{v.resuelta}</span>
                ) : (
                  <Insignia tono="neutro">patrón</Insignia>
                )}
                <Boton
                  className="ml-auto"
                  variante="peligro"
                  disabled={ocupado}
                  onClick={() => void accion(() => api.exclusiones.quitar(v.patron))}
                >
                  <IconTrash size={13} aria-hidden="true" /> Quitar
                </Boton>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </section>
  );
}

export default function Ajustes() {
  const s = useApp((st) => st.snapshot);
  // La puerta se lee aquí para poder enseñar su dirección y su clave sin cambiar
  // de sección: es el dato que hay que copiar en los clientes.
  const [gateway, setGateway] = useState<GatewayEstado | null>(null);
  const cargarGateway = useCallback(async () => {
    try {
      setGateway(await api.gateway.estado());
    } catch {
      // Sin backend no hay puerta que leer; la tarjeta lo dirá con sus "—".
    }
  }, []);
  useEffect(() => {
    void cargarGateway();
  }, [cargarGateway]);
  const filas = useApp((st) => st.servidores);
  // Solo el error de `servers:list`, que es lo que lee esta lista.
  const error = useApp(errorDe("servidores"));
  const cargar = useApp((st) => st.cargarServidores);

  const [tipo, setTipo] = useState("");
  const [nombre, setNombre] = useState("");
  const [puerto, setPuerto] = useState("");
  const [ocupado, setOcupado] = useState(false);
  const [aviso, setAviso] = useState<{ tono: "ok" | "bad"; texto: string } | null>(null);
  const [borradores, setBorradores] = useState<Record<string, Borrador>>({});
  const [confirmarBorrado, setConfirmarBorrado] = useState<string | null>(null);

  const [ajustes, setAjustes] = useState<Ajustes | null>(null);
  const [borradoresAjustes, setBorradoresAjustes] = useState<Record<string, string>>({});
  const [errorAjustes, setErrorAjustes] = useState<string | null>(null);
  const [avisoAjuste, setAvisoAjuste] = useState<Aviso | null>(null);

  useEffect(() => { void cargar(); }, [cargar]);

  const cargarAjustes = async () => {
    try {
      const a = await api.settings.get();
      setAjustes(a);
      // El borrador arranca en el valor EFECTIVO: así "sin guardar" compara
      // siempre con lo que hay de verdad, no con lo que había al abrir.
      setBorradoresAjustes(
        Object.fromEntries(Object.entries(a).map(([clave, v]) => [clave, String(v.valor)])),
      );
      setErrorAjustes(null);
    } catch (e) {
      setErrorAjustes(String(e));
    }
  };

  useEffect(() => { void cargarAjustes(); }, []);

  const guardarAjuste = async (clave: string) => {
    const a = ajustes?.[clave];
    if (!a) return;
    const meta = metaDe(clave);
    const bruto = (borradoresAjustes[clave] ?? "").trim();
    const n = Number(bruto);
    if (bruto === "" || !Number.isFinite(n)) {
      setAvisoAjuste({ tono: "bad", texto: `"${bruto}" no es un número.` });
      return;
    }
    if (n < a.min || n > a.max) {
      setAvisoAjuste({
        tono: "bad",
        texto: `${meta.etiqueta} debe estar entre ${a.min} y ${a.max} ${meta.unidad}.`.trim(),
      });
      return;
    }
    setOcupado(true);
    try {
      await api.settings.set(clave, String(n));
      await cargarAjustes();
      setAvisoAjuste({
        tono: "ok",
        texto: `${meta.etiqueta}: guardado en ${n} ${meta.unidad}. Ya está en efecto.`,
      });
    } catch (e) {
      // Mensaje del backend, sin adornos: es el que explica qué ha fallado.
      setAvisoAjuste({ tono: "bad", texto: String(e) });
    } finally {
      setOcupado(false);
    }
  };

  // Los borradores parten de lo que hay en SQLite y se descartan cuando cambia
  // la lista (tras guardar): así "sin guardar" siempre compara con el backend.
  useEffect(() => {
    setBorradores((prev) => {
      const siguiente: Record<string, Borrador> = {};
      for (const r of filas) {
        const p = prev[r.id];
        siguiente[r.id] = {
          cmd: p && p.cmd !== (r.cmd ?? "") ? p.cmd : (r.cmd ?? ""),
          enabled: p && p.enabled !== r.enabled ? p.enabled : r.enabled,
        };
      }
      return siguiente;
    });
  }, [filas]);

  const idNuevo = useMemo(() => {
    const t = tipo.trim();
    const p = puerto.trim();
    return t && p ? `${t}:${p}` : "";
  }, [tipo, puerto]);

  const errorPuerto = puerto.trim() !== "" && !/^\d+$/.test(puerto.trim());
  const puedeAnadir =
    !ocupado && tipo.trim() !== "" && nombre.trim() !== "" && idNuevo !== "" && !errorPuerto;

  const alta = async () => {
    if (!puedeAnadir) return;
    setOcupado(true);
    setAviso(null);
    try {
      await api.servers.add({
        id: idNuevo,
        name: nombre.trim(),
        kind: tipo.trim(),
        port: Number(puerto.trim()),
      });
      await cargar();
      setAviso({ tono: "ok", texto: `Servidor ${idNuevo} dado de alta.` });
      setNombre("");
    } catch (e) {
      setAviso({ tono: "bad", texto: `No se pudo dar de alta: ${e}` });
    } finally {
      setOcupado(false);
    }
  };

  const guardar = async (id: string) => {
    const b = borradores[id];
    if (!b) return;
    setOcupado(true);
    setAviso(null);
    try {
      await api.servers.update(id, b.cmd, b.enabled);
      await cargar();
      setAviso({ tono: "ok", texto: `${id} actualizado.` });
    } catch (e) {
      setAviso({ tono: "bad", texto: `No se pudo actualizar ${id}: ${e}` });
    } finally {
      setOcupado(false);
    }
  };

  const borrar = async (id: string) => {
    setOcupado(true);
    setAviso(null);
    try {
      await api.servers.remove(id);
      await cargar();
      setAviso({ tono: "ok", texto: `${id} eliminado.` });
    } catch (e) {
      setAviso({ tono: "bad", texto: `No se pudo eliminar ${id}: ${e}` });
    } finally {
      setOcupado(false);
      setConfirmarBorrado(null);
    }
  };

  const sucio = (r: (typeof filas)[number]) => {
    const b = borradores[r.id];
    return !!b && (b.cmd !== (r.cmd ?? "") || b.enabled !== r.enabled);
  };

  return (
    <div className="flex flex-col gap-4">
      {/* El entorno va primero: es lo que se viene a configurar aquí, y afecta a
          toda la app (la puerta que cuenta el uso, el arranque y de dónde salen
          los modelos). Los servidores de motor van después, que es una lista que
          se toca de vez en cuando. */}
      <PuertaEnRed gw={gateway} onCambio={() => void cargarGateway()} />
      <div className="grid items-start gap-3 lg:grid-cols-2">
        <ArranqueAutomatico />
        <CarpetasModelos />
      </div>

      <section className="flex flex-col gap-3">
        <Etiqueta>Servidores configurados ({filas.length})</Etiqueta>
        {filas.length === 0 ? (
          error ? (
            <Vacio titulo="No se pudo leer la tabla de servidores">{error}</Vacio>
          ) : (
            <Card>
              <p className="text-fg-muted text-sm">
                La tabla servers está vacía. Da de alta uno abajo.
              </p>
            </Card>
          )
        ) : (
          <Card className="p-0">
            <ul className="flex flex-col">
              {filas.map((r) => {
                const b = borradores[r.id] ?? { cmd: r.cmd ?? "", enabled: r.enabled };
                return (
                  <li key={r.id} className="border-line-soft flex flex-col gap-2 border-b px-4 py-3 last:border-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <Insignia tono={r.enabled ? "ok" : "neutro"}>
                        {r.enabled ? "habilitado" : "deshabilitado"}
                      </Insignia>
                      <span className="text-sm font-medium">{r.name}</span>
                      <span className="mono text-fg-faint text-xs">{r.id} · puerto {r.port}</span>
                      <span className="ml-auto flex flex-wrap items-center gap-2">
                        {sucio(r) ? <Insignia tono="warn">sin guardar</Insignia> : null}
                        <Boton
                          variante="acento"
                          disabled={ocupado || !sucio(r)}
                          onClick={() => void guardar(r.id)}
                          aria-label={`Guardar los cambios de ${r.name}`}
                        >
                          <IconDeviceFloppy size={12} className="mr-1 inline" aria-hidden="true" />
                          Guardar
                        </Boton>
                        {confirmarBorrado === r.id ? (
                          <>
                            <Boton
                              variante="peligro"
                              disabled={ocupado}
                              onClick={() => void borrar(r.id)}
                              aria-label={`Confirmar la eliminación de ${r.name}`}
                            >
                              Sí, eliminar la fila
                            </Boton>
                            <Boton onClick={() => setConfirmarBorrado(null)}>Cancelar</Boton>
                          </>
                        ) : (
                          <Boton
                            variante="peligro"
                            title="Quita el servidor de la lista de esta app. No para el proceso ni borra nada suyo."
                            onClick={() => setConfirmarBorrado(r.id)}
                            aria-label={`Eliminar ${r.name} de la lista (pedirá confirmación)`}
                          >
                            <IconTrash size={12} className="mr-1 inline" aria-hidden="true" />
                            Eliminar…
                          </Boton>
                        )}
                      </span>
                    </div>

                    <label className="text-fg-faint flex flex-col gap-1 text-xs">
                      Comando de arranque
                      <input
                        value={b.cmd}
                        spellCheck={false}
                        autoComplete="off"
                        onChange={(e) =>
                          setBorradores((prev) => ({
                            ...prev,
                            [r.id]: { ...b, cmd: e.target.value },
                          }))
                        }
                        placeholder="Ollama lo toma por defecto; el resto necesita comando"
                        aria-label={`Comando de arranque de ${r.name}`}
                        className="border-line bg-raised mono w-full rounded-md border px-2 py-1 text-xs"
                      />
                    </label>

                    <label className="text-fg-muted flex items-center gap-2 text-xs">
                      <input
                        type="checkbox"
                        checked={b.enabled}
                        onChange={(e) =>
                          setBorradores((prev) => ({
                            ...prev,
                            [r.id]: { ...b, enabled: e.target.checked },
                          }))
                        }
                      />
                      Habilitado
                    </label>
                  </li>
                );
              })}
            </ul>
          </Card>
        )}
        <p className="text-fg-faint text-xs">
          El backend guarda la marca de habilitado pero todavía no filtra nada con ella: un servidor
          deshabilitado se puede arrancar igual desde Servidores. No lo escondemos, se dice.
        </p>
      </section>

      <section className="flex flex-col gap-3">
        <Etiqueta>Dar de alta un servidor</Etiqueta>
        <Card>
          <div className="grid gap-3 sm:grid-cols-3">
            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Tipo
              <input
                list="tipos-backend"
                value={tipo}
                spellCheck={false}
                autoComplete="off"
                onChange={(e) => setTipo(e.target.value)}
                placeholder="vllm"
                className="border-line bg-raised mono rounded-md border px-2 py-1 text-xs"
              />
            </label>
            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Nombre
              <input
                value={nombre}
                onChange={(e) => setNombre(e.target.value)}
                placeholder="vLLM"
                className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
              />
            </label>
            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Puerto
              <input
                value={puerto}
                inputMode="numeric"
                onChange={(e) => setPuerto(e.target.value)}
                placeholder="8000"
                aria-invalid={errorPuerto}
                className="border-line bg-raised mono rounded-md border px-2 py-1 text-xs"
              />
            </label>
          </div>
          <datalist id="tipos-backend">
            {TIPOS_BACKEND.map((t) => (
              <option key={t} value={t} />
            ))}
          </datalist>

          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Boton variante="acento" disabled={!puedeAnadir} onClick={() => void alta()}>
              Dar de alta
            </Boton>
            <span className="text-fg-faint text-xs">
              {errorPuerto
                ? "El puerto tiene que ser un número."
                : idNuevo
                  ? `Se registrará con id ${idNuevo} (el backend saca el tipo del prefijo del id).`
                  : "El id se compone como tipo:puerto."}
            </span>
          </div>
          <p className="text-fg-faint mt-2 text-xs">
            El comando de arranque se configura después, en la lista de arriba: el alta no lo acepta
            (el backend lo deja a nulo) y sin comando solo arranca Ollama, que lo tiene por defecto.
          </p>
          <p className="text-fg-faint mt-2 text-xs">
            Si ya existe un id igual, la operación lo REEMPLAZA y el comando guardado se pierde
            (el backend inserta con reemplazo y deja el comando a nulo).
          </p>
          {aviso ? (
            <p className={aviso.tono === "ok" ? "text-ok mt-2 text-xs" : "text-bad mt-2 text-xs"} role="status">
              {aviso.texto}
            </p>
          ) : null}
        </Card>
      </section>

      <section className="flex flex-col gap-3">
        <Etiqueta>Ajustes del backend</Etiqueta>
        <Card className="flex flex-col gap-4">
          {ajustes === null ? (
            <p className="text-fg-muted text-sm">
              {errorAjustes
                ? `No se pudieron leer los ajustes: ${errorAjustes}`
                : "Leyendo los ajustes…"}
            </p>
          ) : Object.keys(ajustes).length === 0 ? (
            <p className="text-fg-muted text-sm">
              El backend no declara ningún ajuste editable en esta versión.
            </p>
          ) : (
            Object.keys(ajustes).map((clave) => {
              const a = ajustes[clave];
              const meta = metaDe(clave);
              const borrador = borradoresAjustes[clave] ?? String(a.valor);
              const n = Number(borrador.trim());
              const fueraDeRango =
                borrador.trim() === "" || !Number.isFinite(n) || n < a.min || n > a.max;
              const sinCambios = borrador.trim() === String(a.valor);
              return (
                <div key={clave} className="border-line-soft border-b pb-4 last:border-0 last:pb-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <label htmlFor={`ajuste-${clave}`} className="text-fg text-xs">
                      {meta.etiqueta}
                    </label>
                    {a.booleano ? (
                      // Un sí/no se ofrece como casilla: pedir «0 o 1» a mano para
                      // un interruptor es una forma rara de pedirlo.
                      <label className="text-fg-muted flex items-center gap-2 text-xs">
                        <input
                          id={`ajuste-${clave}`}
                          type="checkbox"
                          checked={borrador.trim() === "1"}
                          onChange={(e) =>
                            setBorradoresAjustes((prev) => ({
                              ...prev,
                              [clave]: e.target.checked ? "1" : "0",
                            }))
                          }
                        />
                        {borrador.trim() === "1" ? "activado" : "apagado"}
                      </label>
                    ) : (
                      <input
                        id={`ajuste-${clave}`}
                        type="number"
                        inputMode="numeric"
                        min={a.min}
                        max={a.max}
                        step={meta.paso}
                        value={borrador}
                        onChange={(e) =>
                          setBorradoresAjustes((prev) => ({ ...prev, [clave]: e.target.value }))
                        }
                        aria-invalid={fueraDeRango}
                        className="border-line bg-raised mono w-28 rounded-md border px-2 py-1 text-xs"
                      />
                    )}
                    {!a.booleano ? <span className="text-fg-faint text-xs">{meta.unidad}</span> : null}
                    <span className="text-fg-muted text-xs">
                      {a.booleano
                        ? "se enciende y se apaga"
                        : `entre ${a.min} y ${a.max}${meta.unidad ? ` ${meta.unidad}` : ""}`}
                    </span>
                    <Insignia tono={a.guardado ? "acento" : "neutro"}>
                      {a.guardado
                        ? "valor guardado"
                        : a.booleano
                          ? `por defecto (${a.por_defecto === 1 ? "activado" : "apagado"})`
                          : `por defecto (${a.por_defecto}${meta.unidad ? ` ${meta.unidad}` : ""})`}
                    </Insignia>
                    <Boton
                      className="ml-auto"
                      variante="acento"
                      disabled={ocupado || sinCambios || fueraDeRango}
                      onClick={() => void guardarAjuste(clave)}
                      aria-label={`Guardar ${meta.etiqueta}`}
                    >
                      Guardar
                    </Boton>
                  </div>
                  <p className="text-fg-faint mt-1 text-xs">
                    {a.descripcion}. Valor en efecto ahora:{" "}
                    <span className="mono">
                      {a.booleano
                        ? a.valor === 1
                          ? "activado"
                          : "apagado"
                        : `${a.valor}${meta.unidad ? ` ${meta.unidad}` : ""}`}
                    </span>
                    .
                  </p>
                </div>
              );
            })
          )}
          {avisoAjuste ? (
            <p
              className={avisoAjuste.tono === "ok" ? "text-ok text-xs" : "text-bad text-xs"}
              role="status"
            >
              {avisoAjuste.texto}
            </p>
          ) : null}
          <p className="text-fg-faint text-xs">
            Todos tienen efecto de verdad: el intervalo se relee en cada vuelta del bucle de fotos,
            así que cambiarlo se nota sin reiniciar la aplicación; la retención decide cuántas horas
            de métricas se conservan en SQLite; y el histórico de disco decide si se mide tu carpeta
            personal (y la de modelos) una vez al día para poder comparar el crecimiento, que es lo
            que avisa en Inicio al pasar del umbral. El rango lo comprueba el backend: si el valor no
            vale, rechaza el cambio y aquí se lee su motivo.
          </p>
        </Card>
      </section>

      <BloqueExclusiones />

      <Provision />

      <section className="flex flex-col gap-3">
        <Etiqueta>Contexto de esta instalación</Etiqueta>
        <Card>
          <Datos
            items={[
              ["Base de datos", "~/.local/share/machinograph/data.db"],
              ["Modelos vigilados", "~/models (solo ficheros .gguf)"],
              ["Arranque del sistema", s ? fechaHora(s.boot) : "—"],
              ["Servidores en la foto", s ? String(s.servers.length) : "—"],
            ]}
          />
          <p className="text-fg-faint mt-3 text-xs">
            Esta parte es de solo lectura porque el backend la tiene fijada en el código: la carpeta
            de modelos vigilada no es configurable. Lo que sí se cambia desde aquí son los dos
            ajustes numéricos de arriba y la instalación automática de herramientas (en «Lo que AI
            Hub necesita»); el resto lo lee el backend tal cual.
          </p>
        </Card>
      </section>
    </div>
  );
}
