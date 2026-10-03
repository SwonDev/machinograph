//! Limpieza de basura: cachés, temporales y registros que se pueden borrar sin
//! miedo porque se regeneran solos.
//!
//! El catálogo está PORTADO de las reglas de Kudu (`rules/linux/*.json`,
//! MIT): sus rutas están comprobadas y, sobre todo, su distinción entre lo que es
//! una caché y lo que es dato del usuario. Aquí no se inventa ninguna ruta: cada
//! entrada dice de dónde sale y qué se pierde al borrarla.
//!
//! Tres decisiones que separan esto del "borra y ya":
//!
//! 1. **Borrar de verdad, no a la papelera.** Mover una caché de 3 GB a la
//!    papelera NO libera espacio hasta que se vacía, así que para basura la
//!    papelera es un espejismo. Se borra definitivamente, y por eso la interfaz
//!    enseña la lista completa de lo que se va a borrar antes de hacerlo.
//! 2. **Antigüedad por regla.** Un borrador de caché de una app que está abierta
//!    ahora mismo no se toca si la regla pide una semana; lo reciente se cuenta
//!    aparte (`recientes`) en vez de colarse.
//! 3. **Lo que necesita root no se hace a medias.** Se mide y se enseña el
//!    comando exacto (`sudo dnf clean packages`), pero no se lanza `sudo` a
//!    escondidas ni se finge que se ha limpiado.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Tope de entradas y de tiempo por escaneo: una caché enorme no puede dejar la
/// ventana colgada, y lo que no se ha mirado se dice.
/// La cifra está medida en este equipo: el catálogo entero (cachés de uv, pip,
/// Flatpak, npm, Gradle…) son ~280 000 entradas y tarda ~7 s. Con 300 000 el
/// escaneo de un equipo NORMAL salía truncado y el total quedaba corto.
const MAX_ENTRADAS: u64 = 1_500_000;
const MAX_TIEMPO: Duration = Duration::from_secs(40);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoRegla {
    /// Se borra el CONTENIDO de la carpeta y se conserva la carpeta.
    Contenido,
    /// Un fichero suelto.
    Fichero,
    /// No se borra desde aquí: hay un comando nativo que lo hace mejor.
    Comando,
}

pub struct Regla {
    pub id: &'static str,
    pub categoria: &'static str,
    pub subcategoria: &'static str,
    pub descripcion: &'static str,
    pub rutas: &'static [&'static str],
    pub tipo: TipoRegla,
    pub min_dias: u32,
    pub root: bool,
    pub comando: Option<&'static str>,
    /// Es una HUELLA de tu actividad, no basura regenerable.
    ///
    /// La diferencia importa: una caché se vuelve a crear sola, pero tu historial
    /// de órdenes, la lista de documentos recientes o el portapapeles **no**. Por
    /// eso las huellas:
    ///
    /// * van en su propia categoría (`privacidad`), con su aviso;
    /// * **no se marcan solas** en la interfaz («marcar todo lo que ocupa» las
    ///   salta), y hay que seleccionarlas a mano;
    /// y el CLI **no las borra** ni con `--aplicar`: hacen falta `--categoria
    /// privacidad`, para que nadie se lleve por delante su historial sin querer.
    pub traza: bool,
    /// **Reinicio de caché de rendimiento** (`cacheReset` de Kudu): una caché de
    /// shaders de GPU, la de Steam… Borrarla no rompe nada, pero la PRIMERA vez
    /// todo va más lento mientras se recompila.
    ///
    /// POR QUÉ NO ES BASURA NORMAL: Kudu las separó en su 3.5 («schedules never
    /// auto-apply cache resets») y aquí igual. Se enseñan y se miden, pero:
    ///
    /// * **no se marcan solas** en la interfaz («marcar todo lo que ocupa» las
    ///   salta): limpiarlas tiene que ser una decisión tuya, un clic a una;
    /// * y **no entran en una limpieza automática** (`ids_a_limpiar` las excluye):
    ///   lo que corre solo no puede dejarte la próxima partida a tirones.
    pub reinicio_cache: bool,
    /// **Revalidar la antigüedad justo antes de borrar** (`deepRecencyCheck` de
    /// Kudu): un subárbol cuyo contenido es antiguo del todo se puede borrar de una
    /// vez, pero se vuelve a comprobar `revalidar` en ese mismo instante por si algo
    /// cambió desde que se midió. Si algo es reciente, el subárbol no se toca.
    pub recencia_profunda: bool,
    /// Escanear `ruta/*/<subdir_hijo>` en vez de `ruta` (`childSubdir` de Kudu):
    /// la caché de un perfil (`cache2`, `caches`) vive a un nivel fijo por debajo
    /// de una carpeta de perfiles, y una ruta fija no sirve para todos.
    pub subdir_hijo: Option<&'static str>,
    /// Borrar SOLO ciertos ficheros dentro de un directorio (ver `CoincidenciaFicheros`).
    pub coincidencia_ficheros: Option<CoincidenciaFicheros>,
    /// Buscar carpetas de caché conocidas dentro de un árbol (ver `CoincidenciaRecursiva`).
    pub coincidencia_recursiva: Option<CoincidenciaRecursiva>,
}

/// Una coincidencia de FICHEROS dentro de un directorio (`fileMatch` de Kudu).
///
/// POR QUÉ: hay basura que no es «todo el directorio» ni «un fichero fijo», sino
/// unos NOMBRES concretos dentro de carpetas que cumplen un patrón: los paquetes
/// que deja el actualizador de las apps Electron (`installer.exe`,
/// `current.blockmap` en carpetas `*-updater`). Borrar la carpeta entera se
/// llevaría por delante lo que no toca, así que aquí **solo se borran los ficheros
/// que casan**, nunca el directorio.
pub struct CoincidenciaFicheros {
    /// Nombres EXACTOS de fichero que se pueden borrar (nada de comodines).
    pub nombres: &'static [&'static str],
    /// Si se pone, solo se miran los subdirectorios DIRECTOS cuyo nombre acabe así.
    pub sufijo_dir: Option<&'static str>,
    /// Antigüedad mínima de cada fichero, en días. En Kudu es obligatoria: un
    /// paquete de actualización de ayer puede seguir en uso.
    pub dias_min: u32,
    /// Si el directorio candidato tiene alguno de estos hijos exactos, **no se toca
    /// nada de él**: una actualización a medias (`pending`) se queda como está.
    /// Se comprueba ANTES de mirar siquiera los ficheros que casan.
    pub saltar_si_existe: &'static [&'static str],
}

/// Búsqueda de carpetas de caché CONOCIDAS dentro de un árbol (`recursiveMatch`
/// de Kudu).
///
/// POR QUÉ: las aplicaciones basadas en Chromium (WebView2 en Windows, las
/// «particiones» de Claude/ChatGPT/Cursor…) reparten su caché en carpetas
/// anidadas a profundidad variable. No hay una ruta fija que las cace, pero
/// tampoco vale recorrer el árbol y borrar «lo que parezca caché»: por eso se
/// exige un ANCLA (p. ej. `EBWebView`), solo se borran los NOMBRES conocidos bajo
/// ella y hay ramas (sesiones, almacenamiento local) que no se inspeccionan jamás.
pub struct CoincidenciaRecursiva {
    /// Nombre EXACTO de la carpeta que tiene que contener al objetivo.
    pub ancla: &'static str,
    /// Rutas relativas hasta el ancla; cada segmento es un nombre exacto o `*`
    /// (un nivel cualquiera), y el último tiene que ser el ancla. Vacío = se busca
    /// el ancla desde la propia ruta base.
    pub rutas_ancla: &'static [&'static str],
    /// Nombres EXACTOS de carpeta de caché que se borran bajo el ancla.
    pub objetivos: &'static [&'static str],
    /// Nombres de carpeta cuyos subárboles NUNCA se inspeccionan (ahí viven
    /// sesiones y datos, no caché).
    pub ancestros_excluidos: &'static [&'static str],
    /// Profundidad máxima bajo cada ancla. 0 = la de por defecto (12).
    pub profundidad: u32,
}

const SIN: Option<&'static str> = None;

const fn regla(
    id: &'static str,
    categoria: &'static str,
    subcategoria: &'static str,
    descripcion: &'static str,
    rutas: &'static [&'static str],
    tipo: TipoRegla,
) -> Regla {
    Regla {
        id,
        categoria,
        subcategoria,
        descripcion,
        rutas,
        tipo,
        min_dias: 0,
        root: false,
        comando: SIN,
        traza: false,
        reinicio_cache: false,
        recencia_profunda: false,
        subdir_hijo: None,
        coincidencia_ficheros: None,
        coincidencia_recursiva: None,
    }
}

/// Construye una regla con antigüedad mínima.
const fn con_dias(mut r: Regla, dias: u32) -> Regla {
    r.min_dias = dias;
    r
}

/// Construye una regla que necesita root, con el comando que la limpia a mano.
const fn con_root(mut r: Regla, comando: &'static str) -> Regla {
    r.root = true;
    r.comando = Some(comando);
    r
}

/// Regla cuyo borrado lo hace mejor una herramienta nativa (pnpm, docker, uv…).
const fn con_comando(mut r: Regla, comando: &'static str) -> Regla {
    r.tipo = TipoRegla::Comando;
    r.comando = Some(comando);
    r
}

/// Categorías, en el orden en que se enseñan.
pub const CATEGORIAS: &[(&str, &str)] = &[
    ("sistema", "Sistema"),
    ("navegadores", "Navegadores"),
    ("apps", "Aplicaciones"),
    ("ia", "Herramientas de IA"),
    ("gpu", "Gráfica"),
    ("juegos", "Juegos"),
    // Las huellas van las últimas porque no son basura: se borran porque TÚ
    // quieres, no porque estorben.
    ("privacidad", "Privacidad (huellas)"),
];

/// El catálogo de LINUX. Cada ruta está tomada de las reglas de Kudu para Linux
/// (MIT) o comprobada en este equipo; ninguna es inventada.
const REGLAS_LINUX: &[Regla] = &[
    /* ── Sistema ──────────────────────────────────────────────────────────── */
    regla("tmp-var", "sistema", "Temporales persistentes", "Temporales que sobreviven al reinicio; se pueden borrar los de más de una semana", &["/var/tmp"], TipoRegla::Contenido).con_dias_como(7).con_recencia_profunda(),
    // Kudu tiene dos objetivos aquí («User Temp Files» = ${TMPDIR} y «System Temp
    // Files» = /tmp). Se funden en uno porque en Linux `${TEMP}` ES /tmp mientras
    // TMPDIR no esté puesto (que es lo normal), y con dos reglas se contaría /tmp
    // dos veces. Los temporales son de los pocos que NO se borran desde aquí:
    // limpiar /tmp del todo (lo de otros usuarios y lo de los servicios) necesita
    // root, y hacerlo a medias se llevaría por delante sockets de la sesión, así
    // que se mide y se enseña el comando nativo, que respeta las edades de
    // tmpfiles.d.
    regla("temporales", "sistema", "Temporales del sistema y de la sesión", "Temporales de /tmp y de ${TMPDIR} (suelen ser la misma carpeta). Se miden, pero limpiarlos del todo necesita root: hay ficheros de servicios y de otros usuarios. Se enseña `systemd-tmpfiles --clean`, que borra solo lo caducado según tmpfiles.d", &["/tmp", "${TEMP}"], TipoRegla::Contenido).con_root_como("sudo systemd-tmpfiles --clean"),
    regla("miniaturas", "sistema", "Miniaturas", "Miniaturas que generan los gestores de archivos (KDE, GNOME); se rehacen al mirar", &["${CACHE}/thumbnails"], TipoRegla::Contenido),
    regla("fontconfig", "sistema", "Caché de fuentes", "Índice de fuentes de fontconfig; se reconstruye solo", &["${CACHE}/fontconfig"], TipoRegla::Contenido),
    regla("gstreamer", "sistema", "Registro de GStreamer", "Registro de plugins multimedia; se regenera al reproducir", &["${CACHE}/gstreamer-1.0"], TipoRegla::Contenido),
    regla("tracker3", "sistema", "Índice de Tracker", "Índice de búsqueda de GNOME (Tracker 3); se reconstruye solo", &["${CACHE}/tracker3", "${CACHE}/tracker"], TipoRegla::Contenido),
    regla("baloo", "sistema", "Índice de Baloo", "Índice de búsqueda de KDE; se reconstruye solo (la reconstrucción tarda)", &["${LOCAL_SHARE}/baloo"], TipoRegla::Contenido),
    regla("libdnf5", "sistema", "Caché de libdnf5", "Metadatos de repositorios que usa DNF en esta máquina; se vuelven a descargar", &["${CACHE}/libdnf5"], TipoRegla::Contenido),
    regla("appstream", "sistema", "Caché de AppStream", "Metadatos de aplicaciones del centro de software; se actualizan solos", &["${CACHE}/appstream"], TipoRegla::Contenido),
    regla("xsession-errors", "sistema", "Errores de la sesión X", "Salida de errores de la sesión gráfica anterior", &["${HOME}/.xsession-errors.old"], TipoRegla::Fichero),
    regla("papelera", "sistema", "Papelera del escritorio", "Lo que hay en la papelera. Es lo ÚNICO que libera de verdad el espacio de lo que se manda allí (un modelo borrado, por ejemplo)", &["${LOCAL_SHARE}/Trash"], TipoRegla::Contenido),
    regla("flatpak-app-cache", "sistema", "Cachés de apps Flatpak", "Caché interna de cada aplicación Flatpak instalada; se regenera", &["${HOME}/.var/app/*/cache"], TipoRegla::Contenido),
    regla("snap-app-cache", "sistema", "Cachés de apps Snap", "Caché interna de cada aplicación Snap instalada; se regenera", &["${HOME}/snap/*/common/.cache"], TipoRegla::Contenido),
    regla("coredumps", "sistema", "Volcados de núcleo", "Volcados de fallo recogidos por systemd-coredump (necesita root)", &["/var/lib/systemd/coredump"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/lib/systemd/coredump/*"),
    regla("crash", "sistema", "Informes de fallo", "Informes recogidos en /var/crash (necesita root)", &["/var/crash"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/crash/*"),
    regla("journal", "sistema", "Registro del sistema (journal)", "Diarios archivados del sistema. Se recortan a 30 días sin tocar los activos (necesita root)", &["/var/log/journal"], TipoRegla::Comando).con_root_como("sudo journalctl --vacuum-time=30d"),
    regla("dnf", "sistema", "Caché de DNF", "Paquetes RPM descargados por DNF; se vuelven a bajar si hacen falta (necesita root)", &["/var/cache/dnf"], TipoRegla::Comando).con_root_como("sudo dnf clean packages"),
    regla("apt", "sistema", "Caché de APT", "Paquetes .deb descargados por APT (necesita root)", &["/var/cache/apt/archives"], TipoRegla::Comando).con_root_como("sudo apt clean"),
    regla("pacman", "sistema", "Caché de Pacman", "Paquetes descargados por Pacman (necesita root)", &["/var/cache/pacman/pkg"], TipoRegla::Comando).con_root_como("sudo pacman -Sc"),
    regla("flatpak-repo-tmp", "sistema", "Temporales de Flatpak", "Restos de operaciones del repositorio de Flatpak (necesita root)", &["/var/lib/flatpak/repo/tmp"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/lib/flatpak/repo/tmp/*"),
    regla("snap-cache", "sistema", "Caché de paquetes Snap", "Paquetes snap ya instalados que snapd conserva descargados; se vuelven a bajar si hicieran falta (necesita root)", &["/var/lib/snapd/cache"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/lib/snapd/cache/*"),
    regla("zypper", "sistema", "Caché de Zypper", "Paquetes RPM descargados por Zypper en openSUSE; se vuelven a bajar (necesita root)", &["/var/cache/zypp/packages"], TipoRegla::Comando).con_root_como("sudo zypper clean --all"),
    regla("fontconfig-sistema", "sistema", "Caché de fuentes del sistema", "Índice de fuentes de fontconfig a nivel de sistema; se reconstruye solo (necesita root)", &["/var/cache/fontconfig"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/cache/fontconfig/*"),
    regla("man-cache", "sistema", "Caché de páginas man", "Páginas de manual ya formateadas; se rehacen al consultarlas (necesita root)", &["/var/cache/man"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/cache/man/*"),

    /* ── Navegadores ──────────────────────────────────────────────────────── */
    regla("chrome", "navegadores", "Google Chrome", "Caché de Chrome (no toca historial, contraseñas ni sesiones)", &["${CACHE}/google-chrome"], TipoRegla::Contenido),
    regla("chromium", "navegadores", "Chromium", "Caché de Chromium", &["${CACHE}/chromium"], TipoRegla::Contenido),
    regla("brave", "navegadores", "Brave", "Caché de Brave", &["${CACHE}/BraveSoftware/Brave-Browser"], TipoRegla::Contenido),
    regla("edge", "navegadores", "Microsoft Edge", "Caché de Edge", &["${CACHE}/microsoft-edge"], TipoRegla::Contenido),
    regla("vivaldi", "navegadores", "Vivaldi", "Caché de Vivaldi", &["${CACHE}/vivaldi"], TipoRegla::Contenido),
    regla("opera", "navegadores", "Opera", "Caché de Opera y Opera GX", &["${CACHE}/opera", "${CACHE}/opera-gx"], TipoRegla::Contenido),
    regla("firefox", "navegadores", "Firefox", "Caché web de Firefox (perfiles incluidos; no toca marcadores ni historial)", &["${CACHE}/mozilla/firefox"], TipoRegla::Contenido),
    regla("firefox-forks", "navegadores", "Zen / LibreWolf / Waterfox", "Caché web de los navegadores derivados de Firefox", &["${CACHE}/zen", "${CACHE}/librewolf", "${CACHE}/waterfox", "${CACHE}/floorp"], TipoRegla::Contenido),
    // Navegadores Chromium que Kudu limpia y a nosotros nos faltaban: la caché
    // vive bajo la raíz de cachés, como en Chrome (`${CACHE}/<navegador>`).
    regla("arc", "navegadores", "Arc", "Caché de Arc; no toca historial, contraseñas ni sesiones", &["${CACHE}/arc/User Data"], TipoRegla::Contenido),
    regla("thorium", "navegadores", "Thorium", "Caché de Thorium", &["${CACHE}/thorium"], TipoRegla::Contenido),
    regla("supermium", "navegadores", "Supermium", "Caché de Supermium", &["${CACHE}/supermium"], TipoRegla::Contenido),
    regla("helium", "navegadores", "Helium", "Caché de Helium", &["${CACHE}/helium"], TipoRegla::Contenido),
    regla("cromite", "navegadores", "Cromite", "Caché de Cromite", &["${CACHE}/cromite"], TipoRegla::Contenido),
    regla("catsxp", "navegadores", "CatsXP", "Caché de CatsXP", &["${CACHE}/ArcSoft/CatsXP"], TipoRegla::Contenido),

    /* ── Aplicaciones (editores y apps de escritorio) ─────────────────────── */
    regla("vscode", "apps", "VS Code", "Caché y registros de VS Code; conserva ajustes y extensiones", &["${CONFIG}/Code/Cache/Cache_Data", "${CONFIG}/Code/Code Cache", "${CONFIG}/Code/GPUCache", "${CONFIG}/Code/logs", "${CONFIG}/Code/CachedExtensionVSIXs", "${CONFIG}/Code/CachedData"], TipoRegla::Contenido),
    regla("vscode-insiders", "apps", "VS Code Insiders", "Caché y registros de VS Code Insiders", &["${CONFIG}/Code - Insiders/Cache/Cache_Data", "${CONFIG}/Code - Insiders/Code Cache", "${CONFIG}/Code - Insiders/GPUCache", "${CONFIG}/Code - Insiders/CachedData", "${CONFIG}/Code - Insiders/CachedExtensionVSIXs", "${CONFIG}/Code - Insiders/logs"], TipoRegla::Contenido),
    regla("vscodium", "apps", "VSCodium", "Caché y registros de VSCodium", &["${CONFIG}/VSCodium/Cache/Cache_Data", "${CONFIG}/VSCodium/Code Cache", "${CONFIG}/VSCodium/GPUCache", "${CONFIG}/VSCodium/CachedData", "${CONFIG}/VSCodium/CachedExtensionVSIXs", "${CONFIG}/VSCodium/logs"], TipoRegla::Contenido),
    regla("zed", "apps", "Zed", "Caché del editor Zed", &["${CACHE}/zed"], TipoRegla::Contenido),
    regla("sublime", "apps", "Sublime Text", "Índice y caché de recursos de Sublime Text", &["${CACHE}/sublime-text"], TipoRegla::Contenido),
    regla("jetbrains", "apps", "IDEs de JetBrains", "Cachés de IntelliJ, PyCharm, WebStorm…; se reconstruyen al reiniciar el IDE", &["${CACHE}/JetBrains"], TipoRegla::Contenido),
    regla("android-studio", "apps", "Android Studio", "Cachés de versiones del IDE; no toca índices ni historial local", &["${CACHE}/Google/*/caches"], TipoRegla::Contenido),
    regla("unity", "apps", "Unity", "Caché global de shaders y GI de Unity; se rehace al abrir un proyecto, así que la primera vez tardará más", &["${CACHE}/unity3d"], TipoRegla::Contenido).con_reinicio_cache(),
    regla("discord", "apps", "Discord", "Caché de Discord (y PTB y Canary)", &["${CONFIG}/discord/Cache/Cache_Data", "${CONFIG}/discord/Code Cache", "${CONFIG}/discord/GPUCache", "${CONFIG}/discordptb/Cache/Cache_Data", "${CONFIG}/discordptb/Code Cache", "${CONFIG}/discordptb/GPUCache", "${CONFIG}/discordcanary/Cache/Cache_Data", "${CONFIG}/discordcanary/Code Cache", "${CONFIG}/discordcanary/GPUCache"], TipoRegla::Contenido),
    regla("slack", "apps", "Slack", "Caché de Slack", &["${CONFIG}/Slack/Cache/Cache_Data", "${CONFIG}/Slack/Code Cache", "${CONFIG}/Slack/GPUCache"], TipoRegla::Contenido),
    regla("teams", "apps", "Microsoft Teams", "Caché web de Teams", &["${CONFIG}/Microsoft/Microsoft Teams/Cache"], TipoRegla::Contenido),
    regla("notion", "apps", "Notion", "Caché de Notion", &["${CONFIG}/Notion/Cache/Cache_Data", "${CONFIG}/Notion/Code Cache", "${CONFIG}/Notion/GPUCache"], TipoRegla::Contenido),
    regla("obsidian", "apps", "Obsidian", "Caché de Obsidian (no toca las notas)", &["${CONFIG}/obsidian/Cache/Cache_Data", "${CONFIG}/obsidian/Code Cache", "${CONFIG}/obsidian/GPUCache"], TipoRegla::Contenido),
    regla("whatsapp", "apps", "WhatsApp Desktop", "Caché de WhatsApp Desktop", &["${CONFIG}/WhatsApp/Cache/Cache_Data"], TipoRegla::Contenido),
    regla("signal", "apps", "Signal", "Caché de Signal Desktop (no toca mensajes)", &["${CONFIG}/Signal/Cache/Cache_Data", "${CONFIG}/Signal/Code Cache", "${CONFIG}/Signal/GPUCache"], TipoRegla::Contenido),
    regla("bitwarden", "apps", "Bitwarden", "Caché de Bitwarden (no toca la bóveda)", &["${CONFIG}/Bitwarden/Cache/Cache_Data", "${CONFIG}/Bitwarden/Code Cache", "${CONFIG}/Bitwarden/GPUCache"], TipoRegla::Contenido),
    regla("1password", "apps", "1Password", "Caché de 1Password (no toca la bóveda)", &["${CONFIG}/1Password/Cache/Cache_Data", "${CONFIG}/1Password/Code Cache", "${CONFIG}/1Password/GPUCache"], TipoRegla::Contenido),
    regla("postman", "apps", "Postman", "Caché de Postman", &["${CONFIG}/Postman/Cache/Cache_Data", "${CONFIG}/Postman/Code Cache", "${CONFIG}/Postman/GPUCache"], TipoRegla::Contenido),
    regla("linear", "apps", "Linear", "Caché de Linear", &["${CONFIG}/Linear/Cache/Cache_Data", "${CONFIG}/Linear/Code Cache", "${CONFIG}/Linear/GPUCache"], TipoRegla::Contenido),
    regla("loom", "apps", "Loom", "Caché de Loom", &["${CONFIG}/Loom/Cache/Cache_Data", "${CONFIG}/Loom/Code Cache", "${CONFIG}/Loom/GPUCache"], TipoRegla::Contenido),
    regla("todoist", "apps", "Todoist", "Caché de Todoist", &["${CONFIG}/Todoist/Cache/Cache_Data", "${CONFIG}/Todoist/Code Cache", "${CONFIG}/Todoist/GPUCache"], TipoRegla::Contenido),
    regla("hyper", "apps", "Hyper", "Caché de la terminal Hyper", &["${CONFIG}/Hyper/Cache/Cache_Data", "${CONFIG}/Hyper/Code Cache", "${CONFIG}/Hyper/GPUCache"], TipoRegla::Contenido),
    regla("termius", "apps", "Termius", "Caché de Termius (no toca los hosts guardados)", &["${CONFIG}/Termius/Cache/Cache_Data", "${CONFIG}/Termius/Code Cache", "${CONFIG}/Termius/GPUCache"], TipoRegla::Contenido),
    regla("ledger", "apps", "Ledger Live", "Caché de Ledger Live", &["${CONFIG}/Ledger Live/Cache/Cache_Data", "${CONFIG}/Ledger Live/Code Cache", "${CONFIG}/Ledger Live/GPUCache"], TipoRegla::Contenido),
    regla("spotify", "apps", "Spotify", "Datos de streaming en caché de Spotify", &["${CACHE}/spotify"], TipoRegla::Contenido),
    regla("vlc", "apps", "VLC", "Carátulas y miniaturas en caché de VLC", &["${CACHE}/vlc"], TipoRegla::Contenido),
    regla("gimp", "apps", "GIMP", "Caché de recursos y render de GIMP", &["${CACHE}/gimp"], TipoRegla::Contenido),
    regla("blender", "apps", "Blender", "Miniaturas y caché de scripts de Blender", &["${CACHE}/blender"], TipoRegla::Contenido),
    regla("libreoffice", "apps", "LibreOffice", "Caché de LibreOffice, incluidas miniaturas de extensiones", &["${CACHE}/libreoffice"], TipoRegla::Contenido),
    regla("inkscape", "apps", "Inkscape", "Caché de fuentes y render de Inkscape", &["${CACHE}/inkscape"], TipoRegla::Contenido),
    regla("krita", "apps", "Krita", "Caché de recursos y render de Krita", &["${CACHE}/krita"], TipoRegla::Contenido),
    regla("filezilla", "apps", "FileZilla", "Caché de listados de directorio de FileZilla", &["${CACHE}/filezilla"], TipoRegla::Contenido),
    regla("transmission", "apps", "Transmission", "Caché de Transmission", &["${CACHE}/transmission"], TipoRegla::Contenido),
    regla("kodi", "apps", "Kodi", "Temporales y miniaturas de Kodi (puede ocupar varios GB; se rehacen)", &["${HOME}/.kodi/temp", "${HOME}/.kodi/userdata/Thumbnails"], TipoRegla::Contenido),
    regla("qbittorrent", "apps", "qBittorrent", "Caché de qBittorrent", &["${CACHE}/qBittorrent"], TipoRegla::Contenido),
    regla("handbrake", "apps", "HandBrake", "Registros de codificaciones pasadas", &["${CONFIG}/ghb/EncodeLogs", "${HOME}/.var/app/fr.handbrake.ghb/config/ghb/EncodeLogs"], TipoRegla::Contenido),
    regla("obs", "apps", "OBS Studio", "Registros y datos del perfilador de OBS", &["${CONFIG}/obs-studio/logs", "${CONFIG}/obs-studio/profiler_data"], TipoRegla::Contenido),
    regla("zoom", "apps", "Zoom", "Caché y registros de Zoom", &["${HOME}/.zoom/data/Cache", "${HOME}/.zoom/data/GPUCache", "${HOME}/.zoom/data/logs", "${HOME}/.zoom/logs"], TipoRegla::Contenido),
    regla("telegram", "apps", "Telegram", "Caché de Telegram Desktop (no toca los mensajes)", &["${LOCAL_SHARE}/TelegramDesktop/tdata/user_data", "${LOCAL_SHARE}/TelegramDesktop/tdata/emoji"], TipoRegla::Contenido),
    regla("wine-temp", "apps", "Wine", "Temporales del prefijo de Wine por defecto", &["${HOME}/.wine/drive_c/windows/temp"], TipoRegla::Contenido),
    // La caché web de Thunderbird es una carpeta `cache2` por perfil, no una ruta
    // fija: `childSubdir` la busca dentro de cada perfil.
    regla("thunderbird", "apps", "Thunderbird", "Caché web por perfil de Thunderbird (`cache2` dentro de cada perfil); no toca el correo", &["${CACHE}/thunderbird"], TipoRegla::Contenido).con_subdir_hijo("cache2"),
    regla("teamviewer", "apps", "TeamViewer", "Caché de TeamViewer y registros de sus sesiones", &["${HOME}/.teamviewer/logs", "${CACHE}/TeamViewer"], TipoRegla::Contenido),
    regla("pidgin", "apps", "Pidgin", "Iconos de contactos que Pidgin guarda en caché; se vuelven a bajar", &["${HOME}/.purple/icons"], TipoRegla::Contenido),
    regla("audacious", "apps", "Audacious", "Caché del reproductor Audacious", &["${CACHE}/audacious"], TipoRegla::Contenido),
    regla("rhythmbox", "apps", "Rhythmbox", "Caché de carátulas y metadatos de Rhythmbox", &["${CACHE}/rhythmbox"], TipoRegla::Contenido),
    // Kudu usa aquí la misma regla para `${CACHE}/zen` (que ya cubre
    // firefox-forks, el directorio de caché entero) y para `${HOME}/.zen`
    // (la parte que faltaba: `cache2` dentro de cada perfil).
    regla("zen-perfiles", "apps", "Zen (perfiles)", "Caché web por perfil de Zen Browser (`cache2` dentro de cada perfil de ${HOME}/.zen); la parte de ${CACHE}/zen ya la cubre la regla de navegadores", &["${HOME}/.zen"], TipoRegla::Contenido).con_subdir_hijo("cache2"),

    /* ── Aplicaciones (cadenas de herramientas y gestores de paquetes) ────── */
    regla("npm", "apps", "Caché de npm", "Paquetes y metadatos que npm se descarga; se vuelven a bajar", &["${HOME}/.npm/_cacache"], TipoRegla::Contenido),
    regla("yarn", "apps", "Caché de Yarn", "Paquetes descargados por Yarn", &["${CACHE}/yarn"], TipoRegla::Contenido),
    regla("pnpm-store", "apps", "pnpm (almacén)", "El almacén de pnpm se limpia mejor con su propio comando: borra solo lo que ningún proyecto usa", &["${LOCAL_SHARE}/pnpm/store"], TipoRegla::Comando).con_comando_como("pnpm store prune"),
    regla("pip", "apps", "Caché de pip", "Ruedas y fuentes descargadas por pip; se vuelven a bajar", &["${CACHE}/pip"], TipoRegla::Contenido),
    regla("pipenv", "apps", "Caché de Pipenv", "Caché de resolución de Pipenv", &["${CACHE}/pipenv"], TipoRegla::Contenido),
    regla("poetry", "apps", "Caché de Poetry", "Metadatos y distribuciones descargadas por Poetry (no toca los entornos virtuales)", &["${CACHE}/pypoetry/cache", "${CACHE}/pypoetry/artifacts"], TipoRegla::Contenido),
    regla("uv", "apps", "uv (caché)", "uv limpia su caché mejor con su comando: conserva lo que sigue en uso", &["${CACHE}/uv"], TipoRegla::Comando).con_comando_como("uv cache prune"),
    regla("bun", "apps", "Caché de Bun", "Paquetes descargados por Bun", &["${HOME}/.bun/install/cache"], TipoRegla::Contenido),
    regla("deno", "apps", "Caché de Deno", "Módulos descargados por Deno; se vuelven a bajar", &["${CACHE}/deno"], TipoRegla::Contenido),
    regla("cargo", "apps", "Caché de Cargo", "Archivos .crate descargados, fuentes extraídas y checkouts de git (se rehacen al compilar)", &["${HOME}/.cargo/registry/cache", "${HOME}/.cargo/registry/src", "${HOME}/.cargo/git/checkouts"], TipoRegla::Contenido),
    regla("go-build", "apps", "Caché de compilación de Go", "Caché del compilador de Go (se rehace en la siguiente compilación)", &["${CACHE}/go-build"], TipoRegla::Contenido),
    regla("go-mod", "apps", "Caché de módulos de Go", "Módulos descargados por Go", &["${HOME}/go/pkg/mod/cache"], TipoRegla::Contenido),
    regla("gradle", "apps", "Caché de Gradle", "Cachés de compilación y registros del demonio de Gradle", &["${HOME}/.gradle/caches", "${HOME}/.gradle/daemon"], TipoRegla::Contenido),
    regla("maven", "apps", "Caché de Maven", "Repositorio local de artefactos de Maven (se vuelven a bajar)", &["${HOME}/.m2/repository"], TipoRegla::Contenido),
    regla("composer", "apps", "Caché de Composer", "Paquetes PHP descargados por Composer", &["${CACHE}/composer"], TipoRegla::Contenido),
    regla("conda", "apps", "Caché de paquetes de Conda", "Paquetes descargados por Conda", &["${HOME}/anaconda3/pkgs", "${HOME}/miniconda3/pkgs", "${HOME}/.conda/pkgs"], TipoRegla::Contenido),
    regla("node-gyp", "apps", "Caché de node-gyp", "Cabeceras de Node.js que descarga node-gyp para módulos nativos", &["${HOME}/.node-gyp"], TipoRegla::Contenido),
    regla("electron", "apps", "Runtime de Electron", "Descargas de Electron de más de una semana; se vuelven a bajar", &["${CACHE}/electron"], TipoRegla::Contenido).con_dias_como(7).con_recencia_profunda(),
    regla("electron-builder", "apps", "Caché de electron-builder", "Descargas de electron-builder", &["${CACHE}/electron-builder"], TipoRegla::Contenido),
    regla("ccache", "apps", "Caché de compilador (ccache/sccache)", "Caché de objetos compilados de C/C++ y Rust (la próxima compilación tarda algo más)", &["${CACHE}/ccache", "${CACHE}/sccache"], TipoRegla::Contenido),
    // OJO: la caché real no está en `${HOME}/.gem/cache` sino en una subcarpeta por
    // versión de Ruby (`${HOME}/.gem/<versión>/cache`), así que la ruta fija no
    // cubría nada. `childSubdir` busca la carpeta `cache` dentro de cada versión.
    regla("ruby-gems", "apps", "Caché de gemas de Ruby", "Ficheros .gem descargados; viven en una carpeta `cache` por versión de Ruby", &["${HOME}/.gem"], TipoRegla::Contenido).con_subdir_hijo("cache"),
    regla("java-cache", "apps", "Caché de Java Web Start", "Caché de descargas de Java Web Start (tecnología retirada)", &["${HOME}/.java/deployment/cache"], TipoRegla::Contenido),
    regla("homebrew", "apps", "Caché de Homebrew", "Botellas y fuentes descargadas por Homebrew en Linux", &["${CACHE}/Homebrew"], TipoRegla::Contenido),
    regla("aws-cli", "apps", "Caché de AWS CLI", "Respuestas de API en caché de AWS CLI; se vuelven a pedir", &["${HOME}/.aws/cli/cache"], TipoRegla::Contenido),
    regla("gcloud", "apps", "Caché de Google Cloud CLI", "Registros y datos en caché de la CLI de Google Cloud", &["${CONFIG}/gcloud/logs", "${CONFIG}/gcloud/cache"], TipoRegla::Contenido),
    regla("docker-build", "apps", "Caché de compilación de Docker", "Docker limpia su caché de compilación mejor con su comando (no toca contenedores, volúmenes ni imágenes)", &["/var/lib/docker"], TipoRegla::Comando).con_comando_como("docker builder prune"),
    regla("typescript", "apps", "Tipos de TypeScript", "Definiciones que el servidor de TypeScript descarga para proyectos JS", &["${CACHE}/typescript"], TipoRegla::Contenido),
    regla("cpptools", "apps", "IntelliSense de C/C++", "Cabeceras precompiladas de la extensión de C/C++ de VS Code", &["${CACHE}/vscode-cpptools/ipch"], TipoRegla::Contenido),

    /* ── Herramientas de IA ───────────────────────────────────────────────── */
    regla("cursor", "ia", "Cursor", "Caché de Cursor; conserva proyectos, historial y ajustes", &["${CONFIG}/Cursor/Cache/Cache_Data", "${CONFIG}/Cursor/Code Cache", "${CONFIG}/Cursor/GPUCache", "${CONFIG}/Cursor/logs", "${CONFIG}/Cursor/CachedData"], TipoRegla::Contenido).con_dias_como(1).con_recencia_profunda(),
    regla("windsurf", "ia", "Windsurf", "Caché de Windsurf; conserva proyectos, historial y ajustes", &["${CONFIG}/Windsurf/Cache/Cache_Data", "${CONFIG}/Windsurf/Code Cache", "${CONFIG}/Windsurf/GPUCache", "${CONFIG}/Windsurf/logs", "${CONFIG}/Windsurf/CachedData"], TipoRegla::Contenido).con_dias_como(1).con_recencia_profunda(),
    regla("claude", "ia", "Claude (caché)", "Cachés de Claude Desktop y Claude Code; NO toca conversaciones, proyectos, configuración ni plugins", &["${CONFIG}/Claude/Cache/Cache_Data", "${CONFIG}/Claude/Code Cache", "${CONFIG}/Claude/GPUCache", "${HOME}/.claude/cache", "${HOME}/.claude/plugins/cache", "${CACHE}/claude", "${CACHE}/claude-cli-nodejs"], TipoRegla::Contenido).con_dias_como(1).con_recencia_profunda(),
    regla("claude-logs", "ia", "Claude (registros)", "Registros de diagnóstico de Claude de más de una semana; nunca toca conversaciones ni historial de ficheros", &["${HOME}/.claude/debug", "${HOME}/.claude/logs"], TipoRegla::Contenido).con_dias_como(7).con_recencia_profunda(),
    regla("codex", "ia", "Codex (caché)", "Cachés de Codex; conserva sesiones, memorias, credenciales y plugins", &["${HOME}/.codex/cache", "${HOME}/.codex/tmp"], TipoRegla::Contenido).con_dias_como(1).con_recencia_profunda(),
    regla("huggingface", "ia", "Hugging Face", "Modelos y blobs descargados con huggingface-cli. OJO: borrar aquí OBLIGA a volver a descargar los modelos; el inventario de modelos los borra a la papelera, esta regla los borra de verdad", &["${CACHE}/huggingface"], TipoRegla::Contenido),
    regla("lmstudio-updater", "ia", "Actualizador de LM Studio", "Restos de las actualizaciones descargadas de LM Studio", &["${CACHE}/lm-studio-updater"], TipoRegla::Contenido),
    regla("llmfit", "ia", "llmfit", "Caché de llmfit (catálogo y descargas a medias); se vuelve a bajar", &["${CACHE}/llmfit"], TipoRegla::Contenido),
    regla("opencode", "ia", "OpenCode", "Caché del agente OpenCode", &["${CACHE}/opencode"], TipoRegla::Contenido),
    regla("ms-playwright", "ia", "Navegadores de Playwright", "Navegadores descargados por Playwright/MCP para automatizar; se vuelven a instalar con `pnpm dlx playwright install`", &["${CACHE}/ms-playwright", "${CACHE}/ms-playwright-mcp"], TipoRegla::Contenido),

    /* ── Gráfica ──────────────────────────────────────────────────────────── */
    //
    // Estas son REINICIO de caché de rendimiento (como Kudu las marca en su
    // gpu-cache): borrarlas no rompe nada, pero la primera vez cada aplicación va
    // más lenta mientras recompila sus shaders. No se marcan solas ni entran en
    // una limpieza automática.
    regla("mesa-shaders", "gpu", "Caché de shaders de Mesa", "Shaders compilados de Mesa (OpenGL/Vulkan); se recompilan al abrir cada aplicación, así que la primera vez irá más lenta", &["${CACHE}/mesa_shader_cache", "${CACHE}/mesa_shader_cache_db"], TipoRegla::Contenido).con_reinicio_cache(),
    regla("nvidia-shaders", "gpu", "Caché de shaders de NVIDIA", "Shaders compilados de NVIDIA (OpenGL/Vulkan); se recompilan al abrir cada aplicación", &["${CACHE}/nvidia/GLCache"], TipoRegla::Contenido).con_reinicio_cache(),
    regla("amd-shaders", "gpu", "Caché de shaders de AMD", "Cachés de shaders y kernels compilados del driver AMD (MIOpen incluido); se recompilan al usarlos", &["${CACHE}/AMD", "${CACHE}/miopen", "${CACHE}/comgr"], TipoRegla::Contenido).con_reinicio_cache(),
    regla("cuda", "gpu", "Caché de computación de CUDA", "Kernels compilados por CUDA (JIT); se recompilan la próxima vez que se usen", &["${HOME}/.nv/ComputeCache"], TipoRegla::Contenido).con_reinicio_cache(),

    /* ── Juegos ───────────────────────────────────────────────────────────── */
    regla("steam", "juegos", "Steam (registros y caché web)", "Registros del cliente de Steam y su caché de descargas web", &["${HOME}/.steam/steam/logs", "${LOCAL_SHARE}/Steam/logs", "${LOCAL_SHARE}/Steam/appcache/httpcache"], TipoRegla::Contenido),
    regla("steam-shaders", "juegos", "Steam (shaders)", "Caché de shaders por juego de la biblioteca por defecto; se recompila al abrir cada juego, así que la primera partida irá más lenta", &["${LOCAL_SHARE}/Steam/steamapps/shadercache"], TipoRegla::Contenido).con_reinicio_cache(),
    regla("lutris", "juegos", "Lutris", "Caché de descargas e instaladores de Lutris", &["${CACHE}/lutris"], TipoRegla::Contenido),
    regla("heroic", "juegos", "Heroic", "Caché web y de GPU de Heroic", &["${CONFIG}/heroic/Cache/Cache_Data", "${CONFIG}/heroic/GPUCache"], TipoRegla::Contenido),
    regla("itch", "juegos", "itch.io", "Caché y registros de itch.io", &["${CONFIG}/itch/Cache/Cache_Data", "${CONFIG}/itch/logs"], TipoRegla::Contenido),
    regla("minecraft", "juegos", "Minecraft", "Registros, informes de fallo y caché del lanzador de Minecraft", &["${HOME}/.minecraft/logs", "${HOME}/.minecraft/crash-reports", "${HOME}/.minecraft/webcache2"], TipoRegla::Contenido),

    /* ── Privacidad: huellas de tu actividad ──────────────────────────────── */
    //
    // OJO CON ESTAS: no son basura que se regenere. Borrar el historial de órdenes
    // no lo vuelve a crear nadie, así que van marcadas como huella (`traza`) y no
    // se borran si no las pides por su nombre (ni en la interfaz ni en el CLI). La
    // descripción dice qué se pierde, sin adornos.
    regla("hist-bash", "privacidad", "Historial de bash", "Las órdenes que has escrito en bash. NO se recupera: si lo borras, se ha ido", &["${HOME}/.bash_history"], TipoRegla::Fichero).marcar_traza(),
    regla("hist-zsh", "privacidad", "Historial de zsh", "Las órdenes que has escrito en zsh. NO se recupera", &["${HOME}/.zsh_history"], TipoRegla::Fichero).marcar_traza(),
    regla("hist-fish", "privacidad", "Historial de fish", "Las órdenes que has escrito en fish. NO se recupera", &["${HOME}/.local/share/fish/fish_history"], TipoRegla::Fichero).marcar_traza(),
    regla("hist-atuin", "privacidad", "Historial de atuin", "Base de datos del historial compartido de atuin. NO se recupera", &["${HOME}/.local/share/atuin/history.db"], TipoRegla::Fichero).marcar_traza(),
    regla("hist-python", "privacidad", "Historial de Python", "Las últimas órdenes que has escrito dentro del intérprete de Python", &["${HOME}/.python_history", "${HOME}/.python_history-*"], TipoRegla::Fichero).marcar_traza(),
    regla("hist-editor", "privacidad", "Historial de vim y less", "Historial y estado de vim y de less (ficheros abiertos, búsquedas)", &["${HOME}/.viminfo", "${HOME}/.lesshst"], TipoRegla::Fichero).marcar_traza(),
    regla("recientes", "privacidad", "Documentos recientes", "La lista de ficheros que has abierto (freedesktop). NO se recupera", &["${HOME}/.local/share/recently-used.xbel"], TipoRegla::Fichero).marcar_traza(),
    regla("portapapeles-kde", "privacidad", "Portapapeles (KDE)", "Lo que has copiado: el historial del portapapeles de KDE, con sus imágenes y enlaces", &["${HOME}/.local/share/klipper"], TipoRegla::Contenido).marcar_traza(),
    regla("actividad-kde", "privacidad", "Historial de actividad (KDE)", "El registro de actividad de KDE: qué abriste y cuándo", &["${HOME}/.local/share/kactivitymanagerd"], TipoRegla::Contenido).marcar_traza(),
    regla("krunner-estado", "privacidad", "Búsquedas del lanzador (KRunner)", "El estado del lanzador: incluye las últimas búsquedas", &["${HOME}/.local/share/krunnerstaterc"], TipoRegla::Fichero).marcar_traza(),
    regla("zeitgeist", "privacidad", "Registro de actividad (Zeitgeist)", "El registro de actividad de GNOME: qué se abrió y cuándo", &["${HOME}/.local/share/zeitgeist", "${HOME}/.cache/zeitgeist"], TipoRegla::Contenido).marcar_traza(),
    // Kudu marca estas dos como de riesgo «traza»: son registros que cuentan qué
    // se instaló y cuándo en este equipo. Van aquí, con aviso, y necesitan root.
    regla("log-instalador", "privacidad", "Registros del instalador", "Registros de la instalación del sistema: qué se instaló y cuándo. NO se recuperan (necesita root)", &["/var/log/installer"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/log/installer/*").marcar_traza(),
    regla("log-apt", "privacidad", "Registros de APT", "Historial de paquetes de APT (qué se instaló, actualizó o quitó, y cuándo). NO se recupera (necesita root)", &["/var/log/apt"], TipoRegla::Contenido).con_root_como("sudo rm -rf /var/log/apt/*").marcar_traza(),
];

// Los tres `const fn` de arriba se llaman desde la tabla; estos alias existen
// porque Rust no permite encadenar métodos en contexto `const` de forma legible.
impl Regla {
    const fn con_dias_como(self, dias: u32) -> Regla {
        con_dias(self, dias)
    }
    const fn con_root_como(self, comando: &'static str) -> Regla {
        con_root(self, comando)
    }
    const fn con_comando_como(self, comando: &'static str) -> Regla {
        con_comando(self, comando)
    }
    /// Marca la regla como HUELLA de actividad (ver `Regla::traza`).
    const fn marcar_traza(mut self) -> Regla {
        self.traza = true;
        self
    }
    /// Marca la regla como REINICIO de caché de rendimiento (ver
    /// `Regla::reinicio_cache`): se mide y se puede pedir a mano, pero no se marca
    /// sola ni entra en una limpieza automática.
    const fn con_reinicio_cache(mut self) -> Regla {
        self.reinicio_cache = true;
        self
    }
    /// Marca la regla para que revalide la antigüedad justo antes de borrar un
    /// subárbol entero (ver `Regla::recencia_profunda`).
    const fn con_recencia_profunda(mut self) -> Regla {
        self.recencia_profunda = true;
        self
    }
    /// Escanea `ruta/*/<subdir>` (ver `Regla::subdir_hijo`).
    const fn con_subdir_hijo(mut self, subdir: &'static str) -> Regla {
        self.subdir_hijo = Some(subdir);
        self
    }
}

/// Catálogos de los otros sistemas, tomados igualmente de las reglas de Kudu
/// (MIT): `rules/darwin/*.json` y `rules/win32/*.json`.
///
/// Los tres se COMPILAN en todos los sistemas a propósito, aunque solo se use uno:
/// así el compilador y las pruebas revisan también los catálogos de macOS y
/// Windows desde Linux, que es la única forma de que no se pudran sin que nadie se
/// entere (un módulo `#[cfg(macos)]` no se compila en Linux y nadie lo mira).
mod catalogo_macos;
mod catalogo_windows;

/// El catálogo del sistema en el que corre la app.
///
/// Ojo con por qué NO se mezclan: una regla de macOS apunta a `~/Library/...`, y en
/// Linux esa carpeta puede existir por otro motivo. Limpiar con reglas de otro
/// sistema es la forma más fácil de borrar algo que no tocaba.
pub fn catalogo() -> &'static [Regla] {
    if cfg!(target_os = "macos") {
        catalogo_macos::REGLAS
    } else if cfg!(target_os = "windows") {
        catalogo_windows::REGLAS
    } else {
        REGLAS_LINUX
    }
}

/// Los tres catálogos, para las pruebas de integridad.
#[cfg(test)]
fn todos_los_catalogos() -> [(&'static str, &'static [Regla]); 3] {
    [
        ("linux", REGLAS_LINUX),
        ("macos", catalogo_macos::REGLAS),
        ("windows", catalogo_windows::REGLAS),
    ]
}

/* ── Resultados que ve la interfaz ────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize)]
pub struct Objetivo {
    pub id: String,
    pub categoria: String,
    pub subcategoria: String,
    pub descripcion: String,
    pub rutas: Vec<String>,
    pub bytes: u64,
    pub elementos: u64,
    /// Lo que NO se borraría por ser más nuevo que la antigüedad mínima.
    pub recientes: u64,
    pub min_dias: u32,
    pub root: bool,
    pub comando: Option<String>,
    /// No se pudo leer (permisos): el 0 de `bytes` no significa "vacío".
    pub sin_permiso: bool,
    /// El presupuesto se agotó al medir este objetivo: el tamaño es INCOMPLETO y
    /// hay que decirlo, porque un 0 aquí no significa "no hay nada".
    pub parcial: bool,
    /// Es una HUELLA de tu actividad (historial, recientes, portapapeles): no se
    /// regenera sola, así que no se marca ni se borra sin pedirla por su nombre.
    pub traza: bool,
    /// Es un REINICIO de caché de rendimiento (shaders de GPU, Steam…): borrarla
    /// no rompe nada, pero la primera vez todo va más lento. No se marca sola ni
    /// entra en una limpieza automática.
    pub reinicio_cache: bool,
}

/// Qué objetivos se llevaría una limpieza automática (la del CLI con `--aplicar`).
///
/// PURA a propósito, y con prueba propia: aquí vive la protección de las huellas.
/// Solo se limpia lo que se puede limpiar desde el CLI (nada que necesite root,
/// nada que tenga su propio comando, nada vacío) y **las huellas solo si se han
/// pedido por su nombre** (`--categoria privacidad`): un `--aplicar` a secas no
/// puede llevarse por delante el historial de nadie.
///
/// Y **un reinicio de caché de rendimiento no entra NUNCA**: lo que corre solo no
/// puede dejarte la próxima partida a tirones mientras se recompilan los shaders
/// (Kudu lo llamó «schedules never auto-apply cache resets»). Se limpia solo si lo
/// pides a mano.
pub fn ids_a_limpiar(objetivos: &[Objetivo], pide_privacidad: bool) -> Vec<String> {
    objetivos
        .iter()
        .filter(|o| !o.root && o.comando.is_none() && o.bytes > 0)
        .filter(|o| !o.traza || pide_privacidad)
        .filter(|o| !o.reinicio_cache)
        .map(|o| o.id.clone())
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Escaneo {
    pub objetivos: Vec<Objetivo>,
    pub bytes: u64,
    pub elementos: u64,
    pub ms: u64,
    pub truncado: bool,
    /// Objetivos que NO se han medido porque los cubre una exclusión del usuario,
    /// con el patrón que los excluyó. Va aquí para que la interfaz pueda decirlo:
    /// un total que encoge en silencio parece un fallo del programa.
    pub excluidos: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Informe {
    pub mensaje: String,
    pub bytes: u64,
    pub elementos: u64,
    pub limpiados: Vec<String>,
    pub saltados: Vec<String>,
    pub fallos: Vec<String>,
}

struct Presupuesto {
    entradas: u64,
    inicio: Instant,
    agotado: bool,
}

impl Presupuesto {
    fn nuevo() -> Self {
        Self { entradas: 0, inicio: Instant::now(), agotado: false }
    }
    fn gastar(&mut self, n: u64) -> bool {
        self.entradas += n;
        if self.entradas > MAX_ENTRADAS || self.inicio.elapsed() > MAX_TIEMPO {
            self.agotado = true;
            false
        } else {
            true
        }
    }
}

/* ── Resolución de rutas y antigüedad ─────────────────────────────────────── */

/// Las variables que pueden llevar las rutas de un catálogo.
///
/// Vive en `plataforma` porque NO es cosa del catálogo: lo usan también las
/// exclusiones del usuario, que se escriben con las mismas variables (si allí se
/// añade una, aquí funciona sola).
fn expandir(plantilla: &str) -> PathBuf {
    PathBuf::from(crate::plataforma::expandir_plantilla(plantilla))
}

/// Las rutas de una regla que EXISTEN de verdad (los comodines se expanden).
fn resolver(rutas: &[&str]) -> Vec<PathBuf> {
    let mut fuera = Vec::new();
    for r in rutas {
        let p = expandir(r);
        let s = p.to_string_lossy().to_string();
        if s.contains('*') || s.contains('?') || s.contains('[') {
            if let Ok(it) = glob::glob(&s) {
                for e in it.flatten() {
                    if e.exists() {
                        fuera.push(e);
                    }
                }
            }
        } else if p.exists() {
            fuera.push(p);
        }
    }
    fuera.sort();
    fuera.dedup();
    fuera
}

/// En Windows los nombres de carpeta no distinguen mayúsculas (y Kudu compara
/// igual); en el resto sí. Se usa solo para casar nombres de carpeta del
/// `recursiveMatch` y del `fileMatch`.
fn normalizar_nombre(n: &str) -> String {
    if cfg!(target_os = "windows") {
        n.to_lowercase()
    } else {
        n.to_string()
    }
}

/// Tope de carpetas que puede visitar un `recursiveMatch`: una regla recursiva no
/// puede recorrer un árbol entero sin freno (Kudu usa el mismo tope).
const MAX_RECURSIVO: u64 = 100_000;

/// Las rutas de una regla, aplicando `childSubdir` y `recursiveMatch`.
///
/// POR QUÉ NO LO HACE `resolver`: `resolver` expande plantillas y comodines de una
/// ruta escrita, pero `childSubdir` y `recursiveMatch` dependen de lo que HAYA en
/// el disco y de la forma del árbol, así que son otro paso.
fn resolver_regla(regla: &Regla) -> Vec<PathBuf> {
    aplicar_estructura(resolver(regla.rutas), regla)
}

/// Aplica `childSubdir`/`recursiveMatch` a unas bases YA resueltas.
///
/// Se separa de `resolver_regla` para poder probar los dos mecanismos con
/// directorios temporales: una `Regla` del catálogo declara rutas `'static` y una
/// de `std::env::temp_dir()` no lo es.
fn aplicar_estructura(bases: Vec<PathBuf>, regla: &Regla) -> Vec<PathBuf> {
    if bases.is_empty() {
        return bases;
    }
    if let Some(rm) = &regla.coincidencia_recursiva {
        return resolver_recursivo(&bases, rm);
    }
    if let Some(sub) = regla.subdir_hijo {
        let mut fuera = Vec::new();
        for base in &bases {
            let Ok(it) = std::fs::read_dir(base) else {
                continue;
            };
            for e in it.flatten() {
                let hijo = e.path();
                // Solo carpetas de verdad (no enlaces): un enlace a otro sitio no
                // se sigue ni se borra.
                let Ok(m) = std::fs::symlink_metadata(&hijo) else {
                    continue;
                };
                if !m.file_type().is_dir() {
                    continue;
                }
                let sub_path = hijo.join(sub);
                let Ok(sm) = std::fs::symlink_metadata(&sub_path) else {
                    continue;
                };
                if sm.file_type().is_dir() {
                    fuera.push(sub_path);
                }
            }
        }
        fuera.sort();
        fuera.dedup();
        return fuera;
    }
    bases
}

/// Expande un patrón de `rutas_ancla` (`*` = un nivel cualquiera; el último
/// segmento tiene que ser el ancla) desde `base`. No sigue enlaces.
fn expandir_anclas(
    base: &Path,
    patrones: &[&str],
    ancla: &str,
    visitados: &mut u64,
) -> Vec<PathBuf> {
    let ancla = normalizar_nombre(ancla);
    let mut res = Vec::new();
    for patron in patrones {
        let segmentos: Vec<&str> = patron.split('/').collect();
        if segmentos.last().map(|s| normalizar_nombre(s)) != Some(ancla.clone()) {
            continue;
        }
        let mut candidatos: Vec<PathBuf> = vec![base.to_path_buf()];
        for seg in &segmentos {
            let mut siguientes: Vec<PathBuf> = Vec::new();
            for c in &candidatos {
                if *visitados >= MAX_RECURSIVO {
                    break;
                }
                *visitados += 1;
                if *seg == "*" {
                    let Ok(it) = std::fs::read_dir(c) else {
                        continue;
                    };
                    for e in it.flatten() {
                        let p = e.path();
                        let Ok(m) = std::fs::symlink_metadata(&p) else {
                            continue;
                        };
                        if m.file_type().is_dir() {
                            siguientes.push(p);
                        }
                    }
                } else {
                    let p = c.join(seg);
                    let Ok(m) = std::fs::symlink_metadata(&p) else {
                        continue;
                    };
                    if m.file_type().is_dir() {
                        siguientes.push(p);
                    }
                }
            }
            candidatos = siguientes;
            if candidatos.is_empty() {
                break;
            }
        }
        res.extend(candidatos);
    }
    res
}

/// Carpetas de caché CONOCIDAS bajo un ancla, dentro de un árbol (`recursiveMatch`).
/// Ver `CoincidenciaRecursiva`.
fn resolver_recursivo(bases: &[PathBuf], rm: &CoincidenciaRecursiva) -> Vec<PathBuf> {
    let ancla = normalizar_nombre(rm.ancla);
    let objetivos: Vec<String> = rm.objetivos.iter().map(|o| normalizar_nombre(o)).collect();
    let excluidos: Vec<String> = rm.ancestros_excluidos.iter().map(|o| normalizar_nombre(o)).collect();
    let profundidad = if rm.profundidad == 0 { 12 } else { rm.profundidad.min(32) };
    let mut visitados = 0u64;

    // Cada raíz dice si ya estábamos bajo el ancla: si `rutas_ancla` viene puesto,
    // las raíces ya son el ancla; si no, lo será la base cuyo último componente sea
    // el ancla (y si no lo es, hay que bajar hasta encontrarla).
    let mut raices: Vec<(PathBuf, bool)> = Vec::new();
    for base in bases {
        if rm.rutas_ancla.is_empty() {
            let ultimo = base
                .file_name()
                .map(|n| normalizar_nombre(&n.to_string_lossy()))
                .unwrap_or_default();
            raices.push((base.clone(), ultimo == ancla));
        } else {
            for p in expandir_anclas(base, rm.rutas_ancla, rm.ancla, &mut visitados) {
                raices.push((p, true));
            }
        }
    }

    let mut fuera = Vec::new();
    for (raiz, bajo_ancla) in raices {
        let mut cola: Vec<(PathBuf, u32, bool)> = vec![(raiz, 0, bajo_ancla)];
        let mut i = 0;
        while i < cola.len() && visitados < MAX_RECURSIVO {
            let (actual, prof, bajo) = cola[i].clone();
            i += 1;
            visitados += 1;
            let Ok(it) = std::fs::read_dir(&actual) else {
                continue;
            };
            for e in it.flatten() {
                let p = e.path();
                let Ok(m) = std::fs::symlink_metadata(&p) else {
                    continue;
                };
                if !m.file_type().is_dir() {
                    continue;
                }
                let nombre = normalizar_nombre(&e.file_name().to_string_lossy());
                // Las ramas excluidas no se inspeccionan: ahí no hay caché.
                if bajo && excluidos.iter().any(|x| *x == nombre) {
                    continue;
                }
                if bajo && objetivos.iter().any(|x| *x == nombre) {
                    fuera.push(p);
                    continue;
                }
                let bajo_hijo = bajo || nombre == ancla;
                if prof + 1 < profundidad {
                    cola.push((p, prof + 1, bajo_hijo));
                }
            }
        }
    }
    fuera.sort();
    fuera.dedup();
    fuera
}

/// El corte de antigüedad de la regla.
///
/// Las reglas de `fileMatch` llevan su propia antigüedad POR FICHERO
/// (`CoincidenciaFicheros::dias_min`), que es la que manda; el resto usa la de la
/// regla.
fn corte_de_regla(regla: &Regla) -> Option<SystemTime> {
    match &regla.coincidencia_ficheros {
        Some(cf) => corte_de(cf.dias_min),
        None => corte_de(regla.min_dias),
    }
}

/// Autoriza limpiar el CONTENIDO de una carpeta (las reglas «Contenido» no borran
/// la carpeta: borran lo de dentro).
///
/// `almacen::permitida` comprueba rutas que se van a BORRAR y por eso rechaza las
/// raíces del sistema: `/tmp` no se borra. Pero una regla «Contenido» no borra
/// `/tmp`, borra SUS HIJOS, que sí están en una zona permitida (`/tmp/...`). Por
/// eso, cuando la carpeta no pasa como ruta, se sondea con UN hijo real: si
/// `permitida` lo acepta, el prefijo permitido cubre también a los demás hijos (es
/// el mismo prefijo para todo el árbol), así que no hace falta comprobar entrada
/// por entrada. Una carpeta vacía no necesita permiso: no hay nada que borrar.
fn contenido_permitido(dir: &Path) -> Result<(), String> {
    match crate::almacen::permitida(dir) {
        Ok(()) => Ok(()),
        Err(e) => {
            if !dir.is_dir() {
                return Err(e);
            }
            let Ok(it) = std::fs::read_dir(dir) else {
                return Err(e);
            };
            let mut vacia = true;
            for c in it.flatten() {
                let p = c.path();
                let Ok(m) = std::fs::symlink_metadata(&p) else {
                    continue;
                };
                if m.file_type().is_symlink() {
                    continue;
                }
                vacia = false;
                if crate::almacen::permitida(&p).is_ok() {
                    return Ok(());
                }
            }
            if vacia {
                return Ok(());
            }
            Err(e)
        }
    }
}

fn corte_de(dias: u32) -> Option<SystemTime> {
    if dias == 0 {
        None
    } else {
        SystemTime::now().checked_sub(Duration::from_secs(u64::from(dias) * 86_400))
    }
}

fn es_antiguo(meta: &std::fs::Metadata, corte: Option<SystemTime>) -> bool {
    match corte {
        None => true,
        Some(c) => meta.modified().map(|m| m < c).unwrap_or(true),
    }
}

/* ── Medir y borrar, con la MISMA lógica ─────────────────────────────────── */

/// Recorre el contenido de `p` y, si `borrar`, elimina lo que supere el corte.
/// Devuelve (bytes, elementos, recientes).
///
/// Lo que se mide es EXACTAMENTE lo que se borra: la interfaz promete un tamaño
/// y ese tiene que ser el que se libera, no el que ocupa la carpeta.
///
/// `profundo` activa el `deepRecencyCheck` de Kudu: un subárbol cuyo contenido es
/// antiguo del todo se borra de una vez (en lugar de fichero a fichero), pero **justo
/// antes de ese borrado recursivo se revalida** (`revalidar`); si algo cambió y ahora
/// hay contenido reciente, no se toca nada. Sin la revalidación, colapsar el borrado
/// podría llevarse por delante lo que una app acaba de escribir.
fn procesar(
    p: &Path,
    corte: Option<SystemTime>,
    borrar: bool,
    pres: &mut Presupuesto,
    prof: u32,
    profundo: bool,
) -> (u64, u64, u64) {
    let mut bytes = 0u64;
    let mut elems = 0u64;
    let mut recientes = 0u64;
    if pres.agotado {
        return (0, 0, 0);
    }
    let Ok(it) = std::fs::read_dir(p) else {
        return (0, 0, 0);
    };
    for e in it.flatten() {
        if !pres.gastar(1) {
            break;
        }
        let camino = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&camino) else {
            continue;
        };
        let ft = meta.file_type();
        // Un enlace no se sigue ni se borra: podría apuntar fuera de la caché.
        if ft.is_symlink() {
            continue;
        }
        let antiguo = es_antiguo(&meta, corte);
        if ft.is_dir() {
            if prof >= 64 {
                continue;
            }
            if profundo {
                // Primero se RECONOCE el subárbol sin tocarlo. Si todo él es
                // antiguo, se puede borrar de una vez; si hay algo reciente, se
                // entra a borrar solo lo viejo (la app conserva lo que escribe).
                let (b, n, r) = procesar(&camino, corte, false, pres, prof + 1, true);
                if n > 0 && r == 0 {
                    if !borrar {
                        bytes += b;
                        elems += n;
                    } else if revalidar(&camino, corte, pres)
                        && crate::plataforma::papelera::borrar_definitivo(&camino).is_ok()
                    {
                        bytes += b;
                        elems += n;
                    }
                    continue;
                }
                if borrar {
                    let (b2, n2, r2) = procesar(&camino, corte, true, pres, prof + 1, true);
                    bytes += b2;
                    elems += n2;
                    recientes += r2;
                } else {
                    bytes += b;
                    elems += n;
                    recientes += r;
                }
                continue;
            }
            let (b, n, r) = procesar(&camino, corte, borrar, pres, prof + 1, profundo);
            bytes += b;
            elems += n;
            recientes += r;
            // Si la carpeta se ha quedado vacía, se quita también (higiene), pero
            // NO se cuenta como elemento: `elementos` cuenta ficheros, que son
            // los que suman bytes, y así medir y borrar dan el mismo número.
            if borrar
                && n > 0
                && std::fs::read_dir(&camino).map(|mut i| i.next().is_none()).unwrap_or(false)
            {
                let _ = std::fs::remove_dir(&camino);
            }
        } else if antiguo {
            bytes += meta.len();
            if borrar && crate::plataforma::papelera::borrar_definitivo(&camino).is_err() {
                bytes -= meta.len();
            } else {
                elems += 1;
            }
        } else {
            recientes += 1;
        }
    }
    (bytes, elems, recientes)
}

/// Revalida, justo antes del borrado recursivo, que NADA de lo que hay bajo `p`
/// sea más reciente que el corte (ver `Regla::recencia_profunda`).
///
/// Devuelve `false` en cuanto encuentra algo que no debería borrarse (o si el
/// presupuesto se agota: sin poder comprobarlo todo, no se toca). Una carpeta se
/// juzga por su CONTENIDO, no por su propia fecha: su `mtime` cambia cada vez que
/// alguien añade o quita algo dentro y no dice si lo de dentro sigue en uso.
fn revalidar(p: &Path, corte: Option<SystemTime>, pres: &mut Presupuesto) -> bool {
    if pres.agotado {
        return false;
    }
    let Ok(it) = std::fs::read_dir(p) else {
        return false;
    };
    for e in it.flatten() {
        if !pres.gastar(1) {
            return false;
        }
        let camino = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&camino) else {
            return false;
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if !revalidar(&camino, corte, pres) {
                return false;
            }
        } else if !es_antiguo(&meta, corte) {
            return false;
        }
    }
    true
}

/// Borra SOLO los ficheros que casan dentro de las carpetas candidatas; el
/// directorio no se toca nunca (ver `CoincidenciaFicheros`).
fn aplicar_ficheros(
    base: &Path,
    cf: &CoincidenciaFicheros,
    borrar: bool,
    pres: &mut Presupuesto,
) -> (u64, u64, u64) {
    let corte = corte_de(cf.dias_min);
    let mut bytes = 0u64;
    let mut elems = 0u64;
    let mut recientes = 0u64;

    // Qué carpetas se miran: la base, o sus hijas directas cuyo nombre acabe en
    // el sufijo (p. ej. `*-updater`). Un nivel, ni más ni menos.
    let mut candidatos: Vec<PathBuf> = Vec::new();
    match cf.sufijo_dir {
        Some(sufijo) => {
            let sufijo = normalizar_nombre(sufijo);
            let Ok(it) = std::fs::read_dir(base) else {
                return (0, 0, 0);
            };
            for e in it.flatten() {
                let p = e.path();
                let Ok(m) = std::fs::symlink_metadata(&p) else {
                    continue;
                };
                if !m.file_type().is_dir() {
                    continue;
                }
                if normalizar_nombre(&e.file_name().to_string_lossy()).ends_with(&sufijo) {
                    candidatos.push(p);
                }
            }
        }
        None => candidatos.push(base.to_path_buf()),
    }

    for dir in candidatos {
        if pres.agotado {
            break;
        }
        let Ok(it) = std::fs::read_dir(&dir) else {
            continue;
        };
        let entradas: Vec<std::fs::DirEntry> = it.flatten().collect();
        // El bloqueo se mira ANTES de tocar nada: si el directorio tiene un
        // `pending`, hay una actualización a medias y no se borra ni uno de sus
        // ficheros.
        let bloqueado = entradas.iter().any(|e| {
            let nombre = normalizar_nombre(&e.file_name().to_string_lossy());
            cf.saltar_si_existe.iter().any(|s| normalizar_nombre(s) == nombre)
        });
        if bloqueado {
            continue;
        }
        for e in entradas {
            if !pres.gastar(1) {
                break;
            }
            let p = e.path();
            let Ok(m) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if !m.file_type().is_file() {
                continue;
            }
            let nombre = normalizar_nombre(&e.file_name().to_string_lossy());
            if !cf.nombres.iter().any(|n| normalizar_nombre(n) == nombre) {
                continue;
            }
            if !es_antiguo(&m, corte) {
                recientes += 1;
                continue;
            }
            let b = m.len();
            if borrar {
                if crate::plataforma::papelera::borrar_definitivo(&p).is_ok() {
                    bytes += b;
                    elems += 1;
                }
            } else {
                bytes += b;
                elems += 1;
            }
        }
    }
    (bytes, elems, recientes)
}

/// Mide (y borra, si toca) UN camino de una regla.
fn aplicar(
    regla: &Regla,
    camino: &Path,
    corte: Option<SystemTime>,
    borrar: bool,
    pres: &mut Presupuesto,
) -> Result<(u64, u64, u64), String> {
    // `fileMatch` no borra el directorio: elige ficheros dentro. Por eso tiene su
    // propia lógica y no pasa por `procesar`.
    if let Some(cf) = &regla.coincidencia_ficheros {
        return Ok(aplicar_ficheros(camino, cf, borrar, pres));
    }
    match regla.tipo {
        TipoRegla::Contenido => Ok(procesar(camino, corte, borrar, pres, 0, regla.recencia_profunda)),
        TipoRegla::Fichero => {
            let meta = std::fs::symlink_metadata(camino)
                .map_err(|e| format!("no se puede leer {}: {e}", camino.display()))?;
            if !es_antiguo(&meta, corte) {
                return Ok((0, 0, 1));
            }
            let b = meta.len();
            if borrar {
                crate::plataforma::papelera::borrar_definitivo(camino)?;
            }
            Ok((b, 1, 0))
        }
        TipoRegla::Comando => {
            let (b, n, _) = procesar(camino, None, false, pres, 0, false);
            Ok((b, n, 0))
        }
    }
}

fn reglas_de(categorias: Option<&[String]>) -> Vec<&'static Regla> {
    catalogo()
        .iter()
        .filter(|r| match categorias {
            None => true,
            Some(cs) => cs.is_empty() || cs.iter().any(|c| c == r.categoria),
        })
        .collect()
}

/* ── Escanear ─────────────────────────────────────────────────────────────── */

pub fn escanear(categorias: Option<Vec<String>>) -> Result<Escaneo, String> {
    let t0 = Instant::now();
    let filtro = categorias.filter(|c| !c.is_empty());
    let mut pres = Presupuesto::nuevo();
    let mut objetivos: Vec<Objetivo> = Vec::new();
    let mut total_bytes = 0u64;
    let mut total_elems = 0u64;

    // Las EXCLUSIONES del usuario se aplican ANTES de medir: lo excluido no se
    // cuenta, y se dice cuál fue la exclusión (ver `exclusiones.rs`).
    let excl = crate::exclusiones::vigentes();
    let caja = crate::exclusiones::sin_distinguir_caja();
    let mut excluidos: Vec<String> = Vec::new();

    for regla in reglas_de(filtro.as_deref()) {
        let todos = resolver_regla(regla);
        if todos.is_empty() {
            continue;
        }
        let mut caminos: Vec<PathBuf> = Vec::new();
        for c in todos {
            match crate::exclusiones::excluida_con(&c.to_string_lossy(), &excl, caja) {
                Some(v) => {
                    let nota = format!("{}: excluido por «{}»", regla.subcategoria, v.patron);
                    if !excluidos.contains(&nota) {
                        excluidos.push(nota);
                    }
                }
                None => caminos.push(c),
            }
        }
        if caminos.is_empty() {
            continue;
        }
        let corte = corte_de_regla(regla);
        let mut bytes = 0u64;
        let mut elems = 0u64;
        let mut recientes = 0u64;
        let mut sin_permiso = false;
        for camino in &caminos {
            if regla.tipo == TipoRegla::Contenido && std::fs::read_dir(camino).is_err() {
                sin_permiso = true;
                continue;
            }
            match aplicar(regla, camino, corte, false, &mut pres) {
                Ok((b, n, r)) => {
                    bytes += b;
                    elems += n;
                    recientes += r;
                }
                Err(_) => sin_permiso = true,
            }
        }
        objetivos.push(Objetivo {
            id: regla.id.to_string(),
            categoria: regla.categoria.to_string(),
            subcategoria: regla.subcategoria.to_string(),
            descripcion: regla.descripcion.to_string(),
            rutas: caminos.iter().map(|c| c.to_string_lossy().to_string()).collect(),
            bytes,
            elementos: elems,
            recientes,
            min_dias: regla.min_dias,
            root: regla.root,
            comando: regla.comando.map(str::to_string),
            sin_permiso,
            parcial: pres.agotado,
            traza: regla.traza,
            reinicio_cache: regla.reinicio_cache,
        });
        total_bytes += bytes;
        total_elems += elems;
    }

    // Lo que más espacio libera, primero; lo que no se puede tocar por permisos,
    // al final (no se puede borrar nada, así que no compite con lo demás).
    objetivos.sort_by(|a, b| {
        a.root
            .cmp(&b.root)
            .then_with(|| b.bytes.cmp(&a.bytes))
            .then_with(|| a.subcategoria.cmp(&b.subcategoria))
    });

    Ok(Escaneo {
        objetivos,
        bytes: total_bytes,
        elementos: total_elems,
        ms: t0.elapsed().as_millis() as u64,
        truncado: pres.agotado,
        excluidos,
    })
}

/* ── Limpiar ──────────────────────────────────────────────────────────────── */

pub fn limpiar(ids: &[String]) -> Result<Informe, String> {
    if ids.is_empty() {
        return Err("No hay nada seleccionado que limpiar".into());
    }
    let mut pres = Presupuesto::nuevo();
    let mut bytes = 0u64;
    let mut elementos = 0u64;
    let mut limpiados: Vec<String> = Vec::new();
    let mut saltados: Vec<String> = Vec::new();
    let mut fallos: Vec<String> = Vec::new();

    for id in ids {
        let Some(regla) = catalogo().iter().find(|r| r.id == id) else {
            fallos.push(format!("objetivo desconocido: {id}"));
            continue;
        };
        // Lo que necesita root no se lanza a escondidas: se dice el comando.
        if regla.root {
            saltados.push(format!(
                "{} necesita root: {}",
                regla.subcategoria,
                regla.comando.unwrap_or("hazlo a mano como administrador")
            ));
            continue;
        }
        if regla.tipo == TipoRegla::Comando {
            saltados.push(format!(
                "{} se limpia con su propio comando: {}",
                regla.subcategoria,
                regla.comando.unwrap_or("herramienta nativa")
            ));
            continue;
        }
        let caminos = resolver_regla(regla);
        if caminos.is_empty() {
            saltados.push(format!("{}: ya no existe", regla.subcategoria));
            continue;
        }
        // Y las exclusiones se respetan TAMBIÉN al borrar, no solo al medir: si
        // alguien marca un objetivo cuya ruta está excluida (o la añade a la lista
        // entre el escaneo y el borrado), no se toca.
        let excl = crate::exclusiones::vigentes();
        let caja = crate::exclusiones::sin_distinguir_caja();
        let corte = corte_de_regla(regla);
        let mut b_regla = 0u64;
        let mut n_regla = 0u64;
        let mut algun_fallo = false;
        for camino in &caminos {
            if let Some(v) = crate::exclusiones::excluida_con(&camino.to_string_lossy(), &excl, caja) {
                saltados.push(format!(
                    "{}: excluido por «{}»",
                    regla.subcategoria, v.patron
                ));
                continue;
            }
            // La MISMA lista blanca que usa el analizador: no se borra una raíz
            // del sistema ni nada fuera del home o de las rutas de caché. Una regla
            // «Contenido» no borra su carpeta, borra lo de dentro, así que ahí se
            // autoriza el CONTENEDOR (ver `contenido_permitido`).
            let permiso = if regla.tipo == TipoRegla::Contenido {
                contenido_permitido(camino)
            } else {
                crate::almacen::permitida(camino)
            };
            if let Err(e) = permiso {
                fallos.push(e);
                algun_fallo = true;
                continue;
            }
            match aplicar(regla, camino, corte, true, &mut pres) {
                Ok((b, n, _)) => {
                    b_regla += b;
                    n_regla += n;
                }
                Err(e) => {
                    fallos.push(e);
                    algun_fallo = true;
                }
            }
        }
        if n_regla > 0 && !algun_fallo {
            limpiados.push(format!("{} ({} elementos)", regla.subcategoria, n_regla));
        } else if algun_fallo && n_regla > 0 {
            limpiados.push(format!("{} ({} elementos, con fallos)", regla.subcategoria, n_regla));
        }
        bytes += b_regla;
        elementos += n_regla;
    }

    // Cuando no se ha limpiado nada: si fue por fallos, es un error (el usuario
    // tiene que verlo); si fue porque todo lo seleccionado se limpia con root o
    // con su propio comando, es un RESULTADO, no un fallo: el mensaje lo explica.
    if elementos == 0 && limpiados.is_empty() && saltados.is_empty() && !fallos.is_empty() {
        return Err(fallos.first().cloned().unwrap_or_else(|| "no se pudo limpiar nada".into()));
    }
    let mut mensaje = format!(
        "{} liberados en {} elementos ({} objetivos)",
        crate::almacen::legible(bytes),
        elementos,
        limpiados.len()
    );
    if !saltados.is_empty() {
        mensaje.push_str(&format!(". {} saltados", saltados.len()));
    }
    if !fallos.is_empty() {
        mensaje.push_str(&format!(". {} con fallos", fallos.len()));
    }
    Ok(Informe { mensaje, bytes, elementos, limpiados, saltados, fallos })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn escribir_desde(path: &Path, bytes: usize, dias: i64) {
        std::fs::write(path, vec![0u8; bytes]).unwrap();
        if dias > 0 {
            let cuando = SystemTime::now() - Duration::from_secs(dias as u64 * 86_400);
            let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            f.set_modified(cuando).unwrap();
        }
    }

    #[test]
    fn el_catalogo_esta_bien_formado() {
        let mut ids = std::collections::HashSet::new();
        // Se revisan los TRES catálogos, también desde Linux: un catálogo que solo
        // se compila en su sistema es un catálogo que nadie mira hasta que alguien
        // lo usa de verdad.
        for r in todos_los_catalogos().into_iter().flat_map(|(_, c)| c.iter()) {
            assert!(ids.insert(r.id), "id repetido: {}", r.id);
            assert!(!r.descripcion.is_empty(), "{} sin descripción", r.id);
            assert!(!r.rutas.is_empty(), "{} sin rutas", r.id);
            assert!(
                CATEGORIAS.iter().any(|(c, _)| *c == r.categoria),
                "{} tiene categoría desconocida: {}",
                r.id,
                r.categoria
            );
            // Todo lo que necesita root tiene que decir CÓMO se hace a mano.
            if r.root {
                assert!(r.comando.is_some(), "{} necesita root y no dice el comando", r.id);
            }
            // Las huellas van SIEMPRE en la categoría de privacidad: si se pudieran
            // repartir por otras, el aviso de la interfaz y la protección del CLI
            // (que mira `traza`) dejarían de coincidir con lo que se enseña.
            if r.traza {
                assert_eq!(r.categoria, "privacidad", "{} es huella y no está en privacidad", r.id);
            }
            // Los modificadores que cambian CÓMO se recorre un objetivo son
            // excluyentes entre sí (el esquema de Kudu los declara incompatibles):
            // escanear `ruta/*/sub`, buscar carpetas recursivamente y filtrar
            // ficheros no se pueden combinar en la misma regla.
            let modos = [
                r.subdir_hijo.is_some(),
                r.coincidencia_recursiva.is_some(),
                r.coincidencia_ficheros.is_some(),
            ];
            assert!(
                modos.iter().filter(|x| **x).count() <= 1,
                "{} combina childSubdir/recursiveMatch/fileMatch",
                r.id
            );
            if let Some(cf) = &r.coincidencia_ficheros {
                assert!(!cf.nombres.is_empty(), "{}: fileMatch sin nombres", r.id);
                assert!(cf.dias_min >= 1, "{}: fileMatch sin antigüedad mínima", r.id);
            }
            if let Some(rm) = &r.coincidencia_recursiva {
                assert!(!rm.ancla.is_empty(), "{}: recursiveMatch sin ancla", r.id);
                assert!(!rm.objetivos.is_empty(), "{}: recursiveMatch sin objetivos", r.id);
            }
            // Revalidar la antigüedad solo tiene sentido si la regla tiene una
            // antigüedad que revalidar.
            if r.recencia_profunda {
                assert!(r.min_dias >= 1, "{}: recencia profunda sin antigüedad mínima", r.id);
            }
        }
    }

    #[test]
    fn mide_lo_que_borraria_y_respeta_la_antiguedad() {
        let dir = std::env::temp_dir().join("machinograph-limpieza-prueba");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        escribir_desde(&dir.join("viejo.bin"), 1000, 10);
        escribir_desde(&dir.join("nuevo.bin"), 2000, 0);
        escribir_desde(&dir.join("sub/anidado.bin"), 500, 10);

        let regla = regla("prueba", "sistema", "Prueba", "prueba", &["x"], TipoRegla::Contenido);
        let regla = con_dias(regla, 7);
        let corte = corte_de(regla.min_dias);
        let mut pres = Presupuesto::nuevo();

        // Medir: solo cuenta lo antiguo; lo nuevo se cuenta aparte.
        let (b, n, r) = aplicar(&regla, &dir, corte, false, &mut pres).unwrap();
        assert_eq!(b, 1500);
        assert_eq!(n, 2);
        assert_eq!(r, 1);

        // Borrar: exactamente lo mismo, y el reciente tiene que seguir ahí.
        let mut pres2 = Presupuesto::nuevo();
        let (b2, n2, _) = aplicar(&regla, &dir, corte, true, &mut pres2).unwrap();
        assert_eq!((b2, n2), (1500, 2));
        assert!(!dir.join("viejo.bin").exists());
        assert!(!dir.join("sub/anidado.bin").exists());
        assert!(dir.join("nuevo.bin").exists(), "el fichero reciente NO se borra");
        // La carpeta queda (una regla "Contenido" conserva la carpeta).
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sin_antiguedad_minima_se_borra_todo_el_contenido() {
        let dir = std::env::temp_dir().join("machinograph-limpieza-todo");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        escribir_desde(&dir.join("a.bin"), 10, 0);
        escribir_desde(&dir.join("sub/b.bin"), 20, 0);
        let regla = regla("prueba", "sistema", "Prueba", "prueba", &["x"], TipoRegla::Contenido);
        let mut pres = Presupuesto::nuevo();
        let (b, n, r) = aplicar(&regla, &dir, None, true, &mut pres).unwrap();
        assert_eq!(b, 30);
        assert_eq!(n, 2); // dos ficheros (la carpeta `sub` se quita, pero no cuenta)
        assert_eq!(r, 0);
        assert!(dir.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_fichero_suelto_respeta_su_antiguedad() {
        let dir = std::env::temp_dir().join("machinograph-limpieza-fichero");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        escribir_desde(&dir.join("log.viejo"), 42, 30);
        let regla = regla("prueba", "sistema", "Prueba", "prueba", &["x"], TipoRegla::Fichero);
        let regla = con_dias(regla, 7);
        let corte = corte_de(regla.min_dias);
        let mut pres = Presupuesto::nuevo();
        let (b, n, _) = aplicar(&regla, &dir.join("log.viejo"), corte, true, &mut pres).unwrap();
        assert_eq!((b, n), (42, 1));
        assert!(!dir.join("log.viejo").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_intenta_borrar_lo_que_necesita_root() {
        // Lo que necesita root se SALTA con el comando, no se lanza sudo a ciegas.
        let inf = limpiar(&["dnf".to_string()]).unwrap();
        assert_eq!(inf.bytes, 0);
        assert!(inf.saltados.iter().any(|s| s.contains("root")), "{inf:?}");
        assert!(inf.saltados.iter().any(|s| s.contains("dnf clean")), "{inf:?}");
        // Un objetivo que no existe es un fallo, y si es el único, es un error.
        let e = limpiar(&["no-existe".to_string()]).unwrap_err();
        assert!(e.contains("desconocido"), "{e}");
    }

    /// Un objetivo de mentira, para probar la SELECCIÓN sin tocar el disco.
    fn obj(id: &str, bytes: u64, traza: bool) -> Objetivo {
        Objetivo {
            id: id.into(),
            categoria: if traza { "privacidad".into() } else { "sistema".into() },
            subcategoria: id.into(),
            descripcion: String::new(),
            rutas: vec![],
            bytes,
            elementos: 1,
            recientes: 0,
            min_dias: 0,
            root: false,
            comando: None,
            sin_permiso: false,
            parcial: false,
            traza,
            reinicio_cache: false,
        }
    }

    #[test]
    fn la_limpieza_automatica_nunca_se_lleva_una_huella_sin_pedirla() {
        let lista = vec![
            obj("pip", 100, false),
            obj("hist-bash", 4096, true),
            obj("portapapeles", 2048, true),
        ];
        // Sin pedir privacidad: las huellas NO están, y lo demás sí.
        let ids = ids_a_limpiar(&lista, false);
        assert_eq!(ids, vec!["pip".to_string()], "{ids:?}");
        // Pidiéndola por su nombre, sí (y solo entonces).
        let ids = ids_a_limpiar(&lista, true);
        assert_eq!(ids, vec!["pip".to_string(), "hist-bash".to_string(), "portapapeles".to_string()], "{ids:?}");
        // Y lo que no se puede limpiar desde aquí sigue fuera en los dos casos.
        let mut raiz = obj("dnf", 100, false);
        raiz.root = true;
        let mut con_comando = obj("uv", 100, false);
        con_comando.comando = Some("uv cache prune".into());
        let vacio = obj("nada", 0, false);
        let ids = ids_a_limpiar(&[raiz, con_comando, vacio], true);
        assert!(ids.is_empty(), "{ids:?}");
    }

    /// La misma regla, pero contra el catálogo REAL de este sistema (solo lectura:
    /// escanea y decide, no borra nada). El catálogo de mentira podría no parecerse
    /// al de verdad; este no puede.
    #[test]
    fn sobre_el_catalogo_real_ninguna_huella_entra_en_una_limpieza_a_secas() {
        let e = escanear(None).unwrap();
        let huellas: Vec<String> = e
            .objetivos
            .iter()
            .filter(|o| o.traza)
            .map(|o| o.id.clone())
            .collect();
        assert!(
            !huellas.is_empty(),
            "este catálogo no declara ninguna huella: la prueba no estaría probando nada"
        );
        let a_secas = ids_a_limpiar(&e.objetivos, false);
        for h in &huellas {
            assert!(!a_secas.contains(h), "se iba a borrar la huella {h} sin pedirla");
        }
        // Y pidiéndola por su nombre, la huella que se puede limpiar desde aquí sí
        // entra: si `huellas` trae 5 y las 5 son limpiables, tiene que haber más.
        let pidiendo = ids_a_limpiar(&e.objetivos, true);
        assert!(pidiendo.len() > a_secas.len(), "pedir privacidad no cambió nada");
        // Y ningún REINICIO de caché entra en una limpieza automática, ni pidiendo
        // la categoría: lo que corre solo no puede dejarte los shaders sin compilar.
        let reinicios: Vec<String> = e
            .objetivos
            .iter()
            .filter(|o| o.reinicio_cache)
            .map(|o| o.id.clone())
            .collect();
        assert!(!reinicios.is_empty(), "este catálogo no declara ningún reinicio de caché");
        for r in &reinicios {
            assert!(!pidiendo.contains(r), "el reinicio de caché {r} entró en una limpieza");
        }
    }

    #[test]
    fn la_limpieza_automatica_nunca_reinicia_una_cache_de_rendimiento() {
        // Un reinicio de caché es limpiable a mano, pero no puede correr solo.
        let mut shaders = obj("mesa-shaders", 4096, false);
        shaders.reinicio_cache = true;
        let lista = vec![obj("pip", 100, false), shaders];
        for pide in [false, true] {
            let ids = ids_a_limpiar(&lista, pide);
            assert_eq!(ids, vec!["pip".to_string()], "{ids:?}");
        }
    }

    #[test]
    fn child_subdir_encuentra_la_subcarpeta_de_cada_perfil() {
        let base = std::env::temp_dir().join("machinograph-limpieza-subdir");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("perfil-a/cache2")).unwrap();
        std::fs::create_dir_all(base.join("perfil-b/cache2")).unwrap();
        std::fs::create_dir_all(base.join("perfil-c/sin-cache")).unwrap();

        let regla = regla("prueba", "sistema", "Prueba", "prueba", &[], TipoRegla::Contenido)
            .con_subdir_hijo("cache2");
        let rutas = aplicar_estructura(vec![base.clone()], &regla);
        assert_eq!(rutas.len(), 2, "{rutas:?}");
        assert!(rutas.contains(&base.join("perfil-a/cache2")), "{rutas:?}");
        assert!(rutas.contains(&base.join("perfil-b/cache2")), "{rutas:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn file_match_solo_borra_los_ficheros_que_casan() {
        let base = std::env::temp_dir().join("machinograph-limpieza-filematch");
        let _ = std::fs::remove_dir_all(&base);
        let up = base.join("mi-app-updater");
        std::fs::create_dir_all(up.join("otra")).unwrap();
        std::fs::create_dir_all(base.join("otra-app-updater")).unwrap();
        std::fs::create_dir_all(base.join("sin-sufijo")).unwrap();
        escribir_desde(&up.join("installer.exe"), 10, 30);
        escribir_desde(&up.join("current.blockmap"), 20, 30);
        escribir_desde(&up.join("notas.txt"), 40, 30); // no casa: se queda
        escribir_desde(&up.join("otra/nested.exe"), 80, 30); // anidado: ni se mira
        escribir_desde(&base.join("sin-sufijo/installer.exe"), 90, 30); // otro dir: no
        escribir_desde(&base.join("otra-app-updater/installer.exe"), 100, 0); // reciente
        let cf = CoincidenciaFicheros {
            nombres: &["installer.exe", "current.blockmap"],
            sufijo_dir: Some("-updater"),
            dias_min: 14,
            saltar_si_existe: &["pending"],
        };

        // Medir: solo los dos que casan y son antiguos.
        let mut pres = Presupuesto::nuevo();
        let (b, n, r) = aplicar_ficheros(&base, &cf, false, &mut pres);
        assert_eq!((b, n, r), (30, 2, 1), "bytes/elementos/recientes");

        // Borrar: exactamente eso, y NADA más.
        let mut pres2 = Presupuesto::nuevo();
        let (b2, n2, _) = aplicar_ficheros(&base, &cf, true, &mut pres2);
        assert_eq!((b2, n2), (30, 2));
        assert!(!up.join("installer.exe").exists());
        assert!(!up.join("current.blockmap").exists());
        assert!(up.join("notas.txt").exists(), "un fichero que no casa no se toca");
        assert!(up.join("otra/nested.exe").exists(), "lo anidado no se toca");
        assert!(base.join("sin-sufijo/installer.exe").exists(), "otra carpeta no se toca");
        assert!(
            base.join("otra-app-updater/installer.exe").exists(),
            "lo reciente no se toca"
        );
        // El DIRECTORIO no se borra nunca, solo sus ficheros.
        assert!(up.exists(), "el directorio candidato se conserva");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn file_match_se_salta_el_objetivo_si_hay_un_pending() {
        let base = std::env::temp_dir().join("machinograph-limpieza-pending");
        let _ = std::fs::remove_dir_all(&base);
        let up = base.join("mi-app-updater");
        std::fs::create_dir_all(up.join("pending")).unwrap();
        escribir_desde(&up.join("installer.exe"), 10, 30);
        let cf = CoincidenciaFicheros {
            nombres: &["installer.exe"],
            sufijo_dir: Some("-updater"),
            dias_min: 14,
            saltar_si_existe: &["pending"],
        };

        // El bloqueo se mira ANTES de nada: ni se mide ni se borra.
        let mut pres = Presupuesto::nuevo();
        assert_eq!(aplicar_ficheros(&base, &cf, false, &mut pres), (0, 0, 0));
        let mut pres2 = Presupuesto::nuevo();
        assert_eq!(aplicar_ficheros(&base, &cf, true, &mut pres2), (0, 0, 0));
        assert!(up.join("installer.exe").exists(), "con un `pending` no se toca nada");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recursive_match_encuentra_anidadas_y_respeta_exclusiones() {
        let base = std::env::temp_dir().join("machinograph-limpieza-recursivo");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("a/EBWebView/Cache")).unwrap();
        std::fs::create_dir_all(base.join("a/EBWebView/Code Cache")).unwrap();
        std::fs::create_dir_all(base.join("a/EBWebView/Local Storage/x")).unwrap();
        std::fs::create_dir_all(base.join("b/c/EBWebView/GPUCache")).unwrap();
        std::fs::create_dir_all(base.join("a/otra/Cache")).unwrap();

        let rm = CoincidenciaRecursiva {
            ancla: "EBWebView",
            rutas_ancla: &[],
            objetivos: &["Cache", "Code Cache", "GPUCache"],
            ancestros_excluidos: &["Local Storage"],
            profundidad: 8,
        };
        let regla = Regla {
            coincidencia_recursiva: Some(rm),
            ..regla("prueba", "sistema", "Prueba", "prueba", &[], TipoRegla::Contenido)
        };
        let rutas = aplicar_estructura(vec![base.clone()], &regla);
        assert_eq!(rutas.len(), 3, "{rutas:?}");
        assert!(rutas.contains(&base.join("a/EBWebView/Cache")), "{rutas:?}");
        assert!(rutas.contains(&base.join("a/EBWebView/Code Cache")), "{rutas:?}");
        assert!(rutas.contains(&base.join("b/c/EBWebView/GPUCache")), "{rutas:?}");
        // La rama excluida no se inspecciona (y `Cache` sin ancla encima tampoco).
        assert!(
            !rutas.iter().any(|p| p.to_string_lossy().contains("Local Storage")),
            "una rama excluida no puede aparecer: {rutas:?}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recursive_match_respeta_las_rutas_de_ancla() {
        let base = std::env::temp_dir().join("machinograph-limpieza-anclas");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("App1/EBWebView/Cache")).unwrap();
        std::fs::create_dir_all(base.join("App2/EBWebView/GPUCache")).unwrap();
        // Un EBWebView demasiado profundo para el patrón: no se cuela.
        std::fs::create_dir_all(base.join("x/y/z/EBWebView/Cache")).unwrap();

        let rm = CoincidenciaRecursiva {
            ancla: "EBWebView",
            rutas_ancla: &["*/EBWebView"],
            objetivos: &["Cache", "GPUCache"],
            ancestros_excluidos: &[],
            profundidad: 8,
        };
        let regla = Regla {
            coincidencia_recursiva: Some(rm),
            ..regla("prueba", "sistema", "Prueba", "prueba", &[], TipoRegla::Contenido)
        };
        let rutas = aplicar_estructura(vec![base.clone()], &regla);
        assert_eq!(rutas.len(), 2, "{rutas:?}");
        assert!(rutas.contains(&base.join("App1/EBWebView/Cache")), "{rutas:?}");
        assert!(rutas.contains(&base.join("App2/EBWebView/GPUCache")), "{rutas:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn la_revalidacion_no_borra_si_algo_es_reciente() {
        let dir = std::env::temp_dir().join("machinograph-limpieza-revalidar");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        escribir_desde(&dir.join("sub/viejo.bin"), 10, 30);
        escribir_desde(&dir.join("sub/nuevo.bin"), 20, 0);
        let corte = corte_de(7);

        // Con un fichero reciente dentro, NO se puede asegurar que el subárbol esté
        // asentado: la revalidación lo dice y el borrado no se haría.
        let mut pres = Presupuesto::nuevo();
        assert!(!revalidar(&dir, corte, &mut pres), "hay algo reciente");

        // Sin nada reciente, sí se puede borrar de una vez.
        std::fs::remove_file(dir.join("sub/nuevo.bin")).unwrap();
        let mut pres2 = Presupuesto::nuevo();
        assert!(revalidar(&dir, corte, &mut pres2), "todo es antiguo");

        // Y una regla con recencia profunda, en la práctica: el `sub` con algo
        // reciente NO se colapsa (solo cae lo antiguo de dentro, y la carpeta se
        // queda porque el fichero reciente sigue ahí).
        escribir_desde(&dir.join("sub/nuevo.bin"), 20, 0);
        let regla = regla("prueba", "sistema", "Prueba", "prueba", &[], TipoRegla::Contenido)
            .con_dias_como(7)
            .con_recencia_profunda();
        let corte = corte_de_regla(&regla);
        let mut pres3 = Presupuesto::nuevo();
        let (b, n, r) = aplicar(&regla, &dir, corte, true, &mut pres3).unwrap();
        assert_eq!((b, n, r), (10, 1, 1), "{b}/{n}/{r}");
        assert!(!dir.join("sub/viejo.bin").exists());
        assert!(dir.join("sub").exists(), "la carpeta con lo reciente no se colapsa");
        assert!(dir.join("sub/nuevo.bin").exists(), "lo reciente se queda");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn una_regla_profunda_borra_de_una_vez_un_subarbol_asentado() {
        let dir = std::env::temp_dir().join("machinograph-limpieza-colapso");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("asentado/sub")).unwrap();
        escribir_desde(&dir.join("asentado/sub/viejo.bin"), 40, 30);
        escribir_desde(&dir.join("nuevo.bin"), 8, 0);

        let regla = regla("prueba", "sistema", "Prueba", "prueba", &[], TipoRegla::Contenido)
            .con_dias_como(7)
            .con_recencia_profunda();
        let corte = corte_de_regla(&regla);
        let mut pres = Presupuesto::nuevo();
        let (b, n, r) = aplicar(&regla, &dir, corte, true, &mut pres).unwrap();
        assert_eq!((b, n, r), (40, 1, 1), "{b}/{n}/{r}");
        // El subárbol asentado se fue ENTERO (una sola operación recursiva)…
        assert!(!dir.join("asentado").exists(), "el subárbol asentado debería caer de una vez");
        // …y lo reciente de arriba sigue ahí, con la carpeta raíz.
        assert!(dir.join("nuevo.bin").exists());
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn un_contenedor_de_temporales_se_puede_limpiar_por_dentro() {
        // `/tmp` no se borra (es una raíz protegida), pero su contenido SÍ está en
        // una zona permitida: la comprobación de contenedor lo autoriza. Si no,
        // una regla sobre `/tmp` mediría y luego fallaría al limpiar.
        assert!(contenido_permitido(Path::new("/tmp")).is_ok());
        // Una raíz cuyo contenido tampoco está permitido sigue rechazándose.
        assert!(contenido_permitido(Path::new("/etc")).is_err());
    }
}
