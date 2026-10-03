/**
 * El encaje de un modelo, tal y como lo calculó el backend.
 *
 * Lo calcula ÉL SOLO (al arrancar y cada 10 minutos con el planificador nativo
 * de llama.cpp) y lo guarda en la tabla `fits`; la interfaz solo lo lee. Por eso
 * aquí no hay ningún botón: si hay fila, se enseña, y si no la hay es que todavía
 * no se ha calculado (que NO es lo mismo que "no cabe").
 *
 * Tres cosas que no se pueden callar, porque sin ellas el número engaña:
 *  1. CON QUÉ runtime se calculó (varios llama.cpp instalados leen cosas
 *     distintas, y los Modelo local ternarios solo los lee el fork).
 *  2. CUÁNDO: la memoria libre de la GPU cambia sola, así que un encaje de hace
 *     media hora no describe lo que hay ahora. Pasado el margen se dice.
 *  3. Si el cálculo FALLÓ, con su motivo: un `Error` no se pinta como "no hay
 *     dato", porque es un dato: el que dice que ese binario no sabe leerlo.
 */
import { clsx } from "clsx";
import { IconAlertTriangle } from "@tabler/icons-react";
import { Insignia } from "./ui";
import { fechaHora, hace, num, segundosDesde } from "../lib/format";
import { ENCAJE_VIEJO_SEG, etiquetaEncaje, textoTopeCtx, tonoEncaje } from "../lib/modelos";
import type { FitRow } from "../lib/tauri";

/** ¿Este cálculo es más viejo que una vuelta del bucle automático? */
export const esViejo = (fit: FitRow, ahora: number): boolean =>
  (segundosDesde(fit.ts, ahora) ?? 0) > ENCAJE_VIEJO_SEG;

/** El texto de antigüedad, con la fecha exacta en el `title`. */
function Edad({ fit, ahora }: { fit: FitRow; ahora: number }) {
  return (
    <span className="text-fg-faint" title={`Calculado el ${fechaHora(fit.ts)}`}>
      {hace(fit.ts, ahora)}
    </span>
  );
}

/**
 * La línea de datos de un encaje.
 *
 * Corta a propósito: la celda de la tabla no puede crecer, porque cada píxel de
 * más aquí lo pagan las nueve columnas de la derecha (y la tabla ya se desplaza
 * sola dentro de su tarjeta a 960px). El detalle largo del backend va en el
 * `title`.
 */
function topeDe(fit: FitRow): string {
  if (fit.encaje === "NoCabe" && fit.pedido != null) {
    return `entran ${num(fit.ctx_max)} de ${num(fit.pedido)}`;
  }
  if (fit.encaje === "Error") return "sin contexto";
  // Las capas solo se dicen cuando NO son todas: "todas las capas en la GPU" ya
  // lo dice el nivel ("cabe en la GPU"), así que repetirlo era gastar ancho.
  if (fit.ngl < 0) return `hasta ${textoTopeCtx(fit)}`;
  return `hasta ${textoTopeCtx(fit)} · ${num(fit.ngl)} capas en GPU`;
}

/**
 * El encaje para una CELDA de tabla.
 *
 * Dos líneas y ni una más: la tabla tiene que seguir siendo densa (con 20
 * modelos, una celda de tres líneas daba filas de 130px, y comparar dos modelos
 * obligaba a bajar y subir). La primera línea es el NIVEL, la segunda los datos.
 * El texto completo que redactó el backend va en el `title`.
 */
export function CeldaEncaje({
  fit,
  ahora,
  sinLeer = false,
}: {
  fit: FitRow | null | undefined;
  ahora: number;
  /** La lista de encajes no se pudo leer: "sin calcular" sería falso. */
  sinLeer?: boolean;
}) {
  if (!fit) {
    return sinLeer ? (
      <span
        className="text-bad"
        title="Falló la lectura de los encajes guardados: no se sabe si este modelo tiene cálculo o no."
      >
        sin leer
      </span>
    ) : (
      <span
        className="text-fg-faint"
        title="El backend recalcula el encaje de los .gguf de texto cada 10 min. Si este modelo no tiene fila, todavía no le ha tocado (o no es un .gguf de texto, que son los únicos que se encajan)."
      >
        sin calcular
      </span>
    );
  }

  const error = fit.encaje === "Error";
  return (
    <div className="flex flex-col gap-0.5">
      <Insignia tono={tonoEncaje(fit.encaje)}>
        <span title={fit.detalle}>{etiquetaEncaje(fit.encaje)}</span>
      </Insignia>
      <span className="mono text-[11px] whitespace-nowrap">
        {error ? (
          // El motivo, con icono y color: no se queda en "no se pudo calcular".
          <span className="text-bad" title={fit.detalle}>
            <IconAlertTriangle size={11} className="mr-1 inline align-[-1px]" aria-hidden="true" />
            {fit.detalle.length > 34 ? `${fit.detalle.slice(0, 34)}…` : fit.detalle}
          </span>
        ) : (
          <span className="text-fg-muted" title={fit.detalle}>
            {topeDe(fit)}
          </span>
        )}
        {fit.runtime ? <span className="text-fg-faint">{` · ${fit.runtime}`}</span> : null}
        <span className="text-fg-faint"> · </span>
        <Edad fit={fit} ahora={ahora} />
        {esViejo(fit, ahora) ? (
          <span
            className="text-warn"
            title="El backend recalcula cada 10 min y este dato es más viejo: puede no describir lo que hay ahora. Recalcula a mano desde Rendimiento si te hace falta."
          >
            {" · desfasado"}
          </span>
        ) : null}
      </span>
    </div>
  );
}

/**
 * El encaje para una FICHA (Rendimiento): aquí sí cabe la frase que redactó el
 * backend, literal, junto a los datos que la explican.
 */
export function DetalleEncaje({ fit, ahora }: { fit: FitRow | null | undefined; ahora: number }) {
  if (!fit) {
    return (
      <p className="text-fg-muted text-xs">
        Todavía no se ha calculado el encaje de este modelo. El backend lo hace solo (al arrancar y
        cada 10 min) con el planificador de llama.cpp; aquí aparecerá en cuanto le toque.
      </p>
    );
  }

  const error = fit.encaje === "Error";
  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-2">
        <Insignia tono={tonoEncaje(fit.encaje)}>{etiquetaEncaje(fit.encaje)}</Insignia>
        <span className="mono text-fg-muted text-xs">{topeDe(fit)}</span>
        {fit.runtime ? <span className="mono text-fg-faint text-xs">runtime {fit.runtime}</span> : null}
        <span className="text-xs">
          <Edad fit={fit} ahora={ahora} />
        </span>
        {esViejo(fit, ahora) ? (
          <Insignia tono="warn">
            <span title="El backend recalcula cada 10 min y este dato es más viejo: puede no describir lo que hay ahora.">
              desfasado
            </span>
          </Insignia>
        ) : null}
      </div>
      <p className={clsx("text-xs", error ? "text-bad" : "text-fg-muted")} role={error ? "alert" : undefined}>
        {error ? <IconAlertTriangle size={12} className="mr-1 inline" aria-hidden="true" /> : null}
        {fit.detalle}
      </p>
    </div>
  );
}
