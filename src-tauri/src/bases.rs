//! Bases de datos SQLite de las aplicaciones: cuánto espacio hay DENTRO que no
//! hace falta, medido, y un botón para devolverlo sin borrar ni una fila.
//!
//! POR QUÉ EXISTE: Kudu tiene una categoría `databases.json` (MIT) que hace
//! `VACUUM` a las bases SQLite de navegadores, VS Code, Slack, Discord o
//! Thunderbird. La suya solo las lista. La nuestra **mide** y, sobre todo, dice de
//! dónde sale cada cifra.
//!
//! De dónde salen los datos, y ninguna ruta es inventada:
//!
//! * El catálogo son los tres ficheros `rules/<plataforma>/databases.json` de
//!   Kudu, copiados AQUÍ tal cual (`KUDU_LINUX`, `KUDU_DARWIN`, `KUDU_WIN32`) y
//!   parseados en tiempo de ejecución. Se traduce la plataforma actual con
//!   `plataforma::expandir_plantilla`, que ya resuelve `${HOME}`, `${CONFIG}`,
//!   `${APPDATA}`… Si una de esas variables no se puede traducir, el objetivo se
//!   deja FUERA y se dice: no se inventa la ruta.
//! * Las cifras de cada base salen de los PRAGMA de SQLite, no de una estimación:
//!   `page_count × page_size` es lo que ocupa y `freelist_count × page_size` es lo
//!   que un `VACUUM` devuelve al sistema de ficheros. Un navegador con 400 MB de
//!   historial puede tener media base en páginas libres.
//!
//! Tres decisiones que no son de gusto:
//!
//! 1. **Listar es de SOLO LECTURA.** Se abre con `BANDERAS_LECTURA`
//!    (`SQLITE_OPEN_READ_ONLY`) y solo se preguntan PRAGMA. Ni se crea un fichero
//!    ni se toca el que hay: recorrer el catálogo entero para MEDIR no puede
//!    alterar nada.
//! 2. **El VACUUM solo se hace si el usuario lo pide, nunca solo.** Es una acción
//!    explícita, en dos pasos, porque reescribe la base. Antes de empezar se
//!    prueba a tomar el bloqueo de ESCRITURA con un `busy_timeout` corto: si otra
//!    aplicación la tiene abierta en exclusiva, esa base NO se toca y se dice qué
//!    proceso la bloquea (`plataforma::procesos()`), con el pid. `VACUUM` es
//!    atómico y no borra filas, pero con el dueño de la base abierta el riesgo no
//!    compensa: se avisa y se ofrece reintentar.
//! 3. **Lo recuperado se vuelve a MEDIR.** Después de compactar se leen otra vez
//!    los PRAGMA y el tamaño del fichero, y se enseña la diferencia real: entre
//!    "se recuperarían 40 MB" y "se han recuperado 38 MB" hay un fichero de por
//!    medio, y la interfaz tiene que poder decir los dos.
use crate::plataforma;
use rusqlite::{ffi::ErrorCode, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/* ── Presupuesto ──────────────────────────────────────────────────────────── */

/// Tope de bases y de tiempo por pasada. Leer PRAGMA es barato (milisegundos por
/// base), pero esto recorre las carpetas de los navegadores de TODO el equipo: si
/// el usuario tiene veinte perfiles, no puede quedarse la ventana colgada. Lo que
/// no se mire se dice (`truncado`), como hace el limpiador.
const MAX_BASES: u64 = 600;
const MAX_TIEMPO: Duration = Duration::from_secs(20);

/// Cuánto se espera a un bloqueo ajeno antes de dármelo por bloqueada. Corto a
/// propósito: si otra aplicación tiene la base, insistir no la va a soltar, y el
/// usuario está delante de una pantalla.
const ESPERA_BLOQUEO: Duration = Duration::from_millis(400);

/* ── El catálogo de Kudu ──────────────────────────────────────────────────── */

/// Los tres `rules/<plataforma>/databases.json` de Kudu (MIT), tal cual.
///
/// Están embebidos y no se leen de Internet a propósito: el proyecto es
/// local-first (nada sale del equipo) y esto es un catálogo de rutas, no algo que
/// tenga que estar al día cada arranque.
const KUDU_LINUX: &str = r#"{
  "$schema": "../schema/rules.schema.json",
  "type": "databases",
  "sharedDbFileSets": {
    "chromium": ["History", "Cookies", "Network/Cookies", "Favicons", "Top Sites", "Web Data", "Shortcuts"],
    "firefox": ["places.sqlite", "cookies.sqlite", "favicons.sqlite", "formhistory.sqlite", "webappsstore.sqlite", "content-prefs.sqlite"]
  },
  "targets": [
    { "label": "Google Chrome", "basePath": "${CONFIG}/google-chrome", "dbFiles": "$chromium", "multiProfile": true, "description": "Chrome browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Microsoft Edge", "basePath": "${CONFIG}/microsoft-edge", "dbFiles": "$chromium", "multiProfile": true, "description": "Edge browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Brave", "basePath": "${CONFIG}/BraveSoftware/Brave-Browser", "dbFiles": "$chromium", "multiProfile": true, "description": "Brave browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Vivaldi", "basePath": "${CONFIG}/vivaldi", "dbFiles": "$chromium", "multiProfile": true, "description": "Vivaldi browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Opera", "basePath": "${CONFIG}/opera", "dbFiles": "$chromium", "description": "Opera browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Chromium", "basePath": "${CONFIG}/chromium", "dbFiles": "$chromium", "multiProfile": true, "description": "Chromium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Thorium", "basePath": "${CONFIG}/thorium", "dbFiles": "$chromium", "multiProfile": true, "description": "Thorium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Supermium", "basePath": "${CONFIG}/supermium", "dbFiles": "$chromium", "multiProfile": true, "description": "Supermium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Helium", "basePath": "${CONFIG}/helium", "dbFiles": "$chromium", "multiProfile": true, "description": "Helium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Cromite", "basePath": "${CONFIG}/cromite", "dbFiles": "$chromium", "multiProfile": true, "description": "Cromite browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Firefox", "basePath": "${HOME}/.mozilla/firefox", "dbFiles": "$firefox", "multiProfile": true, "profilePattern": ["*.default*", "*.dev-edition*"], "description": "Firefox browsing history, cookies, favicons, form data, and site preferences" },
    { "label": "Discord", "basePath": "${CONFIG}/discord", "dbFiles": ["Network/Cookies"], "description": "Discord network cookies database" },
    { "label": "Slack", "basePath": "${CONFIG}/Slack", "dbFiles": ["Network/Cookies"], "description": "Slack network cookies database" },
    { "label": "Microsoft Teams", "basePath": "${CONFIG}/Microsoft/Microsoft Teams", "dbFiles": ["Network/Cookies"], "description": "Microsoft Teams network cookies database" },
    { "label": "VS Code", "basePath": "${CONFIG}/Code", "dbFiles": ["Network/Cookies", "User/globalStorage/state.vscdb"], "description": "VS Code network cookies and global state database" },
    { "label": "Cursor IDE", "basePath": "${CONFIG}/Cursor", "dbFiles": ["Network/Cookies", "User/globalStorage/state.vscdb"], "description": "Cursor IDE network cookies and global state database" },
    { "label": "Thunderbird", "basePath": "${HOME}/.thunderbird", "dbFiles": ["global-messages-db.sqlite", "places.sqlite", "cookies.sqlite"], "multiProfile": true, "profilePattern": ["*.default*"], "description": "Thunderbird global message index, places, and cookies databases" }
  ]
}"#;
const KUDU_DARWIN: &str = r#"{
  "$schema": "../schema/rules.schema.json",
  "type": "databases",
  "sharedDbFileSets": {
    "chromium": ["History", "Cookies", "Network/Cookies", "Favicons", "Top Sites", "Web Data", "Shortcuts"],
    "firefox": ["places.sqlite", "cookies.sqlite", "favicons.sqlite", "formhistory.sqlite", "webappsstore.sqlite", "content-prefs.sqlite"]
  },
  "targets": [
    { "label": "Google Chrome", "basePath": "${APP_SUPPORT}/Google/Chrome", "dbFiles": "$chromium", "multiProfile": true, "description": "Chrome browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Microsoft Edge", "basePath": "${APP_SUPPORT}/Microsoft Edge", "dbFiles": "$chromium", "multiProfile": true, "description": "Edge browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Brave", "basePath": "${APP_SUPPORT}/BraveSoftware/Brave-Browser", "dbFiles": "$chromium", "multiProfile": true, "description": "Brave browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Vivaldi", "basePath": "${APP_SUPPORT}/Vivaldi", "dbFiles": "$chromium", "multiProfile": true, "description": "Vivaldi browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Opera", "basePath": "${APP_SUPPORT}/com.operasoftware.Opera", "dbFiles": "$chromium", "description": "Opera browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Opera GX", "basePath": "${APP_SUPPORT}/com.operasoftware.OperaGX", "dbFiles": "$chromium", "description": "Opera GX browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Arc", "basePath": "${APP_SUPPORT}/Arc/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Arc browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Chromium", "basePath": "${APP_SUPPORT}/Chromium", "dbFiles": "$chromium", "multiProfile": true, "description": "Chromium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Thorium", "basePath": "${APP_SUPPORT}/Thorium", "dbFiles": "$chromium", "multiProfile": true, "description": "Thorium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Supermium", "basePath": "${APP_SUPPORT}/Supermium", "dbFiles": "$chromium", "multiProfile": true, "description": "Supermium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Helium", "basePath": "${APP_SUPPORT}/net.imput.helium", "dbFiles": "$chromium", "multiProfile": true, "description": "Helium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Cromite", "basePath": "${APP_SUPPORT}/Cromite", "dbFiles": "$chromium", "multiProfile": true, "description": "Cromite browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Firefox", "basePath": "${APP_SUPPORT}/Firefox/Profiles", "dbFiles": "$firefox", "multiProfile": true, "profilePattern": ["*.default*", "*.dev-edition*"], "description": "Firefox browsing history, cookies, favicons, form data, and site preferences" },
    { "label": "Discord", "basePath": "${APP_SUPPORT}/discord", "dbFiles": ["Network/Cookies"], "description": "Discord network cookies database" },
    { "label": "Slack", "basePath": "${APP_SUPPORT}/Slack", "dbFiles": ["Network/Cookies"], "description": "Slack network cookies database" },
    { "label": "Microsoft Teams", "basePath": "${APP_SUPPORT}/Microsoft Teams", "dbFiles": ["Network/Cookies"], "description": "Microsoft Teams network cookies database" },
    { "label": "VS Code", "basePath": "${APP_SUPPORT}/Code", "dbFiles": ["Network/Cookies", "User/globalStorage/state.vscdb"], "description": "VS Code network cookies and global state database" },
    { "label": "Cursor IDE", "basePath": "${APP_SUPPORT}/Cursor", "dbFiles": ["Network/Cookies", "User/globalStorage/state.vscdb"], "description": "Cursor IDE network cookies and global state database" },
    { "label": "Figma", "basePath": "${APP_SUPPORT}/Figma", "dbFiles": ["Network/Cookies"], "description": "Figma network cookies database" },
    { "label": "Safari", "basePath": "${LIBRARY}/Safari", "dbFiles": ["History.db", "CloudTabs.db"], "description": "Safari browsing history and iCloud tabs databases" },
    { "label": "Thunderbird", "basePath": "${APP_SUPPORT}/Thunderbird/Profiles", "dbFiles": ["global-messages-db.sqlite", "places.sqlite", "cookies.sqlite"], "multiProfile": true, "profilePattern": ["*.default*"], "description": "Thunderbird global message index, places, and cookies databases" }
  ]
}"#;
const KUDU_WIN32: &str = r#"{
  "$schema": "../schema/rules.schema.json",
  "type": "databases",
  "sharedDbFileSets": {
    "chromium": ["History", "Cookies", "Network/Cookies", "Favicons", "Top Sites", "Web Data", "Shortcuts"],
    "firefox": ["places.sqlite", "cookies.sqlite", "favicons.sqlite", "formhistory.sqlite", "webappsstore.sqlite", "content-prefs.sqlite"]
  },
  "targets": [
    { "label": "Google Chrome", "basePath": "${LOCALAPPDATA}/Google/Chrome/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Chrome browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Microsoft Edge", "basePath": "${LOCALAPPDATA}/Microsoft/Edge/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Edge browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Brave", "basePath": "${LOCALAPPDATA}/BraveSoftware/Brave-Browser/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Brave browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Vivaldi", "basePath": "${LOCALAPPDATA}/Vivaldi/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Vivaldi browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Opera", "basePath": "${APPDATA}/Opera Software/Opera Stable", "dbFiles": "$chromium", "description": "Opera browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Opera GX", "basePath": "${APPDATA}/Opera Software/Opera GX Stable", "dbFiles": "$chromium", "description": "Opera GX browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Arc", "basePath": "${LOCALAPPDATA}/Arc/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Arc browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Chromium", "basePath": "${LOCALAPPDATA}/Chromium/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Chromium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Thorium", "basePath": "${LOCALAPPDATA}/Thorium/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Thorium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Supermium", "basePath": "${LOCALAPPDATA}/Supermium/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Supermium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Helium", "basePath": "${LOCALAPPDATA}/imput/Helium/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Helium browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Cromite", "basePath": "${LOCALAPPDATA}/Cromite/User Data", "dbFiles": "$chromium", "multiProfile": true, "description": "Cromite browsing history, cookies, favicons, autofill, and shortcut databases" },
    { "label": "Firefox", "basePath": "${APPDATA}/Mozilla/Firefox/Profiles", "dbFiles": "$firefox", "multiProfile": true, "profilePattern": ["*.default*", "*.dev-edition*"], "description": "Firefox browsing history, cookies, favicons, form data, and site preferences" },
    { "label": "Discord", "basePath": "${APPDATA}/discord", "dbFiles": ["Network/Cookies"], "description": "Discord network cookies database" },
    { "label": "Slack", "basePath": "${APPDATA}/Slack", "dbFiles": ["Network/Cookies"], "description": "Slack network cookies database" },
    { "label": "Microsoft Teams", "basePath": "${APPDATA}/Microsoft/Teams", "dbFiles": ["Network/Cookies"], "description": "Microsoft Teams network cookies database" },
    { "label": "VS Code", "basePath": "${APPDATA}/Code", "dbFiles": ["Network/Cookies", "User/globalStorage/state.vscdb"], "description": "VS Code network cookies and global state database" },
    { "label": "Cursor IDE", "basePath": "${APPDATA}/Cursor", "dbFiles": ["Network/Cookies", "User/globalStorage/state.vscdb"], "description": "Cursor IDE network cookies and global state database" },
    { "label": "Figma", "basePath": "${APPDATA}/Figma", "dbFiles": ["Network/Cookies"], "description": "Figma network cookies database" },
    { "label": "Postman", "basePath": "${APPDATA}/Postman", "dbFiles": ["Network/Cookies"], "description": "Postman network cookies database" },
    { "label": "Thunderbird", "basePath": "${APPDATA}/Thunderbird/Profiles", "dbFiles": ["global-messages-db.sqlite", "places.sqlite", "cookies.sqlite"], "multiProfile": true, "profilePattern": ["*.default*"], "description": "Thunderbird global message index, places, and cookies databases" }
  ]
}"#;

/// El catálogo que toca en este sistema.
fn catalogo_json() -> &'static str {
    match plataforma::so() {
        "macos" => KUDU_DARWIN,
        "windows" => KUDU_WIN32,
        _ => KUDU_LINUX,
    }
}

/* ── Definiciones (parseo PURO: se prueba sin tocar el disco) ─────────────── */

/// Una entrada del catálogo, ya con los conjuntos compartidos resueltos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definicion {
    /// El nombre de la aplicación, tal como lo escribe Kudu («Google Chrome»).
    pub label: String,
    /// La carpeta, todavía con variables (`${CONFIG}/google-chrome`).
    pub base_path: String,
    /// Los ficheros DENTRO de esa carpeta (o de cada perfil), ya sin `$conjuntos`.
    pub db_files: Vec<String>,
    /// Si la carpeta contiene un subdirectorio por perfil.
    pub multi_profile: bool,
    /// Qué subdirectorios son perfiles (`*.default*` en Firefox/Thunderbird).
    pub profile_pattern: Vec<String>,
    pub descripcion: String,
}

#[derive(Deserialize)]
struct JsonCatalogo {
    #[serde(default, rename = "sharedDbFileSets")]
    shared: HashMap<String, Vec<String>>,
    targets: Vec<JsonTarget>,
}

#[derive(Deserialize)]
struct JsonTarget {
    label: String,
    #[serde(rename = "basePath")]
    base_path: String,
    #[serde(rename = "dbFiles")]
    db_files: Ficheros,
    #[serde(default, rename = "multiProfile")]
    multi_profile: bool,
    #[serde(default, rename = "profilePattern")]
    profile_pattern: Vec<String>,
    #[serde(default)]
    description: String,
}

/// Kudu escribe `dbFiles` de las dos formas según la entrada: `"$chromium"` (un
/// conjunto, en una cadena) o `["Network/Cookies"]` (una lista). Se aceptan las
/// dos en vez de obligar al fichero a una, que es de Kudu y no se toca.
#[derive(Deserialize)]
#[serde(untagged)]
enum Ficheros {
    Uno(String),
    Varios(Vec<String>),
}

impl Ficheros {
    fn a_lista(&self) -> Vec<String> {
        match self {
            Ficheros::Uno(s) => vec![s.clone()],
            Ficheros::Varios(v) => v.clone(),
        }
    }
}

/// Lee un `databases.json` de Kudu y resuelve sus `$conjuntos` (`$chromium`,
/// `$firefox`). PURA: entra texto, sale la lista de objetivos, sin tocar el disco.
///
/// Un `$conjunto` que no exista es un error y no un hueco silencioso: si Kudu
/// añadiera uno y aquí no se resolviera, saldría una entrada sin ficheros y
/// parecería que esa aplicación no tiene bases.
pub fn parsear_catalogo(json: &str) -> Result<Vec<Definicion>, String> {
    let c: JsonCatalogo = serde_json::from_str(json)
        .map_err(|e| format!("el catálogo de bases de Kudu no se pudo leer: {e}"))?;
    let mut fuera = Vec::with_capacity(c.targets.len());
    for t in c.targets.iter() {
        let mut ficheros: Vec<String> = Vec::new();
        for f in t.db_files.a_lista().iter() {
            match f.strip_prefix('$') {
                Some(nombre) => {
                    let set = c.shared.get(nombre).ok_or_else(|| {
                        format!("«{}» usa el conjunto ${nombre}, que no está en el catálogo", t.label)
                    })?;
                    ficheros.extend(set.iter().cloned());
                }
                None => ficheros.push(f.clone()),
            }
        }
        fuera.push(Definicion {
            label: t.label.clone(),
            base_path: t.base_path.clone(),
            db_files: ficheros,
            multi_profile: t.multi_profile,
            profile_pattern: t.profile_pattern.clone(),
            descripcion: t.description.clone(),
        });
    }
    Ok(fuera)
}

/* ── Resolver las rutas ───────────────────────────────────────────────────── */

/// Traduce las variables de Kudu a las que `plataforma::expandir_plantilla` ya
/// sabe resolver.
///
/// Solo hace falta una: `${APP_SUPPORT}` es el nombre que usa Kudu en macOS y
/// vale exactamente lo mismo que `${CONFIG}` allí (en macOS `config_dir()` es
/// `~/Library/Application Support`, que es justo lo que dice el comentario de
/// `plataforma`, y es la única carpeta que Kudu llama `APP_SUPPORT`). No se añade
/// ninguna variable nueva a `plataforma` ni se compone ninguna ruta a mano.
fn traducir_variables(plantilla: &str) -> String {
    plantilla.replace("${APP_SUPPORT}", "${CONFIG}")
}

/// El primer `${VAR}` que queda sin resolver, si queda alguno.
///
/// Existe para poder DECIRLO en vez de construir una ruta con un `${HOME}` dentro
/// que no existe: una ruta así no encuentra nada y el usuario vería "no hay bases"
/// cuando el problema es que esa variable no se sabe traducir.
pub fn variable_sin_resolver(s: &str) -> Option<String> {
    let inicio = s.find("${")?;
    let resto = &s[inicio..];
    let fin = resto.find('}')?;
    Some(resto[..=fin].to_string())
}

/// La carpeta de una entrada del catálogo, ya expandida.
pub fn base_resuelta(d: &Definicion) -> Result<PathBuf, String> {
    let expandida = plataforma::expandir_plantilla(&traducir_variables(&d.base_path));
    if let Some(v) = variable_sin_resolver(&expandida) {
        return Err(format!(
            "«{}» no se puede resolver en {}: la variable {v} no se traduce aquí",
            d.label,
            plataforma::nombre_so()
        ));
    }
    Ok(PathBuf::from(expandida))
}

/// Los ficheros de esa entrada que EXISTEN, con el perfil del que salen.
///
/// Kudu da carpetas y los ficheros están dentro (`Network/Cookies`,
/// `User/globalStorage/state.vscdb`, `places.sqlite`…). Cuando la entrada es
/// multiperfil, se recorre un nivel: los subdirectorios que casa con
/// `profilePattern` (o todos, si no hay patrón), como «Default» o
/// «xxxx.default-release».
pub fn candidatos(d: &Definicion, base: &Path) -> Vec<(Option<String>, PathBuf)> {
    let mut fuera = Vec::new();
    if d.multi_profile {
        let Ok(it) = std::fs::read_dir(base) else {
            return fuera;
        };
        let mut perfiles: Vec<PathBuf> = it
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter(|p| d.profile_pattern.is_empty() || nombre_coincide(p, &d.profile_pattern))
            .collect();
        perfiles.sort();
        for perfil in perfiles {
            let nombre = perfil.file_name().map(|n| n.to_string_lossy().to_string());
            for f in d.db_files.iter() {
                let ruta = perfil.join(f);
                if ruta.is_file() {
                    fuera.push((nombre.clone(), ruta));
                }
            }
        }
    } else {
        for f in d.db_files.iter() {
            let ruta = base.join(f);
            if ruta.is_file() {
                fuera.push((None, ruta));
            }
        }
    }
    fuera
}

/// ¿El nombre de esta carpeta casa con alguno de los patrones de Kudu?
fn nombre_coincide(p: &Path, patrones: &[String]) -> bool {
    let Some(nombre) = p.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    patrones
        .iter()
        .any(|pat| glob::Pattern::new(pat).map(|g| g.matches(nombre)).unwrap_or(false))
}

/// Todas las rutas de base que el catálogo de este sistema resuelve a un fichero
/// que existe. Es la lista de lo que la aplicación puede llegar a compactar: se
/// usa como permiso, para que una ruta que no venga de aquí no se toque.
pub fn rutas_del_catalogo() -> HashSet<PathBuf> {
    let mut fuera = HashSet::new();
    let Ok(defs) = parsear_catalogo(catalogo_json()) else {
        return fuera;
    };
    for d in &defs {
        let Ok(base) = base_resuelta(d) else { continue };
        for (_, ruta) in candidatos(d, &base) {
            fuera.insert(ruta);
        }
    }
    fuera
}

/* ── Medir (SIEMPRE en solo lectura) ──────────────────────────────────────── */

/// Las banderas con las que se ABRE una base para medirla.
///
/// Solo lectura, y con prueba: si alguien cambia esto por descuido, hay un test
/// que falla y otro que comprueba que un `INSERT` por esta conexión da
/// `SQLITE_READONLY`. Medir el catálogo entero no puede escribir ni un byte.
pub const BANDERAS_LECTURA: OpenFlags = OpenFlags::from_bits_truncate(
    OpenFlags::SQLITE_OPEN_READ_ONLY.bits() | OpenFlags::SQLITE_OPEN_NO_MUTEX.bits(),
);

/// Las banderas de la conexión que SÍ compacta. `READ_WRITE` sin `CREATE`: si la
/// base no está, no se crea una vacía (sería absurdo "compactar" un fichero que
/// no existe, y además dejaría basura).
pub const BANDERAS_ESCRITURA: OpenFlags = OpenFlags::from_bits_truncate(
    OpenFlags::SQLITE_OPEN_READ_WRITE.bits() | OpenFlags::SQLITE_OPEN_NO_MUTEX.bits(),
);

/// Lo que dicen los PRAGMA de una base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Medida {
    /// `PRAGMA page_count`: páginas del fichero, ocupadas o no.
    pub paginas: u64,
    /// `PRAGMA page_size`: bytes de cada página.
    pub pagina_bytes: u64,
    /// `PRAGMA freelist_count`: páginas que no usa nadie.
    pub libres: u64,
    /// `PRAGMA auto_vacuum`, traducido (`ninguno`, `completo`, `incremental`).
    pub auto_vacuum: Option<String>,
    /// `PRAGMA journal_mode` (`wal`, `delete`, `truncate`…).
    pub journal: Option<String>,
}

impl Medida {
    /// Lo que ocupa la base según SQLite: `page_count × page_size`.
    pub fn bytes(&self) -> u64 {
        self.paginas.saturating_mul(self.pagina_bytes)
    }

    /// Lo que recuperaría un `VACUUM`: `freelist_count × page_size`.
    ///
    /// Es la cifra que interesa, y es de SQLite, no nuestra: esas páginas están
    /// dentro del fichero y no las usa nadie.
    pub fn recuperable(&self) -> u64 {
        self.libres.saturating_mul(self.pagina_bytes)
    }

    /// La frase corta del `auto_vacuum`, si está activo.
    pub fn nota_auto_vacuum(&self) -> Option<String> {
        match self.auto_vacuum.as_deref() {
            Some("completo") | Some("incremental") => {
                Some(format!("auto_vacuum {}", self.auto_vacuum.as_deref().unwrap_or("")))
            }
            _ => None,
        }
    }
}

/// Traduce el número del `PRAGMA auto_vacuum` a algo que se pueda leer.
/// `None` si SQLite devuelve un valor que no conocemos: mejor «—» que un nombre
/// inventado.
pub fn nombre_auto_vacuum(v: i64) -> Option<&'static str> {
    match v {
        0 => Some("ninguno"),
        1 => Some("completo"),
        2 => Some("incremental"),
        _ => None,
    }
}

/// Cómo salió una base (en el listado y al compactar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Estado {
    /// Se pudo leer: las cifras de esta fila son medidas.
    Ok,
    /// Otra aplicación la tiene abierta: no se ha medido ni se tocará.
    Bloqueada,
    /// No hay permiso para leer/escribir ese fichero.
    SinPermiso,
    /// Cualquier otro fallo (no es una base SQLite, está corrupta…).
    Error,
}

/// Por qué no se pudo medir o compactar una base.
#[derive(Debug, Clone)]
pub struct Fallo {
    pub estado: Estado,
    pub detalle: String,
}

/// Clasifica un código de error de SQLite. PURA, para poder probar los casos que
/// en la vida real hay que provocar (un bloqueo, un permiso).
pub fn clasificar(codigo: Option<ErrorCode>) -> Estado {
    match codigo {
        Some(ErrorCode::DatabaseBusy) | Some(ErrorCode::DatabaseLocked) | Some(ErrorCode::ReadOnly) => {
            Estado::Bloqueada
        }
        Some(ErrorCode::PermissionDenied) => Estado::SinPermiso,
        Some(ErrorCode::NotADatabase) => Estado::Error,
        _ => Estado::Error,
    }
}

fn fallo_de(e: &rusqlite::Error) -> Fallo {
    Fallo { estado: clasificar(e.sqlite_error_code()), detalle: e.to_string() }
}

fn pragma(conn: &Connection, sql: &str) -> rusqlite::Result<i64> {
    conn.query_row(sql, [], |f| f.get::<_, i64>(0))
}

fn pragma_texto(conn: &Connection, sql: &str) -> Option<String> {
    conn.query_row(sql, [], |f| f.get::<_, String>(0)).ok()
}

/// Mide una base **en solo lectura**. No escribe, no crea ficheros y no toma
/// bloqueos de escritura: si otra aplicación la tiene en exclusiva, esto falla
/// con `SQLITE_BUSY` y se devuelve `Bloqueada` con su motivo.
pub fn medir(ruta: &Path) -> Result<Medida, Fallo> {
    // Antes de abrir nada: distinguir "sin permiso" de "bloqueada" mirando el
    // fichero. Un `File::open` sí dice si se puede leer.
    if let Err(e) = std::fs::File::open(ruta) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return Err(Fallo {
                estado: Estado::SinPermiso,
                detalle: "no hay permiso para leer este fichero".to_string(),
            });
        }
    }
    let conn = Connection::open_with_flags(ruta, BANDERAS_LECTURA).map_err(|e| fallo_de(&e))?;
    let _ = conn.busy_timeout(ESPERA_BLOQUEO);

    let paginas = pragma(&conn, "PRAGMA page_count").map_err(|e| fallo_de(&e))?;
    let pagina_bytes = pragma(&conn, "PRAGMA page_size").map_err(|e| fallo_de(&e))?;
    let libres = pragma(&conn, "PRAGMA freelist_count").map_err(|e| fallo_de(&e))?;
    let auto_vacuum = pragma(&conn, "PRAGMA auto_vacuum")
        .ok()
        .and_then(nombre_auto_vacuum)
        .map(str::to_string);
    let journal = pragma_texto(&conn, "PRAGMA journal_mode");
    Ok(Medida {
        paginas: paginas.max(0) as u64,
        pagina_bytes: pagina_bytes.max(0) as u64,
        libres: libres.max(0) as u64,
        auto_vacuum,
        journal,
    })
}

/* ── Compactar (VACUUM): solo cuando lo pide el usuario ───────────────────── */

/// Compacta una base con `VACUUM`, con el bloqueo de escritura probado ANTES.
///
/// El orden importa: primero se comprueba que nadie la tiene en exclusiva
/// (`BEGIN IMMEDIATE`), y solo entonces se hace el `VACUUM`. Así, cuando otra
/// aplicación la tiene abierta, esto sale con `Bloqueada` **sin haber reescrito
/// nada**: el usuario cierra la aplicación y reintenta, y no se arriesga la base
/// de nadie por ir deprisa.
pub fn compactar(ruta: &Path) -> Result<(), Fallo> {
    let conn = Connection::open_with_flags(ruta, BANDERAS_ESCRITURA).map_err(|e| fallo_de(&e))?;
    let _ = conn.busy_timeout(ESPERA_BLOQUEO);
    // Prueba del bloqueo: toma el bloqueo reservado y lo suelta. `VACUUM` no se
    // puede ejecutar dentro de una transacción, así que la prueba va aparte.
    conn.execute_batch("BEGIN IMMEDIATE; COMMIT;").map_err(|e| fallo_de(&e))?;
    // `VACUUM` es atómico: si se corta a medias, SQLite deja la base como estaba.
    conn.execute_batch("VACUUM;").map_err(|e| fallo_de(&e))?;
    Ok(())
}

fn con_sufijo(p: &Path, sufijo: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(sufijo);
    PathBuf::from(s)
}

/// Lo que ocupa la base EN DISCO: el fichero más su `-wal` si lo hay.
///
/// El `-wal` lo gestiona SQLite y un `VACUUM` puede dejarlo a cero, así que
/// contarlo es lo único que hace honesto el "se han recuperado X": si no, se
/// estaría midiendo solo media historia.
pub fn en_disco(ruta: &Path) -> Option<u64> {
    let principal = std::fs::metadata(ruta).ok()?.len();
    let wal = std::fs::metadata(con_sufijo(ruta, "-wal")).map(|m| m.len()).unwrap_or(0);
    Some(principal + wal)
}

fn tamano_wal(ruta: &Path) -> u64 {
    std::fs::metadata(con_sufijo(ruta, "-wal")).map(|m| m.len()).unwrap_or(0)
}

/* ── Qué proceso la tiene abierta ─────────────────────────────────────────── */

/// Nombres de ejecutable con los que se reconoce la aplicación de una entrada de
/// Kudu. Se compara en minúsculas y EXACTO (un `contains` metería `codec` cuando
/// se busca `code`), así que solo van nombres que existen.
fn ejecutables_de(label: &str) -> Vec<String> {
    let nombres: &[&str] = match label {
        "Google Chrome" => &["chrome", "google-chrome"],
        "Microsoft Edge" => &["msedge"],
        "Brave" => &["brave", "brave-browser"],
        "Vivaldi" => &["vivaldi", "vivaldi-bin"],
        "Opera" | "Opera GX" => &["opera"],
        "Arc" => &["arc"],
        "Chromium" => &["chromium", "chromium-browser"],
        "Thorium" => &["thorium", "thorium-browser"],
        "Supermium" => &["supermium"],
        "Helium" => &["helium"],
        "Cromite" => &["cromite"],
        "Firefox" => &["firefox"],
        "Discord" => &["discord"],
        "Slack" => &["slack"],
        "Microsoft Teams" => &["teams", "ms-teams"],
        "VS Code" => &["code", "code-insiders"],
        "Cursor IDE" => &["cursor"],
        "Figma" => &["figma"],
        "Postman" => &["postman"],
        "Safari" => &["safari"],
        "Thunderbird" => &["thunderbird"],
        // Sin tabla conocida: se prueba con el propio nombre, en minúsculas. No se
        // inventa una lista de nombres parecidos.
        _ => &[],
    };
    if nombres.is_empty() {
        vec![label.to_lowercase()]
    } else {
        nombres.iter().map(|n| n.to_string()).collect()
    }
}

/// El primer argumento de una línea de comandos, sin la carpeta: `cmd` guarda la
/// línea completa y ahí el ejecutable viene con su ruta.
fn primer_argumento(cmd: &str) -> String {
    cmd.split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Los procesos que tienen abierta la base de esa aplicación, en texto para
/// enseñarlo: «chrome (pid 4321)». Un navegador abre decenas, así que a partir de
/// tres se agrupan: se dice cuántos son en vez de soltar una lista enorme.
///
/// PURA (recibe los procesos ya leídos): así se prueba sin tener que abrir de
/// verdad el navegador de nadie.
pub fn procesos_que_la_tienen(app: &str, procesos: &[plataforma::Proceso]) -> Vec<String> {
    let candidatos = ejecutables_de(app);
    let mut encontrados: Vec<String> = Vec::new();
    for p in procesos {
        let nombre = p.nombre.to_lowercase();
        let ejecutable = primer_argumento(&p.cmd).to_lowercase();
        if candidatos.iter().any(|c| *c == nombre || *c == ejecutable) {
            encontrados.push(format!("{} (pid {})", p.nombre, p.pid));
        }
    }
    encontrados.sort();
    encontrados.dedup();
    if encontrados.len() > 3 {
        let resto = encontrados.len() - 3;
        encontrados.truncate(3);
        encontrados.push(format!("y {resto} procesos más"));
    }
    encontrados
}

/* ── El listado que ve la interfaz ────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize)]
pub struct Base {
    /// La aplicación según Kudu («Google Chrome»): es el nombre que el usuario
    /// reconoce para saber qué tiene que cerrar.
    pub app: String,
    pub ruta: String,
    /// El perfil dentro de la carpeta (`Default`, `xxxx.default-release`), si la
    /// entrada es multiperfil.
    pub perfil: Option<String>,
    /// `PRAGMA page_count × page_size`. `null` si no se pudo medir (no es un 0).
    pub bytes: Option<u64>,
    /// `PRAGMA page_count`.
    pub paginas: Option<u64>,
    /// `PRAGMA page_size`.
    pub pagina_bytes: Option<u64>,
    /// `PRAGMA freelist_count`.
    pub libres: Option<u64>,
    /// `PRAGMA freelist_count × page_size`: lo que devolvería un `VACUUM`.
    pub recuperable: Option<u64>,
    /// Lo que ocupa el fichero en disco (más el `-wal`), medido con `stat`.
    pub disco: Option<u64>,
    /// Tamaño del `-wal` si existe: ese fichero lo gestiona SQLite y no se toca.
    pub wal_bytes: u64,
    /// `PRAGMA auto_vacuum`, traducido. `null` si no se pudo leer.
    pub auto_vacuum: Option<String>,
    /// `PRAGMA journal_mode`.
    pub journal: Option<String>,
    pub estado: Estado,
    /// Motivo cuando `estado` no es `ok` (bloqueada, sin permiso, error) o el
    /// aviso de `auto_vacuum` cuando sí lo es.
    pub nota: Option<String>,
    /// El comando equivalente para hacerlo a mano, como nota secundaria: la
    /// aplicación ya lo hace sola si se lo piden.
    pub comando: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listado {
    pub bases: Vec<Base>,
    /// Cuántas bases se han encontrado.
    pub total: usize,
    /// Cuántas se pudieron MEDIR: solo estas suman en `bytes_recuperables`.
    pub medidas: usize,
    pub bloqueadas: usize,
    pub sin_permiso: usize,
    pub errores: usize,
    /// Lo que se recuperaría con `VACUUM` en las bases medidas.
    pub bytes_recuperables: u64,
    /// Lo que ocupan las bases medidas (`page_count × page_size`).
    pub bytes_ocupados: u64,
    /// Lo que ocupan los `-wal` encontrados (los gestiona SQLite).
    pub wal_bytes: u64,
    pub ms: u64,
    /// El presupuesto se agotó: faltan bases por mirar y el total se queda corto.
    pub truncado: bool,
    pub sistema: String,
    /// Objetivos de Kudu que no se pudieron traducir en este sistema. Se enseña:
    /// un catálogo que encoge en silencio parece que no tiene nada que medir.
    pub sin_traducir: Vec<String>,
    /// Aviso del listado (presupuesto agotado, catálogo ilegible…).
    pub nota: Option<String>,
}

struct Presupuesto {
    bases: u64,
    inicio: Instant,
    agotado: bool,
}

impl Presupuesto {
    fn nuevo() -> Self {
        Self { bases: 0, inicio: Instant::now(), agotado: false }
    }
    /// Devuelve `false` cuando ya no queda presupuesto para esta base.
    fn gastar(&mut self) -> bool {
        self.bases += 1;
        if self.bases > MAX_BASES || self.inicio.elapsed() > MAX_TIEMPO {
            self.agotado = true;
            false
        } else {
            true
        }
    }
}

fn comillas(ruta: &Path) -> String {
    // Comillas simples de shell (POSIX), con el truco `'\''` para una comilla
    // dentro del nombre. La ruta la pone el sistema, no el usuario, pero un
    // comando que se copia y se pega tiene que poder ejecutarse tal cual.
    format!("'{}'", ruta.to_string_lossy().replace('\'', "'\\''"))
}

/// El comando equivalente para hacerlo a mano. Es la NOTA secundaria: la
/// aplicación ya hace el `VACUUM` ella misma.
pub fn comando_vacuum(ruta: &Path) -> String {
    format!("sqlite3 {} \"VACUUM;\"", comillas(ruta))
}

/// Una fila ya medida (o con su motivo, si no se pudo). El presupuesto se gasta
/// en el bucle que la llama: aquí no se vuelve a contar nada.
fn fila(app: &str, perfil: Option<&str>, ruta: &Path) -> Base {
    let disco = en_disco(ruta);
    let wal_bytes = tamano_wal(ruta);
    let comando = comando_vacuum(ruta);
    let mut base = Base {
        app: app.to_string(),
        ruta: ruta.to_string_lossy().to_string(),
        perfil: perfil.map(str::to_string),
        bytes: None,
        paginas: None,
        pagina_bytes: None,
        libres: None,
        recuperable: None,
        disco,
        wal_bytes,
        auto_vacuum: None,
        journal: None,
        estado: Estado::Ok,
        nota: None,
        comando,
    };
    match medir(ruta) {
        Ok(m) => {
            base.bytes = Some(m.bytes());
            base.paginas = Some(m.paginas);
            base.pagina_bytes = Some(m.pagina_bytes);
            base.libres = Some(m.libres);
            base.recuperable = Some(m.recuperable());
            base.auto_vacuum = m.auto_vacuum.clone();
            base.journal = m.journal.clone();
            base.nota = m.nota_auto_vacuum();
        }
        Err(f) => {
            base.estado = f.estado;
            base.nota = Some(match f.estado {
                Estado::Bloqueada => format!(
                    "«{app}» (u otro programa) la tiene abierta; ciérrala y vuelve a intentarlo: {detalle}",
                    detalle = f.detalle
                ),
                Estado::SinPermiso => format!("sin permiso para leerla: {}", f.detalle),
                _ => format!("no se pudo leer: {}", f.detalle),
            });
        }
    }
    base
}

fn listar_con(defs: &[Definicion]) -> Listado {
    let inicio = Instant::now();
    let mut pres = Presupuesto::nuevo();
    let mut bases: Vec<Base> = Vec::new();
    let mut sin_traducir: Vec<String> = Vec::new();

    'objetivos: for d in defs {
        let base = match base_resuelta(d) {
            Ok(b) => b,
            Err(_) => {
                sin_traducir.push(d.label.clone());
                continue;
            }
        };
        for (perfil, ruta) in candidatos(d, &base) {
            // Si el presupuesto se agotó, se corta el listado entero: media lista
            // con un total más bajo parecería "aquí no hay nada más que mirar".
            if !pres.gastar() {
                break 'objetivos;
            }
            bases.push(fila(&d.label, perfil.as_deref(), &ruta));
        }
    }

    // Lo que más se recuperaría, primero; a igualdad, por aplicación. Las que no
    // se pudieron medir van al final: no hay cifra que ordenar.
    bases.sort_by(|a, b| {
        b.recuperable
            .unwrap_or(0)
            .cmp(&a.recuperable.unwrap_or(0))
            .then_with(|| a.app.cmp(&b.app))
    });

    let medidas = bases.iter().filter(|b| b.estado == Estado::Ok).count();
    let bloqueadas = bases.iter().filter(|b| b.estado == Estado::Bloqueada).count();
    let sin_permiso = bases.iter().filter(|b| b.estado == Estado::SinPermiso).count();
    let errores = bases.iter().filter(|b| b.estado == Estado::Error).count();
    let bytes_recuperables = bases.iter().filter_map(|b| b.recuperable).sum();
    let bytes_ocupados = bases.iter().filter_map(|b| b.bytes).sum();
    let wal_bytes = bases.iter().map(|b| b.wal_bytes).sum();

    let nota = if pres.agotado {
        Some(format!(
            "El listado se cortó por presupuesto ({} bases): faltan carpetas por mirar, así que el total se queda corto.",
            MAX_BASES
        ))
    } else {
        None
    };

    Listado {
        total: bases.len(),
        bases,
        medidas,
        bloqueadas,
        sin_permiso,
        errores,
        bytes_recuperables,
        bytes_ocupados,
        wal_bytes,
        ms: inicio.elapsed().as_millis() as u64,
        truncado: pres.agotado,
        sistema: plataforma::nombre_so().to_string(),
        sin_traducir,
        nota,
    }
}

/// Mide todas las bases del catálogo de este sistema.
pub fn listar() -> Listado {
    match parsear_catalogo(catalogo_json()) {
        Ok(defs) => listar_con(&defs),
        Err(e) => Listado {
            bases: Vec::new(),
            total: 0,
            medidas: 0,
            bloqueadas: 0,
            sin_permiso: 0,
            errores: 0,
            bytes_recuperables: 0,
            bytes_ocupados: 0,
            wal_bytes: 0,
            ms: 0,
            truncado: false,
            sistema: plataforma::nombre_so().to_string(),
            sin_traducir: Vec::new(),
            nota: Some(e),
        },
    }
}

/* ── Compactar, con lo que hay que cerrar dicho ANTES ─────────────────────── */

/// Una base que el usuario ha pedido compactar. El `app` viaja con la ruta porque
/// es lo que permite decir «esto lo tiene abierto chrome» cuando falla.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Peticion {
    pub app: String,
    pub ruta: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseCompactada {
    pub app: String,
    pub ruta: String,
    pub ok: bool,
    pub estado: Estado,
    pub motivo: Option<String>,
    /// Los procesos que la tenían abierta, si estaba bloqueada.
    pub bloqueantes: Vec<String>,
    /// Lo recuperado DE VERDAD en disco (`stat` antes − después, `-wal` incluido).
    pub liberado: Option<u64>,
    /// Lo que dijo `PRAGMA freelist_count × page_size` ANTES de compactar.
    pub recuperable_antes: Option<u64>,
    /// Y lo que dice DESPUÉS (en una base compactada, cero).
    pub recuperable_despues: Option<u64>,
    /// `page_count × page_size` antes y después.
    pub bytes_antes: Option<u64>,
    pub bytes_despues: Option<u64>,
    /// Lo que ocupaba en disco antes y después (`-wal` incluido).
    pub disco_antes: Option<u64>,
    pub disco_despues: Option<u64>,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Compactacion {
    pub resultados: Vec<BaseCompactada>,
    pub liberado: u64,
    pub compactadas: usize,
    pub bloqueadas: usize,
    pub fallos: usize,
    pub ms: u64,
    pub mensaje: String,
}

/// Qué bases conviene intentar compactar de un listado: las que tienen páginas
/// libres medidas y las que salieron bloqueadas (para reintentar; si siguen
/// bloqueadas, se dirá qué las bloquea). PURA, con prueba.
pub fn candidatas_a_compactar(bases: &[Base]) -> Vec<Peticion> {
    bases
        .iter()
        .filter(|b| match b.estado {
            Estado::Bloqueada => true,
            Estado::Ok => b.recuperable.unwrap_or(0) > 0,
            _ => false,
        })
        .map(|b| Peticion { app: b.app.clone(), ruta: b.ruta.clone() })
        .collect()
}

/// Compacta las bases pedidas, pero SOLO si están en `permitidas`: el conjunto de
/// rutas que el catálogo de Kudu resuelve. Es el permiso que impide que una ruta
/// que no ha salido de aquí (un fichero cualquiera del equipo) se reescriba.
pub fn compactar_verificadas(peticiones: &[Peticion], permitidas: &HashSet<PathBuf>) -> Compactacion {
    let inicio = Instant::now();
    let mut resultados = Vec::with_capacity(peticiones.len());
    for p in peticiones {
        let ruta = PathBuf::from(&p.ruta);
        let arranque = Instant::now();
        if !permitidas.contains(&ruta) {
            resultados.push(BaseCompactada {
                app: p.app.clone(),
                ruta: p.ruta.clone(),
                ok: false,
                estado: Estado::Error,
                motivo: Some(
                    "no es una base del catálogo de Kudu: no se toca (por seguridad)".to_string(),
                ),
                bloqueantes: Vec::new(),
                liberado: None,
                recuperable_antes: None,
                recuperable_despues: None,
                bytes_antes: None,
                bytes_despues: None,
                disco_antes: None,
                disco_despues: None,
                ms: arranque.elapsed().as_millis() as u64,
            });
            continue;
        }

        let disco_antes = en_disco(&ruta);
        let antes = medir(&ruta).ok();
        let mut fila = BaseCompactada {
            app: p.app.clone(),
            ruta: p.ruta.clone(),
            ok: false,
            estado: Estado::Ok,
            motivo: None,
            bloqueantes: Vec::new(),
            liberado: None,
            recuperable_antes: antes.as_ref().map(Medida::recuperable),
            recuperable_despues: None,
            bytes_antes: antes.as_ref().map(Medida::bytes),
            bytes_despues: None,
            disco_antes,
            disco_despues: None,
            ms: 0,
        };

        match compactar(&ruta) {
            Ok(()) => {
                let despues = medir(&ruta).ok();
                let disco_despues = en_disco(&ruta);
                if let (Some(a), Some(d)) = (disco_antes, disco_despues) {
                    fila.liberado = Some(a.saturating_sub(d));
                }
                fila.ok = true;
                fila.recuperable_despues = despues.as_ref().map(Medida::recuperable);
                fila.bytes_despues = despues.as_ref().map(Medida::bytes);
                fila.disco_despues = disco_despues;
                fila.motivo = match (fila.recuperable_antes, fila.recuperable_despues) {
                    (Some(a), Some(d)) => Some(format!(
                        "páginas libres antes: {} · después: {}",
                        crate::almacen::legible(a),
                        crate::almacen::legible(d)
                    )),
                    _ => None,
                };
            }
            Err(f) => {
                fila.estado = f.estado;
                if f.estado == Estado::Bloqueada {
                    fila.bloqueantes = procesos_que_la_tienen(&p.app, &plataforma::procesos());
                }
                fila.motivo = Some(f.detalle);
            }
        }
        fila.ms = arranque.elapsed().as_millis() as u64;
        resultados.push(fila);
    }

    let liberado: u64 = resultados.iter().filter_map(|r| r.liberado).sum();
    let compactadas = resultados.iter().filter(|r| r.ok).count();
    let bloqueadas = resultados.iter().filter(|r| r.estado == Estado::Bloqueada).count();
    let fallos = resultados.len() - compactadas - bloqueadas;
    let mensaje = if resultados.is_empty() {
        "No hay ninguna base que compactar.".to_string()
    } else {
        let mut m = format!(
            "{} bases compactadas · se han recuperado {} de verdad",
            compactadas,
            crate::almacen::legible(liberado)
        );
        if bloqueadas > 0 {
            let quien: Vec<String> = resultados
                .iter()
                .filter(|r| r.estado == Estado::Bloqueada)
                .flat_map(|r| r.bloqueantes.iter().cloned())
                .take(3)
                .collect();
            m.push_str(&format!(". {} bloqueadas", bloqueadas));
            if !quien.is_empty() {
                m.push_str(&format!(" ({})", quien.join(", ")));
            }
            m.push_str(": cierra la aplicación y reintenta");
        }
        if fallos > 0 {
            m.push_str(&format!(". {fallos} no se pudieron compactar"));
        }
        m
    };

    Compactacion {
        resultados,
        liberado,
        compactadas,
        bloqueadas,
        fallos,
        ms: inicio.elapsed().as_millis() as u64,
        mensaje,
    }
}

/// Compacta lo que el usuario pide. Si la lista va vacía, se compactan las
/// candidatas del catálogo (`candidatas_a_compactar`): es lo que hace el botón
/// «compactar las que se puedan», y la regla vive aquí, con prueba, y no en la
/// interfaz.
pub fn compactar_lote(peticiones: &[Peticion]) -> Compactacion {
    let permitidas = rutas_del_catalogo();
    let peticiones: Vec<Peticion> = if peticiones.is_empty() {
        candidatas_a_compactar(&listar().bases)
    } else {
        peticiones.to_vec()
    };
    compactar_verificadas(&peticiones, &permitidas)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Carpeta de prueba propia de cada caso: `cargo test` corre en paralelo y,
    /// compartiendo carpeta, una prueba borraba lo que otra medía.
    fn temporal(caso: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("machinograph-bases-{caso}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Crea una base de verdad con espacio libre DENTRO (filas que se borran): las
    /// páginas quedan en la lista libre y eso es lo que mide `freelist_count`.
    fn base_con_huecos(ruta: &Path) -> Connection {
        let c = Connection::open(ruta).unwrap();
        // Journal clásico: en WAL el fichero no encoge igual y la prueba mediría
        // dos cosas a la vez.
        c.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE t (x BLOB);").unwrap();
        {
            let tx = c.unchecked_transaction().unwrap();
            {
                let mut st = tx.prepare("INSERT INTO t (x) VALUES (randomblob(4096))").unwrap();
                for _ in 0..400 {
                    st.execute([]).unwrap();
                }
            }
            tx.commit().unwrap();
        }
        c.execute_batch("DELETE FROM t;").unwrap();
        c
    }

    #[test]
    fn el_catalogo_de_kudu_se_parsea_y_resuelve_los_conjuntos() {
        let defs = parsear_catalogo(KUDU_LINUX).unwrap();
        assert_eq!(defs.len(), 17, "objetivos del catálogo de Linux");
        let chrome = defs.iter().find(|d| d.label == "Google Chrome").unwrap();
        assert_eq!(chrome.base_path, "${CONFIG}/google-chrome");
        assert_eq!(chrome.db_files.len(), 7, "$chromium tiene 7 ficheros");
        assert!(chrome.db_files.contains(&"Network/Cookies".to_string()));
        assert!(!chrome.db_files.contains(&"User/globalStorage/state.vscdb".to_string()));
        assert!(chrome.multi_profile);
        let firefox = defs.iter().find(|d| d.label == "Firefox").unwrap();
        assert_eq!(firefox.db_files.len(), 6, "$firefox tiene 6 ficheros");
        assert!(firefox.db_files.contains(&"places.sqlite".to_string()));
        assert_eq!(firefox.profile_pattern, vec!["*.default*".to_string(), "*.dev-edition*".to_string()]);
        // Las entradas que no son multiperfil no traen patrón, y las que llevan
        // ficheros concretos los llevan tal cual.
        let discord = defs.iter().find(|d| d.label == "Discord").unwrap();
        assert_eq!(discord.db_files, vec!["Network/Cookies".to_string()]);
        assert!(!discord.multi_profile);
        let code = defs.iter().find(|d| d.label == "VS Code").unwrap();
        assert_eq!(
            code.db_files,
            vec!["Network/Cookies".to_string(), "User/globalStorage/state.vscdb".to_string()]
        );
    }

    #[test]
    fn los_tres_catalogos_de_kudu_son_legibles() {
        for (so, json) in [("linux", KUDU_LINUX), ("macos", KUDU_DARWIN), ("windows", KUDU_WIN32)] {
            let defs = parsear_catalogo(json).unwrap_or_else(|e| panic!("{so}: {e}"));
            assert!(defs.len() >= 17, "{so} tiene {} objetivos", defs.len());
            for d in &defs {
                assert!(!d.label.is_empty(), "{so}: una entrada sin nombre");
                assert!(d.base_path.contains('/') || d.base_path.contains('\\'), "{so}: {}", d.base_path);
                assert!(!d.db_files.is_empty(), "{so}: «{}» sin ficheros", d.label);
                assert!(
                    !d.db_files.iter().any(|f| f.starts_with('$')),
                    "{so}: «{}» dejó un $conjunto sin resolver",
                    d.label
                );
                // Ninguna entrada puede llevar una variable que no conozcamos: si
                // Kudu añade una, esto lo dice en vez de enseñar un hueco.
                if let Ok(base) = base_resuelta(d) {
                    assert_eq!(variable_sin_resolver(&base.to_string_lossy()), None, "{so}: {}", d.label);
                } else {
                    // Solo se admite que no se traduzca si queda una variable: es
                    // lo que `base_resuelta` dice con su error.
                    assert!(variable_sin_resolver(&d.base_path).is_some(), "{so}: {}", d.label);
                }
            }
        }
    }

    #[test]
    fn un_conjunto_que_no_existe_es_un_error_y_no_un_hueco() {
        let roto = r#"{"targets":[{"label":"Inventada","basePath":"/x","dbFiles":["$inventado"]}]}"#;
        let e = parsear_catalogo(roto).unwrap_err();
        assert!(e.contains("$inventado"), "{e}");
    }

    #[test]
    fn las_variables_de_kudu_se_traducen_sin_inventar_rutas() {
        // `${APP_SUPPORT}` es el nombre de macOS y equivale a `${CONFIG}` allí.
        assert_eq!(traducir_variables("${APP_SUPPORT}/Code"), "${CONFIG}/Code");
        // Lo demás se deja tal cual: lo traduce `plataforma`.
        assert_eq!(traducir_variables("${HOME}/.mozilla/firefox"), "${HOME}/.mozilla/firefox");
        assert_eq!(variable_sin_resolver("${HOME}/x").as_deref(), Some("${HOME}"));
        assert_eq!(variable_sin_resolver("/tmp/sin-variables"), None);
        assert_eq!(variable_sin_resolver("a${UNA}b").as_deref(), Some("${UNA}"));

        // Un objetivo de este sistema se resuelve a una ruta SIN variables.
        let defs = parsear_catalogo(catalogo_json()).unwrap();
        for d in &defs {
            if let Ok(b) = base_resuelta(d) {
                assert_eq!(variable_sin_resolver(&b.to_string_lossy()), None, "{}", d.label);
            }
        }
    }

    #[test]
    fn el_perfil_se_encuentra_dentro_de_la_carpeta() {
        let dir = temporal("perfiles");
        let perfil = dir.join("Default");
        std::fs::create_dir_all(&perfil).unwrap();
        let base = base_con_huecos(&perfil.join("History"));
        base.execute_batch("DELETE FROM t;").unwrap();
        // Una carpeta que no es un perfil y un fichero que no toca.
        std::fs::create_dir_all(dir.join("Crashpad")).unwrap();
        std::fs::write(dir.join("sueltos.txt"), b"nada").unwrap();

        let d = Definicion {
            label: "Prueba".into(),
            base_path: dir.to_string_lossy().to_string(),
            db_files: vec!["History".into(), "Cookies".into()],
            multi_profile: true,
            profile_pattern: Vec::new(),
            descripcion: String::new(),
        };
        let c = candidatos(&d, &dir);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].0.as_deref(), Some("Default"));
        assert!(c[0].1.ends_with("Default/History"), "{:?}", c[0].1);

        // Con patrón, solo los perfiles que casan.
        let d2 = Definicion { profile_pattern: vec!["*.default*".into()], ..d.clone() };
        assert!(candidatos(&d2, &dir).is_empty(), "«Default» no casa con *.default*");

        // Y sin multiperfil, el fichero se busca en la carpeta base.
        let d3 = Definicion { multi_profile: false, ..d };
        assert!(candidatos(&d3, &dir).is_empty(), "History no está en la raíz");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_recuperable_es_lo_que_devuelve_el_vacuum() {
        let dir = temporal("vacuum");
        let ruta = dir.join("prueba.sqlite");
        let con = base_con_huecos(&ruta);

        let antes = medir(&ruta).expect("se mide en solo lectura");
        assert!(antes.libres > 0, "hacen falta páginas libres: {antes:?}");
        // La cifra es EXACTAMENTE la de SQLite, no una estimación nuestra.
        assert_eq!(antes.recuperable(), antes.libres * antes.pagina_bytes);
        assert_eq!(antes.bytes(), antes.paginas * antes.pagina_bytes);
        assert!(antes.recuperable() > 0);

        // Aquí SÍ se ejecuta el VACUUM, sobre una base de prueba creada por la
        // prueba: es la única forma de demostrar que la cifra era la de verdad.
        compactar(&ruta).expect("sin nadie usándola, se compacta");

        let despues = medir(&ruta).expect("se vuelve a medir");
        assert_eq!(despues.libres, 0, "VACUUM tiene que dejar la lista libre a cero");
        assert_eq!(despues.recuperable(), 0);
        assert!(
            despues.bytes() < antes.bytes(),
            "el fichero tenía que encoger: {} → {}",
            antes.bytes(),
            despues.bytes()
        );
        drop(con);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn una_base_bloqueada_se_reporta_como_bloqueada_y_no_se_compacta() {
        let dir = temporal("bloqueada");
        let ruta = dir.join("ocupada.sqlite");
        let dueno = base_con_huecos(&ruta);
        // El "otro programa": una conexión con el bloqueo exclusivo tomado.
        dueno.execute_batch("BEGIN EXCLUSIVE;").unwrap();

        let medido = medir(&ruta);
        let fallo = medido.expect_err("con la base en exclusiva no se puede medir");
        assert_eq!(fallo.estado, Estado::Bloqueada, "{fallo:?}");

        let libres_antes: i64 = dueno.query_row("PRAGMA freelist_count", [], |f| f.get(0)).unwrap();
        let filas_antes: i64 = dueno.query_row("SELECT count(*) FROM t", [], |f| f.get(0)).unwrap();

        // Y NO se compacta: el intento falla con "bloqueada" sin tocar nada.
        let fallo = compactar(&ruta).expect_err("no se puede compactar una base en uso");
        assert_eq!(fallo.estado, Estado::Bloqueada, "{fallo:?}");

        let libres_despues: i64 = dueno.query_row("PRAGMA freelist_count", [], |f| f.get(0)).unwrap();
        let filas_despues: i64 = dueno.query_row("SELECT count(*) FROM t", [], |f| f.get(0)).unwrap();
        assert_eq!(libres_antes, libres_despues, "no se ha compactado nada");
        assert_eq!(filas_antes, filas_despues);

        // Se suelta, y entonces sí: la base se compacta de verdad.
        dueno.execute_batch("ROLLBACK;").unwrap();
        compactar(&ruta).expect("ya se puede");
        let libres: i64 = medir(&ruta).unwrap().libres as i64;
        assert_eq!(libres, 0);
        drop(dueno);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_estado_bloqueado_se_clasifica_por_su_codigo() {
        assert_eq!(clasificar(Some(ErrorCode::DatabaseBusy)), Estado::Bloqueada);
        assert_eq!(clasificar(Some(ErrorCode::DatabaseLocked)), Estado::Bloqueada);
        assert_eq!(clasificar(Some(ErrorCode::ReadOnly)), Estado::Bloqueada);
        assert_eq!(clasificar(Some(ErrorCode::PermissionDenied)), Estado::SinPermiso);
        assert_eq!(clasificar(Some(ErrorCode::NotADatabase)), Estado::Error);
        assert_eq!(clasificar(None), Estado::Error);
    }

    #[test]
    fn las_banderas_de_medir_son_de_solo_lectura() {
        assert!(BANDERAS_LECTURA.contains(OpenFlags::SQLITE_OPEN_READ_ONLY));
        assert!(!BANDERAS_LECTURA.contains(OpenFlags::SQLITE_OPEN_READ_WRITE));
        assert!(!BANDERAS_LECTURA.contains(OpenFlags::SQLITE_OPEN_CREATE));
        // Y de verdad: por una conexión abierta así, escribir da SQLITE_READONLY.
        let dir = temporal("solo-lectura");
        let ruta = dir.join("b.sqlite");
        Connection::open(&ruta).unwrap().execute_batch("CREATE TABLE t (x);").unwrap();
        let ro = Connection::open_with_flags(&ruta, BANDERAS_LECTURA).unwrap();
        let e = ro.execute_batch("INSERT INTO t VALUES (1);").unwrap_err();
        assert_eq!(e.sqlite_error_code(), Some(ErrorCode::ReadOnly), "{e}");
        drop(ro);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_escritura_no_crea_bases_nuevas() {
        assert!(BANDERAS_ESCRITURA.contains(OpenFlags::SQLITE_OPEN_READ_WRITE));
        assert!(!BANDERAS_ESCRITURA.contains(OpenFlags::SQLITE_OPEN_CREATE));
        let dir = temporal("sin-crear");
        let inventada = dir.join("no-existe.sqlite");
        assert!(compactar(&inventada).is_err());
        assert!(!inventada.exists(), "no puede aparecer un fichero que no estaba");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_fichero_que_no_es_sqlite_no_es_un_cero() {
        let dir = temporal("no-sqlite");
        let ruta = dir.join("notas.txt");
        std::fs::write(&ruta, b"esto no es una base de datos").unwrap();
        let fallo = medir(&ruta).expect_err("un fichero de texto no es una base");
        assert_eq!(fallo.estado, Estado::Error, "{fallo:?}");
        // Y en el listado va con su estado, sin cifras: `None` no es 0.
        let b = fila("Prueba", None, &ruta);
        assert_eq!(b.estado, Estado::Error);
        assert_eq!(b.recuperable, None);
        assert_eq!(b.bytes, None);
        assert!(b.nota.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_comando_manual_lleva_la_ruta_citada() {
        assert_eq!(
            comando_vacuum(Path::new("/home/alguien/.config/google-chrome/Default/History")),
            "sqlite3 '/home/alguien/.config/google-chrome/Default/History' \"VACUUM;\""
        );
        // Un espacio y una comilla no pueden romper el comando al pegarlo.
        assert_eq!(
            comando_vacuum(Path::new("/tmp/con espacio/y'comilla.db")),
            "sqlite3 '/tmp/con espacio/y'\\''comilla.db' \"VACUUM;\""
        );
    }

    #[test]
    fn los_procesos_que_bloquean_se_encuentran_por_su_nombre() {
        let procs = vec![
            plataforma::Proceso {
                pid: 10,
                nombre: "chrome".into(),
                cmd: "/opt/google/chrome/chrome --type=renderer".into(),
                memoria_mb: 0.0,
                cpu_pct: 0.0,
                uptime_secs: 0,
            },
            plataforma::Proceso {
                pid: 11,
                nombre: "codec".into(),
                cmd: "codec".into(),
                memoria_mb: 0.0,
                cpu_pct: 0.0,
                uptime_secs: 0,
            },
            plataforma::Proceso {
                pid: 12,
                nombre: "firefox".into(),
                cmd: "/usr/lib/firefox/firefox".into(),
                memoria_mb: 0.0,
                cpu_pct: 0.0,
                uptime_secs: 0,
            },
        ];
        let c = procesos_que_la_tienen("Google Chrome", &procs);
        assert_eq!(c, vec!["chrome (pid 10)".to_string()], "«codec» no es «code»");
        assert_eq!(procesos_que_la_tienen("Firefox", &procs), vec!["firefox (pid 12)".to_string()]);
        assert!(procesos_que_la_tienen("Slack", &procs).is_empty());
    }

    #[test]
    fn solo_se_intentan_las_que_tienen_algo_que_recuperar() {
        let b = |estado: Estado, recuperable: Option<u64>, ruta: &str| Base {
            app: "App".into(),
            ruta: ruta.into(),
            perfil: None,
            bytes: Some(1),
            paginas: Some(1),
            pagina_bytes: Some(1),
            libres: Some(1),
            recuperable,
            disco: Some(1),
            wal_bytes: 0,
            auto_vacuum: None,
            journal: None,
            estado,
            nota: None,
            comando: String::new(),
        };
        let bases = vec![
            b(Estado::Ok, Some(4096), "/a"),
            b(Estado::Ok, Some(0), "/b"),
            b(Estado::Bloqueada, None, "/c"),
            b(Estado::SinPermiso, None, "/d"),
            b(Estado::Error, None, "/e"),
        ];
        let c = candidatas_a_compactar(&bases);
        assert_eq!(c.iter().map(|p| p.ruta.as_str()).collect::<Vec<_>>(), vec!["/a", "/c"]);
    }

    #[test]
    fn compactar_solo_toca_lo_que_viene_del_catalogo() {
        let dir = temporal("permiso");
        let ruta = dir.join("ajena.sqlite");
        let con = base_con_huecos(&ruta);
        let libres_antes: i64 = con.query_row("PRAGMA freelist_count", [], |f| f.get(0)).unwrap();
        assert!(libres_antes > 0);

        // Sin la ruta en el permiso (no sale del catálogo), no se toca.
        let peticion = Peticion { app: "Prueba".into(), ruta: ruta.to_string_lossy().to_string() };
        let inf = compactar_verificadas(std::slice::from_ref(&peticion), &HashSet::new());
        assert_eq!(inf.compactadas, 0);
        assert_eq!(inf.resultados[0].estado, Estado::Error);
        assert!(inf.resultados[0].motivo.as_deref().unwrap_or("").contains("catálogo"));
        let libres_sigue: i64 = con.query_row("PRAGMA freelist_count", [], |f| f.get(0)).unwrap();
        assert_eq!(libres_antes, libres_sigue, "no se ha compactado nada");

        // Con el permiso, sí: y lo recuperado se mide antes y después.
        let permitidas: HashSet<PathBuf> = HashSet::from([ruta.clone()]);
        let inf = compactar_verificadas(std::slice::from_ref(&peticion), &permitidas);
        assert_eq!(inf.compactadas, 1, "{:?}", inf.resultados[0]);
        assert_eq!(inf.bloqueadas, 0);
        assert!(inf.resultados[0].liberado.unwrap_or(0) > 0, "{:?}", inf.resultados[0]);
        assert_eq!(inf.resultados[0].recuperable_antes, Some(libres_antes as u64 * medir(&ruta).unwrap().pagina_bytes));
        assert_eq!(inf.resultados[0].recuperable_despues, Some(0));
        assert!(inf.mensaje.contains("se han recuperado"), "{}", inf.mensaje);
        drop(con);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_presupuesto_se_agota_y_lo_dice() {
        let mut p = Presupuesto::nuevo();
        for _ in 0..MAX_BASES {
            assert!(p.gastar(), "dentro del tope tiene que dejar pasar");
        }
        assert!(!p.gastar());
        assert!(p.agotado);
    }

    #[test]
    fn los_nombres_de_auto_vacuum_no_se_inventan() {
        assert_eq!(nombre_auto_vacuum(0), Some("ninguno"));
        assert_eq!(nombre_auto_vacuum(1), Some("completo"));
        assert_eq!(nombre_auto_vacuum(2), Some("incremental"));
        assert_eq!(nombre_auto_vacuum(7), None);
        let m = Medida {
            paginas: 10,
            pagina_bytes: 4096,
            libres: 2,
            auto_vacuum: Some("ninguno".into()),
            journal: Some("wal".into()),
        };
        assert_eq!(m.bytes(), 40_960);
        assert_eq!(m.recuperable(), 8_192);
        assert_eq!(m.nota_auto_vacuum(), None);
    }
}
