# DESIGN.md — Machinograph

Sistema de diseño de Machinograph: lo que hay, por qué está así y las reglas para
seguir tocándolo. Los números **están medidos**, no estimados, y las fuentes
**verificadas en el equipo**.

Herramientas con las que se definió: `ui-ux-pro-max` (patrón, estilo,
tipografía, checklist) e `impeccable` (oficio). Se anota también **dónde se
decide no hacerles caso**, que es tan importante como seguirlas.

---

## 1. Qué es esto y para quién

Un **panel de control de escritorio** para mirar y gobernar una máquina que corre
modelos de IA en local: hardware, servidores de inferencia, modelos de todo tipo,
pantallas y GPU. Lo usa **una persona** (su dueño), en su propio escritorio, y lo
mira muchas veces al día, pocos segundos cada vez.

De ahí salen las tres decisiones que mandan sobre todo lo demás:

1. **Denso pero escaneable.** El ojo entra a buscar un número o un estado. Los
   datos van juntos y en columnas, con la cifra en monoespaciada y grande.
2. **Oscuro, y solo oscuro.** Es una herramienta de trabajo de escritorio; no hay
   variante clara que mantener. `ui-ux-pro-max` propone "Dark Mode (OLED)" con
   negro puro: **no se sigue**, porque el negro absoluto sobre un panel con capas
   aplasta la jerarquía (las tarjetas se distinguen por su capa de fondo, no por
   sombras) y cansa en sesiones largas. Se usa una escala de azul-grafito.
3. **Nada decorativo.** Sin degradados, sin sombras, sin animaciones que no
   expliquen algo. La jerarquía la dan el tamaño, el espacio y el contraste.

**Anti-patrones declarados**: modo claro por defecto, animaciones decorativas,
iconos con emoji, azul de enlace por defecto, tarjetas con sombra difusa.

## 2. Color

Tokens en `src/styles.css` (`@theme`). Todos en **OKLCH**, que es lo que permite
que la escala de fondos sea perceptualmente uniforme.

| Token | OKLCH | Papel |
| --- | --- | --- |
| `--color-bg` | `17.5% 0.012 255` | Fondo de la ventana (lo más atrás) |
| `--color-surface` | `21.5% 0.014 255` | Tarjeta: la unidad de todo el panel |
| `--color-raised` | `25.5% 0.016 255` | Lo que está por encima de una tarjeta (hover, campos) |
| `--color-line` | `31% 0.016 255` | Bordes de controles |
| `--color-line-soft` | `26.5% 0.014 255` | Separadores internos, bordes de tarjeta |
| `--color-fg` | `94% 0.005 255` | Texto principal |
| `--color-fg-muted` | `74% 0.012 255` | Texto secundario |
| `--color-fg-faint` | `65% 0.012 255` | Texto terciario, etiquetas |
| `--color-accent` | `78% 0.13 205` | Acento único (cian de instrumentación) |
| `--color-accent-dim` | `45% 0.08 205` | Acento en bordes y estados pasivos |
| `--color-accent-soft` | `28% 0.05 205` | Fondo de lo seleccionado |
| `--color-ok` | `78% 0.15 155` | Estado correcto |
| `--color-warn` | `82% 0.14 85` | Aviso |
| `--color-bad` | `72% 0.17 25` | Problema |
| `--color-idle` | `65% 0.02 255` | Sin dato / inactivo |

### Reglas de color

- **El color es estado, no decoración.** Verde/ámbar/rojo significan algo. Si un
  dato no tiene estado, va en la escala de grises.
- **El color nunca va solo**: todo estado lleva texto o icono además del color
  (`color-not-only`). Un servidor parado no es rojo: es neutro y dice "parado".
- **Contraste medido** con la fórmula WCAG, sobre los tres fondos posibles. El
  caso más justo es `fg-faint`, con **4,88:1** sobre `raised`; todo lo demás va
  muy por encima (los estados, entre 5,9:1 y 10,8:1). Si se toca un token, hay
  que volver a medirlo: no se estima a ojo.
- **Nada de hex sueltos en los componentes**: siempre token. Si falta un rol, se
  añade al `@theme`, no se improvisa en el sitio.

### Desviación consciente de `ui-ux-pro-max`

Su bloque de colores para este caso devuelve una paleta **clara** (fondo
`#F8FAFC`) mientras el estilo recomendado es "solo oscuro". Es contradictorio, así
que se ignora y se mantiene la paleta propia, que está medida y cumple AA.

## 3. Tipografía

| Papel | Familia | Por qué |
| --- | --- | --- |
| Texto | **Noto Sans** | Es la del sistema (verificado con `fc-list`): no falla y no engorda el binario |
| Cifras y rutas | **Fira Code** | Monoespaciada **instalada** en el equipo y técnica, que es lo que pide un panel de datos |

**Corrección importante**: el fichero declaraba `Inter` y `JetBrains Mono`, y
**ninguna de las dos está instalada** — el webview caía a Noto Sans sin avisar y
la interfaz no se veía como decía su propio diseño. Ahora las familias declaradas
son las que existen.

- Base **14px**, interlineado **1.5**. Las etiquetas de sección, 11px en
  mayúsculas con `letter-spacing: 0.08em`.
- **Las cifras van en monoespaciada siempre** (`.metric`, `.mono`): las columnas
  de números no bailan al cambiar de valor (`number-tabular`).
- Jerarquía por peso y tamaño, no por color: el dato es `fg`, la etiqueta es
  `fg-faint`.

### Números: una sola regla

- **Punto decimal y sin separador de millares**, en todas las cifras y en todas
  las vistas. La coma no separa nada en un número, y fecha y hora van separadas
  por un espacio (`27/09 09:15`), nunca por coma (`27/09, 09:15`).
- **Un solo punto de verdad**: `num()` en `src/lib/format.ts`. Los demás
  formateadores (`mb`, `gb`, `bLegibles`, `pct`) DELEGAN en él en vez de repetir
  su propio `.toFixed()`: cambiar la regla es cambiar una función.
- **El motivo**, que es lo que la defiende: (1) el backend redacta sus mensajes
  con `{:.1}` (punto) y aquí se enseñan **tal cual**, así que una coma en la
  interfaz dejaría dos criterios en la misma pantalla; (2) estos números se copian
  a comandos y se comparan con salidas de `llama.cpp`, que usa punto; (3) sin
  millares, `262144` no se confunde con un decimal.
- **«—», nunca un 0**, cuando el dato no viene (`null`/`NaN`): un 0 afirmaría un
  valor que no tenemos. Vale igual para `vram_gb` nulo en una vía de CPU, que
  significa "no necesita VRAM dedicada".

## 4. Espacio, forma y profundidad

- **Rejilla de 4px** (el ritmo de Tailwind). Los huecos entre bloques: 16px;
  dentro de una tarjeta, 12-16px.
- **Radios**: `--radius-card` para tarjetas, `6px` (rounded-md) para botones y
  campos. Nada de radios distintos para lo mismo.
- **Profundidad por capas de fondo**, no por sombras: `bg` → `surface` → `raised`.
  Una tarjeta dentro de otra se distingue por su borde de 1px (`line-soft`).
- La ventana mínima es **960×640** y a ese ancho no debe haber scroll horizontal
  de página: lo que no quepa, hace scroll dentro de su propio contenedor.

## 5. Movimiento

- Transiciones de **150-300ms**, `ease-out` al entrar. Nada por encima de 400ms.
- Se anima **`transform` y `opacity`**, nunca `width`/`height`/`top`.
- **`prefers-reduced-motion` se respeta** (hay un bloque global que lo reduce a
  prácticamente cero).
- El movimiento explica algo: que algo ha cambiado, que algo está cargando, que
  algo acaba de llegar. Si no explica nada, no se anima.

## 6. Iconos

- **`@tabler/icons-react`**, un solo juego, trazo consistente, tamaños 13/15/18.
- **Nunca emoji** como icono estructural.
- Los decorativos llevan `aria-hidden="true"`; los que son la única etiqueta de un
  botón, un `aria-label`, **empezando por el texto visible** si lo hay (WCAG
  2.5.3: el nombre accesible no puede contradecir lo que se lee).

## 7. Reglas de interacción y accesibilidad

Estas salen del checklist de `ui-ux-pro-max` y son **obligatorias**:

- **Foco siempre visible**: `:focus-visible` con el acento y 2px de separación.
  Nunca `outline: none`.
- **Al cambiar de sección, el foco se mueve al contenido principal** (quien navega
  con teclado o lector de pantalla no se queda perdido en el menú).
- **Tablas**: las columnas de datos se pueden **ordenar** y el estado va en
  `aria-sort`; cada `th` con `scope="col"`. **Excepción declarada**: las tablas que
  no son un conjunto de datos sino una **progresión** (la escalera de concurrencia
  de llmfit y sus vías de ejecución) NO se ordenan: reordenarlas destruiría lo que
  significan. Llevan `caption` que lo explica.
- **Conservar el estado**: al salir de una vista y volver, se recuperan sus
  filtros, su búsqueda y su orden.
- **Acciones destructivas**: color de peligro, separadas físicamente de las
  normales, y **confirmación**. Si se pueden deshacer, se dice cómo (borrar un
  modelo va a la **papelera**, y el mensaje lo recuerda).
- **Estados vacíos honestos**: "no hay nada todavía" con una pista de qué hacer;
  nunca un "no hay nada" cuando lo que hubo fue un **error** (eso se dice aparte).
- **Cargando**: si algo tarda más de ~300ms, se ve que está trabajando; los
  botones se deshabilitan mientras corre su acción.
- **Nada se inventa**: si un dato no está, se enseña "—" o no se enseña. Y si un
  número viene de una estimación en vez de una medición, se etiqueta.
- **Un total que encoge, se explica.** Si un total es más pequeño de lo que el
  usuario espera, la razón está en la pantalla: lo excluido por sus exclusiones (con
  el patrón que lo excluyó), lo que no se pudo leer, lo que se cortó por
  presupuesto. Un número que baja en silencio se lee como un fallo del programa, y
  eso es peor que un número grande.
- **Cada sistema, lo suyo.** Lo que no existe en el sistema del usuario se dice con
  su motivo («aquí los permisos son ACL, no 0700»), y lo que existe pero no se puede
  hacer desde aquí también («ver las pantallas sí, cambiar el modo no: no hay API
  soportada»). Un «bien» de algo que no se ha podido mirar es una mentira, no un
  resumen.

## 8. Los componentes del panel

### La navegación va agrupada, y cada sección dice para qué es

La barra lateral tiene **cuatro grupos** —Modelos, Motor, Equipo y, suelta,
Ajustes— y **quince secciones**. Antes eran doce secciones planas y varias se
solapaban de verdad, no en apariencia:

- **`src/views/Models.tsx` y `src/views/Inventory.tsx` leían la MISMA fuente**
  (`inventario:listar`). Eran dos tablas para los mismos datos.
- **Panel y Sistema partían el hardware**: uno los KPI y la GPU, el otro las
  series, el disco y los procesos. Mirar «cómo va la máquina» obligaba a saltar
  entre las dos.
- **Diagnóstico repetía avisos del Panel**, y «Actualizaciones» y «Registro» eran
  la misma pregunta (qué se ha ejecutado) en dos sitios.

Con grupos, cada sección responde a una pregunta distinta y se ve a qué bloque
pertenece. Y cada una lleva **una línea que dice para qué es**, en la cabecera y
en el `title` del botón: agrupar no basta si dentro sigue sin saberse qué hay.
El reparto es:

| Grupo | Sección | Pregunta que contesta |
| --- | --- | --- |
| — | Inicio | ¿Hay algo que atender? |
| Modelos | Descubrir | ¿Qué me falta por tener y me cabría? |
| | En disco | ¿Qué tengo y cuánto ocupa? |
| | Rendimiento | ¿Cuánto da y por qué vía? |
| Motor | Servidores | ¿Quién está sirviendo? |
| | Conexiones | ¿Quién lo usa? |
| Equipo | Hardware | ¿Cómo va la máquina por dentro? |
| | Pantalla | ¿Qué monitores hay y en qué modo? |
| | Almacenamiento | ¿Qué ocupa el disco y qué puedo borrar? |
| | Optimización | ¿Qué basura puedo tirar sin miedo y qué arranca solo? |
| | Seguridad | ¿Qué se ejecuta sin que lo vea, y qué huellas dejo? |
| | Diagnóstico | ¿Qué está mal y cómo se arregla? |
| | Mantenimiento | ¿Qué se ha ejecutado? |
| — | Ajustes | ¿Qué servidores hay de alta y cómo se comporta la app? |

**Almacenamiento y Optimización son dos preguntas, no una.** Las dos pueden
borrar, y por eso mismo no se juntan: ver un fichero de 8 GB en un analizador y
tener que decidir si es basura o un dato tuyo sería justo el error que se quiere
evitar. La frontera está escrita en cada pantalla:

- En **Almacenamiento** se borran cosas TUYAS, así que por defecto va a la
  **papelera** (se recupera) y el borrado definitivo es una elección explícita,
  con la frase "no se puede deshacer" en la confirmación.
- En **Optimización** se borra **basura regenerable**, así que va directo y de
  verdad: mover una caché de 3 GB a la papelera no libera nada hasta vaciarla, y
  prometer que "se libera" sería mentira. La confirmación dice cuánto y qué se
  borra, y los objetivos que necesitan root **enseñan su comando** en vez de
  lanzarlo a escondidas.

Las dos comparten una regla nueva que sale de aquí: **lo que se mide es
exactamente lo que se borra**. El tamaño que enseña un objetivo de limpieza no es
"lo que ocupa la carpeta", es "lo que se liberaría al pulsar el botón" —contando
la antigüedad mínima de cada regla y dejando fuera lo reciente, que se cuenta
aparte.

Y hay una **tercera clase** de cosa, que no se limpia en ninguna de las dos: las
**huellas de tu actividad** (historiales, recientes, portapapeles). No son cachés
—no se regeneran solas— ni datos que quieras conservar tal cual, así que tienen su
propia sección (**Seguridad**) y su propia regla:

- **No hay «marcar todo».** Se marcan una a una: borrar el historial entero de un
  clic no puede ser un efecto colateral de un botón que dice «lo que ocupa». En
  Optimización, la categoría de huellas ni se lista (y se dice dónde está).
- **Va en dos pasos y NO pasa por la papelera**, con la frase de que no se
  recupera: una huella que quieres borrar no puede quedarse en la basura esperando.
- **La marca vive en el modelo de datos** (`Regla::traza` → `Objetivo::traza`), no
  en la interfaz: así el CLI (`--aplicar`, que necesita `--categoria privacidad`),
  la pantalla y el arnés aplican la MISMA protección, que es lo único que impide
  que una de las tres se olvide.

Tres reglas que acompañan a la agrupación:

1. **Lo que va mal va primero**, y en Inicio. No en un bloque escondido: la
   portada existe para eso.
2. **Nada se dice dos veces.** Si un aviso con remedio está en Inicio, en la
   sección de detalle está el DATO y no el aviso otra vez. La excepción está
   declarada: el reloj de memoria aparece en Inicio (como aviso, con los
   remedios) y en Hardware (como ficha, para vigilarlo), porque son dos
   intenciones distintas sobre el mismo dato y el componente es uno solo
   (`components/RelojMemoria.tsx`). Y cuando el remedio de un aviso está **en
   otra sección**, el aviso no repite esa pantalla: lleva un **botón que salta**
   allí (`setVista`). Hoy hay dos, y los dos están comprobados en el arnés:
   Optimización → Seguridad (las huellas no se borran ahí) y Seguridad →
   Optimización (la lista del arranque, con sus interruptores).
3. **Las columnas de acción van pegadas al borde derecho** (`sticky right-0`) en
   toda tabla que pueda desbordar, y son **iconos con nombre accesible**, no
   botones de texto: ocupaban 210px de los ~1000 que hay y dejaban sin sitio a
   las columnas que importan.

### Los sensores: cada cifra con su origen

Hardware enseña todo lo que el equipo publica por `/sys/class/hwmon`, y ahí hay
una regla nueva que no estaba escrita: **cada medida lleva su procedencia**. El
`title` de cada fila es la ruta sysfs de la que sale el número, así que se puede
comprobar con `cat`. Un panel que dice «66 °C» sin decir de qué chip lo ha leído
no se puede verificar, y lo que no se puede verificar no se puede creer.

Tres consecuencias de esa regla, y las tres se ven en la pantalla:

- **Un 0 no se enseña como medida.** Esta placa tiene 24 sensores desconectados
  que leen 0 exactos. Se descartan y se DICE cuántos («24 sensores desconectados
  no se enseñan»): callarlo haría parecer que faltan datos por un fallo del
  programa, cuando es una característica de la placa.
- **Los umbrales solo se enseñan si están por encima de la lectura.** El
  `nct6683` publica como `temp1_max` el valor de AHORA (40 sobre 40), y el
  `amdgpu` no publica `max` pero sí `crit`. Un «40 de 40 °C» parece un aviso y no
  lo es, así que el umbral que no está por encima se descarta.
- **Lo que aparece dos veces se explica.** Dos drivers publican el mismo chip de
  la placa: uno bien (con nombres) y otro mal (todo a 0). El segundo no se
  esconde: va en un bloque **plegado** con una línea que dice qué pasa.

Y una cifra que necesita dos lecturas (la potencia de la CPU, el caudal de disco
y de red) se enseña como «—» la primera vez, diciendo por qué. Rellenarla con un
0 sería afirmar que la máquina no consume y que no hay tráfico.

### Los componentes

- **`Card`** — la unidad de todo. Título con `.label` y contenido.
- **`Kpi`** — una cifra protagonista con su unidad pequeña al lado; opcionalmente
  barra y chispa (serie de los últimos minutos).
- **`Barra`** — progreso fino; el color lo da el **estado**, no el valor.
- **`Insignia`** — estado en una palabra ("activo", "parado", "degradado").
- **`Boton`** — tres variantes: normal, acento (la acción principal, **una por
  pantalla**) y peligro.
- **`Datos`** — pares clave-valor en monoespaciada para fichas de detalle.
- **`Vacio`** — estado vacío con su explicación.
- **Panel de salida en vivo** — para todo lo que lanza un proceso: cada línea
  aparece cuando llega, y los errores se distinguen (`[err]`).
- **Fila de comprobación** (Diagnóstico) — estado por **texto + icono + color**
  (nunca solo color), su detalle y, si hay algo que hacer, el remedio en un bloque
  aparte. Van primero las que están mal, luego las que no se saben y al final las
  que van bien.
- **Bloque generado con copiar** (Clientes conectados) — el fichero **completo** como
  quedaría, en monoespaciada, con su resumen y su botón de copiar. Cuando el cliente
  admite escritura, debajo va el botón de escribir **en dos pasos**: el primero no
  escribe, y la confirmación dice el destino, el patrón de la copia de seguridad y qué
  pasa si la comprobación falla. La frontera —dónde se escribe y dónde no— va **arriba y
  en tamaño de lectura**, no como nota al pie.
- **Dos caminos de medida** — cuando dos formas de medir lo mismo dan números que
  no se comparan, se enseñan como bloques rotulados y se dice con palabras en qué
  se diferencian y por qué no se mezclan.
- **Barra de borrado en dos pasos** (Almacenamiento y Optimización) — la acción
  destructiva va **separada del resto**, en su propia tarjeta con color de peligro,
  y el primer clic NO borra: abre una confirmación que dice cuántos elementos son,
  cuánto ocupan y qué pasa después. En Almacenamiento se elige entre papelera y
  definitivo, y **la papelera es la opción de partida**: el definitivo hay que
  marcarlo a mano y su aviso dice que no se deshace.
- **Insignias de límite** (Optimización) — "necesita root", "con su comando", "sin
  permiso" y "medición incompleta". No son adornos: cada una explica por qué ese
  objetivo no se limpia desde aquí (o por qué su tamaño puede quedarse corto). El
  que necesita root **no se puede marcar** y lleva su comando exacto con un botón
  de copiar, porque lanzar `sudo` desde la app a escondidas sería lo contrario de
  lo que se predica.

## 9. Cómo se comprueba

Antes de dar por buena una pantalla:

1. `pnpm typecheck` y `pnpm build` en verde.
2. Recorrido real de **todas** las secciones en el binario release (capturas).
3. Contraste: si se han tocado tokens, volver a medir.
4. A 960×640: sin scroll horizontal de página.
5. Con teclado: se puede llegar a todo y el foco se ve.
6. Con `prefers-reduced-motion`: nada se mueve.

**Cómo se acciona la app de verdad, si los clics no llegan:** en esta máquina los clics de `xdotool`
no llegan a la ventana (devuelven éxito y no pasa nada), y el teclado obliga a contar tabulaciones, que
cambian con los datos. Lo que sí funciona es el **árbol de accesibilidad**: con `pyatspi` se localiza
el botón por su nombre y se lanza su acción semántica, y a un campo se le da el foco y se escribe. Es
la forma de probar de punta a punta un flujo con botones en el binario real.

**Y una advertencia que costó tiempo:** el arnés del navegador corre en Chromium y la app usa
**WebKitGTK**. No reparten igual las tablas. Un ancho que Chromium respeta (el `colgroup`,
`table-layout: fixed`) WebKit lo decide por el contenido, así que una tabla puede caber en el arnés y
salirse de la tarjeta en la app. Lo que depende del reparto de una tabla **se mira en la app**, no en
el arnés. De ahí la regla práctica que se aplica ya en el Inventario: la columna de ACCIÓN va
**pegada al borde derecho** (`sticky right-0`) para que el botón de una fila esté siempre a la vista
aunque el resto de la tabla se desplace.

## 10. Sin fricción: detectar, instalar y reparar

El criterio que manda en todo lo que la aplicación hace **por su cuenta**: el usuario no debería tener
que averiguar nada, ni buscar un comando, ni diagnosticar un error de la propia herramienta. De ahí
salen cuatro reglas, y las cuatro están implementadas y probadas:

1. **Detectar antes de pedir.** Lo que se puede saber, se sabe: qué herramientas hay, cuáles faltan y
   de dónde saldrían. La pantalla no pregunta lo que puede medir.
2. **Instalar lo que se puede, sin tocar el sistema.** Las herramientas que la aplicación necesita
   (llmfit, llama.cpp) se descargan de su versión oficial, se comprueban con su **sha256** y se
   verifican **ejecutándolas**, y se guardan dentro de la carpeta de datos del usuario. Un guardia
   impide instalar en cualquier otro sitio, y `sudo` **no se lanza nunca**: lo que necesita permisos
   de administrador se detecta, se dice por qué y se enseña el comando exacto.
3. **Nunca en silencio.** Descargar es una acción de red: se ve, dice qué está trayendo y se puede
   cancelar. Y lo que se instala aparece con su origen y su versión, para poder comprobarlo.
4. **Reparar en vez de avisar.** Lo que la aplicación rompe o gestiona, lo arregla: si el puerto está
   ocupado coge otro y lo dice; si su base de datos se daña, la **aparta** (no la borra) y sigue; si
   el arranque que el usuario activó desaparece, lo vuelve a poner; si un fichero que escribió quedó
   ilegible, lo restaura desde su copia. Y lo que no se pudo arreglar se dice **con lo que haría
   falta**, nunca como un «todo bien».

La frontera, que es lo que hace que esto sea honesto y no magia: **los datos del usuario no se tocan**
(reparar es mover, copiar o reescribir lo que escribió la aplicación, nunca decidir por él), y **lo que
se rompe se conserva** para poder recuperarlo (una base dañada se aparta con fecha y hora).

## 11. Lo que NO se hace

- Modo claro "por si acaso": no hay variante clara y no se finge que la hay.
- Datos de relleno para que la pantalla parezca llena.
- Un icono sin etiqueta donde no se entiende qué hace.
- Llamar «antivirus» a un chequeo de persistencia, ni pintar un «bien» de lo que no
  se ha podido mirar: en Seguridad, lo que no se pudo comprobar se enseña aparte
  («sin comprobar», no «ok») y **el alcance está escrito en la propia pantalla**,
  porque un verde sin decir qué se ha mirado es peor que no decir nada.
- Copiar la estética base de shadcn sin adaptarla: la base sirve, el aspecto es
  de este panel.
- Prometer en el texto lo que el código no hace.
- **Escribir en la configuración de un cliente cuyo formato no se haya comprobado
  leyendo su fichero real**: no se hace. Hoy solo se escribe en `gentle-shell`
  (`providers.<id>.models` como objetos), y siempre con copia de seguridad con fecha,
  escritura atómica, permisos del original, verificación releyendo y restauración
  automática si no cuadra. Para mcode y Codex se detecta qué tienen y se genera el texto
  para pegar a mano, y la interfaz lo dice sin rodeos: reescribir un fichero ajeno, con
  sus claves y comentarios, puede romperlo sin que nadie se entere.
- **Inventar los metadatos de un modelo al conectarlo**: los que ya están declarados en
  el fichero se reutilizan tal cual (contexto medido, `reasoning`, `compat`); los que no,
  se escriben con lo mínimo y se dice cuáles.
