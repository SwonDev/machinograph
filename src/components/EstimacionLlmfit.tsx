/**
 * Estimación de llmfit: plan de hardware y concurrencia. NO es una medición.
 *
 * POR QUÉ ESTE BLOQUE EXISTE Y POR QUÉ DICE LO QUE DICE
 * ---------------------------------------------------------------------------
 * En esta misma vista viven los números MEDIDOS con llama.cpp (llama-fit-params
 * y llama-bench). Estos de aquí son otra cosa: los CALCULA llmfit a partir de los
 * pesos y de sus heurísticas, sin cargar el modelo ni generar un solo token. Son
 * rápidos (~0,5 s) y sirven para decidir antes de gastar minutos; los medidos
 * dicen lo que el equipo da de verdad.
 *
 * Eso se dice con palabras y se ve a simple vista: el título del bloque lo lleva
 * escrito, cada tok/s va marcado «est.» y, si llmfit manda su propio aviso
 * (`estimate_notice`), se enseña tal cual. Mezclar las dos cosas sería el peor
 * fallo posible de esta pantalla: alguien decidiría con un número calculado
 * creyendo que está medido.
 *
 * Nada de esto se lanza solo: los dos cálculos salen de un botón.
 */
import { useState } from "react";
import { IconAlertTriangle, IconMathFunction, IconUsers } from "@tabler/icons-react";
import { api, type LlmfitConcurrencia, type LlmfitPlan, type LlmfitRecursos } from "../lib/tauri";
import { Boton, Card, Datos, Etiqueta, Insignia } from "../components/ui";
import { num } from "../lib/format";

/** Un número con su unidad, o "—" si el dato no viene (nunca "0"). */
function conUnidad(v: number | null | undefined, unidad: string, dec = 1): string {
  return v == null || !Number.isFinite(v) ? "—" : `${num(v, dec)} ${unidad}`;
}

/**
 * Los requisitos de una vía, en una celda: "16.7 GB / 64.0 GB / 4".
 *
 * El `null` de un recurso NO es un cero: llmfit manda `vram_gb: null` en la vía
 * de solo CPU, y eso quiere decir que esa vía no necesita VRAM dedicada. Aquí se
 * enseña "—" y el pie de la tabla lo aclara.
 */
function recursos(r: LlmfitRecursos | null): string {
  if (!r) return "—";
  const gbONada = (v: number | null) => (v == null ? "—" : `${num(v, 1)} GB`);
  const cores = r.cpu_cores == null ? "—" : num(r.cpu_cores);
  return `${gbONada(r.vram_gb)} / ${gbONada(r.ram_gb)} / ${cores}`;
}

/** El nombre de una vía, traducido. Una vía desconocida se enseña tal cual. */
function nombreVia(path: string): string {
  if (path === "gpu") return "GPU · todo en la VRAM";
  if (path === "cpu_offload") return "GPU + CPU · capas repartidas";
  if (path === "cpu_only") return "Solo CPU";
  return path || "—";
}

export default function EstimacionLlmfit() {
  const [modelo, setModelo] = useState("");
  const [contexto, setContexto] = useState("32768");
  const [quant, setQuant] = useState("");
  const [plan, setPlan] = useState<LlmfitPlan | null>(null);
  const [conc, setConc] = useState<LlmfitConcurrencia | null>(null);
  const [errPlan, setErrPlan] = useState<string | null>(null);
  const [errConc, setErrConc] = useState<string | null>(null);
  const [trabajando, setTrabajando] = useState<"plan" | "concurrencia" | null>(null);

  const modeloLimpio = modelo.trim();

  const calcularPlan = async () => {
    if (!modeloLimpio) return;
    setTrabajando("plan");
    try {
      const ctx = Number.parseInt(contexto, 10);
      setPlan(
        await api.llmfit.plan({
          modelo: modeloLimpio,
          // Sin contexto utilizable NO se manda: así manda el 32768 del backend
          // en vez de un valor inventado por la interfaz.
          ...(Number.isFinite(ctx) && ctx > 0 ? { context: ctx } : {}),
          ...(quant.trim() ? { quant: quant.trim() } : {}),
        }),
      );
      setErrPlan(null);
    } catch (e) {
      setErrPlan(String(e));
      setPlan(null);
    } finally {
      setTrabajando(null);
    }
  };

  const calcularConcurrencia = async () => {
    if (!modeloLimpio) return;
    setTrabajando("concurrencia");
    try {
      setConc(await api.llmfit.concurrencia(modeloLimpio));
      setErrConc(null);
    } catch (e) {
      setErrConc(String(e));
      setConc(null);
    } finally {
      setTrabajando(null);
    }
  };

  return (
    /* `id` propio: la etiqueta de la sección va en mayúsculas por el CSS, así que
       este bloque se localiza por su id y no por su texto. */
    <section id="estimacion-llmfit" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <IconMathFunction size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Estimación de llmfit · no es una medición</Etiqueta>
      </div>

      <Card>
        <p className="text-fg-muted text-sm">
          <strong className="text-fg">Esto está calculado, no medido.</strong> Lo estima{" "}
          <code className="mono">llmfit</code> a partir de los pesos del modelo y de sus heurísticas: no
          se carga el modelo ni se genera un token, así que responde en menos de un segundo. Sirve para
          decidir antes de gastar minutos. Lo <strong className="text-fg">medido</strong> con llama.cpp
          (secciones de más abajo, con su tok/s real) es otra cosa y no se mezcla con esto.
        </p>

        <div className="mt-3 flex flex-wrap items-end gap-3">
          <label htmlFor="estim-modelo" className="text-fg-muted flex flex-col gap-1 text-xs">
            Modelo (como lo conoce llmfit)
            <input
              id="estim-modelo"
              value={modelo}
              onChange={(e) => setModelo(e.target.value)}
              placeholder="Qwen2.5 32B Instruct"
              className="border-line bg-raised mono w-64 rounded-md border px-2 py-1 text-xs"
            />
          </label>
          <label htmlFor="estim-contexto" className="text-fg-muted flex flex-col gap-1 text-xs">
            Contexto
            <input
              id="estim-contexto"
              type="number"
              min={1}
              step={1024}
              value={contexto}
              onChange={(e) => setContexto(e.target.value)}
              className="border-line bg-raised mono w-28 rounded-md border px-2 py-1 text-xs"
            />
          </label>
          <label htmlFor="estim-quant" className="text-fg-muted flex flex-col gap-1 text-xs">
            Cuantización (opcional)
            <input
              id="estim-quant"
              value={quant}
              onChange={(e) => setQuant(e.target.value)}
              placeholder="Q4_K_M"
              className="border-line bg-raised mono w-28 rounded-md border px-2 py-1 text-xs"
            />
          </label>
          <Boton
            variante="acento"
            disabled={!modeloLimpio || trabajando != null}
            onClick={() => void calcularPlan()}
          >
            {trabajando === "plan" ? "Calculando…" : "Calcular plan"}
          </Boton>
          <Boton
            disabled={!modeloLimpio || trabajando != null}
            onClick={() => void calcularConcurrencia()}
          >
            <IconUsers size={12} className="mr-1 inline" aria-hidden="true" />
            {trabajando === "concurrencia" ? "Calculando…" : "Calcular concurrencia"}
          </Boton>
        </div>

        <p className="text-fg-faint mt-2 text-xs">
          El modelo se pide por <strong className="text-fg-muted">nombre</strong>, como lo conoce llmfit
          (no la ruta del <code className="mono">.gguf</code>): llmfit busca en su catálogo, y una ruta
          no está en él.
        </p>
      </Card>

      {errPlan ? (
        <Card className="border-bad/40">
          <p className="text-bad text-sm" role="alert">
            No se pudo calcular el plan: {errPlan}
          </p>
        </Card>
      ) : null}

      {plan ? (
        <>
          {plan.estimate_notice ? (
            <Card className="border-warn/40">
              <p className="text-warn text-xs">
                <IconAlertTriangle size={12} className="mr-1 inline" aria-hidden="true" />
                Aviso de llmfit sobre sus propios números: {plan.estimate_notice}
              </p>
            </Card>
          ) : null}

          <Card className="flex flex-col gap-3">
            <div>
              <Etiqueta>Plan de hardware · estimado</Etiqueta>
              <div className="mt-2">
                <Datos
                  items={[
                    ["Modelo", plan.model_name],
                    ["Proveedor", plan.provider],
                    ["Contexto supuesto", num(plan.context)],
                    ["Cuantización", plan.quantization ?? "—"],
                    ["Caché KV", plan.kv_quant ?? "—"],
                    ["Tamaño en disco", conUnidad(plan.disk_size_gb, "GB")],
                    ["Mínimo para moverlo", recursos(plan.minimum)],
                    ["Recomendado", recursos(plan.recommended)],
                  ]}
                />
              </div>
            </div>

            {plan.run_paths.length > 0 ? (
              <div className="overflow-x-auto">
                <table className="w-full min-w-[720px] text-left text-xs">
                  <caption className="text-fg-faint pb-2 text-left text-xs">
                    Formas de ejecutarlo, según llmfit. «Mínimo» y «Recomendado» van como{" "}
                    <span className="mono">VRAM / RAM / núcleos</span>; una raya significa que ese dato
                    no viene, y en una vía de solo CPU la VRAM nula quiere decir que{" "}
                    <strong className="text-fg-muted">no necesita VRAM dedicada</strong>, no cero.
                  </caption>
                  <thead className="text-fg-faint border-line-soft border-b">
                    <tr>
                      <th scope="col" className="px-2 py-2 font-medium">Vía</th>
                      <th scope="col" className="px-2 py-2 font-medium">¿Cabe?</th>
                      <th scope="col" className="px-2 py-2 font-medium">Nivel</th>
                      <th scope="col" className="px-2 py-2 text-right font-medium">tok/s (est.)</th>
                      <th scope="col" className="px-2 py-2 font-medium">Mínimo</th>
                      <th scope="col" className="px-2 py-2 font-medium">Recomendado</th>
                      <th scope="col" className="px-2 py-2 font-medium">Notas</th>
                    </tr>
                  </thead>
                  <tbody>
                    {plan.run_paths.map((v) => (
                      <tr key={v.path} className="border-line-soft border-b last:border-0">
                        <td className="px-2 py-1.5">{nombreVia(v.path)}</td>
                        <td className="px-2 py-1.5">
                          <Insignia tono={v.feasible ? "ok" : "bad"}>{v.feasible ? "cabe" : "no cabe"}</Insignia>
                        </td>
                        <td className="mono px-2 py-1.5">{v.fit_level ?? "—"}</td>
                        <td className="mono px-2 py-1.5 text-right whitespace-nowrap">
                          {num(v.estimated_tps, 1)}
                        </td>
                        <td className="mono px-2 py-1.5 whitespace-nowrap">{recursos(v.minimum)}</td>
                        <td className="mono px-2 py-1.5 whitespace-nowrap">{recursos(v.recommended)}</td>
                        <td className="text-fg-muted max-w-[260px] px-2 py-1.5 text-[11px]">
                          {v.notes.length > 0 ? v.notes.join(" · ") : "—"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="text-fg-faint text-xs">
                llmfit no ha devuelto ninguna forma de ejecutar este modelo.
              </p>
            )}
          </Card>
        </>
      ) : null}

      {errConc ? (
        <Card className="border-bad/40">
          <p className="text-bad text-sm" role="alert">
            No se pudo calcular la concurrencia: {errConc}
          </p>
        </Card>
      ) : null}

      {conc ? (
        <Card className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <Etiqueta>Capacidad · sesiones simultáneas (calculado)</Etiqueta>
            {conc.fit_level ? <Insignia tono="neutro">{conc.fit_level}</Insignia> : null}
          </div>

          <Datos
            items={[
              ["Modelo", conc.model ?? "—"],
              ["Modo de ejecución", conc.run_mode ?? "—"],
              [
                "Contexto máximo para el objetivo",
                conc.max_context_for_target == null ? "—" : num(conc.max_context_for_target),
              ],
              ["Presupuesto de caché KV", conUnidad(conc.estimate?.kv_budget_gb, "GB")],
              ["Cuantización de la caché", conc.estimate?.kv_quant ?? "—"],
              ["VRAM disponible", conUnidad(conc.estimate?.pool_gb, "GB")],
              [
                "Pesos residentes",
                conc.estimate?.weights_resident_gb == null
                  ? "—"
                  : conUnidad(conc.estimate.weights_resident_gb, "GB"),
              ],
              // Tres datos que da llmfit y que antes se tiraban: sin ellos, la
              // escalera se lee pero no se entiende por qué se para donde se para.
              ["Cuantización del modelo", conc.estimate?.quant ?? "—"],
              [
                "Contexto nativo del modelo",
                conc.estimate?.native_context == null ? "—" : num(conc.estimate.native_context),
              ],
              ...(conc.estimate?.per_session_recurrent_gb == null
                ? []
                : [
                    [
                      "Capas recurrentes por sesión",
                      conUnidad(conc.estimate.per_session_recurrent_gb, "GB"),
                    ] as [string, string],
                  ]),
            ]}
          />

          {conc.estimate && conc.estimate.ladder.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full min-w-[560px] text-left text-xs">
                <caption className="text-fg-faint pb-2 text-left text-xs">
                  Cuántas sesiones caben a la vez según el contexto. Es un cálculo de MEMORIA (los pesos
                  se cargan una vez y cada sesión añade su caché), no una prueba bajo carga: no dice a
                  qué velocidad irán, solo cuántas caben. La escalera va de menos a más contexto y en ese
                  orden, porque es una progresión.
                </caption>
                <thead className="text-fg-faint border-line-soft border-b">
                  <tr>
                    <th scope="col" className="px-2 py-2 text-right font-medium">Contexto pedido</th>
                    <th scope="col" className="px-2 py-2 text-right font-medium">Contexto efectivo</th>
                    <th scope="col" className="px-2 py-2 text-right font-medium">KV por sesión</th>
                    <th scope="col" className="px-2 py-2 text-right font-medium">Sesiones a la vez</th>
                  </tr>
                </thead>
                <tbody>
                  {conc.estimate.ladder.map((e) => (
                    <tr
                      key={e.requested_context}
                      className="border-line-soft border-b last:border-0"
                    >
                      <td className="mono px-2 py-1.5 text-right">{num(e.requested_context)}</td>
                      <td className="mono px-2 py-1.5 text-right">{num(e.effective_context)}</td>
                      <td className="mono px-2 py-1.5 text-right whitespace-nowrap">
                        {conUnidad(e.per_session_kv_gb, "GB")}
                      </td>
                      <td className="mono px-2 py-1.5 text-right whitespace-nowrap">
                        {e.max_sessions > 0 ? (
                          num(e.max_sessions)
                        ) : (
                          <span className="text-warn">0 · no cabe ni una</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <p className="text-fg-faint text-xs">
              llmfit no ha devuelto la escalera de contextos de este modelo.
            </p>
          )}
        </Card>
      ) : null}
    </section>
  );
}
