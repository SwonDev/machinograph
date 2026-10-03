/**
 * Rendimiento: ¿me cabe este modelo y a qué velocidad iría?
 *
 * Machinograph ya sabía qué `.gguf` hay en disco, pero no si caben en la GPU ni a qué
 * velocidad irían. Eso lo contestan dos herramientas NATIVAS de llama.cpp:
 *   - `llama-fit-params --fit on` → los argumentos ajustados a la memoria libre
 *     (de ahí sale el contexto máximo que cabe).
 *   - `llama-bench -o json`       → tokens/s REALES de prefill y de generación.
 *
 * Tres hechos que la interfaz cuenta y no puede callarse:
 *
 * 1. Hay VARIOS runtimes de llama.cpp instalados y no todos leen todos los
 *    modelos: los Modelo local ternarios (PQ2_0, PTQ1_0) hacen fallar al llama.cpp
 *    oficial con «invalid ggml type» y solo los lee el fork. Por eso existe el
 *    selector de runtime y por eso «auto» prueba los instalados hasta dar con uno
 *    que sepa leerlo: el backend dice en el propio mensaje CUÁL ha usado.
 *    Como solo tenemos nombre y ruta (ni versión ni procedencia), aquí NO se
 *    afirma cuál es el fork salvo por el nombre del directorio, y se dice que es
 *    una deducción.
 * 2. El resultado depende de la caché KV. El mismo 27B da un máximo de 137472 con
 *    la caché por defecto (f16) y de 262144 con `-ctk q4_0 -ctv q4_0` más flash
 *    attention, que es como se sirve de verdad. Las dos acciones usan esa
 *    configuración (`kv: true`), y eso es lo que hace comparables los números.
 * 3. Medir es CARO: carga el modelo y genera de verdad, así que puede tardar
 *    minutos. Aquí NADA se lanza solo ni en bucle: todo sale de un botón.
 */
import { useEffect, useMemo, useState } from "react";
import { clsx } from "clsx";
import { IconGauge, IconRulerMeasure, IconTerminal2 } from "@tabler/icons-react";
import { useApp, ejecutar, errorDe, type ResultadoAccion } from "../store";
import { Boton, Card, Etiqueta, Insignia, Vacio, useReloj } from "../components/ui";
import { DetalleEncaje } from "../components/Encaje";
import EstimacionLlmfit from "../components/EstimacionLlmfit";
import MedicionServidor from "../components/MedicionServidor";
import { bLegibles, fechaHora, num } from "../lib/format";
import { encajeDe } from "../lib/modelos";

/** Último tramo de una ruta: en la tabla no cabe el camino entero. */
function basename(ruta: string): string {
  const i = ruta.lastIndexOf("/");
  return i >= 0 ? ruta.slice(i + 1) : ruta;
}

/** El backend guarda `prefill`/`decode`; un valor raro se enseña tal cual. */
function tipoMedida(tipo: string): string {
  if (tipo === "prefill") return "prefill";
  if (tipo === "decode") return "generación";
  return tipo || "—";
}

/**
 * De quién es cada runtime, SOLO por el nombre del directorio.
 *
 * El backend no publica versión ni procedencia, así que en el resto de casos se
 * devuelve `null` en vez de adivinar: el nombre de una carpeta es una pista
 * floja, y decir «este es el oficial» cuando no se sabe sería peor que callarse.
 */
function pistaRuntime(nombre: string): string | null {
  const n = nombre.toLowerCase();
  if (n.includes("oficial") || n.includes("official"))
    return "parece el oficial (deducido del nombre del directorio)";
  if (n.includes("fork") || n.includes("custom") || n.includes("propia"))
    return "parece una compilación propia (deducido del nombre del directorio)";
  return null;
}

/**
 * El color de un resultado. Ojo: «NO cabe con lo pedido» llega con código de
 * ÉXITO —es una respuesta válida a una pregunta—, así que va en ámbar y no en
 * rojo, que es para lo que de verdad falló.
 */
function tonoResultado(r: ResultadoAccion): string {
  if (!r.ok) return "text-bad";
  if (r.mensaje.startsWith("NO cabe")) return "text-warn";
  return "text-fg-muted";
}

/** Los valores numéricos solo se mandan si son números de verdad. */
function enteroPositivo(texto: string, min: number): number | null {
  const n = Number.parseInt(texto, 10);
  return Number.isFinite(n) && n >= min ? n : null;
}

export default function Rendimiento() {
  const runtimes = useApp((st) => st.runtimes);
  const medidas = useApp((st) => st.benchmarks);
  const fits = useApp((st) => st.fits);
  const inventario = useApp((st) => st.inventario);
  const cargarFits = useApp((st) => st.cargarFits);
  const cargarInventario = useApp((st) => st.cargarInventario);
  const cargarRuntimes = useApp((st) => st.cargarRuntimes);
  const cargarBenchmarks = useApp((st) => st.cargarBenchmarks);
  // Cuatro lecturas distintas (los runtimes, el histórico, los encajes y el
  // inventario) con cuatro errores distintos: si compartieran uno, un fallo
  // taparía el aviso de otro.
  const errRuntimes = useApp(errorDe("runtimes"));
  const errBench = useApp(errorDe("benchmarks"));
  const errFits = useApp(errorDe("fits"));
  const errInventario = useApp(errorDe("inventario"));
  const linea = useApp((st) => st.lineaAccion);
  const enCurso = useApp((st) => st.accionEnCurso);
  // Los textos de antigüedad del encaje se refrescan solos cada 30 s.
  const ahora = useReloj();

  /**
   * Parámetros comunes a todas las tarjetas.
   *
   * El enunciado pide el selector de runtime como COMÚN, y repetir cinco campos
   * por modelo sería un muro de inputs con decenas de `.gguf` en disco. El
   * mensaje que devuelve el backend lleva dentro el contexto pedido y el runtime
   * usado, así que el resultado guardado se explica solo aunque luego cambies los
   * campos.
   */
  const [runtime, setRuntime] = useState("auto");
  const [ctx, setCtx] = useState("262144");
  const [prompt, setPrompt] = useState("512");
  const [gen, setGen] = useState("128");
  const [reps, setReps] = useState("3");
  /** Último resultado por modelo (clave: la ruta completa, que es única). */
  const [resultados, setResultados] = useState<Record<string, ResultadoAccion>>({});

  useEffect(() => {
    void cargarRuntimes();
    void cargarBenchmarks();
  }, [cargarRuntimes, cargarBenchmarks]);

  // Los encajes ya están calculados por el backend (arranque + cada 10 min): aquí
  // solo se leen, al abrir la vista. Nunca en bucle.
  useEffect(() => {
    if (fits == null) void cargarFits();
  }, [fits, cargarFits]);

  // La LISTA de modelos ya no viaja en la foto (`Snapshot.disk_models` se retiró
  // porque era una segunda fuente de verdad que podía discrepar): sale del
  // inventario, que es la única, y el backend lo tiene cacheado 60 s. Es una
  // lectura de sistema de ficheros, así que se pide al abrir, no en bucle.
  useEffect(() => {
    if (inventario == null) void cargarInventario();
  }, [inventario, cargarInventario]);

  /**
   * Los modelos que se enseñan: los `.gguf` de TEXTO del inventario MÁS los que
   * tienen un encaje guardado.
   *
   * Por qué el inventario y no la foto: `Snapshot.disk_models` era una SEGUNDA
   * fuente de verdad (solo miraba `~/models`) y podía discrepar de la lista real.
   * El inventario las une todas y es la única fuente.
   *
   * Por qué solo `tipo === "texto"`: esta vista mide encaje y tokens/s con
   * llama.cpp, y eso solo se puede hacer con los modelos de texto de ese motor.
   * Enseñar aquí los safetensors de ComfyUI, los ONNX de piper o los proyectores
   * de visión sería llenar la vista de fichas que nunca van a tener ni encaje ni
   * medida, con un "todavía no se ha calculado" que nunca se cumpliría.
   *
   * La unión con los encajes se mantiene: el backend puede tener calculado uno de
   * un modelo que el filtro deje fuera, y esta vista no esconde un dato suyo.
   *
   * Por ruta y no por tamaño: la lista tiene que quedarse quieta mientras se
   * lanzan mediciones, o el botón que acabas de pulsar se mueve de sitio.
   */
  const modelos = useMemo(() => {
    const base = (inventario ?? [])
      .filter((m) => m.tipo === "texto")
      .map((m) => ({
        path: m.ruta,
        rel: m.nombre,
        quant: m.quant,
        // De dónde sale el fichero ("llama.cpp", "LM Studio"…): es el dato que
        // antes daba la carpeta bajo `~/models`, y sirve igual para separarlos.
        familia: m.familia as string | null,
        size_bytes: m.tamano_bytes as number | null,
      }));
    const vistos = new Set(base.map((b) => b.path));
    for (const f of fits ?? []) {
      if (vistos.has(f.modelo)) continue;
      vistos.add(f.modelo);
      // Los que solo conocemos por su encaje no traen cuantización, ni familia,
      // ni tamaño: se dejan en `null` en vez de deducirlos del nombre.
      base.push({ path: f.modelo, rel: basename(f.modelo), quant: null, familia: null, size_bytes: null });
    }
    return base.sort((a, b) => a.rel.localeCompare(b.rel));
  }, [inventario, fits]);

  const calcularEncaje = async (ruta: string) => {
    const pedido = enteroPositivo(ctx, 1);
    const r = await ejecutar("perf:fit", {
      modelo: ruta,
      // Explícito a propósito: `kv: true` = caché KV q4_0 + flash attention, la
      // configuración con la que se sirve aquí. Es lo que hace que el 262144 sea
      // un número real y no el máximo de la caché f16 de fábrica.
      kv: true,
      // Sin contexto, el backend contesta solo el máximo que cabe.
      ...(pedido != null ? { ctx: pedido } : {}),
      runtime,
    });
    setResultados((prev) => ({ ...prev, [ruta]: r }));
    // El backend guarda el resultado y emite `ai:fit`, así que la ficha de arriba
    // se actualiza sola. Se relee igualmente por si el evento se perdió.
    if (r.ok) void cargarFits();
  };

  const medir = async (ruta: string) => {
    const p = enteroPositivo(prompt, 1);
    const g = enteroPositivo(gen, 1);
    const n = enteroPositivo(reps, 1);
    const r = await ejecutar("perf:bench", {
      modelo: ruta,
      ...(p != null ? { prompt: p } : {}),
      ...(g != null ? { gen: g } : {}),
      // Los topes 1..10 también los aplica el backend; aquí solo se acota la
      // entrada para pedir algo razonable.
      ...(n != null ? { reps: Math.min(10, n) } : {}),
      runtime,
    });
    setResultados((prev) => ({ ...prev, [ruta]: r }));
    // Cada medida va a SQLite: se relee el histórico para que la fila nueva
    // aparezca sin tener que recargar la ventana.
    await cargarBenchmarks();
  };

  /** La primera fila del histórico es la referencia de comparación. */
  const referencia = medidas[0];

  return (
    <div className="flex flex-col gap-4">
      {/* ── Qué mide esto (y qué no) ──────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <Etiqueta>Cómo leer estos números</Etiqueta>
        <Card>
          <p className="text-fg-muted text-sm">
            <strong className="text-fg">Encaje</strong>: cuánto contexto cabe con ese modelo, según la
            memoria libre de la GPU (y la RAM, si el runtime reparte capas). No es una propiedad del
            fichero: el mismo <code className="mono">.gguf</code> da un máximo distinto según lo que
            tengas cargado. Lo calcula <strong className="text-fg">el backend solo</strong> (al arrancar
            y cada 10 minutos) y aquí se enseña con el runtime que lo hizo y su antigüedad: si el dato es
            viejo, se dice, porque la memoria libre cambia sola.
          </p>
          <p className="text-fg-muted mt-2 text-sm">
            <strong className="text-fg">Velocidad</strong>: tokens/s reales, medidos cargando el
            modelo y generando de verdad, así que <strong className="text-fg">tarda</strong> (minutos,
            con modelos grandes). Eso NO se puede calcular solo: cada medición sale de un botón, y
            NUNCA se lanza en bucle.
          </p>
          {/* La frontera entre lo estimado y lo medido, dicha antes de que el
              usuario llegue a los números: en esta misma vista conviven los dos,
              y confundirlos es el fallo más caro que se puede cometer aquí. */}
          <p className="text-fg-muted mt-2 text-sm">
            <strong className="text-fg">Ojo con de dónde sale cada número.</strong> En esta vista hay
            tres cosas distintas y van siempre etiquetadas: lo <strong className="text-fg">estimado</strong>{" "}
            por <code className="mono">llmfit</code> (bloque «Estimación de llmfit», calculado sin cargar
            nada), lo <strong className="text-fg">medido en aislado</strong> con{" "}
            <code className="mono">llama-bench</code> (fichas de modelo) y lo{" "}
            <strong className="text-fg">medido sirviendo</strong> con{" "}
            <code className="mono">llmfit bench</code> (bloque «Medir sirviendo»). Un número calculado y
            uno medido no son la misma clase de dato, y los de un camino de medida tampoco se comparan
            con los del otro.
          </p>
          <p className="text-fg-muted mt-2 text-sm">
            Los dos números dependen de la <strong className="text-fg">caché KV</strong>: el mismo
            27B da un máximo de 137472 con la caché por defecto (f16) y de{" "}
            <strong className="text-fg">262144</strong> con{" "}
            <code className="mono">-ctk q4_0 -ctv q4_0</code> más flash attention, que es como se
            sirve aquí. Estas acciones usan esa configuración, y por eso sus resultados se pueden
            comparar entre sí.
          </p>
        </Card>
      </section>

      {/* ── Runtimes detectados ───────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <Etiqueta>
          {runtimes.length > 0
            ? `Runtimes de llama.cpp detectados (${runtimes.length})`
            : "Runtimes de llama.cpp detectados"}
        </Etiqueta>

        {/* Tres estados, no dos: vacío de verdad (leyó y no encontró ninguno) no
            es lo mismo que "la lectura falló", que es lo que diría esta tarjeta
            si no se distinguieran. */}
        {runtimes.length === 0 && errRuntimes ? (
          <Vacio titulo="No se pudo leer la lista de runtimes">{errRuntimes}</Vacio>
        ) : runtimes.length === 0 ? (
          <Card>
            <p className="text-fg-muted text-sm">
              No se ha detectado ningún runtime de llama.cpp. Se busca en{" "}
              <code className="mono">~/.local/bin</code>, en las herramientas que la aplicación
              instala por su cuenta y en los directorios que diga{" "}
              <code className="mono">MACHINOGRAPH_LLAMA_DIRS</code>.
            </p>
          </Card>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {runtimes.map((r) => {
              const pista = pistaRuntime(r.nombre);
              return (
                <Card key={r.dir}>
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium">{r.nombre}</span>
                    {/* "si trae encaje/banco": el `title` lleva la ruta del
                        ejecutable, que es el dato que hay detrás del distintivo. */}
                    <Insignia tono={r.fit ? "ok" : "neutro"}>
                      <span title={r.fit ?? "Este runtime no trae llama-fit-params"}>
                        {r.fit ? "encaje" : "sin encaje"}
                      </span>
                    </Insignia>
                    <Insignia tono={r.bench ? "ok" : "neutro"}>
                      <span title={r.bench ?? "Este runtime no trae llama-bench"}>
                        {r.bench ? "banco" : "sin banco"}
                      </span>
                    </Insignia>
                  </div>
                  <p className="mono text-fg-muted mt-1 truncate text-xs" title={r.dir}>
                    {r.dir}
                  </p>
                  {pista ? <p className="text-fg-faint mt-1 text-xs">{pista}</p> : null}
                  <dl className="mt-2 flex flex-col gap-0.5 text-xs">
                    <div className="flex gap-2">
                      <dt className="text-fg-faint shrink-0">fit-params</dt>
                      <dd className="mono truncate" title={r.fit ?? undefined}>
                        {r.fit ?? "—"}
                      </dd>
                    </div>
                    <div className="flex gap-2">
                      <dt className="text-fg-faint shrink-0">banco</dt>
                      <dd className="mono truncate" title={r.bench ?? undefined}>
                        {r.bench ?? "—"}
                      </dd>
                    </div>
                  </dl>
                </Card>
              );
            })}
          </div>
        )}

        <p className="text-fg-faint text-xs">
          Hay varios runtimes instalados y <strong className="text-fg-muted">no todos leen todos
          los modelos</strong>: los Modelo local ternarios (PQ2_0, PTQ1_0) hacen fallar al llama.cpp
          oficial con «invalid ggml type» y solo los lee el fork. Con «auto», el backend prueba los
          que haya hasta encontrar uno que sepa leerlo, y el mensaje dice cuál ha usado. De cada
          runtime solo sabemos su nombre y su ruta, así que no se etiqueta cuál es el fork salvo
          cuando el nombre del directorio lo deja claro.
        </p>
      </section>

      {/* ── Parámetros comunes ────────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <Etiqueta>Parámetros comunes</Etiqueta>
        <Card>
          <div className="flex flex-wrap items-end gap-3">
            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Runtime
              <select
                value={runtime}
                onChange={(e) => setRuntime(e.target.value)}
                className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
              >
                <option value="auto">auto — prueba los instalados</option>
                {runtimes.map((r) => (
                  <option key={r.dir} value={r.nombre}>
                    {r.nombre}
                    {r.fit ? "" : " (sin encaje)"}
                    {r.bench ? "" : " (sin banco)"}
                  </option>
                ))}
              </select>
            </label>

            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Contexto deseado
              <input
                type="number"
                min={1}
                step={1024}
                value={ctx}
                onChange={(e) => setCtx(e.target.value)}
                className="border-line bg-raised mono w-32 rounded-md border px-2 py-1 text-xs"
              />
            </label>

            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Prompt (tok)
              <input
                type="number"
                min={1}
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                className="border-line bg-raised mono w-20 rounded-md border px-2 py-1 text-xs"
              />
            </label>

            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Generación (tok)
              <input
                type="number"
                min={1}
                value={gen}
                onChange={(e) => setGen(e.target.value)}
                className="border-line bg-raised mono w-20 rounded-md border px-2 py-1 text-xs"
              />
            </label>

            <label className="text-fg-muted flex flex-col gap-1 text-xs">
              Repeticiones (1–10)
              <input
                type="number"
                min={1}
                max={10}
                value={reps}
                onChange={(e) => setReps(e.target.value)}
                className="border-line bg-raised mono w-16 rounded-md border px-2 py-1 text-xs"
              />
            </label>
          </div>
          <p className="text-fg-faint mt-2 text-xs">
            Con el contexto en blanco se calcula solo el máximo que cabe. El encaje y la medición usan
            la caché KV <code className="mono">q4_0</code> + flash attention, que es como se sirve
            aquí: por eso estos números son comparables entre sí y no lo son con los de un llama.cpp
            «de fábrica» (caché f16).
          </p>
        </Card>
      </section>

      {/* ── Estimación (llmfit) y medición sirviendo ───────────────────── */}
      {/* Van juntas y ANTES de las fichas: primero lo que se calcula en
          segundos para decidir, después lo que se mide de verdad y tarda. */}
      <EstimacionLlmfit />
      <MedicionServidor />

      {/* ── Modelos: encaje (ya calculado) y medición EN AISLADO ──────── */}
      <section className="flex flex-col gap-3">
        <Etiqueta>
          {inventario
            ? `Modelos de texto en disco · encaje y velocidad en aislado (${modelos.length})`
            : "Modelos de texto en disco"}
        </Etiqueta>

        {/* El error de los encajes no se calla: si falló su lectura, las fichas
            dirían "todavía no se ha calculado" y la culpa no sería del modelo. */}
        {errFits ? (
          <Card className="border-bad/40">
            <p className="text-bad text-sm">
              No se pudieron leer los encajes guardados, así que las fichas de abajo no pueden decir
              cuál cabe:
            </p>
            <p className="text-bad mt-1 text-sm" role="alert">
              {errFits}
            </p>
          </Card>
        ) : null}

        {inventario == null ? (
          <Vacio titulo={errInventario ? "No se pudo leer el inventario de modelos" : "Cargando"}>
            {errInventario}
          </Vacio>
        ) : modelos.length === 0 ? (
          <Card>
            <p className="text-fg-muted text-sm">
              No se han encontrado modelos <code className="mono">.gguf</code> de texto en el
              inventario. Los de otras familias (ComfyUI, TTS, visión) no salen aquí: esta vista mide
              con llama.cpp y con ellos no se puede.
            </p>
          </Card>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {modelos.map((m) => {
              const res = resultados[m.path];
              const fit = encajeDe(fits, m.path);
              return (
                <Card key={m.path}>
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="mono truncate text-xs" title={m.path}>
                      {m.rel}
                    </span>
                    {m.familia ? <Insignia tono="neutro">{m.familia}</Insignia> : null}
                    <span className="text-fg-faint mono ml-auto text-xs whitespace-nowrap">
                      {m.quant ?? "—"} · {bLegibles(m.size_bytes, 1)}
                    </span>
                  </div>

                  {/* El encaje ya calculado, sin pulsar nada. */}
                  <div className="border-line-soft mt-3 border-t pt-2">
                    <Etiqueta>Encaje (automático)</Etiqueta>
                    <div className="mt-1">
                      <DetalleEncaje fit={fit} ahora={ahora} />
                    </div>
                  </div>

                  <div className="mt-3 flex flex-wrap gap-1.5">
                    {/* Recalcular es la vía manual: el cálculo normal ya lo hace el
                        backend solo. Por eso este botón NO es el principal de la
                        ficha: lo es medir, que es lo único que no puede salir solo. */}
                    <Boton
                      disabled={!!enCurso}
                      title="Fuerza un cálculo nuevo con los parámetros de arriba (el automático usa la caché KV y el runtime que sepa leerlo)."
                      onClick={() => void calcularEncaje(m.path)}
                      aria-label={`Recalcular el encaje de ${m.rel}`}
                    >
                      <IconRulerMeasure size={12} className="mr-1 inline" aria-hidden="true" />
                      Recalcular encaje
                    </Boton>
                    <Boton
                      variante="acento"
                      disabled={!!enCurso}
                      // Este es el camino EN AISLADO (llama-bench sobre un proceso
                      // aparte). Se dice en el `title` y en el nombre accesible
                      // porque al lado está el otro camino, que mide lo mismo de
                      // otra forma y da otros números.
                      title="En aislado con llama-bench: carga el modelo en un proceso aparte, no pasa por el servidor. No es comparable con la medición sirviendo."
                      onClick={() => void medir(m.path)}
                      aria-label={`Medir rendimiento de ${m.rel} en aislado (llama-bench)`}
                    >
                      <IconGauge size={12} className="mr-1 inline" aria-hidden="true" />
                      Medir rendimiento
                    </Boton>
                  </div>

                  {/* El mensaje del backend se enseña LITERAL: ya dice el contexto
                      pedido y el runtime usado, así que se explica solo. */}
                  {res ? (
                    <p
                      className={clsx("mt-2 text-xs", tonoResultado(res))}
                      role={res.ok ? undefined : "alert"}
                    >
                      {res.mensaje}
                    </p>
                  ) : null}
                </Card>
              );
            })}
          </div>
        )}
      </section>

      {/* ── Salida en vivo ────────────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <IconTerminal2 size={15} className="text-accent" aria-hidden="true" />
          <Etiqueta>{enCurso ? `Salida · ${enCurso}` : "Salida de acciones"}</Etiqueta>
          {enCurso ? <Insignia tono="acento">en curso</Insignia> : null}
        </div>
        <Card className="p-0">
          {/* Mismo panel que Servidores y Actualizaciones (`lineaAccion`), pero
              línea a línea: el backend prefija los errores con "[err] " y así se
              ven de un vistazo sin cambiar de sitio. */}
          <pre
            aria-live="polite"
            aria-label="Salida de la acción en curso"
            className="mono max-h-64 min-h-24 overflow-auto p-3 text-xs leading-relaxed"
          >
            {linea.length > 0
              ? linea.map((l, i) => (
                  <div key={i} className={l.startsWith("[err] ") ? "text-bad" : undefined}>
                    {l}
                  </div>
                ))
              : "Sin salida todavía."}
          </pre>
        </Card>
      </section>

      {/* ── Histórico de medidas ──────────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <Etiqueta>
          {medidas.length > 0 ? `Medidas guardadas (${medidas.length})` : "Medidas guardadas"}
        </Etiqueta>

        {medidas.length === 0 ? (
          errBench ? (
            <Vacio titulo="No se pudo leer el histórico de medidas">{errBench}</Vacio>
          ) : (
            <Card>
              <p className="text-fg-muted text-sm">
                Todavía no se ha medido nada desde la app. Mide un modelo y quedará aquí con su
                runtime y su build, que son los dos datos que hacen falta para saber si dos filas se
                pueden comparar.
              </p>
            </Card>
          )
        ) : (
          <>
            {/* Con filas de una lectura anterior se enseña la tabla, pero también
                el aviso de que ya no es fresca. */}
            {errBench ? (
              <Card className="border-bad/40">
                <p className="text-bad text-sm">
                  Falló la última lectura del histórico; esto es lo último que se pudo leer.
                </p>
                <p className="text-bad mt-1 text-sm" role="alert">
                  {errBench}
                </p>
              </Card>
            ) : null}

            <Card className="overflow-x-auto p-0">
              <table className="w-full min-w-[720px] text-left text-xs">
                <caption className="text-fg-faint px-4 pt-3 text-left text-xs">
                  Mediciones de distinto runtime o distinto build{" "}
                  <strong className="text-fg-muted">no son comparables</strong>: cambia el binario (y
                  con él lo que sabe leer) y cambian los tokens/s. Las filas marcadas se apartan de
                  la primera en runtime o en build.
                </caption>
                <thead className="text-fg-faint border-line-soft border-b">
                  <tr>
                    <th scope="col" className="px-4 py-2 font-medium">Cuándo</th>
                    <th scope="col" className="px-4 py-2 font-medium">Modelo</th>
                    <th scope="col" className="px-4 py-2 font-medium">Runtime</th>
                    <th scope="col" className="px-4 py-2 font-medium">Tipo</th>
                    <th scope="col" className="px-4 py-2 text-right font-medium">tok/s</th>
                    <th scope="col" className="px-4 py-2 font-medium">Build</th>
                  </tr>
                </thead>
                <tbody>
                  {medidas.map((m, i) => {
                    const distinta =
                      referencia != null &&
                      (m.runtime !== referencia.runtime || m.build !== referencia.build);
                    return (
                      <tr
                        key={`${m.ts}-${m.runtime}-${m.tipo}-${i}`}
                        className="border-line-soft hover:bg-raised border-b last:border-0"
                      >
                        <td className="mono px-4 py-1.5 whitespace-nowrap">{fechaHora(m.ts)}</td>
                        <td
                          className="mono max-w-[240px] truncate px-4 py-1.5"
                          title={m.modelo || undefined}
                        >
                          {m.modelo ? basename(m.modelo) : "—"}
                        </td>
                        <td className="mono px-4 py-1.5">{m.runtime}</td>
                        <td className="px-4 py-1.5">{tipoMedida(m.tipo)}</td>
                        <td className="mono px-4 py-1.5 text-right whitespace-nowrap">
                          {num(m.tok_s, 1)}
                          <span className="text-fg-faint"> ± {num(m.desviacion, 1)}</span>
                        </td>
                        <td className="px-4 py-1.5">
                          <span className="mono">{m.build}</span>
                          {distinta ? (
                            <Insignia tono="warn">
                              <span title="Esta fila es de otro runtime o de otro build que la primera: sus tokens/s no se pueden leer junto a los de ella.">
                                no comparable
                              </span>
                            </Insignia>
                          ) : null}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </Card>
          </>
        )}
      </section>
    </div>
  );
}
