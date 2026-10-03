/**
 * Medir SIRVIENDO: `llmfit:medir` (llmfit bench) contra el servidor que ya sirve.
 *
 * POR QUÉ HAY DOS CAMINOS DE MEDIR (y esto tiene que leerse)
 * ---------------------------------------------------------------------------
 *   - EN AISLADO (`perf:bench` → llama-bench): carga el modelo por su cuenta, en
 *     un proceso aparte, con sus propios flags. Vale para comparar runtimes y
 *     builds entre sí. Su botón está en cada ficha de modelo.
 *   - SIRVIENDO (`llmfit:medir` → llmfit bench): manda peticiones al servidor que
 *     está en marcha (llama-swap) y mide lo que de VERDAD se sirve, con su proxy
 *     y su configuración de contexto. No vale para comparar runtimes: hay un
 *     servidor de por medio.
 *
 * Los números de un camino y del otro NO son comparables, así que van siempre
 * etiquetados: aquí con palabras y en el histórico con su columna `Runtime`
 * (`llama-bench` frente a `llmfit (…)`).
 *
 * Medir es CARO (carga el modelo y genera de verdad, minutos con modelos
 * grandes): sale de un botón, nunca solo y nunca en bucle.
 */
import { useMemo, useState } from "react";
import { IconAlertTriangle, IconGauge, IconServerCog } from "@tabler/icons-react";
import { ejecutar, useApp, type ResultadoAccion } from "../store";
import { Boton, Card, Etiqueta, Insignia } from "../components/ui";

/** Los dos caminos, en paralelo y con lo que distingue a cada uno. */
function Caminos() {
  const caminos = [
    {
      titulo: "En aislado · llama-bench",
      donde: "Su botón está en cada ficha de modelo, más abajo.",
      puntos: [
        "Carga el modelo él mismo, en un proceso aparte y con sus propios flags.",
        "Vale para comparar runtimes y builds entre sí.",
        "No pasa por el servidor: no mide lo que sirves de verdad.",
      ],
      tono: "neutro" as const,
    },
    {
      titulo: "Sirviendo · llmfit bench",
      donde: "Su botón es el de aquí abajo.",
      puntos: [
        "Manda peticiones al servidor que está en marcha (llama-swap), con su proxy y su contexto.",
        "Mide lo que de verdad se sirve, que es lo que ve quien usa la API.",
        "No vale para comparar runtimes: hay un servidor de por medio.",
      ],
      tono: "acento" as const,
    },
  ];
  return (
    <div className="grid gap-2 md:grid-cols-2">
      {caminos.map((c) => (
        <div key={c.titulo} className="border-line-soft bg-bg rounded-md border p-2.5">
          <div className="flex flex-wrap items-center gap-2">
            <Insignia tono={c.tono}>{c.titulo}</Insignia>
          </div>
          <ul className="text-fg-muted mt-2 flex list-disc flex-col gap-1 pl-4 text-xs">
            {c.puntos.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
          <p className="text-fg-faint mt-2 text-[11px]">{c.donde}</p>
        </div>
      ))}
    </div>
  );
}

export default function MedicionServidor() {
  const s = useApp((st) => st.snapshot);
  const enCurso = useApp((st) => st.accionEnCurso);
  const cargarBenchmarks = useApp((st) => st.cargarBenchmarks);

  const [provider, setProvider] = useState("llamacpp");
  const [url, setUrl] = useState("");
  const [runs, setRuns] = useState("3");
  const [modelo, setModelo] = useState("");
  const [todos, setTodos] = useState(false);
  const [faltaModelo, setFaltaModelo] = useState(false);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  /**
   * Los identificadores que publican los servidores de la foto.
   *
   * Es lo único que se puede ofrecer de verdad: llmfit bench le pregunta al
   * servidor por sus modelos, así que aquí se ofrecen ESOS, no los del catálogo
   * de llmfit (que son otros nombres). Se sugiere, no se obliga.
   */
  const ids = useMemo(() => {
    const salida = new Set<string>();
    for (const sv of s?.servers ?? []) for (const m of sv.models) salida.add(m.id);
    return [...salida].sort();
  }, [s]);

  const medir = async () => {
    const m = modelo.trim();
    // Sin modelo ni «todos» la orden no mide nada: se dice antes de llamar, en
    // vez de mandar una petición que el backend tendría que rechazar.
    if (!m && !todos) {
      setFaltaModelo(true);
      return;
    }
    setFaltaModelo(false);
    const n = Number.parseInt(runs, 10);
    const r = await ejecutar("llmfit:medir", {
      // Con «todos» NO se manda modelo: `--all` ya los coge del servidor, y los
      // dos juntos en la misma orden serían contradictorios.
      ...(todos ? {} : { modelo: m }),
      provider: provider.trim() || "llamacpp",
      ...(url.trim() ? { url: url.trim() } : {}),
      // 1..10 es lo que aplica el backend; se acota aquí para no pedir barbaridades.
      runs: Number.isFinite(n) ? Math.min(10, Math.max(1, n)) : 3,
      todos,
    });
    setResultado(r);
    // La medida va al histórico de SQLite: se relee para que su fila aparezca ya.
    await cargarBenchmarks();
  };

  const idModelo = "medir-modelo";
  const idUrl = "medir-url";
  const idProvider = "medir-provider";
  const idRuns = "medir-runs";
  const idTodos = "medir-todos";

  return (
    /* `id` propio, por el mismo motivo que en los otros paneles nuevos. */
    <section id="medicion-servidor" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <IconServerCog size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Medir sirviendo · media contra el servidor en marcha</Etiqueta>
      </div>

      <Card className="flex flex-col gap-3">
        <Caminos />

        <p className="text-fg-muted text-xs">
          <strong className="text-fg">Los números de los dos caminos no son comparables</strong> y por eso
          no se enseñan juntos: en el histórico, la columna <span className="mono">Runtime</span> dice de
          cuál viene cada fila (<span className="mono">llama-bench</span> o{" "}
          <span className="mono">llmfit (…)</span>).
        </p>

        <p className="text-fg-faint text-xs">
          <IconAlertTriangle size={12} className="mr-1 inline" aria-hidden="true" />
          Medir tarda: el servidor carga el modelo y genera de verdad. Con modelos grandes, minutos.
        </p>

        <div className="flex flex-wrap items-end gap-3">
          <label htmlFor={idProvider} className="text-fg-muted flex flex-col gap-1 text-xs">
            Proveedor
            <input
              id={idProvider}
              value={provider}
              onChange={(e) => setProvider(e.target.value)}
              className="border-line bg-raised mono w-32 rounded-md border px-2 py-1 text-xs"
            />
          </label>

          <label htmlFor={idUrl} className="text-fg-muted flex flex-col gap-1 text-xs">
            URL (opcional)
            <input
              id={idUrl}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="vacío = la del proveedor"
              className="border-line bg-raised mono w-56 rounded-md border px-2 py-1 text-xs"
            />
          </label>

          <label htmlFor={idRuns} className="text-fg-muted flex flex-col gap-1 text-xs">
            Pasadas (1–10)
            <input
              id={idRuns}
              type="number"
              min={1}
              max={10}
              value={runs}
              onChange={(e) => setRuns(e.target.value)}
              className="border-line bg-raised mono w-16 rounded-md border px-2 py-1 text-xs"
            />
          </label>

          <label htmlFor={idModelo} className="text-fg-muted flex flex-col gap-1 text-xs">
            Modelo que sirve el servidor
            <input
              id={idModelo}
              list="medir-modelos"
              value={modelo}
              onChange={(e) => setModelo(e.target.value)}
              placeholder="el id que publica el servidor"
              className="border-line bg-raised mono w-64 rounded-md border px-2 py-1 text-xs"
            />
            <datalist id="medir-modelos">
              {ids.map((id) => (
                <option key={id} value={id} />
              ))}
            </datalist>
          </label>

          <label htmlFor={idTodos} className="text-fg-muted flex items-center gap-2 pb-1 text-xs">
            <input
              id={idTodos}
              type="checkbox"
              checked={todos}
              onChange={(e) => setTodos(e.target.checked)}
              className="accent-accent"
            />
            Todos los del servidor
          </label>

          <Boton variante="acento" disabled={!!enCurso} onClick={() => void medir()}>
            <IconGauge size={12} className="mr-1 inline" aria-hidden="true" />
            {enCurso === "llmfit:medir" ? "Midiendo…" : "Medir contra el servidor"}
          </Boton>
        </div>

        {faltaModelo ? (
          <p className="text-warn text-xs" role="alert">
            Escribe el id de un modelo o marca «Todos los del servidor»: sin uno de los dos no hay nada
            que medir.
          </p>
        ) : null}

        {/* El mensaje del backend se enseña LITERAL: ya dice el proveedor, las
            pasadas y los tokens/s que ha sacado. */}
        {resultado ? (
          <p
            className={resultado.ok ? "text-fg-muted text-xs" : "text-bad text-xs"}
            role={resultado.ok ? undefined : "alert"}
          >
            {resultado.mensaje}
          </p>
        ) : null}
      </Card>
    </section>
  );
}
