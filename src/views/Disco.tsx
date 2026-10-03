/**
 * Modelos: TODO lo que hay en disco, de cualquier familia y tipo.
 *
 * Por qué se rehízo: la versión anterior leía `snapshot.disk_models`, que solo
 * miraba `~/models` y buscaba `.gguf`. Decía "6 ficheros · 29,0 GB" cuando en
 * este equipo hay 20 modelos y ~45 GB repartidos en cuatro familias (llama.cpp,
 * LM Studio, piper y Coqui TTS). Ese número era FALSO. Ahora la fuente es
 * `inventario:listar`, que recorre todas las raíces conocidas.
 *
 * DE DÓNDE SALE CADA DATO, que importa para no leer de más:
 *  - `familia` y `motor` son de DÓNDE está el fichero (la raíz que lo contiene y
 *    el programa que lo usa). Es un dato, no una deducción.
 *  - `tipo` se DEDUCE de la ubicación y del nombre (una voz de piper está en la
 *    carpeta de piper; una lora, en `loras`), no de abrir el fichero. Es fiable,
 *    pero es una deducción, y se dice así.
 *  - Si un tipo no tiene ficheros, NO aparece: no se rellena con ceros inventados.
 *  - El ENCAJE lo calcula el backend solo (arranque + cada 10 min) y aquí solo se
 *    enseña, con el runtime que lo calculó y su antigüedad. La columna se ordena
 *    como cualquier otra: en un equipo con 20 modelos, lo que se busca es
 *    justamente "cuáles caben enteros en la GPU".
 */
import { useEffect, useMemo, useState } from "react";
import { clsx } from "clsx";
import { IconFolderOpen, IconRefresh, IconSearch, IconTrash } from "@tabler/icons-react";
import { useApp, cargandoDe, errorDe, ejecutar, type ResultadoAccion } from "../store";
import { Boton, Card, Etiqueta, Insignia, ThOrden, Vacio, useReloj, type Dir } from "../components/ui";
import { CeldaEncaje } from "../components/Encaje";
import { bLegibles, fechaHora } from "../lib/format";
import { encajeDe, etiquetaTipo, tonoTipo, TIPOS_MODELO } from "../lib/modelos";
import { api, type ModeloInventario } from "../lib/tauri";

/** Para el filtro por motor: `null` no es una cadena vacía, es "no consta". */
const SIN_MOTOR = "__sin_motor__";

/** Las columnas por las que se puede ordenar. El orden lo fija el `th`. */
const COLUMNAS = ["nombre", "tipo", "formato", "tamano", "encaje", "modificado"] as const;
type Col = (typeof COLUMNAS)[number];

/**
 * Valida lo que venga del estado guardado.
 *
 * Dos casos, y el segundo es el que importa: una columna que YA NO EXISTE en la
 * tabla (o alguien vuelve con el orden guardado de la versión anterior, donde
 * “motor” era columna propia) no puede dejar la tabla ordenada por un criterio
 * invisible. Si no vale, se cae a la familia, que es la columna en la que se
 * juntó. */
const esCol = (v: string): v is Col => (COLUMNAS as readonly string[]).includes(v);
const colValida = (v: string): Col => {
  if (esCol(v)) return v;
  // Órdenes guardados de versiones anteriores, cuando «familia» y «motor» eran
  // columnas propias: ahora esa información va dentro de la celda del nombre, así
  // que su orden se cae al nombre (que es la columna que la contiene) y no a un
  // criterio invisible.
  if (v === "familia" || v === "motor") return "nombre";
  return "tamano";
};

/**
 * Orden del encaje: de mejor a peor. `null` (sin calcular) NO es un valor más,
 * va al final siempre, porque no es "ni bueno ni malo": es que no hay dato.
 *
 * `Error` va debajo de `NoCabe` a propósito: "no cabe" describe el equipo y
 * "error" describe que ese binario ni siquiera sabe leer el fichero, que es peor
 * noticia para quien quiere usarlo.
 */
function rangoEncaje(encaje: string | null | undefined): number | null {
  if (encaje == null) return null;
  if (encaje === "Gpu") return 3;
  if (encaje === "Mixto") return 2;
  if (encaje === "NoCabe") return 1;
  if (encaje === "Error") return 0;
  return -1;
}

/** Numérico con los nulos SIEMPRE al final (no valen como 0). */
function numero(a: number | null, b: number | null, dir: Dir): number {
  if (a == null && b == null) return 0;
  if (a == null) return 1;
  if (b == null) return -1;
  return dir === "asc" ? a - b : b - a;
}

function texto(a: string, b: string, dir: Dir): number {
  const r = a.localeCompare(b);
  return dir === "asc" ? r : -r;
}

/**
 * Texto donde el vacío significa "no hay dato" (una cuantización que no se puede
 * deducir, un motor que no consta): esas filas van al final en LOS DOS sentidos,
 * para que no se cuelen entre los valores reales como si fueran uno más.
 */
function textoConVaciosAlFinal(a: string | null, b: string | null, dir: Dir): number {
  const va = a ?? "";
  const vb = b ?? "";
  if (va === "" && vb === "") return 0;
  if (va === "") return 1;
  if (vb === "") return -1;
  return texto(va, vb, dir);
}

/** Totales de una lista, agrupados por tipo y en el orden de `TIPOS_MODELO`. */
function totalesPorTipo(modelos: ModeloInventario[]) {
  const mapa = new Map<string, { ficheros: number; bytes: number }>();
  for (const m of modelos) {
    const t = mapa.get(m.tipo) ?? { ficheros: 0, bytes: 0 };
    t.ficheros += 1;
    t.bytes += m.tamano_bytes;
    mapa.set(m.tipo, t);
  }
  const pos = (t: string) => {
    const i = TIPOS_MODELO.indexOf(t as (typeof TIPOS_MODELO)[number]);
    return i === -1 ? TIPOS_MODELO.length : i;
  };
  return [...mapa.entries()].sort((a, b) => pos(a[0]) - pos(b[0]) || a[0].localeCompare(b[0]));
}

/**
 * Las acciones de una fila.
 *
 * El borrado va en dos pasos a propósito: aquí hay modelos de 6 y 9 GB, y un clic
 * accidental duele. Y como NO se borra de verdad (va a la papelera del
 * escritorio), el aviso lo dice ANTES de pulsar: se puede deshacer.
 */
function AccionesFila({
  modelo,
  enCurso,
  onCambio,
}: {
  modelo: ModeloInventario;
  enCurso: string | null;
  onCambio: () => void;
}) {
  const [confirmando, setConfirmando] = useState(false);
  const [res, setRes] = useState<ResultadoAccion | null>(null);
  /**
   * Si este modelo se está SIRVIENDO ahora mismo. Se consulta al abrir la
   * confirmación (una petición local, rápida) y se dice ANTES de borrar: el
   * backend para el motor solo y lo cuenta en el resultado, pero cortar lo que
   * alguien esté generando no puede ser una sorpresa.
   */
  const [servido, setServido] = useState<string | null>(null);
  useEffect(() => {
    if (!confirmando) return;
    let vivo = true;
    api
      .memoria()
      .then((m) => {
        if (!vivo) return;
        const cargado = m.modelos.find((x) => x.ruta === modelo.ruta);
        setServido(cargado ? cargado.nombre : null);
      })
      // Si no se puede saber (no hay motor, no contesta), no se inventa nada: no
      // se avisa. El backend sigue parando el motor si resulta que sí lo servía.
      .catch(() => vivo && setServido(null));
    return () => {
      vivo = false;
    };
  }, [confirmando, modelo.ruta]);

  const borrar = async () => {
    const r = await ejecutar("modelo:borrar", { ruta: modelo.ruta });
    setRes(r);
    setConfirmando(false);
    // Si se movió, el inventario ya no es el de antes: se relee (y si el backend
    // se negó porque la ruta no es de una carpeta conocida, el error se enseña).
    if (r.ok) onCambio();
  };

  const abrir = async () => {
    setRes(await ejecutar("modelo:abrir-carpeta", { ruta: modelo.ruta }));
  };

  return (
    <div className="flex flex-col items-end gap-1">
      {/* `flex-nowrap` a propósito: si los dos botones se apilan, la fila crece a
          80px y la tabla deja de ser escaneable. Es mejor que la tabla sea un poco
          más ancha (ya hace scroll dentro de su tarjeta) que más alta. */}
      <div className="flex flex-nowrap items-center justify-end gap-1.5">
        {/* El `aria-label` EMPIEZA por el texto visible ("Abrir carpeta", "Borrar"):
            así quien use control por voz puede decir lo que lee en pantalla
            (WCAG 2.5.3) y a la vez sabe de qué fichero es el botón. */}
        <Boton
          disabled={!!enCurso}
          onClick={() => void abrir()}
          title="Abrir la carpeta que lo contiene"
          aria-label={`Abrir carpeta: ${modelo.nombre}`}
        >
          <IconFolderOpen size={13} aria-hidden="true" />
        </Boton>
        {confirmando ? (
          <>
            <Boton
              variante="peligro"
              disabled={!!enCurso}
              onClick={() => void borrar()}
              aria-label={`Sí, a la papelera: ${modelo.nombre}`}
            >
              Sí, a la papelera
            </Boton>
            <Boton onClick={() => setConfirmando(false)}>No</Boton>
          </>
        ) : (
          <Boton
            variante="peligro"
            disabled={!!enCurso}
            onClick={() => setConfirmando(true)}
            title="Mover a la papelera (se puede recuperar)"
            aria-label={`Borrar ${modelo.nombre} (se moverá a la papelera)`}
          >
            <IconTrash size={13} aria-hidden="true" />
          </Boton>
        )}
      </div>
      {confirmando ? (
        <p className="text-fg-muted max-w-[320px] text-right text-[11px]" role="alert">
          {/* Se recuerda ANTES de pulsar y también después: el mensaje "se puede
              deshacer" es la mitad del trato, no un adorno. */}
          Se moverá a la PAPELERA ({bLegibles(modelo.tamano_bytes, 1)}): se puede recuperar desde el gestor de
          archivos. No se borra de verdad.
          {servido ? (
            <span className="text-warn mt-1 block">
              Y se está sirviendo ahora ({servido}): se parará antes de borrarlo, así que dejará de responder.
            </span>
          ) : null}
        </p>
      ) : null}
      {res ? (
        <p
          className={clsx("max-w-[280px] text-right text-[11px]", res.ok ? "text-fg-muted" : "text-bad")}
          role={res.ok ? undefined : "alert"}
        >
          {res.mensaje}
        </p>
      ) : null}
    </div>
  );
}

export default function Disco() {
  const inventario = useApp((st) => st.inventario);
  const cargar = useApp((st) => st.cargarInventario);
  const cargandoInv = useApp(cargandoDe("inventario"));
  // El inventario tiene su PROPIA lectura, así que su error es el suyo (no el de
  // la foto, que es otra cosa).
  const error = useApp(errorDe("inventario"));
  // Los encajes son otra lectura más (SQLite), con su propio error.
  const fits = useApp((st) => st.fits);
  const cargarFits = useApp((st) => st.cargarFits);
  const errorFits = useApp(errorDe("fits"));
  const enCurso = useApp((st) => st.accionEnCurso);
  const ui = useApp((st) => st.ui.disco);
  const setUi = useApp((st) => st.setUi);
  // La antigüedad de un encaje se tiene que refrescar sola aunque no llegue nada
  // nuevo: si no, "hace 4 min" se quedaría congelado y mentiría.
  const ahora = useReloj();

  const col: Col = colValida(ui.col);

  // Se pide al abrir la vista, y solo si aún no está: es una lectura que recorre
  // carpetas enteras, así que no se repite en cada montaje. Tras borrar se
  // refresca a mano (ver `AccionesFila`).
  useEffect(() => {
    if (inventario == null) void cargar();
  }, [inventario, cargar]);

  // Los encajes son baratos (una tabla de SQLite) y los calcula el backend solo:
  // se leen al abrir y nunca en bucle.
  useEffect(() => {
    if (fits == null) void cargarFits();
  }, [fits, cargarFits]);

  const familias = useMemo(
    () => [...new Set((inventario ?? []).map((m) => m.familia))].sort(),
    [inventario],
  );
  const motores = useMemo(
    () =>
      [...new Set((inventario ?? []).map((m) => m.motor).filter((x): x is string => x != null))].sort(),
    [inventario],
  );
  const haySinMotor = useMemo(() => (inventario ?? []).some((m) => m.motor == null), [inventario]);
  const tipos = useMemo(
    () => [...new Set((inventario ?? []).map((m) => m.tipo))].sort(),
    [inventario],
  );

  const lista = useMemo(() => {
    const textoBuscado = ui.q.trim().toLowerCase();
    const filtrados = (inventario ?? []).filter((m) => {
      if (ui.tipo && m.tipo !== ui.tipo) return false;
      if (ui.familia && m.familia !== ui.familia) return false;
      // El filtro de motor tiene tres estados: "todos" (""), un motor concreto y
      // "sin motor" (los ficheros cuyo motor no consta).
      if (ui.motor === SIN_MOTOR) {
        if (m.motor != null) return false;
      } else if (ui.motor && m.motor !== ui.motor) {
        return false;
      }
      if (!textoBuscado) return true;
      return `${m.nombre} ${m.ruta} ${m.formato} ${m.familia} ${m.motor ?? ""}`
        .toLowerCase()
        .includes(textoBuscado);
    });
    const porNombre = (a: ModeloInventario, b: ModeloInventario) =>
      texto(a.nombre, b.nombre, "asc");
    const cmp = (a: ModeloInventario, b: ModeloInventario): number => {
      switch (col) {
        case "nombre":
          return texto(a.nombre, b.nombre, ui.dir);
        case "tipo":
          return texto(a.tipo, b.tipo, ui.dir) || porNombre(a, b);
        case "formato":
          // Formato primero y cuantización después: la celda enseña las dos
          // cosas, así que ordenar solo por una dejaría la columna desordenada a
          // la vista. El vacío (los ficheros donde no hay nada que deducir) se va
          // al final en los DOS sentidos, que no es un valor más.
          return (
            texto(a.formato, b.formato, ui.dir) ||
            textoConVaciosAlFinal(a.quant, b.quant, ui.dir) ||
            porNombre(a, b)
          );
        case "tamano":
          return numero(a.tamano_bytes, b.tamano_bytes, ui.dir) || porNombre(a, b);
        case "modificado":
          return numero(a.modificado, b.modificado, ui.dir) || porNombre(a, b);
        case "encaje": {
          const fa = encajeDe(fits, a.ruta)?.encaje ?? null;
          const fb = encajeDe(fits, b.ruta)?.encaje ?? null;
          return numero(rangoEncaje(fa), rangoEncaje(fb), ui.dir) || porNombre(a, b);
        }
      }
    };
    return [...filtrados].sort(cmp);
  }, [inventario, fits, ui.q, ui.tipo, ui.familia, ui.motor, ui.dir, col]);

  const totalGlobal = totalesPorTipo(inventario ?? []);
  const bytesGlobal = (inventario ?? []).reduce((a, m) => a + m.tamano_bytes, 0);
  const bytesFiltrado = lista.reduce((a, m) => a + m.tamano_bytes, 0);
  const conEncaje = useMemo(
    () => (inventario ?? []).filter((m) => encajeDe(fits, m.ruta) != null).length,
    [inventario, fits],
  );
  const hayFiltros =
    ui.q.trim() !== "" || ui.tipo !== "" || ui.familia !== "" || ui.motor !== "";

  if (inventario == null) {
    // "No se pudo leer" y "trayendo" no son lo mismo y no se pintan igual.
    return (
      <Vacio titulo={error ? "No se pudo leer el inventario de modelos" : "Leyendo el inventario…"}>
        {error}
      </Vacio>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {/* ── Totales por tipo: lo que se quiere ver de un vistazo ───────────── */}
      <section className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Etiqueta>Inventario por tipo</Etiqueta>
          <Insignia tono="neutro">
            {inventario.length} ficheros · {bLegibles(bytesGlobal, 1)}
          </Insignia>
          <Boton className="ml-auto" disabled={!!enCurso || cargandoInv} onClick={() => void cargar()}>
            <IconRefresh size={12} className="mr-1 inline" aria-hidden="true" />
            {cargandoInv ? "Leyendo…" : "Actualizar"}
          </Boton>
        </div>
        {totalGlobal.length === 0 ? (
          <Card>
            <p className="text-fg-muted text-sm">No se ha encontrado ningún modelo en las carpetas conocidas.</p>
          </Card>
        ) : (
          <div className="flex flex-wrap gap-2">
            {totalGlobal.map(([t, v]) => (
              <div
                key={t}
                className="border-line-soft bg-surface flex items-center gap-2 rounded-md border px-2.5 py-1.5"
              >
                <Insignia tono={tonoTipo(t)}>{etiquetaTipo(t)}</Insignia>
                <span className="text-fg-muted text-xs">
                  {v.ficheros} {v.ficheros === 1 ? "fichero" : "ficheros"} · {bLegibles(v.bytes, 1)}
                </span>
              </div>
            ))}
          </div>
        )}
        <p className="text-fg-faint text-xs">
          <strong className="text-fg-muted">Familia</strong> y <strong className="text-fg-muted">motor</strong> dicen
          de qué carpeta sale el fichero y qué programa lo usa (dato real). El{" "}
          <strong className="text-fg-muted">tipo</strong> se deduce de su ubicación y su nombre (una voz de piper
          está en la carpeta de piper): no se abre el fichero para adivinarlo. La{" "}
          <strong className="text-fg-muted">cuantización</strong> también se deduce, y solo en los{" "}
          <code className="mono">.gguf</code>: en otros formatos no hay nada que leer, así que se enseña —.
        </p>
      </section>

      {/* ── Filtros: se combinan entre sí ─────────────────────────────────── */}
      <Card>
        <div className="flex flex-wrap items-end gap-3">
          <label className="text-fg-muted flex flex-col gap-1 text-xs">
            Tipo
            <select
              value={ui.tipo}
              onChange={(e) => setUi("disco", { tipo: e.target.value })}
              className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
            >
              <option value="">todos</option>
              {tipos.map((t) => (
                <option key={t} value={t}>
                  {etiquetaTipo(t)}
                </option>
              ))}
            </select>
          </label>

          <label className="text-fg-muted flex flex-col gap-1 text-xs">
            Familia
            <select
              value={ui.familia}
              onChange={(e) => setUi("disco", { familia: e.target.value })}
              className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
            >
              <option value="">todas</option>
              {familias.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
          </label>

          <label className="text-fg-muted flex flex-col gap-1 text-xs">
            Motor
            <select
              value={ui.motor}
              onChange={(e) => setUi("disco", { motor: e.target.value })}
              className="border-line bg-raised rounded-md border px-2 py-1 text-xs"
            >
              <option value="">todos</option>
              {motores.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
              {haySinMotor ? <option value={SIN_MOTOR}>sin motor</option> : null}
            </select>
          </label>

          {/* Con el orden guardado entre vistas, hace falta una salida clara. */}
          {hayFiltros ? (
            <Boton
              onClick={() =>
                setUi("disco", { q: "", tipo: "", familia: "", motor: "" })
              }
            >
              Limpiar filtros
            </Boton>
          ) : null}

          <div className="border-line ml-auto flex items-center gap-2 rounded-md border px-2 py-1">
            <IconSearch size={14} className="text-fg-faint" aria-hidden="true" />
            <input
              value={ui.q}
              onChange={(e) => setUi("disco", { q: e.target.value })}
              placeholder="Buscar por nombre, ruta o formato…"
              aria-label="Buscar modelos"
              className="placeholder:text-fg-faint w-56 bg-transparent text-xs"
            />
          </div>
        </div>
        {/* Los totales que importan son los de lo FILTRADO, no solo los globales. */}
        <p className="text-fg-faint mt-2 text-xs">
          {lista.length} de {inventario.length} ficheros · {bLegibles(bytesFiltrado, 1)} con estos filtros. El{" "}
          <strong className="text-fg-muted">encaje</strong> lo calcula el backend solo (al arrancar y cada 10 min)
          para los <code className="mono">.gguf</code> de texto: {conEncaje} de {inventario.length} tienen ya su
          cálculo.
        </p>
        {/* Si la lectura de encajes falló, se dice: si no, cada fila parecería
            "sin calcular" y sería la vista la que estaría ocultando el fallo. */}
        {errorFits ? (
          <p className="text-bad mt-1 text-xs" role="alert">
            No se pudieron leer los encajes guardados: {errorFits}
          </p>
        ) : null}
      </Card>

      {/* ── Tabla ─────────────────────────────────────────────────────────── */}
      <Card className="overflow-x-auto p-0">
        {lista.length === 0 ? (
          <p className="text-fg-muted p-4 text-sm">Ningún modelo coincide con el filtro.</p>
        ) : (
          <table className="w-full min-w-[1100px] text-left text-xs">
            <caption className="text-fg-faint px-4 pt-3 text-left text-xs">
              Pulsa el nombre de una columna para ordenar por ella (y otra vez para invertir el sentido).{" "}
              <strong className="text-fg-muted">Encaje</strong> es el cálculo automático del backend, con su
              antigüedad.
            </caption>
            <thead className="text-fg-faint border-line-soft border-b">
              <tr>
                <ThOrden col="nombre" actual={col} dir={ui.dir} onOrdenar={(c, d) => setUi("disco", { col: c, dir: d })}>
                  Nombre
                </ThOrden>
                <ThOrden col="tipo" actual={col} dir={ui.dir} onOrdenar={(c, d) => setUi("disco", { col: c, dir: d })}>
                  Tipo
                </ThOrden>
                <ThOrden
                  col="formato"
                  actual={col}
                  dir={ui.dir}
                  onOrdenar={(c, d) => setUi("disco", { col: c, dir: d })}
                  titulo="Formato del fichero y, cuando se puede deducir del nombre, su cuantización (solo en los .gguf: va en el nombre, y es lo que distingue dos ficheros del mismo modelo). En safetensors, ONNX o PyTorch no se deduce y se enseña «—», que no es «desconocida»."
                >
                  Formato
                </ThOrden>
                <ThOrden
                  col="tamano"
                  actual={col}
                  dir={ui.dir}
                  primero="desc"
                  alineado="der"
                  onOrdenar={(c, d) => setUi("disco", { col: c, dir: d })}
                >
                  Tamaño
                </ThOrden>
                <ThOrden
                  col="encaje"
                  actual={col}
                  dir={ui.dir}
                  primero="desc"
                  onOrdenar={(c, d) => setUi("disco", { col: c, dir: d })}
                >
                  Encaje
                </ThOrden>
                <ThOrden
                  col="modificado"
                  actual={col}
                  dir={ui.dir}
                  primero="desc"
                  onOrdenar={(c, d) => setUi("disco", { col: c, dir: d })}
                >
                  Modificado
                </ThOrden>
                {/* La columna de acciones va PEGADA al borde derecho (`sticky`),
                    porque en WebKitGTK el reparto de anchos lo decide el
                    contenido: sin esto, los botones de una fila quedaban fuera
                    de la vista y había que desplazar la tabla entera para
                    encontrarlos. Son dos ICONOS con nombre accesible y `title`
                    (no botones de texto): con texto, la columna medía 210px de
                    las ~1000 que hay, y el resto de columnas se quedaba sin
                    sitio. */}
                <th
                  scope="col"
                  className="bg-surface border-line-soft sticky right-0 z-10 min-w-[92px] border-l px-4 py-2 text-right font-medium"
                >
                  Acciones
                </th>
              </tr>
            </thead>
            <tbody>
              {lista.map((m) => (
                <tr key={m.ruta} className="border-line-soft hover:bg-raised border-b align-top last:border-0">
                  <td className="max-w-[260px] px-4 py-2">
                    <div className="mono truncate" title={m.ruta}>
                      {m.nombre}
                    </div>
                    {/* La familia y el motor van AQUÍ, en pequeño, y no en
                        columna propia: contestan a "de dónde sale esto", que es
                        contexto para leer el nombre, no un dato por el que se
                        recorra la tabla. El motor solo se pone cuando APORTA
                        algo: si se llama igual que la familia, repetirlo era
                        gastar una línea. */}
                    <div className="text-fg-faint text-[11px] truncate">
                      {m.familia}
                      {m.motor && m.motor !== m.familia ? ` · ${m.motor}` : ""}
                    </div>
                  </td>
                  <td className="px-4 py-2">
                    <Insignia tono={tonoTipo(m.tipo)}>{etiquetaTipo(m.tipo)}</Insignia>
                  </td>
                  {/* Formato y cuantización en una celda: son la misma pregunta
                      ("¿qué es este fichero?"). El `—` es para los formatos donde
                      NO HAY cuantización que deducir (un .safetensors no la lleva
                      en el nombre): poner "desconocida" sería afirmar un dato que
                      no existe. */}
                  <td className="mono px-4 py-2 whitespace-nowrap">
                    {m.formato}
                    {m.quant ? (
                      <span className="text-fg-muted">{` · ${m.quant}`}</span>
                    ) : (
                      <span className="text-fg-faint"> · —</span>
                    )}
                  </td>
                  <td className="mono px-4 py-2 text-right whitespace-nowrap">{bLegibles(m.tamano_bytes, 1)}</td>
                  <td className="px-4 py-2">
                    <CeldaEncaje fit={encajeDe(fits, m.ruta)} ahora={ahora} sinLeer={errorFits != null} />
                  </td>
                  <td className="text-fg-muted mono px-4 py-2 whitespace-nowrap">{fechaHora(m.modificado)}</td>
                  <td className="bg-surface border-line-soft sticky right-0 z-10 border-l px-4 py-2">
                    <AccionesFila modelo={m} enCurso={enCurso} onCambio={() => void cargar()} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  );
}
