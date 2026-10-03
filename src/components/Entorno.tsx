/**
 * Lo del entorno que se configura desde Ajustes: la red, el arranque automático y
 * las carpetas de modelos.
 *
 * LAS TRES COSAS SE PUEDEN COMPROBAR ANTES DE TOCARLAS, y por eso van juntas:
 *  - La dirección: se lee del sistema y se ENSEÑA la URL con la que quedaría
 *    (con la IP de red de verdad, no con un ejemplo), antes de guardar nada.
 *  - El arranque automático: es un fichero, y se dice cuál y qué comando lleva.
 *    Activarlo escribe el binario QUE ESTÁ EN MARCHA, no una ruta inventada.
 *  - Las carpetas de modelos: son las que el inventario recorre de verdad, así que
 *    esta lista no puede discrepar de lo que aparece en En disco.
 *
 * Y la frontera de seguridad se dice ARRIBA, en tamaño de lectura: abrir la puerta
 * a la red es lo único de esta pantalla que expone la máquina a los demás, y un
 * aviso al pie no lo leería nadie.
 */
import { useCallback, useEffect, useState } from "react";
import { IconDeviceDesktop, IconFolder, IconWorld } from "@tabler/icons-react";
import { api, type ArranqueAuto, type CarpetaModelos, type EntornoRed, type GatewayEstado } from "../lib/tauri";
import { useApp } from "../store";
import { Boton, Card, Datos, Etiqueta, Insignia } from "./ui";

/**
 * Dónde vive el arranque automático en cada sistema. Se enseña el de ESTE sistema
 * (el `so` viene de la foto): antes ponía siempre «~/.config/autostart», que en
 * macOS y Windows es una ruta que no existe.
 */
const MECANISMO_ARRANQUE: Record<string, string> = {
  linux:
    "En Linux es un lanzador de escritorio en ~/.config/autostart (el mecanismo estándar del escritorio).",
  macos:
    "En macOS es un LaunchAgent tuyo en ~/Library/LaunchAgents, que launchd carga al iniciar sesión.",
  windows:
    "En Windows es un valor del usuario en la clave Run del registro (HKCU).",
};

/* ── Red ──────────────────────────────────────────────────────────────────── */

/** Las opciones de escucha, que son tres y con significados distintos. */
function opcionesEscucha(red: EntornoRed | null, actual: string) {
  const ip = red?.ip ?? null;
  const lista = [
    {
      valor: "127.0.0.1",
      etiqueta: "Solo este equipo",
      detalle: "Solo se puede conectar quien esté en esta máquina. Es lo más seguro.",
      expone: false,
    },
  ];
  if (ip) {
    lista.push({
      valor: ip,
      etiqueta: `Solo esta red (${ip})`,
      detalle: `Alcanzable desde la red local en la que está este equipo, y solo por esa dirección.`,
      expone: true,
    });
  }
  lista.push({
    valor: "0.0.0.0",
    etiqueta: "Todas las direcciones",
    detalle:
      "Acepta conexiones por cualquier dirección de este equipo, incluidas las de otras redes si las tiene.",
    expone: true,
  });
  return lista.map((o) => ({ ...o, activa: actual === o.valor }));
}

export function PuertaEnRed({
  gw,
  onCambio,
}: {
  gw: GatewayEstado | null;
  onCambio: () => void;
}) {
  const [red, setRed] = useState<EntornoRed | null>(null);
  const [ocupado, setOcupado] = useState(false);
  const [aviso, setAviso] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const cargar = useCallback(async () => {
    try {
      setRed(await api.entorno.red());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const cambiar = async (direccion: string) => {
    setOcupado(true);
    setError(null);
    try {
      setAviso(await api.gateway.configurar({ direccion }));
      onCambio();
    } catch (e) {
      setError(String(e));
    } finally {
      setOcupado(false);
    }
  };

  const opciones = opcionesEscucha(red, gw?.direccion ?? "127.0.0.1");
  // El puerto REAL si la puerta está escuchando (puede no ser el configurado: si
  // estaba ocupado, arranca en el siguiente). Enseñar el configurado mandaría a
  // los clientes a una dirección donde no hay nada.
  const urlRed = red?.ip && gw ? `http://${red.ip}:${gw.puerto_escuchando ?? gw.puerto}/v1` : null;

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconWorld size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Puerta de enlace en la red</Etiqueta>
        {gw?.requiere_clave ? (
          <Insignia tono="ok">pide clave</Insignia>
        ) : (
          <Insignia tono="warn">sin clave</Insignia>
        )}
      </div>

      {/* La frontera, arriba y en tamaño de lectura. */}
      <p className="text-fg-muted text-xs">
        Esto es lo único de Machinograph que expone la máquina a los demás: la puerta sirve el modelo a quien la llame.
        Con <strong className="text-fg">clave obligatoria</strong> es razonable abrirla a tu red local; sin clave,
        cualquiera que llegue al puerto puede usar tu GPU. Nada más del programa se expone: el panel, los modelos y
        las acciones siguen siendo solo de este equipo.
      </p>

      <div className="mt-3 flex flex-col gap-2">
        {opciones.map((o) => (
          <label
            key={o.valor}
            className={`flex cursor-pointer items-start gap-2 rounded-md border px-3 py-2 ${
              o.activa ? "border-accent/50 bg-accent-soft" : "border-line-soft"
            }`}
          >
            <input
              type="radio"
              name="gateway-direccion"
              checked={o.activa}
              disabled={ocupado || gw == null}
              onChange={() => void cambiar(o.valor)}
              className="mt-0.5 accent-accent"
            />
            <span className="min-w-0">
              <span className="text-sm">{o.etiqueta}</span>
              {o.expone && !gw?.requiere_clave ? (
                <Insignia tono="warn">
                  <span className="ml-1">sin clave: cualquiera de la red puede usarlo</span>
                </Insignia>
              ) : null}
              <span className="text-fg-muted block text-xs">{o.detalle}</span>
            </span>
          </label>
        ))}
      </div>

      <div className="mt-3">
        <Datos
          items={[
            ["Escucha en", <span key="e" className="mono">{gw?.direccion ?? "—"}:{gw?.puerto ?? "—"}</span>],
            [
              "Desde otro equipo",
              urlRed ? (
                <span key="u" className="mono">{urlRed}</span>
              ) : (
                <span key="u" className="text-fg-faint">
                  — (este equipo no tiene red)
                </span>
              ),
            ],
            [
              "Interfaces",
              <span key="i" className="text-xs">
                {red == null
                  ? "leyendo…"
                  : red.interfaces.length === 0
                    ? "—"
                    : red.interfaces.map((i) => `${i.nombre} ${i.ip}`).join(" · ")}
              </span>,
            ],
          ]}
        />
      </div>

      {aviso ? <p className="text-fg-muted mt-2 text-xs">{aviso}</p> : null}
      {error ? (
        <p className="text-bad mt-2 text-xs" role="alert">
          {error}
        </p>
      ) : null}
      <p className="text-fg-faint mt-2 text-xs">
        Cambiar de dirección reinicia la puerta. La URL de «desde otro equipo» es la que hay que apuntar en el
        cliente: si tu red cambia (te vas a otra casa, entras por una VPN), cambia con ella.
      </p>
    </Card>
  );
}

/* ── Arranque automático ──────────────────────────────────────────────────── */

export function ArranqueAutomatico() {
  const [estado, setEstado] = useState<ArranqueAuto | null>(null);
  const [ocupado, setOcupado] = useState(false);
  const [aviso, setAviso] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // El sistema sale de la foto: el texto tiene que decir dónde vive el arranque
  // EN ESTE sistema (en Linux un lanzador, en macOS un LaunchAgent, en Windows una
  // clave del registro), no siempre una ruta de Linux.
  const so = useApp((st) => st.snapshot?.so);

  const cargar = useCallback(async () => {
    try {
      setEstado(await api.entorno.arranque());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void cargar();
  }, [cargar]);

  const cambiar = async (activar: boolean) => {
    setOcupado(true);
    setError(null);
    try {
      setAviso(await api.entorno.setArranque(activar));
      await cargar();
    } catch (e) {
      setError(String(e));
    } finally {
      setOcupado(false);
    }
  };

  // Si no se pudo comprobar, NO se dice «desactivado»: eso sería afirmar algo que
  // no se sabe. Se dice sin comprobar y con el motivo.
  const sinComprobar = !!estado?.error;

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconDeviceDesktop size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Arranque al iniciar sesión</Etiqueta>
        <Insignia tono={sinComprobar ? "warn" : estado?.activado ? "ok" : "neutro"}>
          {sinComprobar ? "sin comprobar" : estado?.activado ? "activado" : "desactivado"}
        </Insignia>
      </div>
      <p className="text-fg-muted text-xs">
        {MECANISMO_ARRANQUE[so ?? ""] ??
          "Lo gestiona el propio sistema, con el mecanismo estándar de la plataforma."}{" "}
        Activar solo escribe ahí; desactivar lo borra y no deja nada más, así que también se puede
        quitar a mano.
      </p>
      <div className="mt-3">
        <Datos
          items={[
            [
              // En Windows no es un fichero: es una clave del registro. El rótulo lo dice.
              so === "windows" ? "Clave" : "Fichero",
              <span key="f" className="mono break-all text-xs">{estado?.fichero || "—"}</span>,
            ],
            [
              "Lanzaría",
              <span key="c" className="mono break-all text-xs">
                {estado?.comando ?? (estado?.activado ? "el arranque no dice con qué comando" : "—")}
              </span>,
            ],
          ]}
        />
      </div>
      {estado?.error ? (
        <p className="text-warn mt-2 text-xs" role="status">
          No se pudo comprobar el arranque: {estado.error}
        </p>
      ) : null}
      <div className="mt-3 flex items-center gap-2">
        <Boton
          variante={estado?.activado ? "normal" : "acento"}
          disabled={ocupado || estado == null || sinComprobar}
          onClick={() => void cambiar(!estado?.activado)}
        >
          {estado?.activado ? "Desactivar" : "Activar"}
        </Boton>
        {aviso ? <span className="text-fg-muted text-xs">{aviso}</span> : null}
      </div>
      {error ? (
        <p className="text-bad mt-2 text-xs" role="alert">
          {error}
        </p>
      ) : null}
      <p className="text-fg-faint mt-2 text-xs">
        El comando que se escribe es la ruta del binario que está en marcha ahora mismo
        {estado?.comando ? (
          <>
            {" "}
            (<span className="mono">{estado.comando}</span>)
          </>
        ) : null}
        , no una ruta fija: si mueves la app, el arranque apunta a donde esté.
      </p>
    </Card>
  );
}

/* ── Carpetas de modelos ──────────────────────────────────────────────────── */

export function CarpetasModelos() {
  const [carpetas, setCarpetas] = useState<CarpetaModelos[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.entorno
      .carpetas()
      .then((r) => setCarpetas(r.carpetas))
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconFolder size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Carpetas de modelos</Etiqueta>
        <span className="text-fg-faint ml-auto text-xs">
          {carpetas == null ? "leyendo…" : `${carpetas.filter((c) => c.existe).length} de ${carpetas.length} existen`}
        </span>
      </div>
      <p className="text-fg-muted text-xs">
        Estas son las carpetas que recorre el inventario, con la familia a la que pertenecen. Son las MISMAS que se
        miran para En disco: aquí no hay una lista aparte que se pueda quedar vieja. La variable{" "}
        <span className="mono">MACHINOGRAPH_MODEL_DIRS</span> permite añadir más sin tocar el programa.
      </p>
      {error ? (
        <p className="text-bad mt-2 text-xs" role="alert">
          No se pudieron leer: {error}
        </p>
      ) : null}
      <ul className="mt-2 flex flex-col gap-1">
        {(carpetas ?? []).map((c) => (
          <li key={c.ruta} className="row">
            <span className="min-w-0 flex-1 truncate" title={c.ruta}>
              <span className="mono text-xs">{c.ruta}</span>
            </span>
            <Insignia tono="neutro">{c.familia}</Insignia>
            {c.existe ? (
              <Insignia tono="ok">existe</Insignia>
            ) : (
              <Insignia tono="neutro">
                <span title="Esta carpeta no está en este equipo. No es un error: no todo el mundo tiene cada motor instalado.">
                  no está
                </span>
              </Insignia>
            )}
          </li>
        ))}
      </ul>
    </Card>
  );
}
