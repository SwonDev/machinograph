/**
 * Diagnóstico: la salud del ENTORNO, comprobada bajo demanda.
 *
 * Qué es y qué no es: esto NO son métricas (eso es Sistema) ni el resultado de
 * una acción. Es una lista de comprobaciones ("¿hay driver?", "¿el reloj de
 * memoria está clavado?", "¿queda disco?", "¿está llmfit?"), cada una con su
 * estado y, si hay algo que hacer, con el remedio. La comprobación recorre el
 * entorno entero, así que se lanza al abrir la sección y cuando el usuario pulsa
 * el botón: NUNCA en bucle.
 *
 * Dos cosas que esta vista separa a propósito y que no se pueden pintar igual:
 *   - "no hay nada que comprobar" (el backend no devolvió comprobaciones), y
 *   - "no se pudo comprobar" (la llamada falló).
 *
 * El estado va SIEMPRE por texto + icono + color: el color es el tercer aviso,
 * nunca el único (quien no distinga verde de rojo tiene que poder leer la lista).
 */
import { useCallback, useEffect, useState } from "react";
import {
  IconAlertTriangle,
  IconCircleCheck,
  IconCircleX,
  IconHelpHexagon,
  IconRefresh,
  IconTools,
} from "@tabler/icons-react";
import { cargandoDe, errorDe, useApp } from "../store";
import { api } from "../lib/tauri";
import type {
  Comprobacion,
  ComprobacionSalud,
  EstadoDiagnostico,
  EstadoSalud,
  RevisionSalud,
} from "../lib/tauri";
import { Boton, Card, Etiqueta, Insignia, Vacio } from "../components/ui";
import type { Tono } from "../components/ui";

/**
 * Cómo se enseña cada estado. `orden` es la prioridad de la lista: primero lo
 * que está mal, luego lo que no se sabe, y lo que va bien al final (es lo que
 * menos se necesita leer).
 */
const ESTADOS: Record<
  EstadoDiagnostico,
  { texto: string; Icono: typeof IconCircleCheck; tono: Tono; clase: string; orden: number }
> = {
  problema: { texto: "problema", Icono: IconCircleX, tono: "bad", clase: "text-bad", orden: 0 },
  aviso: { texto: "aviso", Icono: IconAlertTriangle, tono: "warn", clase: "text-warn", orden: 1 },
  // No es un problema ni un aprobado: es "no se pudo comprobar". Va en tono
  // neutro y en medio de la lista para no contarlo como bueno.
  desconocido: {
    texto: "sin comprobar",
    Icono: IconHelpHexagon,
    tono: "neutro",
    clase: "text-fg-faint",
    orden: 2,
  },
  ok: { texto: "todo bien", Icono: IconCircleCheck, tono: "ok", clase: "text-ok", orden: 3 },
};

function FilaComprobacion({ c }: { c: Comprobacion }) {
  // Un estado que no conocemos no se traduce ni se disfraza: se enseña TAL CUAL
  // (el backend puede añadir uno nuevo mañana) y se trata como "sin comprobar".
  const e = ESTADOS[c.estado];
  const Icono = e?.Icono ?? ESTADOS.desconocido.Icono;
  return (
    <li className="flex items-start gap-3 px-4 py-3">
      <Icono size={16} className={`mt-0.5 shrink-0 ${e?.clase ?? ESTADOS.desconocido.clase}`} aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-medium">{c.titulo}</span>
          <Insignia tono={e?.tono ?? "neutro"}>{e?.texto ?? c.estado}</Insignia>
        </div>
        <p className="text-fg-muted mt-1 text-xs whitespace-pre-wrap break-words">{c.detalle}</p>
        {c.como_arreglarlo ? (
          <div className="bg-raised border-line-soft mt-2 rounded border px-2.5 py-1.5">
            <Etiqueta>Cómo arreglarlo</Etiqueta>
            <p className="mono mt-1 text-xs whitespace-pre-wrap break-words">{c.como_arreglarlo}</p>
          </div>
        ) : null}
      </div>
    </li>
  );
}

/**
 * Cómo se enseña cada estado de la autorreparación. `orden` es la prioridad:
 * primero lo que sigue roto (que es lo que el usuario tiene que mirar), luego lo
 * que se arregló, y lo que estaba bien al final.
 *
 * «reparado» no es un aprobado: dice que algo ESTABA roto y se ha arreglado, y por
 * eso va en tono de aviso y siempre con el detalle de qué se hizo.
 */
const ESTADOS_SALUD: Record<
  EstadoSalud,
  { texto: string; Icono: typeof IconCircleCheck; tono: Tono; clase: string; orden: number }
> = {
  no_se_pudo: { texto: "no se pudo", Icono: IconCircleX, tono: "bad", clase: "text-bad", orden: 0 },
  reparado: { texto: "reparado", Icono: IconTools, tono: "warn", clase: "text-warn", orden: 1 },
  correcto: { texto: "correcto", Icono: IconCircleCheck, tono: "ok", clase: "text-ok", orden: 2 },
};

function FilaSalud({ c }: { c: ComprobacionSalud }) {
  const e = ESTADOS_SALUD[c.estado] ?? ESTADOS_SALUD.no_se_pudo;
  const Icono = e.Icono;
  return (
    <li className="flex items-start gap-3 px-4 py-3">
      <Icono size={16} className={`mt-0.5 shrink-0 ${e.clase}`} aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-medium">{c.titulo}</span>
          <Insignia tono={e.tono}>{e.texto}</Insignia>
        </div>
        <p className="text-fg-muted mt-1 text-xs whitespace-pre-wrap break-words">{c.detalle}</p>
        {c.como_arreglarlo ? (
          <div className="bg-raised border-line-soft mt-2 rounded border px-2.5 py-1.5">
            <Etiqueta>Qué hacer</Etiqueta>
            <p className="mt-1 text-xs whitespace-pre-wrap break-words">{c.como_arreglarlo}</p>
          </div>
        ) : null}
      </div>
    </li>
  );
}

/**
 * Autorreparación: lo que la app se arregla SOLA al arrancar.
 *
 * Es una tarjeta aparte de la comprobación del entorno porque responde a otra
 * pregunta: aquella dice «cómo está la máquina»; esta, «qué se le había roto a la
 * propia app y qué ha hecho con ello» (el puerto de la puerta, la base del
 * histórico, su entrada de arranque y los ficheros de configuración que escribió).
 *
 * Al abrir la sección se pide el estado SIN reparar: mirar no escribe nada.
 * «Reparar ahora» es la versión a mano, la que se pulsa cuando aquí sale un «no se
 * pudo». Lo que se repare queda en el historial de acciones.
 */
function Autorreparacion() {
  const [revision, setRevision] = useState<RevisionSalud | null>(null);
  const [cargando, setCargando] = useState(false);
  const [reparando, setReparando] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const revisar = useCallback(async (reparar: boolean) => {
    setCargando(true);
    setReparando(reparar);
    try {
      setRevision(await api.salud.revisar(reparar));
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setCargando(false);
      setReparando(false);
    }
  }, []);

  useEffect(() => {
    void revisar(false);
  }, [revisar]);

  const filas = [...(revision?.comprobaciones ?? [])].sort(
    (a, b) => (ESTADOS_SALUD[a.estado]?.orden ?? 99) - (ESTADOS_SALUD[b.estado]?.orden ?? 99),
  );

  // El resumen de una línea: el del backend cuando lo hay, y si no el motivo por el
  // que no se pudo comprobar. Nunca un «todo correcto» sin haber comprobado.
  const resumen = error
    ? `No se pudo comprobar: ${error}`
    : !revision
      ? "Comprobando qué se puede reparar…"
      : revision.resumen;

  return (
    <Card className="p-0">
      <div className="border-line-soft flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
        <IconTools size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Autorreparación</Etiqueta>
        <span className="text-fg-faint text-xs">
          se revisa sola al arrancar: aquí solo lo lanzas a mano
        </span>
        <Boton
          variante="acento"
          className="ml-auto"
          disabled={cargando}
          onClick={() => void revisar(true)}
        >
          <IconRefresh size={13} className="mr-1 inline" aria-hidden="true" />
          {reparando ? "Reparando…" : cargando ? "Comprobando…" : "Reparar ahora"}
        </Boton>
      </div>

      {/* `role="status"`: el resumen se anuncia sin robar el foco. */}
      <div
        className="border-line-soft border-b px-4 py-2"
        role="status"
        aria-live="polite"
      >
        <span className={error ? "text-bad text-sm" : "text-sm"}>{resumen}</span>
      </div>

      {filas.length > 0 ? (
        <ul className="divide-line-soft divide-y">
          {filas.map((c) => (
            <FilaSalud key={c.id} c={c} />
          ))}
        </ul>
      ) : (
        <div className="px-4 py-3">
          <p className="text-fg-muted text-xs">
            {error
              ? "La revisión no llegó a responder, así que no hay nada que enseñar. Prueba otra vez."
              : "Sin comprobaciones todavía."}
          </p>
        </div>
      )}
    </Card>
  );
}

export default function Diagnostico() {
  const comprobaciones = useApp((st) => st.diagnostico);
  const cargar = useApp((st) => st.cargarDiagnostico);
  const cargando = useApp(cargandoDe("diagnostico"));
  const error = useApp(errorDe("diagnostico"));

  useEffect(() => {
    // Una comprobación al abrir la sección, y solo si en esta sesión todavía no
    // hay resultado. El estado se mira con `getState()` a propósito: si fuera una
    // dependencia reactiva, un fallo dejaría `diagnostico` en `null` y el paso de
    // "cargando" a "no cargando" volvería a disparar la comprobación, en bucle.
    if (useApp.getState().diagnostico == null) void cargar();
  }, [cargar]);

  const total = comprobaciones?.length ?? 0;
  const cuenta: Record<EstadoDiagnostico, number> = { ok: 0, aviso: 0, problema: 0, desconocido: 0 };
  for (const c of comprobaciones ?? []) cuenta[ESTADOS[c.estado] ? c.estado : "desconocido"] += 1;

  const resumen = comprobaciones
    ? [
        `Van bien ${cuenta.ok} de ${total}`,
        cuenta.problema ? `${cuenta.problema} con problema` : null,
        cuenta.aviso ? `${cuenta.aviso} con aviso` : null,
        cuenta.desconocido ? `${cuenta.desconocido} sin comprobar` : null,
      ]
        .filter(Boolean)
        .join(" · ")
    : "";

  // Lo que está mal primero. Se ordena una copia: `comprobaciones` es el estado
  // de la tienda y no se toca.
  const filas = [...(comprobaciones ?? [])].sort((a, b) => {
    const oa = ESTADOS[a.estado]?.orden ?? ESTADOS.desconocido.orden;
    const ob = ESTADOS[b.estado]?.orden ?? ESTADOS.desconocido.orden;
    return oa - ob;
  });

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Etiqueta>
          {comprobaciones ? `Última comprobación · ${total} comprobaciones` : "Comprobación del entorno"}
        </Etiqueta>
        <Boton
          variante="acento"
          className="ml-auto"
          disabled={cargando}
          onClick={() => void cargar()}
          aria-describedby="diag-nota"
        >
          <IconRefresh size={13} className="mr-1 inline" aria-hidden="true" />
          {cargando ? "Comprobando…" : "Comprobar ahora"}
        </Boton>
      </div>

      {/* `role="status"`: al llegar el resultado se anuncia sin robar el foco. */}
      <div role="status" aria-live="polite">
        {cargando ? (
          <Card>
            <Etiqueta>Comprobando</Etiqueta>
            <p className="text-fg-muted mt-2 text-sm">
              Recorriendo GPU, reloj de memoria, motor, llmfit, disco e inventario. Se comprueba cuando se
              pide: no hay sondeo en bucle.
            </p>
          </Card>
        ) : null}

        {/* "No se pudo comprobar" NO es "todo bien" ni "no hay nada": se dice el
            motivo literal. Y con un resultado anterior en la mano, el fallo no lo
            borra (enseñarlo callado como si fuera de ahora sí sería mentir): se
            avisa arriba de que lo de abajo es la última comprobación que terminó. */}
        {error && comprobaciones == null ? (
          <Vacio titulo="No se pudo comprobar">
            <span className="text-bad">{error}</span> No es un diagnóstico del equipo: es que la
            comprobación no llegó a responder. Prueba otra vez con el botón.
          </Vacio>
        ) : error ? (
          <Card className="border-bad/40">
            <p className="text-bad text-sm" role="alert">
              La última comprobación no terminó: {error}
            </p>
            <p className="text-fg-muted mt-1 text-xs">
              Lo que hay debajo es lo que respondió la última que sí terminó. No es el estado de ahora.
            </p>
          </Card>
        ) : null}

        {comprobaciones && total === 0 ? (
          <Vacio titulo="Sin comprobaciones">
            La comprobación respondió, pero no devolvió ninguna lista. Eso no es un fallo: significa que no
            hay nada configurado que mirar.
          </Vacio>
        ) : null}
      </div>

      {comprobaciones && total > 0 ? (
        <Card className="p-0">
          <div className="border-line-soft flex flex-wrap items-center gap-2 border-b px-4 py-2.5">
            <span className="text-sm" role="status" aria-live="polite">
              {resumen}
            </span>
          </div>
          <ul className="divide-line-soft divide-y">
            {filas.map((c) => (
              <FilaComprobacion key={c.id} c={c} />
            ))}
          </ul>
        </Card>
      ) : null}

      {/* La autorreparación va DEBAJO de la comprobación del entorno a propósito:
          primero se ve cómo está la máquina, y luego qué se ha arreglado la app a
          sí misma. Las dos listas no se mezclan porque responden a preguntas
          distintas. */}
      <Autorreparacion />

      <p id="diag-nota" className="text-fg-faint flex items-start gap-1.5 text-xs">
        <IconTools size={13} className="mt-0.5 shrink-0" aria-hidden="true" />
        <span>
          Los remedios de la lista de arriba son órdenes y rutas de este equipo, y se enseñan tal cual: esa
          comprobación no ejecuta ninguna por su cuenta. La autorreparación sí actúa, pero solo sobre lo que
          la propia app escribió (su base, su arranque y los ficheros de configuración que ella tocó), y dice
          siempre qué ha hecho.
        </span>
      </p>
    </div>
  );
}
