/**
 * Los sensores del equipo: temperaturas, ventiladores, voltajes, potencia de la
 * CPU, caudal de los discos y de la red.
 *
 * REGLA QUE MANDA EN TODO ESTE FICHERO: cada cifra lleva de dónde sale. No es
 * adorno, es lo que permite comprobarla: el `title` de cada fila lleva la ruta
 * sysfs exacta, así que se puede hacer `cat` de esa ruta y ver el mismo número.
 * Un panel que enseña "67 °C" sin decir de qué chip lo ha leído no se puede
 * verificar, y entonces no se puede creer.
 *
 * Y la otra regla: un sensor desconectado NO se pinta. Esta placa tiene cinco
 * temperaturas que leen 0 exactos porque no hay nada conectado ahí; enseñar
 * "0 °C" sería afirmar una medida. Se dice cuántos se han dejado fuera para que
 * su ausencia no parezca un fallo del programa.
 */
import { clsx } from "clsx";
import { IconBolt, IconPlugConnected, IconTemperature, IconWaveSine, IconWind } from "@tabler/icons-react";
import { Barra, Card, Datos, Etiqueta, Insignia } from "./ui";
import { bPorSegundo, num } from "../lib/format";
import type { CaudalDisco, CaudalRed, FrecuenciaCpu, GrupoSensores, Sensor } from "../lib/tauri";

/* ── Utilidades ───────────────────────────────────────────────────────────── */

/** El color de una temperatura, por sus umbrales: crítico, aviso, normal. */
function nivelTemperatura(s: Sensor): "ok" | "warn" | "bad" {
  const valor = s.valor;
  if (s.critico != null && valor >= s.critico - 5) return "bad";
  if (s.critico != null && valor >= s.critico - 20) return "warn";
  if (s.max != null && valor >= s.max - 5) return "warn";
  // Sin umbrales publicados no se inventa un color: gris.
  return "ok";
}

/** ¿Esta temperatura es alta "de verdad"? Se enseña en la etiqueta. */
function etiquetaTemperatura(s: Sensor): string | null {
  if (s.critico != null) return `crítico ${num(s.critico)} ${s.unidad}`;
  if (s.max != null) return `máximo ${num(s.max)} ${s.unidad}`;
  return null;
}

/** Una fila de sensor: etiqueta, valor y, si hay, barra contra su umbral. */
function FilaSensor({ s, barra = false }: { s: Sensor; barra?: boolean }) {
  const nivel = s.clase === "temperatura" ? nivelTemperatura(s) : "ok";
  // La barra necesita un tope: el umbral crítico si lo hay, el máximo si no, y
  // solo si ninguno existe se usa el propio valor (barra llena), que es mejor que
  // inventarse una escala.
  const tope = s.critico ?? s.max ?? s.valor;
  return (
    <li className="row" title={`${s.fuente}`}>
      <span className="min-w-0 flex-1 truncate text-sm">{s.etiqueta}</span>
      {barra && tope > 0 ? (
        <span className="w-24 shrink-0">
          <Barra valor={(s.valor / tope) * 100} nivel={nivel} />
        </span>
      ) : null}
      <span className={clsx("mono w-24 shrink-0 text-right text-sm", nivel === "bad" && "text-bad", nivel === "warn" && "text-warn")}>
        {num(s.valor, s.valor < 10 ? 2 : 0)} {s.unidad}
      </span>
    </li>
  );
}

/**
 * El aviso de un chip que no está leyendo esta placa (dos drivers, un chip).
 *
 * No se esconde: se PLIEGA. Esta placa la publican dos drivers a la vez —
 * `nct6687`, que lee los 5 ventiladores que giran y pone nombre a todo, y
 * `nct6775`, que lee 0 en sus 7 ventiladores y no etiqueta ninguno de sus 14
 * voltajes—, y las dos listas juntas daban 41 filas donde la mitad eran del mismo
 * chip. El aviso dice qué pasa y el bloque se abre si se quiere ver.
 */
function AvisoChipDuplicado({ g }: { g: GrupoSensores }) {
  if (!g.ventiladores_a_cero) return null;
  return (
    <p className="text-fg-faint mt-1 text-[11px]">
      Todos sus ventiladores leen 0 mientras otro chip sí los ve girar: es la firma del mismo chip leído por otro
      driver
      {g.driver ? (
        <>
          {" "}
          (<span className="mono">{g.driver}</span>)
        </>
      ) : null}
      . El que da los nombres de esta placa es el otro.
    </p>
  );
}

/** ¿Este grupo es el duplicado de otro chip de la misma placa? */
const esDuplicado = (g: GrupoSensores) => g.ventiladores_a_cero;

/* ── Temperaturas ─────────────────────────────────────────────────────────── */

/** Temperaturas de todos los chips, incluidos los discos. */
export function Temperaturas({
  grupos,
  discos,
  descartados,
}: {
  grupos: GrupoSensores[];
  discos: { nombre: string; temp_c: number | null; fuente: string }[];
  descartados: number;
}) {
  const conTemperatura = grupos
    .map((g) => ({ g, temps: g.items.filter((s) => s.clase === "temperatura") }))
    .filter((x) => x.temps.length > 0);

  // Si ningún grupo viene de `/sys/class/hwmon`, es que este sistema no tiene esa
  // interfaz: el backend solo pudo leer las temperaturas que publica el sistema y
  // las metió en un único grupo sintético (`chip === "sistema"`). Se dice qué
  // falta y por qué, en vez de enseñar la tarjeta a medias como si no hubiera más
  // sensores. En Linux con hwmon esto es siempre `false` y no cambia nada.
  const sinHwmon = grupos.length > 0 && grupos.every((g) => g.chip === "sistema");

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconTemperature size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Temperaturas</Etiqueta>
        <span className="text-fg-faint ml-auto text-xs">
          {descartados > 0
            ? `${descartados} ${descartados === 1 ? "sensor desconectado" : "sensores desconectados"} no se enseñan`
            : "todos los sensores leen"}
        </span>
      </div>

      <div className="grid items-start gap-4 lg:grid-cols-2">
        {conTemperatura.map(({ g, temps }) => (
          <div key={g.chip}>
            <div className="text-fg-muted flex items-center gap-2 text-xs">
              <span>{g.chip_legible}</span>
              {g.driver ? <span className="mono text-fg-faint text-[11px]">{g.driver}</span> : null}
              {g.descartados > 0 ? (
                <Insignia tono="neutro">{g.descartados} sin conectar</Insignia>
              ) : null}
            </div>
            <ul className="mt-1">
              {temps.map((s) => (
                <FilaSensor key={`${s.chip}-${s.etiqueta}`} s={s} barra />
              ))}
            </ul>
            <AvisoChipDuplicado g={g} />
          </div>
        ))}

        {/* Los discos: NVMe por hwmon y SATA por SMART, con su fuente dicha. */}
        {discos.length > 0 ? (
          <div>
            <div className="text-fg-muted text-xs">Discos</div>
            <ul className="mt-1">
              {discos.map((d) => (
                <li key={d.nombre} className="row" title={d.fuente}>
                  <span className="min-w-0 flex-1 truncate text-sm">{d.nombre}</span>
                  <span className="mono w-24 shrink-0 text-right text-sm">
                    {d.temp_c == null ? <span className="text-fg-faint">—</span> : `${num(d.temp_c, 0)} °C`}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </div>

      {sinHwmon ? (
        <p className="text-fg-muted mt-3 text-sm">
          Ventiladores, voltajes y potencia del paquete no aparecen porque este sistema no publica los sensores de la
          placa a un programa sin privilegios: Linux los da por <span className="mono">/sys/class/hwmon</span>; macOS
          los tiene en el SMC (pide root) y Windows los expone por ACPI/WMI con elevación. Lo que sí se puede leer sin
          privilegios está arriba, y ningún hueco se rellena con ceros.
        </p>
      ) : null}

      {conTemperatura.length === 0 && discos.length === 0 ? (
        <p className="text-fg-muted text-sm">
          No hay ninguna temperatura que enseñar: este sistema no publica lecturas que se puedan leer sin
          privilegios. Linux las da por <span className="mono">/sys/class/hwmon</span>; macOS y Windows solo por su
          API del sistema (SMC y ACPI/WMI), que pide permisos de administrador. No se rellena el hueco con ceros.
        </p>
      ) : null}
      <p className="text-fg-faint mt-2 text-xs">
        {sinHwmon ? (
          "Cada fila lleva en su título de dónde sale el número, tal como lo publica este sistema."
        ) : (
          <>
            Cada fila lleva en su título la ruta de la que sale el número: se puede comprobar con{" "}
            <span className="mono">cat</span> sin instalar nada.
          </>
        )}
      </p>
    </Card>
  );
}

/* ── Ventiladores ─────────────────────────────────────────────────────────── */

export function Ventiladores({ grupos }: { grupos: GrupoSensores[] }) {
  const filas = grupos.flatMap((g) =>
    g.items
      .filter((s) => s.clase === "ventilador")
      .map((s) => ({ ...s, driver: g.driver, duplicado: g.ventiladores_a_cero })),
  );
  if (filas.length === 0) return null;
  // Los del chip duplicado (todos a 0) van al final y plegados: si no, sus 7
  // filas de "0 rpm" se leen como si esta máquina tuviera siete ventiladores
  // parados cuando lo que pasa es que ese chip no está leyendo la placa.
  const reales = filas.filter((f) => !f.duplicado);
  const duplicados = filas.filter((f) => f.duplicado);
  const girando = reales.filter((f) => f.valor > 0).length;

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconWind size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Ventiladores</Etiqueta>
        <span className="text-fg-faint ml-auto text-xs">
          {girando} de {reales.length} girando
        </span>
      </div>
      <ul>
        {reales.map((f) => (
          <li key={`${f.chip}-${f.etiqueta}`} className="row" title={`${f.fuente} · ${f.chip_legible}`}>
            <span className="min-w-0 flex-1 truncate text-sm">{f.etiqueta}</span>
            <span className="text-fg-faint truncate text-[11px]">{f.chip_legible}</span>
            <span className={clsx("mono w-20 shrink-0 text-right text-sm", f.valor === 0 && "text-fg-faint")}>
              {num(f.valor)} rpm
            </span>
          </li>
        ))}
      </ul>
      {duplicados.length > 0 ? (
        <details className="border-line-soft mt-2 rounded-md border px-3 py-2">
          <summary className="text-fg-muted cursor-pointer text-xs">
            {duplicados.length} canales más, todos a 0 (posible chip duplicado)
          </summary>
          <ul className="mt-1">
            {duplicados.map((f) => (
              <li key={`${f.chip}-${f.etiqueta}`} className="row" title={`${f.fuente} · ${f.chip_legible}`}>
                <span className="mono min-w-0 flex-1 truncate text-sm">{f.etiqueta}</span>
                <span className="text-fg-faint truncate text-[11px]">{f.chip_legible}</span>
                <span className="mono text-fg-faint w-20 shrink-0 text-right text-sm">0 rpm</span>
              </li>
            ))}
          </ul>
          <p className="text-fg-faint mt-1 text-[11px]">
            Un ventilador a 0 rpm puede ser un canal sin nada conectado o uno parado por su curva. Estos vienen de un
            chip de la placa que no ve girar ninguno, mientras otro sí: probablemente sea el mismo chip.
          </p>
        </details>
      ) : null}
      <p className="text-fg-faint mt-2 text-xs">
        Un ventilador a 0 rpm puede ser un canal sin ventilador conectado o uno parado por su curva: el dato es el
        dato, no se interpreta por ti.
      </p>
    </Card>
  );
}

/* ── Voltajes ─────────────────────────────────────────────────────────────── */

export function Voltajes({ grupos }: { grupos: GrupoSensores[] }) {
  const porChip = grupos
    .map((g) => ({ g, items: g.items.filter((s) => s.clase === "voltaje" || s.clase === "corriente") }))
    .filter((x) => x.items.length > 0);
  if (porChip.length === 0) return null;

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconWaveSine size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Voltajes y corrientes</Etiqueta>
      </div>
      <div className="grid items-start gap-4 lg:grid-cols-2">
        {porChip.map(({ g, items }) =>
          esDuplicado(g) ? (
            <details key={g.chip} className="border-line-soft rounded-md border px-3 py-2">
              <summary className="text-fg-muted cursor-pointer text-xs">
                {g.chip_legible} · {items.length} canales sin etiquetar (posible chip duplicado)
              </summary>
              <ul className="mt-1">
                {items.map((s) => (
                  <FilaSensor key={`${s.chip}-${s.etiqueta}`} s={s} />
                ))}
              </ul>
              <AvisoChipDuplicado g={g} />
            </details>
          ) : (
            <div key={g.chip}>
              <div className="text-fg-muted text-xs">{g.chip_legible}</div>
              <ul className="mt-1">
                {items.map((s) => (
                  <FilaSensor key={`${s.chip}-${s.etiqueta}`} s={s} />
                ))}
              </ul>
            </div>
          ),
        )}
      </div>
      <p className="text-fg-faint mt-2 text-xs">
        Los nombres son los de la placa ("CPU Vcore", "DRAM", "+12V"). Los que no llevan nombre son canales que el
        driver no sabe etiquetar en esta placa: se enseñan con su identificador en vez de inventarles uno.
      </p>
    </Card>
  );
}

/* ── CPU: potencia y frecuencia ───────────────────────────────────────────── */

export function PotenciaFrecuencia({
  potencia,
  fuente,
  frecuencia,
  nucleos,
  cpuPct,
}: {
  potencia: number | null;
  /**
   * De dónde sale la potencia (`hardware.cpu_potencia_fuente`), o `null` si este
   * sistema NO tiene contador de energía. Es lo que distingue «espera a la
   * siguiente lectura» de «aquí no se puede medir»: sin este dato, el hueco decía
   * lo primero para siempre en macOS y Windows, que es falso.
   */
  fuente?: string | null;
  frecuencia: FrecuenciaCpu;
  nucleos: number;
  cpuPct: number;
}) {
  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconBolt size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>CPU · potencia y frecuencia</Etiqueta>
      </div>
      <Datos
        items={[
          [
            "Potencia del paquete",
            potencia != null ? (
              <span>{num(potencia, 1)} W</span>
            ) : fuente === null ? (
              // `null` aquí NO significa «espera a la siguiente lectura»: es que
              // este sistema no tiene contador de energía (ver
              // `hardware.cpu_potencia_fuente`). Decir «se mide en la siguiente
              // lectura» prometería un dato que no va a llegar nunca.
              <span
                className="text-fg-faint"
                title="La potencia se despeja de un contador de energía acumulado, y este sistema no lo publica a un programa sin privilegios (macOS: SMC, pide root; Windows: ACPI/WMI, pide elevación)."
              >
                — no disponible en este sistema
              </span>
            ) : (
              <span className="text-fg-faint" title="Se despeja de un contador acumulado: hace falta una segunda lectura para poder dividir por el tiempo.">
                — (se mide en la siguiente lectura)
              </span>
            ),
          ],
          ["Carga", `${num(cpuPct)} % de ${nucleos} ${nucleos === 1 ? "núcleo" : "núcleos"}`],
          [
            "Frecuencia",
            frecuencia.nucleos === 0 ? (
              <span className="text-fg-faint">este sistema no publica la frecuencia de la CPU</span>
            ) : (
              <span>
                {num(frecuencia.media_mhz, 0)} MHz de media ({num(frecuencia.min_mhz, 0)}–
                {num(frecuencia.max_mhz, 0)} en {frecuencia.nucleos} núcleos)
              </span>
            ),
          ],
        ]}
      />
      {fuente === null ? (
        <p className="text-fg-faint mt-2 text-xs">
          La potencia del paquete se despeja de un contador de energía acumulado (en Linux, AMD{" "}
          <span className="mono">zenergy</span> o RAPL), y este sistema no lo publica sin privilegios: no hay ningún
          número que enseñar y no se estima desde el consumo de CPU, porque sería un dato inventado. La carga y la
          frecuencia sí se publican y van arriba.
        </p>
      ) : (
        <p className="text-fg-faint mt-2 text-xs">
          La potencia sale del contador de energía del procesador (AMD <span className="mono">zenergy</span>), dividiendo
          la energía gastada entre el tiempo: es una MEDIDA, no una estimación de consumo. La primera lectura tras
          arrancar no puede darla y se dice, en vez de enseñar un cero.
        </p>
      )}
    </Card>
  );
}

/* ── Discos y red: caudal ─────────────────────────────────────────────────── */

export function Caudal({
  discos,
  red,
}: {
  discos: CaudalDisco[];
  red: CaudalRed[];
}) {
  // Solo los discos con algo de tráfico o los que existen: una tabla de 8 ceros
  // no informa. Los que están a 0 se cuentan aparte.
  const discosActivos = discos.filter((d) => d.leer_b_s > 0 || d.escribir_b_s > 0);
  const redActiva = red.filter((i) => i.activa);

  return (
    <Card>
      <div className="mb-3 flex items-center gap-2">
        <IconPlugConnected size={15} className="text-accent" aria-hidden="true" />
        <Etiqueta>Caudal</Etiqueta>
        <span className="text-fg-faint ml-auto text-xs">bytes por segundo, ahora</span>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <div>
          <div className="text-fg-muted text-xs">Discos</div>
          {discos.length === 0 ? (
            <p className="text-fg-muted mt-1 text-sm">No se pudo leer el caudal de los discos.</p>
          ) : discosActivos.length === 0 ? (
            <p className="text-fg-muted mt-1 text-sm">
              Ningún disco está leyendo ni escribiendo ahora mismo ({discos.length} discos vigilados).
            </p>
          ) : (
            <ul className="mt-1">
              {discosActivos.map((d) => (
                <li key={d.nombre} className="row">
                  <span className="mono min-w-0 flex-1 truncate text-sm">{d.nombre}</span>
                  <span className="text-fg-muted mono text-xs" title="Lectura">
                    ↓ {bPorSegundo(d.leer_b_s)}
                  </span>
                  <span className="text-fg-muted mono text-xs" title="Escritura">
                    ↑ {bPorSegundo(d.escribir_b_s)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div>
          <div className="text-fg-muted text-xs">Red</div>
          {red.length === 0 ? (
            <p className="text-fg-muted mt-1 text-sm">No se pudo leer el caudal de red.</p>
          ) : redActiva.length === 0 ? (
            <p className="text-fg-muted mt-1 text-sm">
              Ninguna interfaz con tráfico ahora mismo ({red.length} interfaces vigiladas, sin contar el lazo local).
            </p>
          ) : (
            <ul className="mt-1">
              {redActiva.map((i) => (
                <li key={i.nombre} className="row">
                  <span className="mono min-w-0 flex-1 truncate text-sm">{i.nombre}</span>
                  <span className="text-fg-muted mono text-xs">↓ {bPorSegundo(i.rx_b_s)}</span>
                  <span className="text-fg-muted mono text-xs">↑ {bPorSegundo(i.tx_b_s)}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
      <p className="text-fg-faint mt-2 text-xs">
        Los dos caudales se calculan comparando los contadores del kernel con la lectura anterior, así que aparece
        medio segundo después de abrir y no en la primera foto.
      </p>
    </Card>
  );
}
