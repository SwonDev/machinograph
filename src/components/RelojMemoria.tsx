/**
 * El reloj de memoria de la GPU (MCLK): lectura, aviso y remedios.
 *
 * POR QUÉ ES UN COMPONENTE APARTE: el mismo dato se mira en dos sitios y con dos
 * intenciones distintas —Inicio lo usa para AVISAR de que algo va 15 veces más
 * lento, Hardware para VIGILAR el nivel y los niveles disponibles—, y tener dos
 * copias de la lectura y de los botones sería tener dos sitios donde equivocarse
 * (y dos lecturas de sysfs que pueden discrepar). Aquí vive una sola vez.
 *
 * El fallo, medido en esta máquina: el MCLK se queda clavado en su nivel mínimo
 * (96 MHz) aunque la GPU esté al 100 %, y no da ningún error. Es de Display Core
 * en amdgpu ([drm/amd#2657]). El 27B pasó de 3,27 a 45,81 tok/s al dejar de estar
 * degradado; de ahí que el aviso mande sobre cualquier otro.
 */
import { useCallback, useEffect, useState } from "react";
import { clsx } from "clsx";
import { IconAlertTriangle, IconRefresh } from "@tabler/icons-react";
import { useApp, ejecutar, type ResultadoAccion } from "../store";
import { api, type EstadoMclk } from "../lib/tauri";
import { Boton, Card, Insignia, type Tono } from "./ui";
import { mhz, num } from "../lib/format";

/** Nivel del MCLK: `ok` (leyó), `sin-gpu` (no hay amdgpu), `error` (no se pudo). */
export type LecturaMclk =
  | { fase: "cargando" }
  | { fase: "error"; mensaje: string }
  | { fase: "sin-gpu" }
  | { fase: "ok"; datos: EstadoMclk };

/**
 * Lee el reloj de memoria y lo relee con cada foto (`ts`).
 *
 * Es una lectura de sysfs, así que cabe en cada foto: el aviso aparece y
 * desaparece con el fallo real sin que nadie tenga que refrescar a mano.
 */
export function useMclk(ts: number | undefined): { lectura: LecturaMclk; releer: () => void } {
  const [lectura, setLectura] = useState<LecturaMclk>({ fase: "cargando" });

  const releer = useCallback(() => {
    api.gpu
      .mclk()
      .then((d) => setLectura(d ? { fase: "ok", datos: d } : { fase: "sin-gpu" }))
      .catch((e) => setLectura({ fase: "error", mensaje: String(e) }));
  }, []);

  useEffect(() => {
    releer();
  }, [releer, ts]);

  return { lectura, releer };
}

/**
 * Los DOS remedios, en orden y con su porqué.
 *
 * El orden no es estético: ciclar el modo de pantalla es la vía que se MIDIÓ que
 * funciona (de 96 a 1000 MHz), no necesita root, no pierde la VRAM y no corta una
 * generación en curso, así que va primero y en acento. Reiniciar el motor gráfico
 * es la vía fuerte: SÍ necesita root y PIERDE la VRAM, así que no se lanza de un
 * clic, se convierte en una pregunta.
 */
export function AccionesMclk({ enCurso, onRecargar }: { enCurso: string | null; onRecargar: () => void }) {
  const [confirmando, setConfirmando] = useState(false);
  const [resultado, setResultado] = useState<ResultadoAccion | null>(null);

  const lanzar = async (kind: "gpu:arreglar" | "gpu:reiniciar") => {
    const r = await ejecutar(kind);
    setResultado(r);
    setConfirmando(false);
    // El backend ya relee el reloj antes y después y lo cuenta en el mensaje,
    // pero esta lectura deja los niveles al día sin esperar a la siguiente foto.
    onRecargar();
  };

  return (
    <div className="mt-1 flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        {/* Sin `aria-label`: el botón ya lleva texto visible y ese texto ES su
            nombre accesible. Ponerle un `aria-label` distinto rompería WCAG 2.5.3
            (quien use voz diría lo que lee en pantalla y no coincidiría). */}
        <Boton variante="acento" disabled={!!enCurso} onClick={() => void lanzar("gpu:arreglar")}>
          <IconRefresh size={12} className="mr-1 inline" aria-hidden="true" />
          Arreglar (ciclo de pantalla)
        </Boton>

        {confirmando ? (
          <span className="border-bad/50 flex flex-wrap items-center gap-2 rounded-md border px-2 py-1">
            <span className="text-bad text-xs" role="alert">
              ¿Seguro? Reiniciar PIERDE la VRAM y corta lo que estés generando.
            </span>
            <Boton
              variante="peligro"
              disabled={!!enCurso}
              onClick={() => void lanzar("gpu:reiniciar")}
            >
              Sí, reiniciar
            </Boton>
            <Boton onClick={() => setConfirmando(false)}>No</Boton>
          </span>
        ) : (
          <Boton variante="peligro" disabled={!!enCurso} onClick={() => setConfirmando(true)}>
            <IconAlertTriangle size={12} className="mr-1 inline" aria-hidden="true" />
            Reiniciar GPU…
          </Boton>
        )}
      </div>

      <p className="text-fg-faint text-xs">
        Primero <strong className="text-fg-muted">arreglar</strong>: cicla el modo de pantalla y lo deja como
        estaba. No necesita root, no pierde la VRAM y no corta lo que se esté generando (la pantalla parpadea un
        instante). <strong className="text-fg-muted">Reiniciar</strong> es la vía fuerte, para cuando el ciclo de
        pantalla no sube el reloj: necesita root y pierde la VRAM.
      </p>

      {resultado ? (
        <p
          className={clsx("text-xs", resultado.ok ? "text-fg-muted" : "text-bad")}
          role={resultado.ok ? undefined : "alert"}
        >
          {resultado.mensaje}
        </p>
      ) : null}
    </div>
  );
}

/**
 * El aviso de que el reloj está clavado en el mínimo.
 *
 * Devuelve `null` cuando no hay nada que avisar: el componente se puede soltar en
 * una vista sin que haya que comprobar antes si toca.
 */
export function AvisoMclk({ lectura, onRecargar }: { lectura: LecturaMclk; onRecargar: () => void }) {
  const enCurso = useApp((st) => st.accionEnCurso);
  if (lectura.fase !== "ok" || !lectura.datos.degradado) return null;
  const mclk = lectura.datos;

  return (
    <Card className="border-bad/40">
      <div className="flex items-start gap-2">
        <IconAlertTriangle size={16} className="text-bad mt-0.5 shrink-0" aria-hidden="true" />
        <div className="flex min-w-0 flex-col gap-1">
          <p className="text-sm" role="alert">
            <strong className="text-bad">Reloj de memoria de la GPU degradado.</strong> Con la GPU trabajando (
            {num(Math.max(mclk.gpu_busy, mclk.mem_busy))} %), el reloj de memoria sigue en el mínimo (
            {mhz(mclk.activo_mhz)}) en vez de subir a {mhz(mclk.max_mhz)}. Es lo que hace que los modelos vayan ~15
            veces más lentos, y no da ningún error.
          </p>
          <p className="text-fg-muted text-xs">
            Medido en este equipo con <span className="mono">llama-bench</span>: el 27B pasó de 3,27 a 45,81 tok/s
            y el 8B de 9,45 a 142,94 al dejar de estar degradado. Al revés: esas cifras son medidas, no
            estimaciones.
          </p>
          <AccionesMclk enCurso={enCurso} onRecargar={onRecargar} />
        </div>
      </div>
    </Card>
  );
}

/**
 * La ficha del reloj para vigilarlo: nivel activo, niveles de la tarjeta y
 * veredicto del backend.
 *
 * Los niveles se enseñan TODOS con el activo marcado, porque el valor solo
 * significa algo sabiendo de qué escala sale (96 es el mínimo de esta tarjeta,
 * no un número suelto). El color lo lleva la insignia del activo, así que un
 * lector de pantalla oye "activo" sin depender del color.
 */
export function PanelMclk({ lectura, onRecargar }: { lectura: LecturaMclk; onRecargar: () => void }) {
  const enCurso = useApp((st) => st.accionEnCurso);

  if (lectura.fase === "cargando") {
    return <p className="text-fg-muted text-xs">Leyendo el reloj de memoria…</p>;
  }
  if (lectura.fase === "error") {
    return (
      <p className="text-bad text-xs" role="alert">
        No se pudo leer el reloj de memoria: {lectura.mensaje}
      </p>
    );
  }
  if (lectura.fase === "sin-gpu") {
    return (
      <p className="text-fg-muted text-xs">
        Esta máquina no expone <code className="mono">pp_dpm_mclk</code>: no hay ninguna GPU amdgpu que
        vigilar.
      </p>
    );
  }

  const mclk = lectura.datos;
  const tonoNivel: Tono = mclk.degradado ? "bad" : "neutro";

  return (
    <div className="flex flex-col gap-2">
      <div className="flex justify-between text-xs">
        <span className="text-fg-faint">Reloj de memoria</span>
        <span className="mono">
          {mhz(mclk.activo_mhz)} de {mhz(mclk.max_mhz)}
        </span>
      </div>
      {/* Los niveles reales de la tarjeta, con el activo marcado: así se ve de un
          vistazo si está en el mínimo (96) o arriba (1000). */}
      <ul className="flex flex-wrap gap-1.5" aria-label="Niveles de reloj de memoria disponibles">
        {mclk.niveles.map((n) => (
          <li key={n.idx}>
            <Insignia tono={n.activo ? tonoNivel : "neutro"}>
              {n.mhz} MHz{n.activo ? " · activo" : ""}
            </Insignia>
          </li>
        ))}
      </ul>
      <p className="text-fg-faint text-xs">
        Carga: GPU {num(mclk.gpu_busy)} % · memoria {num(mclk.mem_busy)} %
      </p>
      {/* El veredicto lo redacta el backend: se enseña literal. */}
      <p className={clsx("text-xs", mclk.degradado ? "text-bad" : "text-fg-muted")}>{mclk.veredicto}</p>

      {/* Cuando está degradado, los botones van TAMBIÉN aquí: en Hardware no hay
          un aviso aparte arriba (el panel de al lado ya dice que está degradado
          en rojo), así que este es el único sitio donde se puede arreglar. */}
      <AccionesMclk enCurso={enCurso} onRecargar={onRecargar} />
    </div>
  );
}
