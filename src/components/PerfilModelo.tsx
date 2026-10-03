/**
 * El perfil de un modelo en cinco ejes, y la recomendación según lo que priorices.
 *
 * DE DÓNDE SALE CADA EJE, que es lo que hace que esto no sea un adorno: los cinco
 * son componentes que **llmfit ya calcula** (`score_components`) al cruzar el
 * modelo con el hardware de este equipo. No se inventan ejes ni se normaliza nada
 * a mano:
 *
 *   Velocidad  → `speed`     (tokens/s estimados contra el ancho de banda real)
 *   Calidad    → `quality`   (la nota de capacidad del modelo)
 *   Encaje     → `fit`       (cuánto se ajusta a la memoria disponible)
 *   Contexto   → `context`   (cuánto contexto se puede servir de verdad)
 *   Holgura    → 100 − `utilization_pct`  (lo que NO ocupa: deja sitio a lo demás)
 *
 * `holgura` es el único que se calcula aquí, y es una resta sobre un dato de
 * llmfit (`utilization_pct`), no una estimación propia. Se dice en la interfaz.
 *
 * Y el radar no decide nada: el orden de la tabla lo decide el peso que elija el
 * usuario entre velocidad y calidad. El radar solo enseña POR QUÉ ese modelo está
 * arriba.
 */
import { Card, Etiqueta, Insignia } from "./ui";
import { num } from "../lib/format";
import type { LlmfitModelo } from "../lib/tauri";

/** Los cinco ejes del perfil, en el orden en que se pintan. */
export interface EjePerfil {
  etiqueta: string;
  /** 0..100 */
  valor: number;
  /** El dato exacto de llmfit del que sale, para el `title`. */
  origen: string;
}

/**
 * Los cinco ejes de un modelo, o `null` si llmfit no dio componentes.
 *
 * Sin componentes NO se pinta un radar con ceros: un pentágono vacío se leería
 * como "este modelo no vale para nada", y lo que pasa es que no hay dato.
 */
export function ejesDe(m: LlmfitModelo): EjePerfil[] | null {
  const c = m.score_components;
  if (!c) return null;
  return [
    { etiqueta: "Velocidad", valor: c.speed, origen: `speed de llmfit: ${num(c.speed, 1)}` },
    { etiqueta: "Calidad", valor: c.quality, origen: `quality de llmfit: ${num(c.quality, 1)}` },
    { etiqueta: "Encaje", valor: c.fit, origen: `fit de llmfit: ${num(c.fit, 1)}` },
    { etiqueta: "Contexto", valor: c.context, origen: `context de llmfit: ${num(c.context, 1)}` },
    {
      etiqueta: "Holgura",
      valor:
        m.utilization_pct == null
          ? 0
          : Math.max(0, Math.min(100, 100 - m.utilization_pct)),
      origen:
        m.utilization_pct == null
          ? "llmfit no dijo cuánta memoria ocupa"
          : `100 − utilization_pct (${num(m.utilization_pct, 1)}) — lo que deja libre`,
    },
  ];
}

/**
 * La puntuación de un modelo según lo que priorices.
 *
 * `preferencia` va de 0 (lo más rápido) a 100 (lo más capaz), y se aplica como un
 * peso lineal entre `speed` y `quality`. El ENCAJE multiplica, no suma: un modelo
 * que no cabe no puede ganar por muy rápido que sea. Es la misma idea que el
 * "Fits my machine" de Magnitude, pero con los números que ya da llmfit.
 *
 * OJO CON EL SENTIDO DEL PESO: 0 es «más rápido», así que 0 pone todo el peso en
 * `speed`. La primera versión lo tenía al revés (`speed * p`), y el resultado era
 * que el deslizador hacía justo lo contrario de lo que decía su etiqueta. Lo
 * encontró la comprobación del arnés, que mueve el deslizador a los dos extremos
 * y mira qué modelo queda el primero.
 */
export function puntuacion(m: LlmfitModelo, preferencia: number): number | null {
  const c = m.score_components;
  if (!c) return null;
  const p = Math.max(0, Math.min(100, preferencia)) / 100;
  const base = c.speed * (1 - p) + c.quality * p;
  // El encaje va de 0 a 100: como factor, 0 anula y 100 no cambia nada.
  const factor = c.fit / 100;
  return base * factor;
}

/** El pentágono. SVG a mano: son cinco puntos y cuatro anillos. */
function Radar({ ejes }: { ejes: EjePerfil[] }) {
  const CX = 130;
  const CY = 120;
  const R = 82;
  const punto = (i: number, radio: number): [number, number] => {
    // Empieza arriba (-90°) y reparte los cinco ejes a partes iguales.
    const ang = -Math.PI / 2 + (i * 2 * Math.PI) / ejes.length;
    return [CX + Math.cos(ang) * radio, CY + Math.sin(ang) * radio];
  };
  const anillo = (radio: number) =>
    ejes.map((_, i) => punto(i, radio).join(",")).join(" ");
  const perfil =
    ejes
      .map((e, i) => `${i === 0 ? "M" : "L"} ${punto(i, (Math.max(0, Math.min(100, e.valor)) / 100) * R).join(" ")}`)
      .join(" ") + " Z";

  return (
    <svg viewBox="0 0 260 250" className="mx-auto block w-full max-w-[260px]" role="img" aria-label={`Perfil del modelo: ${ejes.map((e) => `${e.etiqueta} ${num(e.valor, 0)} de 100`).join(", ")}`}>
      <title>{ejes.map((e) => e.origen).join("; ")}</title>
      {[0.25, 0.5, 0.75, 1].map((f) => (
        <polygon key={f} points={anillo(R * f)} fill="none" className="stroke-line-soft" strokeWidth="1" />
      ))}
      {ejes.map((e, i) => {
        const [x, y] = punto(i, R);
        return <line key={e.etiqueta} x1={CX} y1={CY} x2={x} y2={y} className="stroke-line-soft" strokeWidth="1" />;
      })}
      <path d={perfil} className="fill-accent/15 stroke-accent" strokeWidth="2" strokeLinejoin="round" />
      {ejes.map((e, i) => {
        const [x, y] = punto(i, R + 22);
        const ancla = Math.abs(x - CX) < 6 ? "middle" : x > CX ? "start" : "end";
        return (
          <text key={e.etiqueta} x={x} y={y} textAnchor={ancla} dominantBaseline="middle">
            <tspan className="fill-fg-muted" fontSize="11">
              {e.etiqueta}
            </tspan>
            <tspan x={x} dy="14" className="fill-fg" fontSize="12" fontWeight="500">
              {num(e.valor, 0)}
            </tspan>
          </text>
        );
      })}
    </svg>
  );
}

/**
 * La recomendación según la preferencia, con su radar.
 *
 * Va ARRIBA de la tabla y es lo primero que se lee: "con este ajuste, lo que
 * mejor le sienta a tu equipo es X, y por esto". El orden de la tabla es el mismo
 * criterio, así que lo de abajo confirma lo de arriba.
 */
export function RecomendacionPreferida({
  modelo,
  preferencia,
  onPreferencia,
  total,
}: {
  modelo: LlmfitModelo | null;
  preferencia: number;
  onPreferencia: (v: number) => void;
  total: number;
}) {
  const ejes = modelo ? ejesDe(modelo) : null;

  return (
    <Card>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Etiqueta>Recomendación según lo que priorices</Etiqueta>
        <span className="text-fg-faint ml-auto text-xs">
          sobre {total} {total === 1 ? "modelo" : "modelos"} de la lista
        </span>
      </div>

      {/* El control: un deslizador entre velocidad y capacidad. Se dice el peso
          EXACTO al lado porque un deslizador sin número no se puede repetir. */}
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-fg-muted text-xs">Más rápido</span>
        <input
          type="range"
          min={0}
          max={100}
          step={5}
          value={preferencia}
          onChange={(e) => onPreferencia(Number(e.target.value))}
          aria-label="Peso entre velocidad y capacidad"
          aria-valuetext={`${preferencia} % de peso a la capacidad, ${100 - preferencia} % a la velocidad`}
          className="accent-accent h-1 min-w-[160px] flex-1"
        />
        <span className="text-fg-muted text-xs">Más capaz</span>
        <Insignia tono="neutro">
          {preferencia === 50
            ? "equilibrado"
            : preferencia < 50
              ? `${100 - preferencia} % velocidad`
              : `${preferencia} % capacidad`}
        </Insignia>
      </div>

      {modelo == null ? (
        <p className="text-fg-muted mt-3 text-sm">
          No hay ningún modelo con la puntuación de llmfit entre los que cumplen los filtros.
        </p>
      ) : (
        <div className="mt-4 grid items-center gap-4 lg:grid-cols-[minmax(0,1fr)_260px]">
          <div className="min-w-0">
            <div className="text-xs text-fg-faint">Con este ajuste, lo que mejor le sienta a este equipo</div>
            <div className="mono mt-1 truncate text-sm" title={modelo.name}>
              {modelo.name}
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
              {modelo.provider ? <span className="text-fg-muted">{modelo.provider}</span> : null}
              {modelo.fit_level ? <Insignia tono="ok">{modelo.fit_level}</Insignia> : null}
              {modelo.best_quant ? <Insignia tono="neutro">{modelo.best_quant}</Insignia> : null}
              {modelo.estimated_tps != null ? (
                <span className="mono text-fg-muted">{num(modelo.estimated_tps, 1)} tok/s estimados</span>
              ) : null}
              {modelo.memory_required_gb != null ? (
                <span className="mono text-fg-muted">{num(modelo.memory_required_gb, 1)} GB de memoria</span>
              ) : null}
            </div>
            {ejes ? (
              <ul className="mt-3 flex flex-col gap-0.5 text-xs">
                {ejes.map((e) => (
                  <li key={e.etiqueta} className="flex items-center gap-2">
                    <span className="text-fg-faint w-20 shrink-0">{e.etiqueta}</span>
                    <span className="mono w-10 shrink-0 text-right">{num(e.valor, 0)}</span>
                    <span className="text-fg-faint truncate" title={e.origen}>
                      {e.origen}
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-fg-muted mt-3 text-xs">
                llmfit no ha dado componentes de puntuación para este modelo, así que no hay perfil que dibujar.
                Un pentágono a cero diría que no vale para nada, y lo que pasa es que no hay dato.
              </p>
            )}
          </div>
          {ejes ? <Radar ejes={ejes} /> : null}
        </div>
      )}
    </Card>
  );
}
