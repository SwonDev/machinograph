/**
 * Bases de datos SQLite: el espacio que hay DENTRO de tus bases y se puede
 * devolver sin borrar ni una fila.
 *
 * De dónde sale esto: Kudu tiene una categoría `databases.json` (MIT) que hace
 * `VACUUM` a las bases SQLite de navegadores, VS Code, Slack, Discord o
 * Thunderbird. La suya solo las lista. Aquí se MIDE antes y después, para poder
 * decir cuánto se ha recuperado DE VERDAD y no cuánto se estima.
 *
 * Tres cosas que no son de gusto, y que se ven en pantalla:
 *
 * 1. **Cada cifra dice de dónde sale.** «Ocupa» es `PRAGMA page_count ×
 *    page_size`; «Se recuperaría» es `PRAGMA freelist_count × page_size`, que es
 *    exactamente lo que devuelve un `VACUUM`; «en disco» es el fichero con `stat`
 *    (más su `-wal`, que gestiona SQLite). Una base que no se pudo medir enseña
 *    «—», nunca un 0: un 0 sería una medición falsa.
 * 2. **Compactar lo hacemos nosotros, pero solo si lo pides.** No hay que copiar
 *    ningún comando ni instalar `sqlite3`: el motor va dentro de la aplicación.
 *    Antes de tocar una base se prueba su bloqueo de escritura; si otra
 *    aplicación la tiene abierta, esa base NO se toca y se dice qué proceso la
 *    bloquea, con un botón para reintentar.
 * 3. **El aviso de cerrar la aplicación está a la vista**, no escondido en un
 *    desplegable: es la única condición que el usuario tiene que cumplir.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { IconAlertTriangle, IconRefresh } from "@tabler/icons-react";
import {
  api,
  type BaseCompactada,
  type BaseSqlite,
  type CompactacionBases,
  type EstadoBaseSqlite,
  type ListadoBases,
} from "../lib/tauri";
import { Boton, BotonCopiar, Card, Etiqueta, Insignia, Kpi, Th, Vacio } from "./ui";
import type { Tono } from "./ui";
import { bLegibles, num } from "../lib/format";

/** Lo que se manda compactar: la ruta y la aplicación (para decir quién la bloquea). */
type Peticion = { app: string; ruta: string };

/** Cómo se enseña el estado de una base que no se pudo medir. */
const MARCAS: Record<EstadoBaseSqlite, { texto: string; tono: Tono }> = {
  ok: { texto: "medida", tono: "neutro" },
  bloqueada: { texto: "bloqueada", tono: "warn" },
  sin_permiso: { texto: "sin permiso", tono: "warn" },
  error: { texto: "no se pudo leer", tono: "bad" },
};

/** ¿Esta base se puede intentar compactar? Las bloqueadas también: para reintentar. */
function intentable(b: BaseSqlite): boolean {
  if (b.estado === "bloqueada") return true;
  return b.estado === "ok" && (b.recuperable ?? 0) > 0;
}

/**
 * La letra pequeña del tamaño: lo que hay ADEMÁS de lo que dice el PRAGMA.
 *
 * `disco` es el fichero con `stat` (más su `-wal`, que gestiona SQLite). Cuando
 * hay WAL se dice él solo, para no sumar dos veces la misma cosa.
 */
function Disco({ b }: { b: BaseSqlite }) {
  if (b.wal_bytes > 0) {
    return <>WAL: {bLegibles(b.wal_bytes, 1)} (lo gestiona SQLite; el VACUUM no lo toca)</>;
  }
  if (b.disco != null && b.bytes != null && b.disco !== b.bytes) {
    return <>en disco: {bLegibles(b.disco, 1)}</>;
  }
  if (b.disco != null && b.bytes == null) {
    return <>en disco: {bLegibles(b.disco, 1)}</>;
  }
  return null;
}

/** El resultado de una base compactada (o del intento fallido). */
function ResultadoFila({ r }: { r: BaseCompactada }) {
  return (
    <li className="border-line-soft border-b py-1 last:border-0">
      <span className="text-fg text-xs">
        {r.app} <span className="mono text-fg-faint">{r.ruta}</span>
      </span>
      <span className="text-fg-muted block text-xs">
        {r.ok ? (
          <>
            recuperados {bLegibles(r.liberado ?? 0, 1)}
            {r.recuperable_antes != null && r.recuperable_despues != null ? (
              <span className="text-fg-faint">
                {" "}
                (páginas libres: {bLegibles(r.recuperable_antes, 1)} → {bLegibles(r.recuperable_despues, 1)})
              </span>
            ) : null}
          </>
        ) : (
          <>
            {MARCAS[r.estado].texto}
            {r.motivo ? `: ${r.motivo}` : ""}
            {r.bloqueantes.length > 0 ? ` — la tienen abierta: ${r.bloqueantes.join(", ")}` : ""}
          </>
        )}
      </span>
    </li>
  );
}

export function Bases() {
  const [datos, setDatos] = useState<ListadoBases | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);
  const [confirmando, setConfirmando] = useState<{ peticiones: Peticion[]; texto: string } | null>(null);
  const [enCurso, setEnCurso] = useState(false);
  const [resultado, setResultado] = useState<CompactacionBases | null>(null);

  const cargar = useCallback(async () => {
    setCargando(true);
    try {
      setDatos(await api.bases.listar());
      setError(null);
    } catch (e) {
      setError(String(e));
      setDatos(null);
    } finally {
      setCargando(false);
    }
  }, []);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const compactar = useCallback(
    async (peticiones: Peticion[]) => {
      setEnCurso(true);
      try {
        const inf = await api.bases.compactar(peticiones);
        setResultado(inf);
        setError(null);
        // Después de compactar se vuelve a MEDIR: lo de arriba pasa a ser lo que
        // queda, no lo que había.
        setDatos(await api.bases.listar());
      } catch (e) {
        setError(String(e));
      } finally {
        setEnCurso(false);
        setConfirmando(null);
      }
    },
    [],
  );

  const lista = datos?.bases ?? [];
  const recuperables = useMemo(
    () => lista.filter((b) => b.estado === "ok" && (b.recuperable ?? 0) > 0),
    [lista],
  );
  const pendientes = useMemo(() => lista.filter(intentable), [lista]);
  const hayAlgoQueHacer = pendientes.length > 0;

  if (datos == null) {
    return (
      <Card>
        <Etiqueta>{error ? "No se pudieron medir las bases de datos" : "Midiendo las bases de datos…"}</Etiqueta>
        {error ? (
          <p className="text-fg-muted mt-2 text-xs">{error}</p>
        ) : (
          <p className="text-fg-muted mt-2 text-xs">
            Se están leyendo los PRAGMA de las bases de las aplicaciones que documenta Kudu
            (navegadores, VS Code, Slack, Discord, Thunderbird…). Solo se leen: no se escribe nada.
          </p>
        )}
        {cargando ? null : (
          <div className="mt-3">
            <Boton onClick={() => void cargar()}>
              <IconRefresh size={13} aria-hidden="true" /> Volver a intentar
            </Boton>
          </div>
        )}
      </Card>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Kpi
          etiqueta="Se recuperaría"
          valor={bLegibles(datos.bytes_recuperables, 1)}
          // El plural se conjuga: «1 bases» no se escribe así.
          pie={
            recuperables.length === 1
              ? "1 base con páginas libres"
              : `${num(recuperables.length)} bases con páginas libres`
          }
        />
        <Kpi
          etiqueta="Bases encontradas"
          valor={num(datos.total)}
          pie={`${num(datos.medidas)} medidas · ${num(datos.errores)} ilegibles`}
        />
        <Kpi
          etiqueta="Bloqueadas"
          valor={num(datos.bloqueadas)}
          pie={datos.bloqueadas === 0 ? "ninguna en uso" : "cierra la aplicación y reintenta"}
        />
        <Kpi
          etiqueta="Ocupan"
          valor={bLegibles(datos.bytes_ocupados, 1)}
          pie={datos.wal_bytes > 0 ? `+ ${bLegibles(datos.wal_bytes, 1)} en ficheros -wal` : "medido con PRAGMA"}
        />
      </section>

      <Card className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <Etiqueta>Bases de datos: espacio recuperable sin borrar nada</Etiqueta>
          {cargando ? <Insignia tono="neutro">midiendo…</Insignia> : null}
          <Boton className="ml-auto" onClick={() => void cargar()} disabled={cargando || enCurso}>
            <IconRefresh size={13} aria-hidden="true" /> Volver a medir
          </Boton>
        </div>

        {/* El aviso importante, a la vista y no escondido: es la única condición
            que el usuario tiene que cumplir antes de compactar. */}
        <p className="text-warn flex items-start gap-1.5 text-xs" role="note">
          <IconAlertTriangle size={13} aria-hidden="true" className="mt-0.5 shrink-0" />
          <span>
            Antes de compactar, <strong className="text-fg">cierra las aplicaciones</strong> cuyos datos
            estén aquí dentro (navegador, VS Code, Slack, Discord, Thunderbird…). Si una base está en uso,
            Machinograph <strong className="text-fg">no la toca</strong> y te dice qué programa la tiene abierta.
            El <span className="mono">VACUUM</span> lo hace la propia aplicación (no hace falta{" "}
            <span className="mono">sqlite3</span>): reescribe la base, es atómico, no borra filas ni
            preferencias, y necesita algo de espacio libre temporal.
          </span>
        </p>

        {datos.truncado || datos.nota ? (
          <p className="text-fg-faint text-xs" role="status">
            {datos.nota}
          </p>
        ) : null}

        {datos.sin_traducir.length > 0 ? (
          <p className="text-fg-faint text-xs" role="note">
            {num(datos.sin_traducir.length)} objetivos del catálogo de Kudu no se pueden resolver en{" "}
            {datos.sistema} y no se han medido (no se inventa la ruta): {datos.sin_traducir.join(", ")}.
          </p>
        ) : null}

        {resultado ? (
          <div className="bg-raised border-line-soft rounded-md border p-2" role="status">
            <span className="text-fg text-xs">Se han recuperado {bLegibles(resultado.liberado, 1)} de verdad.</span>
            <span className="text-fg-muted text-xs"> {resultado.mensaje}</span>
            <ul className="mt-1">
              {resultado.resultados.map((r) => (
                <ResultadoFila key={r.ruta} r={r} />
              ))}
            </ul>
          </div>
        ) : null}

        {error ? (
          <p className="text-bad text-xs" role="alert">
            {error}
          </p>
        ) : null}

        {confirmando ? (
          <div className="border-line-soft flex flex-wrap items-center gap-2 border-t pt-3" role="alert">
            <span className="text-fg-muted text-xs">{confirmando.texto}</span>
            <Boton
              variante="acento"
              disabled={enCurso}
              onClick={() => void compactar(confirmando.peticiones)}
            >
              Sí, compactar
            </Boton>
            <Boton onClick={() => setConfirmando(null)} disabled={enCurso}>
              No
            </Boton>
          </div>
        ) : (
          <div className="border-line-soft flex flex-wrap items-center gap-2 border-t pt-3">
            <Boton
              variante="acento"
              disabled={!hayAlgoQueHacer || enCurso || cargando}
              onClick={() =>
                setConfirmando({
                  peticiones: [],
                  texto:
                    "Se compactarán TODAS las bases que tengan algo que recuperar (las que no estén en uso). " +
                    "Las que otra aplicación tenga abiertas se dejan como están y se dirá cuál es.",
                })
              }
            >
              Compactar las que se puedan
            </Boton>
            <span className="text-fg-faint text-xs">
              {hayAlgoQueHacer
                ? `${num(recuperables.length)} con páginas libres${datos.bloqueadas > 0 ? ` · ${num(datos.bloqueadas)} bloqueadas (para reintentar)` : ""}`
                : "no hay nada que compactar ahora mismo"}
            </span>
          </div>
        )}

        {lista.length === 0 ? (
          <Vacio titulo="No se ha encontrado ninguna base SQLite en este sistema">
            Kudu documenta bases dentro de Chrome, Firefox, Edge, Brave, VS Code, Cursor, Slack,
            Discord, Teams y Thunderbird. En este equipo no existe ninguna de esas rutas: no hay
            nada que medir, y no se enseña un 0 como si lo hubiera.
          </Vacio>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-xs">
              <caption className="sr-only">
                Bases SQLite del sistema, con lo que ocupa cada una y lo que recuperaría un VACUUM
              </caption>
              <thead>
                <tr className="border-line-soft border-b">
                  <Th>Aplicación</Th>
                  <Th>Base</Th>
                  <Th alineado="der">Ocupa</Th>
                  <Th alineado="der">Se recuperaría</Th>
                  <Th>Nota</Th>
                </tr>
              </thead>
              <tbody>
                {lista.map((b) => {
                  const marca = MARCAS[b.estado];
                  const libres = b.recuperable ?? 0;
                  return (
                    <tr key={b.ruta} className="border-line-soft hover:bg-raised align-top border-b last:border-0">
                      <td className="text-fg px-4 py-1.5">
                        {b.app}
                        {b.perfil ? <span className="text-fg-faint block">{b.perfil}</span> : null}
                      </td>
                      <td className="max-w-[520px] px-4 py-1.5">
                        <div className="mono text-fg-muted truncate" title={b.ruta}>
                          {b.ruta}
                        </div>
                        {b.journal === "wal" && b.wal_bytes === 0 ? (
                          <div className="text-fg-faint text-[11px]">
                            en modo WAL (el diario lo gestiona SQLite)
                          </div>
                        ) : null}
                        <div className="mt-1 flex items-center gap-2">
                          <code className="mono text-fg-faint bg-raised rounded px-1.5 py-0.5">{b.comando}</code>
                          <BotonCopiar texto={b.comando} que={`el comando para compactar ${b.ruta}`} />
                        </div>
                      </td>
                      <td className="mono px-4 py-1.5 text-right whitespace-nowrap">
                        {b.bytes == null ? <span className="text-fg-faint">—</span> : bLegibles(b.bytes, 1)}
                        <span className="text-fg-faint block text-[11px] font-normal">
                          <Disco b={b} />
                        </span>
                      </td>
                      <td className="mono px-4 py-1.5 text-right whitespace-nowrap">
                        {b.recuperable == null ? (
                          <span className="text-fg-faint">—</span>
                        ) : (
                          bLegibles(b.recuperable, 1)
                        )}
                      </td>
                      <td className="px-4 py-1.5">
                        <div className="flex flex-wrap items-center gap-1.5">
                          <Insignia tono={marca.tono}>{marca.texto}</Insignia>
                          {b.estado === "ok" && libres === 0 ? (
                            <Insignia tono="neutro">sin páginas libres</Insignia>
                          ) : null}
                          {b.auto_vacuum ? <Insignia tono="neutro">auto_vacuum {b.auto_vacuum}</Insignia> : null}
                        </div>
                        {b.nota ? <div className="text-fg-faint mt-0.5">{b.nota}</div> : null}
                        {intentable(b) ? (
                          <div className="mt-1">
                            <Boton
                              disabled={enCurso || cargando}
                              onClick={() =>
                                setConfirmando({
                                  peticiones: [{ app: b.app, ruta: b.ruta }],
                                  texto:
                                    b.estado === "bloqueada"
                                      ? `Se reintentará compactar ${b.ruta}. Si sigue en uso, no se tocará y se dirá qué programa la tiene abierta.`
                                      : `Se compactará ${b.ruta} (${bLegibles(libres, 1)} recuperables). Cierra antes ${b.app} si la tienes abierta.`,
                                })
                              }
                            >
                              {b.estado === "bloqueada" ? "Reintentar" : "Compactar"}
                            </Boton>
                          </div>
                        ) : null}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}

        <p className="text-fg-faint text-xs">
          Cifras medidas, no estimadas: «Ocupa» es <span className="mono">PRAGMA page_count × page_size</span>,
          «Se recuperaría» es <span className="mono">PRAGMA freelist_count × page_size</span> (lo que devuelve
          un <span className="mono">VACUUM</span>) y el tamaño en disco se lee con{" "}
          <span className="mono">stat</span>, sumando el <span className="mono">-wal</span> cuando existe. Lo
          que no se pudo medir enseña «—», no un 0.
        </p>
      </Card>
    </div>
  );
}
