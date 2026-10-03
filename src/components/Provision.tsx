/**
 * «Lo que Machinograph necesita»: las herramientas que la app usa, de dónde salen y en
 * qué estado están, con un botón para prepararlas todas.
 *
 * POR QUÉ UNA TARJETA Y NO UN AVISO EMERGENTE: instalar es una acción de RED, así
 * que no puede pasar en silencio. Aquí se ve qué se está bajando, el porcentaje
 * REAL (bytes descargados sobre el total que da la API de releases), la velocidad
 * MEDIDA y se puede cancelar. Y cuando algo no se puede instalar sola (amd-smi
 * viene con ROCm y necesita root) NO se finge: se dice el motivo y el comando
 * exacto del gestor de paquetes de este sistema.
 *
 * DE DÓNDE SALE CADA COSA: el estado lo da `provision:estado`, que ejecuta cada
 * binario para comprobar que arranca; el progreso lo empuja el backend por
 * `ai:provision`. La tarjeta no inventa ningún número.
 */
import { useCallback, useEffect, useState } from "react";
import { IconDownload, IconRefresh, IconX } from "@tabler/icons-react";
import {
  api,
  onProvision,
  type FaseProvision,
  type HerramientaProvision,
  type ProgresoProvision,
  type ProvisionEstado,
} from "../lib/tauri";
import { Barra, Boton, BotonCopiar, Card, Datos, Etiqueta, Insignia, type Tono } from "./ui";
import { bLegibles, bPorSegundo, dur, num } from "../lib/format";

/** Cómo se llama cada estado, en una palabra. */
const ETIQUETA_ESTADO: Record<HerramientaProvision["estado"], string> = {
  listo: "listo",
  falta: "falta",
  descargando: "descargando",
  roto: "roto",
  noinstalable: "no instalable",
};

const TONO_ESTADO: Record<HerramientaProvision["estado"], Tono> = {
  listo: "ok",
  falta: "warn",
  descargando: "acento",
  roto: "bad",
  noinstalable: "neutro",
};

const ETIQUETA_FASE: Record<FaseProvision, string> = {
  preparando: "preparando",
  descargando: "descargando",
  verificando: "verificando",
  extrayendo: "extrayendo",
  terminada: "terminada",
  cancelada: "cancelada",
  fallida: "falló",
};

/** Las fases que siguen en marcha (por eso se puede cancelar). */
const EN_MARCHA: FaseProvision[] = ["preparando", "descargando", "verificando", "extrayendo"];

export function Provision() {
  const [datos, setDatos] = useState<ProvisionEstado | null>(null);
  const [progreso, setProgreso] = useState<ProgresoProvision | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ocupado, setOcupado] = useState(false);

  const recargar = useCallback(async () => {
    try {
      setDatos(await api.provision.estado());
      setError(null);
    } catch (e) {
      // Fuera del host Tauri (Vite en el navegador) esto falla siempre: se dice.
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void recargar();
  }, [recargar]);

  useEffect(() => {
    let baja: (() => void) | null = null;
    let desmontado = false;
    onProvision((a) => {
      if (desmontado) return;
      setProgreso(a.en_curso ?? a.ultimo);
      // Cuando ya no hay nada en curso, se vuelve a pedir el estado: es lo que
      // hace que una fila pase de «descargando» a «listo» (con su versión) sin
      // recargar la vista a mano.
      if (!a.en_curso) void recargar();
    })
      .then((u) => (desmontado ? u() : (baja = u)))
      .catch(() => {
        /* fuera del host Tauri no hay eventos: la tarjeta se queda con lo que tenga */
      });
    return () => {
      desmontado = true;
      baja?.();
    };
  }, [recargar]);

  const lanzar = async (accion: () => Promise<string>) => {
    setOcupado(true);
    setError(null);
    setAviso(null);
    try {
      setAviso(await accion());
    } catch (e) {
      setError(String(e));
    } finally {
      setOcupado(false);
    }
  };

  const cambiarAuto = async () => {
    if (!datos) return;
    const nuevo = !datos.auto_provision;
    setOcupado(true);
    setError(null);
    try {
      const valor = await api.provision.auto(nuevo);
      setDatos({ ...datos, auto_provision: valor });
    } catch (e) {
      setError(String(e));
    } finally {
      setOcupado(false);
    }
  };

  const herramientas = datos?.herramientas ?? [];
  const enCurso = progreso != null && EN_MARCHA.includes(progreso.fase);
  const pendientes = herramientas.filter((h) => h.estado === "falta" || h.estado === "roto");
  const manualesNuevas = herramientas.filter((h) => h.estado === "noinstalable");
  const todoListo = datos != null && pendientes.length === 0 && manualesNuevas.length === 0;

  return (
    <section className="flex flex-col gap-3">
      <Etiqueta>Lo que Machinograph necesita</Etiqueta>
      <Card className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-fg-muted flex-1 text-xs">
            {datos == null
              ? "Comprobando…"
              : todoListo
                ? "Todo listo: las herramientas que usa Machinograph están instaladas y arrancan."
                : pendientes.length > 0
                  ? `${pendientes.length} herramienta(s) por preparar. Se instalan en tu carpeta de datos, sin permisos de administrador.`
                  : "Todo lo instalable está listo; hay algo que solo puedes instalar tú (abajo, con su comando)."}
          </span>
          <Boton
            variante="acento"
            disabled={ocupado || datos == null || pendientes.length === 0}
            onClick={() => void lanzar(() => api.provision.instalar())}
          >
            <IconDownload size={12} className="mr-1 inline" aria-hidden="true" />
            Preparar todo
          </Boton>
          <Boton
            disabled={ocupado || datos == null}
            onClick={() => void lanzar(() => api.provision.reparar())}
            title="Vuelve a comprobar que cada binario arranca y repara lo que esté roto"
          >
            <IconRefresh size={12} className="mr-1 inline" aria-hidden="true" />
            Comprobar ahora
          </Boton>
          {enCurso ? (
            <Boton variante="peligro" disabled={ocupado} onClick={() => void lanzar(() => api.provision.cancelar())}>
              <IconX size={12} className="mr-1 inline" aria-hidden="true" />
              Cancelar
            </Boton>
          ) : null}
        </div>

        {/* Lo que NO se puede instalar desde la app, dicho en una línea: el motivo
            (root) y el nombre. El comando exacto de este sistema, con su botón de
            copiar, va en la fila de cada una. */}
        {manualesNuevas.length > 0 ? (
          <p className="text-warn text-xs" role="status">
            Pendiente de root (la app no puede hacerlo sola): {manualesNuevas.map((h) => h.nombre).join(", ")}. Su
            comando exacto está abajo, con botón de copiar.
          </p>
        ) : null}

        {/* El progreso, con las cifras REALES: el porcentaje sale del total que
            publica la API y la velocidad se mide aquí, comparando dos lecturas. */}
        {progreso ? (
          <div className="border-line-soft flex flex-col gap-2 border-b pb-3">
            <div className="flex flex-wrap items-center gap-2">
              <Insignia tono={progreso.fase === "fallida" ? "bad" : progreso.fase === "terminada" ? "ok" : "acento"}>
                {ETIQUETA_FASE[progreso.fase]}
              </Insignia>
              <span className="mono min-w-0 flex-1 truncate text-xs">
                {progreso.herramienta || "Instalación"}
              </span>
            </div>
            {progreso.fase === "descargando" ? (
              <>
                <Barra valor={progreso.pct ?? 0} />
                <Datos
                  items={[
                    [
                      "Progreso",
                      <span key="p" className="mono">
                        {progreso.pct == null ? "—" : `${num(progreso.pct, 1)} %`}
                        <span className="text-fg-muted">
                          {` · ${bLegibles(progreso.bajado_bytes)} de ${bLegibles(progreso.total_bytes)}`}
                        </span>
                      </span>,
                    ],
                    [
                      "Velocidad",
                      progreso.b_s == null ? (
                        <span key="v" className="text-fg-faint">
                          — (midiendo)
                        </span>
                      ) : (
                        <span key="v" className="mono">
                          {bPorSegundo(progreso.b_s)}
                        </span>
                      ),
                    ],
                    [
                      "Queda",
                      progreso.eta_s == null ? (
                        <span key="e" className="text-fg-faint">
                          —
                        </span>
                      ) : (
                        <span key="e" className="mono">
                          unos {dur(progreso.eta_s)}
                        </span>
                      ),
                    ],
                  ]}
                />
              </>
            ) : null}
            <p className="mono text-fg-faint truncate text-[11px]" title={progreso.linea}>
              {progreso.linea}
            </p>
            {progreso.error ? (
              <p className="text-bad text-xs" role="alert">
                {progreso.error}
              </p>
            ) : null}
          </div>
        ) : null}

        {/* Una fila por herramienta: estado, de dónde sale, versión y ruta. */}
        {herramientas.length === 0 ? (
          <p className="text-fg-muted text-sm">
            No se pudo leer la lista de herramientas. Suele ser que el backend no está disponible
            (por ejemplo, abierto en el navegador en vez de en la aplicación).
          </p>
        ) : (
          <ul className="flex flex-col divide-y divide-[var(--color-line-soft)]">
            {herramientas.map((h) => (
              <li key={h.id} className="flex flex-col gap-1 py-2 first:pt-0 last:pb-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-fg text-xs font-medium">{h.nombre}</span>
                  <Insignia tono={TONO_ESTADO[h.estado]}>{ETIQUETA_ESTADO[h.estado]}</Insignia>
                  {h.imprescindible ? <Insignia tono="neutro">imprescindible</Insignia> : null}
                  {h.version ? (
                    <span className="mono text-fg-muted text-[11px]">{h.version}</span>
                  ) : null}
                </div>
                <p className="text-fg-muted text-xs">{h.para_que}</p>
                <p className="mono text-fg-faint break-all text-[11px]" title={h.origen}>
                  {h.ruta ?? h.origen}
                </p>
                {h.ruta ? (
                  <p className="mono text-fg-faint break-all text-[11px]">sale de {h.origen}</p>
                ) : null}
                {h.detalle ? (
                  <p className={h.estado === "roto" ? "text-bad text-xs" : "text-fg-muted text-xs"}>
                    {h.detalle}
                  </p>
                ) : null}
                {h.motivo_manual ? <p className="text-fg-muted text-xs">{h.motivo_manual}</p> : null}
                {h.comando_manual && h.estado === "noinstalable" ? (
                  <div className="flex flex-wrap items-center gap-2">
                    <code className="mono border-line bg-raised rounded-md border px-2 py-1 text-[11px]">
                      {h.comando_manual}
                    </code>
                    <BotonCopiar texto={h.comando_manual} que={`el comando para instalar ${h.nombre}`} />
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
        )}

        <label className="flex items-start gap-2 text-xs">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={datos?.auto_provision ?? false}
            disabled={ocupado || datos == null}
            onChange={() => void cambiarAuto()}
          />
          <span className="text-fg-muted">
            Instalar automáticamente lo que falte al arrancar. Por defecto está activada; lo que se
            descargue se verá aquí, con su progreso y su botón de cancelar. La preferencia se guarda
            en <code className="mono">provision.json</code>, dentro de la carpeta de datos de Machinograph.
          </span>
        </label>

        {aviso ? (
          <p className="text-ok text-xs" role="status">
            {aviso}
          </p>
        ) : null}
        {error ? (
          <p className="text-bad text-xs" role="alert">
            {error}
          </p>
        ) : null}
      </Card>
    </section>
  );
}
