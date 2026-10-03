/**
 * Mantenimiento: lo que se ha ejecutado.
 *
 * Fusión de las dos vistas que antes eran secciones propias ("Actualizaciones" y
 * "Registro"): las dos contestan a la misma pregunta —qué se ha lanzado y qué
 * contestó— y separarlas obligaba a mirar en dos sitios para reconstruir una
 * sesión. Aquí van en UNA página con cuatro bloques rotulados, en el orden en que
 * se usan: lanzar, ver lo lanzado desde aquí, ver lo que hizo la app y volver
 * atrás con lo que la app tocó (el Centro de recuperación).
 *
 * Tres fuentes distintas, y no se mezclan a propósito:
 *  - La SALIDA EN VIVO, que llega por `ai:update-line` y solo existe mientras la
 *    ventana esté abierta (la store la limita a 400 líneas).
 *  - El HISTORIAL de comandos (tabla `updates` de SQLite): sobrevive al reinicio.
 *  - El REGISTRO de acciones (tabla `actions`): lo que hizo la app por su cuenta.
 *
 * El campo de comando es de TEXTO LIBRE a propósito: el backend ejecuta lo que le
 * mandes (`update:run` con `shlex::split`), así que inventarse una lista de
 * "componentes oficiales" sería mentir sobre lo que la app sabe hacer.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { clsx } from "clsx";
import {
  IconArrowBackUp,
  IconPlayerPlay,
  IconRefresh,
  IconTerminal2,
  IconTrash,
} from "@tabler/icons-react";
import { useApp, cargandoDe, ejecutar, errorDe, type ResultadoAccion } from "../store";
import { api, type CopiaRow, type FuenteActualizacion } from "../lib/tauri";
import { Boton, BotonCopiar, Card, Etiqueta, Insignia, Th, Vacio } from "../components/ui";
import { bLegibles, fechaHora, hora } from "../lib/format";

/* ── Bloque 1: lanzar un comando y ver su salida ──────────────────────────── */

function LanzarComando() {
  const linea = useApp((st) => st.lineaAccion);
  const enCurso = useApp((st) => st.accionEnCurso);
  const [cmd, setCmd] = useState("");

  const lanzar = () => {
    const limpio = cmd.trim();
    if (!limpio) return;
    void ejecutar("update:run", { cmd: limpio });
  };

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <IconTerminal2 size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Ejecutar un comando</Etiqueta>
      </div>
      <Card>
        <form
          className="flex flex-wrap items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            lanzar();
          }}
        >
          <label htmlFor="cmd-mantenimiento" className="text-fg-muted text-xs">
            Comando
          </label>
          <input
            id="cmd-mantenimiento"
            value={cmd}
            onChange={(e) => setCmd(e.target.value)}
            placeholder="p. ej. flatpak update -y"
            spellCheck={false}
            autoComplete="off"
            className="border-line bg-raised mono min-w-[220px] flex-1 rounded-md border px-2 py-1 text-xs"
          />
          <Boton variante="acento" type="submit" disabled={!!enCurso || cmd.trim().length === 0}>
            <IconPlayerPlay size={12} className="mr-1 inline" aria-hidden="true" />
            Ejecutar
          </Boton>
        </form>
        <p className="text-fg-faint mt-2 text-xs">
          Se ejecuta sin shell intermedia, así que los encadenamientos con <code className="mono">|</code> o{" "}
          <code className="mono">&amp;&amp;</code> no funcionan: lanza un comando, no una receta. Cada ejecución
          queda registrada abajo.
        </p>
      </Card>

      <Card className="p-0">
        <div className="border-line-soft flex items-center gap-2 border-b px-3 py-2">
          <Etiqueta>{enCurso ? `Salida · ${enCurso}` : "Salida del comando en curso"}</Etiqueta>
          {enCurso ? <Insignia tono="acento">en curso</Insignia> : null}
        </div>
        <pre
          aria-live="polite"
          aria-label="Salida del comando en curso"
          className="mono max-h-64 min-h-24 overflow-auto p-3 text-xs leading-relaxed"
        >
          {linea.length > 0 ? linea.join("\n") : "Sin salida todavía."}
        </pre>
      </Card>
    </section>
  );
}

/* ── Bloque 2: los comandos lanzados desde aquí ───────────────────────────── */

function HistorialComandos() {
  const ups = useApp((st) => st.actualizaciones);
  const cargar = useApp((st) => st.cargarActualizaciones);
  // Solo el error de ESTA lectura (`updates:recent`), no el de cualquier otra.
  const error = useApp(errorDe("actualizaciones"));
  const [abierta, setAbierta] = useState<number | null>(null);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  // Se selecciona por índice y no por marca de tiempo: dos ejecuciones en el
  // mismo segundo comparten `ts` y enseñarían la salida equivocada.
  const recientes = useMemo(() => ups.slice(0, 50), [ups]);
  const seleccionada = abierta != null ? (recientes[abierta] ?? null) : null;

  return (
    <section className="flex flex-col gap-3">
      {/* El recuento se omite si no hay filas: un "(0)" junto a un error de
          lectura se lee como "no hay nada registrado", y eso no lo sabemos. */}
      <Etiqueta>{ups.length > 0 ? `Comandos ejecutados (${ups.length})` : "Comandos ejecutados"}</Etiqueta>

      {/* Tres estados, no dos: vacío de verdad (leyó y no hay nada) es distinto
          de "la lectura falló", que es lo que decía esta tarjeta cuando el
          backend no estaba disponible. */}
      {ups.length === 0 ? (
        error ? (
          <Vacio titulo="No se pudo leer el historial de comandos">{error}</Vacio>
        ) : (
          <Card>
            <p className="text-fg-muted text-sm">
              Todavía no se ha ejecutado nada desde la app. Lo que lances arriba quedará registrado con su
              salida.
            </p>
          </Card>
        )
      ) : (
        <>
          {/* Con filas de una lectura anterior se enseña la tabla, pero también
              el aviso de que ya no es fresca. */}
          {error ? (
            <Card className="border-bad/40">
              <p className="text-bad text-sm">
                Falló la última lectura del historial; esto es lo último que se pudo leer.
              </p>
              <p className="text-bad mt-1 text-sm" role="alert">
                {error}
              </p>
            </Card>
          ) : null}

          <Card className="overflow-x-auto p-0">
            <table className="w-full min-w-[560px] text-left text-xs">
              <thead className="text-fg-faint border-line-soft border-b">
                <tr>
                  <th scope="col" className="px-4 py-2 font-medium">Cuándo</th>
                  <th scope="col" className="px-4 py-2 font-medium">Comando</th>
                  <th scope="col" className="px-4 py-2 font-medium">Resultado</th>
                  <th scope="col" className="px-4 py-2 text-right font-medium">Salida</th>
                </tr>
              </thead>
              <tbody>
                {recientes.map((u, i) => (
                  <tr key={`${u.ts}-${i}`} className="border-line-soft hover:bg-raised border-b last:border-0">
                    <td className="mono px-4 py-1.5 whitespace-nowrap">{fechaHora(u.ts)}</td>
                    <td className="mono max-w-[320px] truncate px-4 py-1.5" title={u.cmd}>
                      {u.cmd}
                    </td>
                    <td className="px-4 py-1.5">
                      <Insignia tono={u.ok ? "ok" : "bad"}>
                        {u.ok ? "ok" : `fallo · código ${u.code ?? "señal"}`}
                      </Insignia>
                    </td>
                    <td className="px-4 py-1.5 text-right">
                      <Boton
                        onClick={() => setAbierta(abierta === i ? null : i)}
                        aria-expanded={abierta === i}
                        aria-label={`${abierta === i ? "Ocultar" : "Ver"} la salida de ${u.cmd}`}
                      >
                        {abierta === i ? "Ocultar" : "Ver"}
                      </Boton>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>
        </>
      )}

      {seleccionada ? (
        <Card className="p-0">
          <div className="border-line-soft flex flex-wrap items-center gap-2 border-b px-4 py-2">
            <Etiqueta>Salida de {seleccionada.cmd}</Etiqueta>
            <span className="text-fg-faint mono ml-auto text-xs">{fechaHora(seleccionada.ts)}</span>
          </div>
          <pre className="mono max-h-64 overflow-auto p-3 text-xs leading-relaxed">
            {seleccionada.output.trim() || "El backend no guardó salida de esta ejecución."}
          </pre>
        </Card>
      ) : null}
    </section>
  );
}

/* ── Bloque 3: lo que hizo la app ─────────────────────────────────────────── */

function RegistroAcciones() {
  const acciones = useApp((st) => st.acciones);
  const cargar = useApp((st) => st.cargarAcciones);
  // Solo el error de ESTA lectura (`actions:recent`): si lo que falló fue otra
  // cosa, este bloque no tiene por qué decirlo.
  const error = useApp(errorDe("acciones"));
  // Los filtros viven en la tienda: al cambiar de sección esta vista se desmonta
  // y un `useState` local se perdería, así que al volver aparecerían en blanco.
  const ui = useApp((st) => st.ui.log);
  const setUi = useApp((st) => st.setUi);
  const { resultado, tipo } = ui;

  useEffect(() => {
    void cargar();
  }, [cargar]);

  // El backend ya devuelve las más nuevas primero (ORDER BY ts DESC), pero el
  // orden se fija aquí también porque la cabecera lo promete.
  const ordenadas = useMemo(() => [...acciones].sort((a, b) => b.ts - a.ts), [acciones]);

  const tipos = useMemo(() => [...new Set(ordenadas.map((a) => a.kind))].sort(), [ordenadas]);

  const visibles = useMemo(
    () =>
      ordenadas.filter((a) => {
        if (resultado === "ok" && !a.ok) return false;
        if (resultado === "fallo" && a.ok) return false;
        if (tipo !== "todos" && a.kind !== tipo) return false;
        return true;
      }),
    [ordenadas, resultado, tipo],
  );

  const fallos = ordenadas.filter((a) => !a.ok).length;

  return (
    <section className="flex flex-col gap-3">
      {/* Los filtros solo sirven si hay algo que filtrar: con la lista vacía no
          se pintan (así no aparece un "0 de 0 acciones" al lado de un error). */}
      {ordenadas.length > 0 ? (
        <div className="flex flex-wrap items-center gap-3">
          <Etiqueta>
            Acciones de la app ({visibles.length} de {ordenadas.length} · {fallos} con fallo)
          </Etiqueta>
          <label className="text-fg-muted ml-auto flex items-center gap-2 text-xs">
            Resultado
            <select
              value={resultado}
              onChange={(e) => setUi("log", { resultado: e.target.value as "todos" | "ok" | "fallo" })}
              className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
            >
              <option value="todos">todos</option>
              <option value="ok">solo ok</option>
              <option value="fallo">solo fallos</option>
            </select>
          </label>
          <label className="text-fg-muted flex items-center gap-2 text-xs">
            Acción
            <select
              value={tipo}
              onChange={(e) => setUi("log", { tipo: e.target.value })}
              aria-label="Filtrar por tipo de acción"
              className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
            >
              <option value="todos">todas</option>
              {tipos.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
          </label>
          {resultado !== "todos" || tipo !== "todos" ? (
            <Boton onClick={() => setUi("log", { resultado: "todos", tipo: "todos" })}>Limpiar filtros</Boton>
          ) : null}
        </div>
      ) : (
        <Etiqueta>Acciones de la app</Etiqueta>
      )}

      {ordenadas.length === 0 ? (
        error ? (
          <Vacio titulo="No se pudo leer el registro de acciones">{error}</Vacio>
        ) : (
          <Card>
            <p className="text-fg-muted text-sm">
              Todavía no hay acciones registradas. Las que se lancen desde la app (arrancar un servidor, matar
              un proceso, calcular un encaje) aparecerán aquí.
            </p>
          </Card>
        )
      ) : (
        <>
          {error ? (
            <Card className="border-bad/40">
              <p className="text-bad text-sm">
                Falló la última lectura del registro; esto es lo último que se pudo leer.
              </p>
              <p className="text-bad mt-1 text-sm" role="alert">
                {error}
              </p>
            </Card>
          ) : null}

          <Card className="overflow-x-auto p-0">
            {visibles.length === 0 ? (
              <p className="text-fg-muted p-4 text-sm">
                Ninguna acción coincide con los filtros. Prueba con “todos”.
              </p>
            ) : (
              <table className="w-full min-w-[560px] text-left text-xs">
                <thead className="text-fg-faint border-line-soft border-b">
                  <tr>
                    <th scope="col" className="px-4 py-2 font-medium">Hora</th>
                    <th scope="col" className="px-4 py-2 font-medium">Acción</th>
                    <th scope="col" className="px-4 py-2 font-medium">Detalle</th>
                    <th scope="col" className="px-4 py-2 font-medium">Resultado</th>
                  </tr>
                </thead>
                <tbody>
                  {visibles.slice(0, 200).map((a, i) => (
                    <tr
                      key={`${a.ts}-${a.kind}-${i}`}
                      className="border-line-soft hover:bg-raised border-b last:border-0"
                    >
                      <td className="mono text-fg-faint px-4 py-1.5 whitespace-nowrap">{hora(a.ts)}</td>
                      <td className="mono px-4 py-1.5 whitespace-nowrap">{a.kind}</td>
                      <td className="text-fg-muted max-w-[280px] truncate px-4 py-1.5" title={a.detail}>
                        {a.detail || "—"}
                      </td>
                      <td className="px-4 py-1.5">
                        {a.ok ? <Insignia tono="ok">ok</Insignia> : <Insignia tono="bad">fallo</Insignia>}
                        {!a.ok && a.message ? <span className="text-bad ml-2">{a.message}</span> : null}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Card>
        </>
      )}
    </section>
  );
}

/* ── Bloque 4: el Centro de recuperación ──────────────────────────────────── */

/**
 * Las copias de los ficheros que la app ha TOCADO, y cómo volver atrás.
 *
 * POR QUÉ ESTÁ AQUÍ: es la otra cara de «lo que ha hecho la app». Machinograph escribe en
 * ficheros que no son suyos (la configuración de un cliente de IA, una entrada de
 * arranque) y siempre hace una copia antes; esto es donde se ven y se restauran.
 * Es el Centro de recuperación de Kudu, con la regla de esta casa: la copia vive AL
 * LADO del original (`config.json.bak-20261003-130501`) porque es donde uno la
 * busca con un gestor de archivos, y aquí está el índice para no tener que
 * recorrer el disco.
 *
 * Restaurar **también** es reversible: antes de pisar el original se copia lo que
 * hay ahora, y el mensaje lo dice.
 */
function BloqueCopias() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("copias"));
  const error = useApp(errorDe("copias"));
  const enCurso = useApp((st) => st.accionEnCurso);

  const [copias, setCopias] = useState<CopiaRow[] | null>(null);
  // Restaurar pide confirmación porque pisa un fichero; borrar una copia, no (no
  // toca el original).
  const [confirmando, setConfirmando] = useState<number | null>(null);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  const cargar = useCallback(async () => {
    setCargando("copias", true);
    try {
      setCopias(await api.copias.listar());
      limpiarError("copias");
    } catch (e) {
      setError("copias", String(e));
      setCopias(null);
    } finally {
      setCargando("copias", false);
    }
  }, [setCargando, setError, limpiarError]);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const restaurar = async (id: number) => {
    const r = await ejecutar("copias:restaurar", { id });
    setResultado(r);
    setConfirmando(null);
    await cargar();
  };

  const borrar = async (id: number) => {
    const r = await ejecutar("copias:borrar", { id });
    setResultado(r);
    await cargar();
  };

  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>Centro de recuperación</Etiqueta>
        <Boton className="ml-auto" onClick={() => void cargar()} disabled={cargando || !!enCurso}>
          <IconRefresh size={13} aria-hidden="true" /> Volver a leer
        </Boton>
      </div>
      <p className="text-fg-muted text-xs">
        Cada vez que Machinograph escribe en un fichero que no es suyo (la configuración de un cliente de
        IA, una entrada de arranque) deja una copia <span className="mono">.bak-</span> al lado del
        original. Aquí se ven y se restauran.
      </p>

      {resultado ? (
        <p className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")} role={resultado.ok ? "status" : "alert"}>
          {resultado.mensaje}
        </p>
      ) : null}

      {copias == null ? (
        <Vacio titulo={error ? "No se pudieron leer las copias" : "Leyendo las copias…"}>
          {error ?? null}
        </Vacio>
      ) : copias.length === 0 ? (
        <Vacio titulo="Todavía no hay copias">
          Aparecerán aquí en cuanto la app toque un fichero de otro programa (conectar un cliente,
          activar o desactivar el arranque).
        </Vacio>
      ) : (
        <Card className="overflow-x-auto p-0">
          <table className="w-full text-left text-xs">
            <caption className="sr-only">Copias de seguridad de los ficheros que ha modificado Machinograph</caption>
            <thead>
              <tr className="border-line-soft border-b">
                <Th>Cuándo</Th>
                <Th>Fichero</Th>
                <Th>Motivo</Th>
                <Th alineado="der">Estado</Th>
                <Th alineado="der">Acciones</Th>
              </tr>
            </thead>
            <tbody>
              {copias.map((c) => (
                <tr key={c.id} className="border-line-soft border-b last:border-0">
                  <td className="text-fg-faint px-4 py-1.5 whitespace-nowrap">{fechaHora(c.ts)}</td>
                  <td className="mono max-w-[320px] truncate px-4 py-1.5" title={`${c.ruta_original} → ${c.ruta_copia}`}>
                    {c.ruta_original}
                  </td>
                  <td className="text-fg-muted px-4 py-1.5">{c.motivo}</td>
                  <td className="px-4 py-1.5 text-right">
                    <Insignia tono={c.existe ? "neutro" : "warn"}>
                      {c.existe ? bLegibles(c.bytes, 1) : "la copia ya no está"}
                    </Insignia>
                  </td>
                  <td className="px-4 py-1.5">
                    <div className="flex flex-nowrap items-center justify-end gap-1.5">
                      {confirmando === c.id ? (
                        <>
                          <Boton
                            variante="peligro"
                            disabled={!!enCurso}
                            onClick={() => void restaurar(c.id)}
                            aria-label={`Sí, restaurar ${c.ruta_original}`}
                          >
                            Sí, restaurar
                          </Boton>
                          <Boton onClick={() => setConfirmando(null)}>No</Boton>
                        </>
                      ) : (
                        <>
                          <Boton
                            disabled={!!enCurso || !c.existe}
                            onClick={() => setConfirmando(c.id)}
                            title="Devuelve el fichero a como estaba en la copia (antes se guarda lo que hay ahora)"
                            aria-label={`Restaurar ${c.ruta_original}`}
                          >
                            <IconArrowBackUp size={13} aria-hidden="true" /> Restaurar
                          </Boton>
                          <Boton
                            variante="peligro"
                            disabled={!!enCurso}
                            onClick={() => void borrar(c.id)}
                            title="Quita esta copia (el fichero original no se toca)"
                            aria-label={`Borrar la copia de ${c.ruta_original}`}
                          >
                            <IconTrash size={13} aria-hidden="true" />
                          </Boton>
                        </>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
      {cargando ? <p className="text-fg-faint text-xs">Leyendo…</p> : null}
    </section>
  );
}

/* ── Bloque 5: las actualizaciones ────────────────────────────────────────── */

/**
 * Qué está desactualizado, según la herramienta de ESTE sistema.
 *
 * POR QUÉ NO HAY UN «BUSCAR ACTUALIZACIONES» PROPIO: cada sistema tiene su
 * herramienta (rpm-ostree en un Fedora atómico, flatpak, brew, winget,
 * softwareupdate) y esa es la que sabe qué hay y de dónde se baja. Machinograph ejecuta
 * SU comprobación, la traduce a una lista y enseña el comando exacto que la
 * aplica. No hay canal de versiones propio ni se baja nada por su cuenta.
 *
 * Y no se comprueba sola al abrir: cada comprobación puede tardar y algunas
 * consultan su repositorio (brew tarda bastante), así que se pide con el botón.
 */
function BloqueActualizaciones() {
  const setError = useApp((st) => st.setError);
  const limpiarError = useApp((st) => st.limpiarError);
  const setCargando = useApp((st) => st.setCargando);
  const cargando = useApp(cargandoDe("actualizar"));
  const enCurso = useApp((st) => st.accionEnCurso);

  const [fuentes, setFuentes] = useState<FuenteActualizacion[] | null>(null);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  const comprobar = useCallback(async () => {
    setCargando("actualizar", true);
    setResultado(null);
    try {
      setFuentes(await api.actualizar.comprobar());
      limpiarError("actualizar");
    } catch (e) {
      setError("actualizar", String(e));
      setFuentes(null);
    } finally {
      setCargando("actualizar", false);
    }
  }, [setCargando, setError, limpiarError]);

  const pendientes = (fuentes ?? []).reduce((a, f) => a + f.actualizaciones.length, 0);
  const disponibles = (fuentes ?? []).filter((f) => f.disponible);

  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>Actualizaciones</Etiqueta>
        {fuentes ? (
          <Insignia tono={pendientes > 0 ? "warn" : "ok"}>
            {pendientes > 0 ? `${pendientes} pendientes` : "todo al día"}
          </Insignia>
        ) : null}
        <Boton className="ml-auto" onClick={() => void comprobar()} disabled={cargando || !!enCurso}>
          <IconRefresh size={13} aria-hidden="true" /> {fuentes ? "Volver a comprobar" : "Comprobar ahora"}
        </Boton>
      </div>

      {fuentes == null ? (
        <Vacio titulo={cargando ? "Comprobando…" : "Sin comprobar todavía"}>
          {cargando
            ? "Se está preguntando a la herramienta de este sistema. Puede tardar: alguna consulta su repositorio."
            : "Pulsa «Comprobar ahora». Se usa la herramienta de actualizaciones de tu sistema (no un canal propio)."}
        </Vacio>
      ) : (
        <div className="flex flex-col gap-3">
          {disponibles.length === 0 ? (
            <Vacio titulo="No hay ninguna herramienta de actualizaciones reconocida">
              En este sistema no se ha encontrado rpm-ostree, flatpak, brew, winget, dnf, apt,
              pacman, zypper ni softwareupdate.
            </Vacio>
          ) : null}
          {disponibles.map((f) => (
            <Card key={f.id} className="flex flex-col gap-2">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-fg text-xs font-medium">{f.nombre}</span>
                <Insignia tono={f.actualizaciones.length > 0 ? "warn" : "neutro"}>
                  {f.actualizaciones.length > 0 ? `${f.actualizaciones.length} pendientes` : "al día"}
                </Insignia>
                {f.requiere_root ? <Insignia tono="neutro">necesita root</Insignia> : null}
                {f.requiere_reinicio ? <Insignia tono="warn">aplica al reiniciar</Insignia> : null}
                <BotonCopiar className="ml-auto" texto={f.comando_aplicar} que={`el comando de ${f.nombre}`} />
                <Boton
                  disabled={!!enCurso || f.actualizaciones.length === 0}
                  onClick={async () => {
                    const r = await ejecutar("update:run", { cmd: f.comando_aplicar });
                    setResultado(r);
                  }}
                  title="Ejecuta el comando; su salida sale en el primer bloque de esta página"
                  aria-label={`Ejecutar ${f.comando_aplicar}`}
                >
                  <IconPlayerPlay size={13} aria-hidden="true" /> Ejecutar
                </Boton>
              </div>
              {f.actualizaciones.length > 0 ? (
                <ul className="text-fg-muted mono grid gap-0.5 text-xs">
                  {f.actualizaciones.slice(0, 20).map((a, i) => (
                    <li key={`${f.id}-${i}`} className="truncate" title={a}>
                      {a}
                    </li>
                  ))}
                  {f.actualizaciones.length > 20 ? (
                    <li className="text-fg-faint">… y {f.actualizaciones.length - 20} más</li>
                  ) : null}
                </ul>
              ) : null}
              <p className="text-fg-faint mono truncate text-xs" title={f.comando_aplicar}>
                se aplica con: {f.comando_aplicar}
              </p>
              {f.nota ? (
                <p className="text-warn text-xs" role="status">
                  Aviso de la herramienta: {f.nota}
                </p>
              ) : null}
              {f.error ? <p className="text-bad text-xs">{f.error}</p> : null}
            </Card>
          ))}
        </div>
      )}

      {resultado ? (
        <p className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")} role={resultado.ok ? "status" : "alert"}>
          {resultado.mensaje} — la salida completa está en «Lanzar un comando», arriba.
        </p>
      ) : null}
    </section>
  );
}

/* ── La página ────────────────────────────────────────────────────────────── */

export default function Mantenimiento() {
  return (
    <div className="flex flex-col gap-6">
      <LanzarComando />
      <BloqueActualizaciones />
      <HistorialComandos />
      <RegistroAcciones />
      <BloqueCopias />
    </div>
  );
}
