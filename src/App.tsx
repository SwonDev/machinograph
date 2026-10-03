/**
 * Armazón de la aplicación: barra lateral AGRUPADA + cabecera de contexto.
 *
 * POR QUÉ AGRUPADA: antes eran doce secciones planas y varias se solapaban
 * (Modelos e Inventario leían la misma fuente, Panel y Sistema partían el
 * hardware, Diagnóstico repetía avisos del Panel). Los cuatro grupos responden a
 * cuatro preguntas distintas —qué me toca, qué modelos, quién sirve quién usa,
 * cómo va la máquina— y cada sección lleva UNA línea que dice para qué es: el
 * problema no era que faltara información, era que no se sabía qué había dónde.
 *
 * El estado NO se pide aquí: el backend empuja `ai:snapshot` con el intervalo
 * del ajuste `snapshot_interval_ms` (2 s por defecto) y este
 * componente solo se suscribe una vez (y se da de baja al desmontar, que en
 * StrictMode pasa enseguida — de ahí el patrón de `desmontado`).
 */
import { useEffect, useRef } from "react";
import { clsx } from "clsx";
import {
  IconActivity, IconAdjustments, IconBox, IconCpu, IconDatabase, IconDeviceDesktop, IconGauge,
  IconPlugConnected, IconRecycle, IconRefresh, IconServer, IconShieldLock, IconSparkles, IconStack2, IconStethoscope,
  IconChartHistogram, IconAlertTriangle,
} from "@tabler/icons-react";
import { useApp, GRUPOS, type Vista } from "./store";
import { onAction, onFit, onSnapshot, onUpdateLine, api } from "./lib/tauri";
import { dur } from "./lib/format";
import { Insignia } from "./components/ui";
import Inicio from "./views/Inicio";
import Servers from "./views/Servers";
import Disco from "./views/Disco";
import Pantalla from "./views/Pantalla";
import Hardware from "./views/Hardware";
import Almacenamiento from "./views/Almacenamiento";
import Optimizacion from "./views/Optimizacion";
import Seguridad from "./views/Seguridad";
import Diagnostico from "./views/Diagnostico";
import Descubrir from "./views/Descubrir";
import Rendimiento from "./views/Rendimiento";
import Mantenimiento from "./views/Mantenimiento";
import Uso from "./views/Uso";
import Conexiones from "./views/Conexiones";
import Ajustes from "./views/Ajustes";

/**
 * Nombre y propósito de cada sección.
 *
 * El propósito se enseña en la cabecera y en el `title` del botón: es lo que
 * convierte "Rendimiento" en "qué mide y de dónde sale" sin tener que entrar.
 */
const SECCIONES: Record<Vista, { nombre: string; para: string; Icono: typeof IconActivity }> = {
  inicio: {
    nombre: "Inicio",
    para: "Qué está pasando y qué te toca hacer",
    Icono: IconActivity,
  },
  descubrir: {
    nombre: "Descubrir",
    para: "Modelos que le encajan a este equipo, con su nota y su estimación",
    Icono: IconSparkles,
  },
  disco: {
    nombre: "En disco",
    para: "Todo lo que ocupa espacio, de cualquier familia y tipo",
    Icono: IconBox,
  },
  rendimiento: {
    nombre: "Rendimiento",
    para: "Encaje medido, plan de hardware, capacidad simultánea y banco de pruebas",
    Icono: IconGauge,
  },
  servidores: {
    nombre: "Servidores",
    para: "Qué motor está en marcha, en qué puerto y qué modelos sirve",
    Icono: IconServer,
  },
  uso: {
    nombre: "Uso",
    para: "Tokens, velocidad y tiempo de respuesta de lo que se ha servido",
    Icono: IconChartHistogram,
  },
  conexiones: {
    nombre: "Conexiones",
    para: "Qué cliente usa este motor y con qué configuración",
    Icono: IconPlugConnected,
  },
  hardware: {
    nombre: "Hardware",
    para: "CPU, memoria, GPU y disco, con las últimas horas",
    Icono: IconCpu,
  },
  pantalla: {
    nombre: "Pantalla",
    para: "Salidas conectadas, modos y cómo reaplicarlos",
    Icono: IconDeviceDesktop,
  },
  almacenamiento: {
    nombre: "Almacenamiento",
    para: "Qué ocupa el disco, ordenado por tamaño, y qué se puede borrar",
    Icono: IconDatabase,
  },
  optimizacion: {
    nombre: "Optimización",
    para: "Basura que se puede limpiar sin miedo y programas que arrancan solos",
    Icono: IconRecycle,
  },
  seguridad: {
    nombre: "Seguridad",
    para: "Qué se ejecuta sin que lo veas, con su prueba, y qué huellas dejas",
    Icono: IconShieldLock,
  },
  diagnostico: {
    nombre: "Diagnóstico",
    para: "Qué está mal, por qué y cómo arreglarlo",
    Icono: IconStethoscope,
  },
  mantenimiento: {
    nombre: "Mantenimiento",
    para: "Lo que se ha ejecutado, el registro de la app y las copias para volver atrás",
    Icono: IconRefresh,
  },
  ajustes: {
    nombre: "Ajustes",
    para: "Qué servidores hay dados de alta y cómo se comporta la app",
    Icono: IconAdjustments,
  },
};

export default function App() {
  const vista = useApp((s) => s.vista);
  const setVista = useApp((s) => s.setVista);
  const snap = useApp((s) => s.snapshot);
  const error = useApp((s) => s.ultimoError);
  const aplicar = useApp((s) => s.aplicarSnapshot);
  const anadirLinea = useApp((s) => s.anadirLinea);
  const aplicarFit = useApp((s) => s.aplicarFit);
  const setError = useApp((s) => s.setError);
  const cargarServidores = useApp((s) => s.cargarServidores);
  const cargarHistorial = useApp((s) => s.cargarHistorial);

  /**
   * Región de contenido, para poder llevar el foco al cambiar de sección.
   *
   * Quien navega con teclado o lector de pantalla cambia de sección con la barra
   * lateral: si el foco se quedara en el botón de la barra, el siguiente `Tab`
   * seguiría moviéndose por los botones de sección en vez de entrar en lo que
   * acaba de aparecer. Con `tabindex="-1"` la región se puede enfocar sin
   * meterse en el orden normal de tabulación.
   */
  const contenido = useRef<HTMLElement | null>(null);
  const primeraVez = useRef(true);
  useEffect(() => {
    // Al arrancar NO se roba el foco (nadie ha cambiado nada todavía).
    if (primeraVez.current) {
      primeraVez.current = false;
      return;
    }
    contenido.current?.focus();
  }, [vista]);

  useEffect(() => {
    let desmontado = false;
    const bajas: (() => void)[] = [];

    // El viewport CSS tiene que llegar al mínimo del diseño (960x640; ver
    // DESIGN.md §4). El backend no puede saberlo —en X11 su `scale_factor` dice 1
    // mientras WebKit pinta a 1,45, así que la ventana abría con 882x551— y aquí
    // sí se sabe: se mide y se pide la corrección UNA vez. Si ya llega, el
    // backend no toca nada; y si el usuario encoge la ventana luego, no se le
    // discute (esto solo corre al arrancar).
    if (window.innerWidth < 960 || window.innerHeight < 640) {
      api
        .ajustarVentana(window.innerWidth, window.innerHeight)
        .catch(() => {
          // Fuera del host Tauri no hay ventana que ajustar, y tampoco es un
          // fallo que haya que enseñar: la interfaz sigue siendo la misma.
        });
    }

    // Foto inicial (el evento llega con el intervalo configurado, pero la
    // primera ventana no debería estar vacía mientras tanto).
    api
      .snapshot()
      .then((s) => {
        if (!desmontado) aplicar(s);
      })
      .catch((e) => {
        // Sin backend la primera lectura también falla: se deja dicho, y el
        // guardia `desmontado` evita escribir cuando la ventana ya no está.
        if (!desmontado) setError("foto", String(e));
      });
    void cargarServidores();

    /**
     * Alta de una suscripción a los eventos del backend.
     *
     * Fuera del host Tauri `listen()` RECHAZA (no hay puente que registre el
     * callback): sin `.catch` cada suscripción dejaba un rechazo de promesa sin
     * capturar —tres `pageerror` en Chromium— y el usuario no se enteraba de que
     * no había conexión. Aquí el motivo se guarda en el estado, y la baja se
     * aplica al desmontar (si ya se desmontó, se da de baja en el acto).
     */
    const suscribir = (alta: Promise<() => void>) => {
      alta
        .then((u) => (desmontado ? u() : bajas.push(u)))
        .catch((e) => {
          if (!desmontado) setError("eventos", String(e));
        });
    };

    suscribir(onSnapshot((s) => !desmontado && aplicar(s)));
    suscribir(onUpdateLine((l) => !desmontado && anadirLinea(l)));
    // Un encaje recién calculado a mano se guarda y su fila se refresca sin volver
    // a pedir la tabla entera. Ojo: el bucle AUTOMÁTICO del backend no emite este
    // evento, así que al abrir la vista se pide `fits:listar` entera.
    suscribir(onFit((f) => !desmontado && aplicarFit(f)));
    // Una acción terminada puede haber cambiado los servidores Y dejar una fila
    // nueva en `actions`: se recargan las dos cosas, no solo la primera.
    suscribir(
      onAction(() => {
        if (!desmontado) void Promise.all([cargarServidores(), cargarHistorial()]);
      }),
    );

    return () => {
      desmontado = true;
      bajas.forEach((b) => b());
    };
  }, [aplicar, anadirLinea, aplicarFit, cargarServidores, cargarHistorial, setError]);

  // Sin foto el número de activos es DESCONOCIDO, no cero: `null` deja la
  // insignia fuera (igual que la de procesos IA) en vez de afirmar un 0.
  const activos = snap ? snap.servers.filter((s) => s.state === "active").length : null;
  const seccion = SECCIONES[vista];

  return (
    <div className="flex h-full min-h-0">
      {/* Primer elemento enfocable de la página y visible solo al enfocarlo: quien
          navega con teclado no tiene que recorrer la barra lateral entera. */}
      <a
        href="#contenido"
        className="sr-only focus:not-sr-only focus:bg-surface focus:border-accent/50 focus:text-accent focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:border focus:px-3 focus:py-1.5 focus:text-xs"
      >
        Saltar al contenido
      </a>

      {/* ── Barra lateral ─────────────────────────────────────────────── */}
      <nav
        aria-label="Secciones"
        className="border-line-soft bg-surface flex w-[196px] shrink-0 flex-col border-r"
      >
        <div className="border-line-soft flex items-center gap-2 border-b px-4 py-3">
          <IconStack2 size={18} className="text-accent" aria-hidden="true" />
          <span className="text-sm font-semibold tracking-tight">Machinograph</span>
        </div>
        <div className="flex-1 overflow-y-auto p-2">
          {GRUPOS.map((grupo) => (
            <div key={grupo.id} className={grupo.nombre ? "mt-3 first:mt-0" : "first:mt-0"}>
              {/* El rótulo del grupo NO es un encabezado navegable: es una guía
                  visual para agrupar; quien use lector de pantalla oye la lista
                  de secciones con su propósito, que es lo que necesita. */}
              {grupo.nombre ? (
                <div className="label text-fg-faint px-2.5 pt-1 pb-1.5" aria-hidden="true">
                  {grupo.nombre}
                </div>
              ) : null}
              <ul aria-label={grupo.nombre ?? undefined}>
                {grupo.vistas.map((id) => {
                  const { nombre, para, Icono } = SECCIONES[id];
                  return (
                    <li key={id}>
                      <button
                        type="button"
                        onClick={() => setVista(id)}
                        title={para}
                        aria-current={vista === id ? "page" : undefined}
                        className={clsx(
                          "flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-[13px] transition-colors",
                          vista === id
                            ? "bg-accent-soft text-accent"
                            : "text-fg-muted hover:bg-raised hover:text-fg",
                        )}
                      >
                        <Icono size={16} aria-hidden="true" />
                        {nombre}
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}
        </div>
        <div className="border-line-soft text-fg-faint border-t px-4 py-3 text-[11px]">
          {/* Si hay error y sigue sin llegar la foto, "Conectando…" sería una
              mentira que se queda fija: se dice que no hay backend. */}
          {snap
            ? `Encendido hace ${dur(snap.uptime_secs)}`
            : error
              ? "Sin conexión con el backend"
              : "Conectando…"}
        </div>
      </nav>

      {/* ── Contenido ─────────────────────────────────────────────────── */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="border-line-soft bg-surface border-b px-5 py-2.5">
          <div className="flex items-center gap-3">
            <h1 id="titulo-seccion" className="text-sm font-semibold">
              {seccion.nombre}
            </h1>
            {/* Una línea que dice PARA QUÉ es la sección. El problema que
                resuelve: con doce secciones planas no se sabía qué había dónde
                —"Modelos" e "Inventario" parecían lo mismo—, y eso no se arregla
                solo con agrupar. */}
            <p className="text-fg-faint hidden truncate text-xs md:block">{seccion.para}</p>
            <div className="ml-auto flex shrink-0 items-center gap-2">
              {error ? (
                // Aquí sí cabe el último error de cualquier origen (es global), y
                // se enseña el motivo literal; el `title` guarda el texto entero.
                <Insignia tono="bad">
                  <IconAlertTriangle size={11} className="mr-1" aria-hidden="true" />
                  <span title={error.mensaje}>{error.mensaje.slice(0, 60)}</span>
                </Insignia>
              ) : null}
              {activos != null ? (
                <Insignia tono={activos > 0 ? "ok" : "neutro"}>
                  {activos} {activos === 1 ? "servidor activo" : "servidores activos"}
                </Insignia>
              ) : null}
              {snap ? <Insignia tono="neutro">{snap.ai_procs.length} procesos IA</Insignia> : null}
            </div>
          </div>
        </header>

        {/* La base de datos no se ha podido abrir. Va aquí, fuera de las
            secciones, porque afecta a TODA la app: sin BD no hay histórico, ni
            ajustes, ni encajes guardados. Y se dice qué sigue funcionando, que
            también es información: la foto y las acciones no dependen de ella.
            Antes de esto, el caso directamente mataba el proceso sin decir nada. */}
        {snap?.db_error ? (
          <div role="alert" className="border-bad/40 bg-bad/10 flex items-start gap-2 border-b px-5 py-2.5">
            <IconAlertTriangle size={16} className="text-bad mt-0.5 shrink-0" aria-hidden="true" />
            <div className="min-w-0 text-xs">
              <strong className="text-bad">Sin base de datos.</strong>{" "}
              <span className="text-fg-muted">
                No se puede guardar ni leer el histórico, los ajustes, los encajes ya calculados ni
                el registro de acciones. La foto del sistema, las mediciones y las acciones siguen
                funcionando.
              </span>
              {/* El motivo, literal y sin reescribir: es del sistema, no nuestro. */}
              <div className="mono text-fg-faint mt-1 break-words">{snap.db_error}</div>
            </div>
          </div>
        ) : null}

        <main
          id="contenido"
          ref={contenido}
          tabIndex={-1}
          aria-labelledby="titulo-seccion"
          className="min-h-0 flex-1 overflow-y-auto p-5"
        >
          {vista === "inicio" && <Inicio />}
          {vista === "servidores" && <Servers />}
          {vista === "disco" && <Disco />}
          {vista === "pantalla" && <Pantalla />}
          {vista === "hardware" && <Hardware />}
          {vista === "almacenamiento" && <Almacenamiento />}
          {vista === "optimizacion" && <Optimizacion />}
          {vista === "seguridad" && <Seguridad />}
          {vista === "diagnostico" && <Diagnostico />}
          {vista === "descubrir" && <Descubrir />}
          {vista === "rendimiento" && <Rendimiento />}
          {vista === "mantenimiento" && <Mantenimiento />}
          {vista === "uso" && <Uso />}
          {vista === "conexiones" && <Conexiones />}
          {vista === "ajustes" && <Ajustes />}
        </main>
      </div>
    </div>
  );
}
