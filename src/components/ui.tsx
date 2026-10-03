/**
 * Primitivas de interfaz de Machinograph.
 *
 * Se inspiran en shadcn/ui pero están RE-ESTILIZADAS con los tokens de
 * `styles.css` (verde-azulado apagado, bordes de 1px, cero sombras). La regla:
 * si un componente no aporta jerarquía o estado, no se dibuja.
 *
 * Todas son accesibles por teclado y anuncian su estado (`aria-*`) donde toca.
 */
import { clsx } from "clsx";
import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { IconArrowsSort, IconChevronDown, IconChevronUp, IconClipboard } from "@tabler/icons-react";

export function Card({
  children,
  className,
  ...rest
}: { children: ReactNode; className?: string } & React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={clsx("card p-4", className)} {...rest}>
      {children}
    </div>
  );
}

export function Etiqueta({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={clsx("label", className)}>{children}</div>;
}

/** Cifra protagonista con su etiqueta, unidad y barra de uso opcional. */
export function Kpi({
  etiqueta,
  valor,
  unidad,
  uso,
  pie,
  serie,
  nivel = "ok",
}: {
  etiqueta: string;
  valor: string;
  unidad?: string;
  uso?: number;
  pie?: ReactNode;
  serie?: number[];
  nivel?: "ok" | "warn" | "bad" | "idle";
}) {
  return (
    <Card className="flex flex-col gap-2">
      <Etiqueta>{etiqueta}</Etiqueta>
      <div className="flex items-baseline gap-1.5">
        <span className="metric">{valor}</span>
        {unidad ? <span className="text-fg-faint text-xs">{unidad}</span> : null}
      </div>
      {serie && serie.length > 1 ? <Spark datos={serie} nivel={nivel} /> : null}
      {uso != null ? <Barra valor={uso} nivel={nivel} /> : null}
      {pie ? <div className="text-fg-muted text-xs">{pie}</div> : null}
    </Card>
  );
}

/** Barra de uso fina. El color NO es decorativo: sale del umbral. */
export function Barra({ valor, nivel = "ok" }: { valor: number; nivel?: "ok" | "warn" | "bad" | "idle" }) {
  const v = Math.max(0, Math.min(100, valor || 0));
  return (
    <div
      className={clsx("bar", nivel === "warn" && "is-warn", nivel === "bad" && "is-bad")}
      role="meter"
      aria-valuenow={Math.round(v)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <span style={{ width: `${v}%` }} />
    </div>
  );
}

/** Los tonos de `Insignia`. Se exporta para que el resto del código no repita la unión. */
export type Tono = "neutro" | "ok" | "warn" | "bad" | "acento";

export function Insignia({
  children,
  tono = "neutro",
}: {
  children: ReactNode;
  tono?: Tono;
}) {
  const tonos: Record<string, string> = {
    neutro: "border-line text-fg-muted",
    ok: "border-ok/40 text-ok",
    warn: "border-warn/40 text-warn",
    bad: "border-bad/40 text-bad",
    acento: "border-accent/40 text-accent",
  };
  return (
    <span
      className={clsx(
        "inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] leading-none",
        tonos[tono],
      )}
    >
      {children}
    </span>
  );
}

export function Boton({
  children,
  variante = "normal",
  className,
  // `ref` se saca de `...rest` a propósito y se pasa EXPLÍCITAMENTE al `<button>`
  // de abajo: al desestructurarla, dejaba de viajar en el spread, y el foco que
  // se pide para un botón recién aparecido (la confirmación de escritura) no
  // llegaba a ninguna parte. Se comprobó en la app real: `document.activeElement`
  // seguía siendo `<body>`.
  ref,
  ...rest
}: { children: ReactNode; variante?: "normal" | "acento" | "peligro"; ref?: React.Ref<HTMLButtonElement> } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const variantes: Record<string, string> = {
    normal: "border-line text-fg hover:bg-raised",
    acento: "border-accent/50 text-accent hover:bg-accent-soft",
    peligro: "border-bad/50 text-bad hover:bg-bad/10",
  };
  return (
    <button
      type="button"
      ref={ref}
      className={clsx(
        // `whitespace-nowrap`: una etiqueta de botón que se parte en dos líneas
        // deja de ser un botón y engorda la fila (en una tabla con 10 columnas,
        // "Abrir carpeta" se partía y ponía la fila a 123px).
        "rounded-md border px-2.5 py-1 text-xs whitespace-nowrap transition-colors disabled:opacity-40 disabled:pointer-events-none",
        variantes[variante],
        // `className` se fusiona, no sustituye: al ir en `...rest` DESPUÉS de
        // `className`, pasar una clase suelta (por ejemplo `ml-auto`) borraba
        // todo el estilo del botón.
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  );
}

/**
 * Chispa de serie temporal en canvas.
 * Canvas y no SVG a propósito: se repinta con cada foto y así no se recrean nodos ni
 * se fuerza al navegador a recalcular el layout en cada punto.
 */
export function Spark({ datos, nivel = "ok", alto = 28 }: { datos: number[]; nivel?: string; alto?: number }) {
  const ref = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const dpr = window.devicePixelRatio || 1;
    const ancho = cv.clientWidth || 200;
    cv.width = ancho * dpr;
    cv.height = alto * dpr;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, ancho, alto);

    const n = datos.length;
    if (n < 2) return;
    const max = 100;
    const x = (i: number) => (i / (n - 1)) * ancho;
    const y = (v: number) => alto - (Math.max(0, Math.min(max, v)) / max) * (alto - 2) - 1;

    const color =
      nivel === "bad"
        ? getComputedStyle(document.documentElement).getPropertyValue("--color-bad")
        : nivel === "warn"
          ? getComputedStyle(document.documentElement).getPropertyValue("--color-warn")
          : getComputedStyle(document.documentElement).getPropertyValue("--color-accent");

    // Área
    ctx.beginPath();
    ctx.moveTo(x(0), y(datos[0]));
    for (let i = 1; i < n; i++) ctx.lineTo(x(i), y(datos[i]));
    ctx.lineTo(ancho, alto);
    ctx.lineTo(0, alto);
    ctx.closePath();
    ctx.globalAlpha = 0.14;
    ctx.fillStyle = color;
    ctx.fill();
    ctx.globalAlpha = 1;

    // Línea
    ctx.beginPath();
    ctx.moveTo(x(0), y(datos[0]));
    for (let i = 1; i < n; i++) ctx.lineTo(x(i), y(datos[i]));
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "round";
    ctx.stroke();
  }, [datos, nivel, alto]);

  return <canvas ref={ref} className="w-full" style={{ height: alto }} aria-hidden="true" />;
}

/**
 * Estado vacío con título: sirve tanto para "aún no hay datos" como para "no se
 * pudo leer". Existe porque fuera del host Tauri (Vite en el navegador) todas
 * las llamadas `invoke` fallan y ninguna vista debe quedarse en blanco.
 */
export function Vacio({ titulo, children }: { titulo: string; children?: ReactNode }) {
  return (
    <Card>
      <Etiqueta>{titulo}</Etiqueta>
      {children ? <p className="text-fg-muted mt-2 text-sm">{children}</p> : null}
    </Card>
  );
}

/**
 * Botón de copiar al portapapeles, con su confirmación.
 *
 * Está aquí porque se usa en tres sitios (la limpieza, la programación y las
 * actualizaciones) y en los tres hace lo mismo: copiar un texto que hay que pegar
 * en otro sitio. Si el portapapeles no deja (fuera del host Tauri, o sin permiso),
 * no se pinta un error: el texto se puede seleccionar a mano, que es lo que haría
 * cualquiera.
 */
export function BotonCopiar({ texto, que, className }: { texto: string; que: string; className?: string }) {
  const [copiado, setCopiado] = useState(false);
  useEffect(() => {
    if (!copiado) return;
    const t = setTimeout(() => setCopiado(false), 2000);
    return () => clearTimeout(t);
  }, [copiado]);
  return (
    <Boton
      className={className}
      onClick={() => {
        navigator.clipboard
          .writeText(texto)
          .then(() => setCopiado(true))
          .catch(() => setCopiado(false));
      }}
      aria-label={`Copiar ${que}`}
      title={`Copiar ${que}`}
    >
      <IconClipboard size={12} aria-hidden="true" /> {copiado ? "Copiado" : "Copiar"}
    </Boton>
  );
}

/** Rejilla de campos clave-valor para las fichas de detalle. */
export function Datos({ items }: { items: [string, ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
      {items.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-fg-faint">{k}</dt>
          <dd className="mono text-fg">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

/* ── Tablas ordenables ────────────────────────────────────────────────────── */

/** Sentido de la ordenación. Se exporta para que el estado guardado use el mismo. */
export type Dir = "asc" | "desc";

/**
 * Cabecera de columna ordenable (`th` con `aria-sort`).
 *
 * Por qué un BOTÓN dentro del `th` y no el `th` a secas: así la columna se puede
 * ordenar con el teclado y quien usa lector de pantalla oye un control, no un
 * texto muerto. El `aria-sort` va en el `th` (es donde lo busca el lector) y
 * cambia con el estado, y el icono + el color de la cabecera dicen cuál manda y
 * en qué sentido sin depender solo del color.
 *
 * Regla de clic, igual en las tres tablas: la columna activa invierte el sentido
 * y una columna nueva empieza por `primero` (ascendente, o descendente en las
 * numéricas, donde lo que se busca es "el más grande primero").
 */
export function ThOrden({
  col,
  actual,
  dir,
  primero = "asc",
  onOrdenar,
  children,
  titulo,
  alineado = "izq",
  className,
}: {
  col: string;
  actual: string;
  dir: Dir;
  primero?: Dir;
  onOrdenar: (col: string, dir: Dir) => void;
  children: ReactNode;
  /** Aclaración de la columna (qué significa o de dónde sale). Solo el `title`. */
  titulo?: string;
  alineado?: "izq" | "der";
  className?: string;
}) {
  const activa = actual === col;
  const Icono = !activa ? IconArrowsSort : dir === "asc" ? IconChevronUp : IconChevronDown;
  return (
    <th
      scope="col"
      aria-sort={activa ? (dir === "asc" ? "ascending" : "descending") : "none"}
      className={clsx("px-4 py-0 font-medium", alineado === "der" && "text-right", className)}
    >
      <button
        type="button"
        title={titulo}
        onClick={() => onOrdenar(col, activa ? (dir === "asc" ? "desc" : "asc") : primero)}
        // Sin `aria-label`: el nombre accesible ES el texto visible, que es lo que
        // hay que poder decir por voz (WCAG 2.5.3). El estado lo lleva el `th`.
        className={clsx(
          "inline-flex w-full items-center gap-1 rounded-sm py-2",
          alineado === "der" ? "justify-end" : "justify-start",
          activa ? "text-fg" : "hover:text-fg-muted",
        )}
      >
        {children}
        <Icono size={12} aria-hidden="true" className={activa ? undefined : "opacity-50"} />
      </button>
    </th>
  );
}

/** Cabecera NO ordenable, con el mismo relleno que las ordenables. */
export function Th({ children, alineado = "izq", className }: {
  children: ReactNode;
  alineado?: "izq" | "der";
  className?: string;
}) {
  return (
    <th
      scope="col"
      className={clsx("px-4 py-2 font-medium", alineado === "der" && "text-right", className)}
    >
      {children}
    </th>
  );
}

/**
 * Reloj local que re-renderiza cada `ms`.
 *
 * Para qué: los textos de antigüedad ("hace 4 min") se congelarían sin esto, y un
 * "hace 4 min" de hace media hora es una mentira. No pide nada al backend: solo
 * vuelve a mirar la hora.
 */
export function useReloj(ms = 30_000): number {
  const [ahora, setAhora] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setAhora(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return ahora;
}
