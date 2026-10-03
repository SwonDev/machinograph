/**
 * El panel de descargas: qué se está bajando, cuánto lleva y pararlo.
 *
 * POR QUÉ ES UN COMPONENTE Y NO UN BOTÓN QUE VUELCA LÍNEAS: una descarga de 20 GB
 * dura media hora, y durante ese rato el usuario necesita tres cosas que el texto
 * suelto no da: cuánto lleva (barra y porcentaje), cuánto queda (velocidad y
 * tiempo) y poder cortarla. Nada de eso es decoración.
 *
 * DE DÓNDE SALE CADA CIFRA, que es lo que la hace creíble:
 *  - El porcentaje y los GB los dice `llmfit download` en su salida (se leyeron de
 *    su código). Se enseñan tal cual.
 *  - La VELOCIDAD y el TIEMPO QUE QUEDA los mide Machinograph comparando dos lecturas de
 *    bytes con su tiempo, porque llmfit no los publica. Hasta que hay dos
 *    lecturas, se enseña «—» y se dice por qué: un «0 MB/s» parecería que la
 *    descarga está parada.
 *  - La última línea de llmfit se enseña LITERAL, para poder comprobarla.
 */
import { useEffect, useRef, useState } from "react";
import { IconDownload, IconRefresh, IconX } from "@tabler/icons-react";
import { api, onDescarga, type Descarga } from "../lib/tauri";
import { Barra, Boton, Card, Datos, Etiqueta, Insignia } from "./ui";
import { bPorSegundo, dur, num } from "../lib/format";

/** El nombre de cada fase, en una palabra. */
const ETIQUETA_FASE: Record<Descarga["fase"], string> = {
  preparando: "preparando",
  descargando: "descargando",
  cerrando: "verificando",
  terminada: "terminada",
  cancelada: "cancelada",
  fallida: "falló",
};

const TONO_FASE: Record<Descarga["fase"], "ok" | "acento" | "bad" | "neutro"> = {
  preparando: "acento",
  descargando: "acento",
  cerrando: "acento",
  terminada: "ok",
  cancelada: "neutro",
  fallida: "bad",
};

/**
 * El panel.
 *
 * Se alimenta de dos sitios y los dos hacen falta: el ESTADO inicial (por si la
 * descarga empezó antes de abrir esta vista, o antes de recargar) y el EVENTO
 * `ai:descarga`, que es lo que hace que la barra avance sin pedir nada.
 */
export function PanelDescarga() {
  const [d, setD] = useState<Descarga | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ocupado, setOcupado] = useState(false);
  // La descarga en curso se marca como "vista" para no volver a pedir el estado
  // en cada línea que llega.
  const pidioEstado = useRef(false);

  useEffect(() => {
    if (pidioEstado.current) return;
    pidioEstado.current = true;
    api.descarga
      .estado()
      .then((e) => {
        // El estado guarda la ÚLTIMA descarga, aunque ya haya terminado: si nunca
        // hubo ninguna, la fase es "preparando" con el modelo vacío, y eso no se
        // enseña como si hubiera una en curso.
        if (e.modelo) setD(e);
      })
      .catch(() => {
        // Sin backend no hay descargas que mirar; el panel no se pinta.
      });
  }, []);

  useEffect(() => {
    let baja: (() => void) | null = null;
    let desmontado = false;
    onDescarga((nueva) => {
      if (!desmontado) setD(nueva);
    })
      .then((u) => (desmontado ? u() : (baja = u)))
      .catch(() => {
        /* fuera del host Tauri no hay eventos: el panel se queda con lo que tenga */
      });
    return () => {
      desmontado = true;
      baja?.();
    };
  }, []);

  if (!d) return null;
  const enCurso = ["preparando", "descargando", "cerrando"].includes(d.fase);

  const cancelar = async () => {
    setOcupado(true);
    setError(null);
    try {
      setAviso(await api.descarga.cancelar());
    } catch (e) {
      setError(String(e));
    } finally {
      setOcupado(false);
    }
  };

  const pct = d.pct == null ? null : Math.max(0, Math.min(100, d.pct));

  return (
    <Card>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <IconDownload size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Descarga</Etiqueta>
        <Insignia tono={TONO_FASE[d.fase]}>{ETIQUETA_FASE[d.fase]}</Insignia>
        <span className="mono min-w-0 flex-1 truncate text-xs" title={d.modelo}>
          {d.modelo}
        </span>
        {enCurso ? (
          <Boton variante="peligro" disabled={ocupado} onClick={() => void cancelar()}>
            <IconX size={12} className="mr-1 inline" aria-hidden="true" />
            Cancelar
          </Boton>
        ) : null}
      </div>

      {/* La barra. Sin porcentaje todavía (llmfit aún no lo ha dicho) va en gris y
          con el texto de la fase: una barra a 0% parecería parada. */}
      {pct == null ? (
        <div className="bg-raised h-1 w-full overflow-hidden rounded-full" aria-hidden="true" />
      ) : (
        <Barra valor={pct} />
      )}

      <div className="mt-3">
        <Datos
          items={[
            [
              "Progreso",
              <span key="p" className="mono">
                {pct == null ? "—" : `${num(pct, 1)} %`}
                {d.descargado_gb != null && d.total_gb != null ? (
                  <span className="text-fg-muted">
                    {` · ${num(d.descargado_gb, 1)} de ${num(d.total_gb, 1)} GB`}
                  </span>
                ) : null}
              </span>,
            ],
            [
              "Velocidad",
              d.b_s == null ? (
                <span key="v" className="text-fg-faint" title="Se mide comparando dos lecturas de bytes; llmfit no la publica. Aparece al segundo de empezar.">
                  — (midiendo)
                </span>
              ) : (
                <span key="v" className="mono">{bPorSegundo(d.b_s)}</span>
              ),
            ],
            [
              "Queda",
              d.eta_s == null ? (
                <span key="e" className="text-fg-faint">
                  —
                </span>
              ) : (
                <span key="e" className="mono">unos {dur(d.eta_s)}</span>
              ),
            ],
            [
              "Va a",
              <span key="c" className="mono break-all text-xs">
                {d.carpeta ?? "—"}
              </span>,
            ],
          ]}
        />
      </div>

      {/* La última línea de llmfit, LITERAL: es la prueba de lo que está pasando y
          permite compararla con lo que imprime el binario a mano. */}
      <p className="mono text-fg-faint mt-2 truncate text-[11px]" title={d.linea}>
        {d.linea}
      </p>

      {d.error ? (
        <p className="text-bad mt-2 text-xs" role="alert">
          {d.error}
        </p>
      ) : null}
      {aviso ? <p className="text-fg-muted mt-2 text-xs">{aviso}</p> : null}
      {error ? (
        <p className="text-bad mt-2 text-xs" role="alert">
          {error}
        </p>
      ) : null}

      {d.fase === "terminada" ? (
        <p className="text-fg-muted mt-2 text-xs">
          Terminada. El modelo se puede servir en cuanto aparezca en <strong className="text-fg">En disco</strong>: si
          no sale, el inventario todavía no ha vuelto a mirar la carpeta.
        </p>
      ) : null}
      {d.fase === "fallida" ? (
        <p className="text-fg-muted mt-2 flex items-center gap-2 text-xs">
          <IconRefresh size={12} aria-hidden="true" />
          Se puede volver a lanzar desde el botón del modelo en la tabla: no queda a medias.
        </p>
      ) : null}
    </Card>
  );
}
