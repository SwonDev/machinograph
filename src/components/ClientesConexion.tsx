/**
 * Clientes conectados: quién más habla con el motor local, y su evidencia.
 *
 * POR QUÉ ESTE PANEL DICE LO QUE DICE
 * ---------------------------------------------------------------------------
 * Machinograph NO escribe en la configuración de otros clientes. Esto solo DETECTA
 * qué hay y GENERA el texto de una propuesta; revisarlo y pegarlo es cosa del
 * usuario. No es una limitación por no haberlo hecho: reescribir ficheros de
 * configuración ajenos (con sus claves, su formato y sus comentarios) puede
 * romperlos sin que nadie se entere, así que el aviso va ARRIBA y bien visible,
 * no como nota al pie.
 *
 * La otra regla del panel: `como_lo_tiene` son LÍNEAS REALES del fichero del
 * cliente. Son la PRUEBA de lo que se afirma, así que se pueden leer enteras y
 * se enseñan tal cual, sin resumir.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { IconCopy, IconDeviceFloppy, IconInfoCircle, IconPlugConnected } from "@tabler/icons-react";
import { cargandoDe, errorDe, useApp } from "../store";
import { api, type AplicadoConexion, type ClienteConexion, type PropuestaConexion } from "../lib/tauri";
import { Boton, Card, Datos, Etiqueta, Insignia, Vacio } from "../components/ui";

/** Valor de fábrica del endpoint local: el motor de llama-swap. */
const ENDPOINT_DEFECTO = "http://127.0.0.1:8080/v1";

/**
 * Formulario + bloque generado para UN cliente.
 *
 * Dos pasos, a propósito: `Generar bloque` NO escribe nada —pide al backend el
 * fichero final tal cual quedaría, para poder revisarlo— y `Escribir…` es el
 * único botón que toca el disco, pide confirmación y solo aparece donde el
 * formato está comprobado. Lo que se revisa es byte a byte lo que se escribe:
 * las dos llamadas llevan los mismos argumentos.
 */
function FormularioPropuesta({ cliente }: { cliente: ClienteConexion }) {
  const uid = (campo: string) => `prop-${cliente.id}-${campo}`;
  const s = useApp((st) => st.snapshot);
  const [id, setId] = useState("local");
  const [nombre, setNombre] = useState("Local (llama-swap)");
  const [endpoint, setEndpoint] = useState(ENDPOINT_DEFECTO);
  const [apiKind, setApiKind] = useState("openai-completions");
  /**
   * Los modelos: `null` = todavía no se ha escrito nada, y entonces se enseñan
   * los que PUBLICa el servidor que está en marcha (son los que existen de
   * verdad). En cuanto el usuario escribe, manda lo suyo. Sin esto, el primer
   * clic en «Generar bloque» no tendría de dónde sacarlos.
   */
  const [modelosEscritos, setModelosEscritos] = useState<string | null>(null);
  /**
   * Lo que se propone por defecto: lo que SU fichero ya declara más lo que
   * publica el servidor, sin repetir.
   *
   * El primero es imprescindible: proponer solo lo del servidor hacía que
   * aplicar la propuesta tal cual quitara de su configuración los modelos que el
   * servidor no anuncia (pasó en este equipo con uno que ya estaba declarado).
   * El orden pone delante lo que ya tenía, porque es lo que no se puede perder.
   */
  const porDefecto = useMemo(() => {
    const out: string[] = [...cliente.modelos_declarados];
    const vistos = new Set(out);
    const delServidor: string[] = [];
    for (const sv of s?.servers ?? []) {
      for (const m of sv.models) {
        if (!vistos.has(m.id)) {
          vistos.add(m.id);
          delServidor.push(m.id);
        }
      }
    }
    return [...out, ...delServidor].join(", ");
  }, [s, cliente.modelos_declarados]);
  const modelos = modelosEscritos ?? porDefecto;
  const [propuesta, setPropuesta] = useState<PropuestaConexion | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [generando, setGenerando] = useState(false);
  const [copiado, setCopiado] = useState<"ok" | "manual" | null>(null);
  const bloque = useRef<HTMLPreElement | null>(null);
  /** Paso de la escritura: `null` = no se ha pedido, `confirmando` = esperando el sí. */
  const [escribiendo, setEscribiendo] = useState<"confirmando" | "aplicando" | null>(null);
  const [aplicado, setAplicado] = useState<AplicadoConexion | null>(null);
  const [errorEscritura, setErrorEscritura] = useState<string | null>(null);
  /**
   * El foco se lleva AL BOTÓN de confirmar en cuanto aparece.
   *
   * Por qué: al pulsar «Escribir…» el botón se sustituye por el bloque de
   * confirmación, y el elemento que tenía el foco desaparece del árbol. Medido en
   * la app real: el foco se quedaba en nada (sin anillo visible) y había que
   * tabular para llegar al botón, que es justo lo que no puede pasar en un paso
   * que ESCRIBE en un fichero.
   */
  const botonConfirmar = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (escribiendo === "confirmando") botonConfirmar.current?.focus();
  }, [escribiendo]);

  /**
   * Los argumentos, en un solo sitio: la propuesta y la escritura tienen que
   * llevar EXACTAMENTE lo mismo, o lo que se revisa no sería lo que se escribe.
   * Los modelos se escriben sueltos ("a b, c") y se parten por comas o espacios.
   */
  const argumentos = () => ({
    cliente: cliente.id,
    id,
    nombre,
    endpoint,
    api: apiKind,
    modelos: modelos.split(/[\s,]+/).filter(Boolean),
  });

  const generar = async () => {
    setGenerando(true);
    setCopiado(null);
    // Lo que se estaba revisando ya no describe lo que hay en pantalla: si se
    // cambia el formulario y se vuelve a generar, el resultado anterior se cae
    // (y con él, el botón de escribir, que escribiría OTRA cosa).
    setAplicado(null);
    setErrorEscritura(null);
    setEscribiendo(null);
    try {
      const p = await api.conexiones.propuesta(argumentos());
      setPropuesta(p);
      setError(null);
    } catch (e) {
      // El backend redacta el motivo (cliente sin propuesta, formato raro…): se
      // enseña literal y se retira el bloque anterior, que ya no describe esto.
      setError(String(e));
      setPropuesta(null);
    } finally {
      setGenerando(false);
    }
  };

  const copiar = async () => {    if (!propuesta) return;
    try {
      await navigator.clipboard.writeText(propuesta.contenido);
      setCopiado("ok");
    } catch {
      // Sin permiso de portapapeles (o sin él, fuera de Tauri) se selecciona el
      // bloque para que un Ctrl+C lo copie: decir "copiado" sin copiar sería
      // mentira, así que se dice lo que pasa y lo que queda por hacer.
      const nodo = bloque.current;
      if (nodo) {
        const rango = document.createRange();
        rango.selectNodeContents(nodo);
        const sel = window.getSelection();
        sel?.removeAllRanges();
        sel?.addRange(rango);
      }
      setCopiado("manual");
    }
  };

  /**
   * El único camino de esta app que MODIFICA la configuración de otro programa.
   *
   * Solo se ofrece donde el backend admite escritura (`admite_escritura`), y el
   * backend hace el resto: copia de seguridad con fecha, escritura atómica,
   * permisos del original y verificación releyendo, con restauración si no
   * cuadra. Aquí no se promete "hecho": se enseña lo que devolvió el backend,
   * incluida la ruta de la copia para poder volver atrás.
   */
  const escribir = async () => {
    setEscribiendo("aplicando");
    setErrorEscritura(null);
    try {
      const r = await api.conexiones.aplicar(argumentos());
      setAplicado(r);
      // Los distintivos de arriba (existe / apunta a local) acaban de quedar
      // viejos: se vuelve a detectar para que digan lo que hay AHORA.
      void useApp.getState().cargarClientes();
    } catch (e) {
      // El backend redacta el motivo (y, si hubo que restaurar, lo dice): se
      // enseña literal, sin envolverlo.
      setErrorEscritura(String(e));
      setAplicado(null);
    } finally {
      setEscribiendo(null);
    }
  };

  const campo = (
    clave: string,
    etiqueta: string,
    valor: string,
    set: (v: string) => void,
    ayuda?: string,
  ) => (
    <label htmlFor={uid(clave)} className="flex flex-col gap-1">
      <span className="label">{etiqueta}</span>
      <input
        id={uid(clave)}
        value={valor}
        onChange={(e) => set(e.target.value)}
        className="border-line bg-raised mono rounded-md border px-2 py-1 text-xs"
      />
      {ayuda ? <span className="text-fg-faint text-[11px]">{ayuda}</span> : null}
    </label>
  );

  return (
    <div className="border-line-soft mt-1 flex flex-col gap-3 border-t pt-3">
      <Etiqueta>Propuesta para {cliente.nombre} · así quedaría su fichero</Etiqueta>
      <div className="grid gap-2 md:grid-cols-2">
        {campo("id", "id", id, setId, "Cómo se llamará este motor en su lista.")}
        {campo("nombre", "nombre", nombre, setNombre, "Lo que verás en su interfaz.")}
        {campo("endpoint", "endpoint", endpoint, setEndpoint, "El motor local que sirve los modelos.")}
        {campo("api", "api", apiKind, setApiKind, "El tipo de API que habla el motor.")}
      </div>
      <label htmlFor={uid("modelos")} className="flex flex-col gap-1">
        <span className="label">modelos</span>
        <textarea
          id={uid("modelos")}
          value={modelos}
          onChange={(e) => setModelosEscritos(e.target.value)}
          rows={2}
          placeholder="Separados por comas o espacios"
          className="border-line bg-raised mono rounded-md border px-2 py-1 text-xs"
        />
        <span className="text-fg-faint text-[11px]">
          Por defecto van los que ya tiene configurados ese cliente MÁS los que publica tu servidor, para
          que aplicar la propuesta no te quite ninguno; puedes cambiarlos. El generador del bloque pide al
          menos uno, y si algo no cuadra te dirá qué.
        </span>
      </label>

      <div className="flex flex-wrap items-center gap-2">
        <Boton variante="acento" disabled={generando} onClick={() => void generar()}>
          <IconPlugConnected size={12} className="mr-1 inline" aria-hidden="true" />
          {generando ? "Generando…" : "Generar bloque"}
        </Boton>
        <span className="text-fg-faint text-xs">
          Generar solo calcula: no toca {cliente.config}.
        </span>
      </div>

      {error ? (
        <p className="text-bad text-xs" role="alert">
          No se pudo generar: {error}
        </p>
      ) : null}

      {propuesta ? (
        <div className="flex flex-col gap-2">
          <p className="text-fg-muted text-xs">{propuesta.resumen}</p>
          <div className="flex flex-wrap items-center gap-2">
            <Etiqueta>
              {propuesta.formato} · destino {propuesta.destino}
            </Etiqueta>
            <Boton className="ml-auto" onClick={() => void copiar()}>
              <IconCopy size={12} className="mr-1 inline" aria-hidden="true" />
              Copiar
            </Boton>
          </div>
          <pre
            ref={bloque}
            className="bg-bg mono border-line-soft max-h-72 overflow-auto rounded-md border p-2.5 text-[11px] leading-relaxed"
          >
            {propuesta.contenido}
          </pre>
          {copiado ? (
            <p
              className={copiado === "ok" ? "text-fg-muted text-xs" : "text-warn text-xs"}
              role="status"
            >
              {copiado === "ok"
                ? "Copiado al portapapeles. Pégalo tú en su configuración cuando lo hayas revisado."
                : "No se pudo copiar solo: el bloque queda seleccionado, pulsa Ctrl+C y pégalo tú."}
            </p>
          ) : null}

          {/* ── Escribir: solo donde el formato está comprobado ───────────── */}
          {cliente.admite_escritura ? (
            <div className="border-line-soft flex flex-col gap-2 border-t pt-3">
              {aplicado ? (
                <Card className="border-ok/40">
                  <p className="text-xs" role="status">
                    <strong className="text-ok">Escrito y comprobado.</strong>{" "}
                    <span className="text-fg-muted">{aplicado.resumen}</span>
                  </p>
                  <div className="mt-2">
                    <Datos
                      items={[
                        ["Fichero", aplicado.destino],
                        ["Copia de seguridad", aplicado.copia],
                        [
                          "¿Ya apunta a local?",
                          aplicado.apunta_local ? "Sí, comprobado releyendo el fichero" : "No se ha podido comprobar",
                        ],
                      ]}
                    />
                  </div>
                </Card>
              ) : null}

              {errorEscritura ? (
                <p className="text-bad text-xs" role="alert">
                  No se escribió: {errorEscritura}
                </p>
              ) : null}

              {escribiendo === "aplicando" ? (
                <p className="text-fg-muted text-xs" role="status">
                  Escribiendo y comprobando…
                </p>
              ) : escribiendo === "confirmando" ? (
                <>
                  <p className="text-xs leading-relaxed">
                    Se escribirá <strong>esto mismo</strong> en{" "}
                    <span className="mono">{propuesta.destino}</span>. Antes se guardará una copia en{" "}
                    <span className="mono">{propuesta.copia_patron}</span> y, si la comprobación posterior
                    no cuadra, se dejará el fichero como estaba.
                  </p>
                  <div className="flex flex-wrap items-center gap-2">
                    <Boton
                      variante="acento"
                      ref={botonConfirmar}
                      onClick={() => void escribir()}
                      onKeyDown={(e) => {
                        // Escape cancela: es lo que espera cualquiera que navegue
                        // con teclado, y evita quedar atrapado en un paso que
                        // escribe en un fichero ajeno.
                        if (e.key === "Escape") setEscribiendo(null);
                      }}
                      // El nombre accesible EMPIEZA por el texto visible (WCAG 2.5.3): con un
                      // `aria-label` que no lo contuviera, quien navega por voz no
                      // podría decir «pulsa Sí, escribir con copia». Y el destino
                      // va detrás, que es contexto útil.
                      aria-label={`Sí, escribir con copia en ${cliente.config}`}
                    >
                      Sí, escribir con copia
                    </Boton>
                    <Boton onClick={() => setEscribiendo(null)}>Cancelar</Boton>
                  </div>
                </>
              ) : (
                <div className="flex flex-wrap items-center gap-2">
                  <Boton
                    variante="peligro"
                    disabled={!cliente.existe || !propuesta.contenido}
                    title={
                      cliente.existe
                        ? "Escribe este proveedor en su fichero. Se guarda una copia antes y se comprueba después."
                        : "No hay configuración previa: no se crea de cero, porque no se sabe qué más necesita."
                    }
                    onClick={() => setEscribiendo("confirmando")}
                  >
                    <IconDeviceFloppy size={12} className="mr-1 inline" aria-hidden="true" />
                    Escribir en {cliente.nombre}…
                  </Boton>
                  <span className="text-fg-faint text-xs">
                    Con copia de seguridad y comprobación posterior. Es el único cliente donde se escribe:
                    su formato está comprobado.
                  </span>
                </div>
              )}
            </div>
          ) : (
            <p className="text-fg-faint border-line-soft border-t pt-3 text-xs">
              De este cliente no se escribe nada: su formato no está comprobado aquí. Revisa el bloque y
              pégalo tú.
            </p>
          )}
        </div>
      ) : null}
    </div>
  );
}

export default function ClientesConexion() {
  const clientes = useApp((st) => st.clientes);
  const cargar = useApp((st) => st.cargarClientes);
  const cargando = useApp(cargandoDe("clientes"));
  const error = useApp(errorDe("clientes"));

  useEffect(() => {
    // Se detecta al abrir la sección, y solo si esta sesión no tiene ya la
    // lista: es una lectura de ficheros ajenos y no se repite sin motivo. El
    // botón "Volver a detectar" la pide otra vez cuando el usuario quiera.
    if (useApp.getState().clientes == null) void cargar();
  }, [cargar]);

  return (
    /* Con `id` propio: es un panel que se cita desde fuera (y así se puede
       localizar sin depender de su texto, que va en mayúsculas por el CSS). */
    <section id="clientes-conectados" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <IconPlugConnected size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Clientes conectados</Etiqueta>
        <Boton className="ml-auto" disabled={cargando} onClick={() => void cargar()}>
          {cargando ? "Detectando…" : "Volver a detectar"}
        </Boton>
      </div>

      {/* El aviso de frontera va aquí, a la vista y en tamaño de lectura: quien
          mire esta sección tiene que saber dónde se escribe y dónde no. */}
      <Card className="border-accent/40 bg-accent-soft">
        <div className="flex items-start gap-2.5">
          <IconInfoCircle size={16} className="text-accent mt-0.5 shrink-0" aria-hidden="true" />
          <div className="text-xs leading-relaxed">
            <p className="text-fg">
              <strong>Machinograph solo escribe donde el formato está comprobado leyendo su fichero real</strong>:
              hoy son <strong>gentle-shell</strong> y <strong>Pi</strong>, que usan el mismo JSON
              (<span className="mono">providers.&lt;id&gt;.models</span> como objetos). Y no escribe a
              ciegas: guarda una copia con la fecha al lado del original, escribe de forma atómica, conserva
              los permisos y <strong>vuelve a leer el fichero para comprobarlo</strong>; si algo no cuadra, lo
              deja como estaba y te dice dónde quedó la copia.
            </p>
            <p className="text-fg-muted mt-1">
              En los demás clientes no se toca nada: se detecta qué tienen y se genera el texto para que lo
              revises y lo pegues tú. Motivo: reescribir un fichero de configuración ajeno (con sus claves,
              sus comentarios y su formato) puede romperlo sin que te enteres, y ahí no hay forma de
              comprobar que quedó bien.
            </p>
          </div>
        </div>
      </Card>

      {error && clientes == null ? (
        <Vacio titulo="No se pudieron detectar">
          <span className="text-bad">{error}</span> Eso es un fallo de la detección, no significa que no
          haya clientes.
        </Vacio>
      ) : cargando && clientes == null ? (
        <Vacio titulo="Detectando">Leyendo las configuraciones de otros clientes de este equipo.</Vacio>
      ) : clientes && clientes.length === 0 ? (
        <Vacio titulo="Sin clientes">
          No se ha encontrado ningún cliente de IA conocido en este equipo. Nada que conectar.
        </Vacio>
      ) : (
        <div className="flex flex-col gap-3">
          {/* Un fallo al VOLVER a detectar no borra lo ya detectado: se dice que
              la última lectura falló y se enseña lo último que sí se pudo leer. */}
          {error ? (
            <Card className="border-bad/40">
              <p className="text-bad text-sm" role="alert">
                Falló la última detección: {error}
              </p>
              <p className="text-fg-muted mt-1 text-xs">
                Lo de abajo es lo último que se pudo leer, no necesariamente lo que hay ahora.
              </p>
            </Card>
          ) : null}
          {(clientes ?? []).map((c) => (
            <Card key={c.id} className="flex flex-col gap-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium">{c.nombre}</span>
                <span className="mono text-fg-faint text-xs">{c.id}</span>
                <Insignia tono={c.existe ? "ok" : "neutro"}>
                  {c.existe ? "configuración encontrada" : "sin configuración"}
                </Insignia>
                <Insignia tono={c.apunta_local ? "ok" : "neutro"}>
                  {c.apunta_local ? "apunta a local" : "no apunta a local"}
                </Insignia>
              </div>

              <Datos items={[["Fichero", c.config]]} />
              <p className="text-fg-muted text-xs">{c.nota}</p>

              {c.como_lo_tiene.length > 0 ? (
                <div>
                  <Etiqueta>Evidencia · líneas de su configuración, tal cual</Etiqueta>
                  <pre className="bg-bg mono border-line-soft mt-1.5 max-h-40 overflow-auto rounded-md border p-2.5 text-[11px] leading-relaxed whitespace-pre-wrap break-all">
                    {c.como_lo_tiene.join("\n")}
                  </pre>
                </div>
              ) : (
                <p className="text-fg-faint text-xs">
                  {c.existe
                    ? "Su configuración existe, pero no tiene ninguna línea que mencione un endpoint local."
                    : "Su fichero de configuración no existe en este equipo, así que no hay nada que leer."}
                </p>
              )}

              {c.admite_escritura ? <FormularioPropuesta cliente={c} /> : null}
            </Card>
          ))}
        </div>
      )}
    </section>
  );
}
