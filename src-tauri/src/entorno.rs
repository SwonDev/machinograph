//! Lo del entorno de la aplicación que se configura desde Ajustes y no es ni un
//! motor ni una métrica: la dirección de red en la que se puede alcanzar la
//! puerta de enlace, el arranque automático y las carpetas de modelos.
//!
//! POR QUÉ ESTÁ APARTE: son tres cosas que no tienen nada que ver entre sí, pero
//! las tres comparten una propiedad: **se pueden comprobar antes de tocarlas**. La
//! dirección se lee del sistema (no se inventa), el arranque automático es un
//! fichero que se puede leer y borrar, y las carpetas son las que el inventario
//! recorre de verdad. Nada de esto se guarda en la base de datos: es el entorno.

use std::net::UdpSocket;
// `PathBuf` no se importa: en Windows no se usa (la ruta del plist y del .desktop
// solo existe en Linux/macOS) y un `use` sin usar sería un aviso en ese target.
use std::path::Path;

use serde::Serialize;

/* ── Dirección de red ─────────────────────────────────────────────────────── */

/// Una interfaz de red con su dirección IPv4.
#[derive(Debug, Clone, Serialize)]
pub struct Interfaz {
    pub nombre: String,
    pub ip: String,
}

/// La dirección IPv4 con la que este equipo sale a la red.
///
/// Cómo se obtiene, que no es adivinando: se abre un socket UDP y se le pide al
/// núcleo que elija la ruta hacia una dirección externa. **No se envía nada** (UDP
/// no conecta, solo fija la ruta en la tabla del núcleo), pero el núcleo sí dice
/// qué dirección LOCAL usaría. Es la forma de saberlo sin depender de `ip`,
/// `ifconfig` ni de un servicio web.
///
/// Devuelve `None` si no hay red: una máquina sin salida no tiene dirección con la
/// que ser alcanzada desde fuera, y eso es un dato, no un fallo.
pub fn ip_local() -> Option<String> {
    // 8.8.8.8 no se contacta: solo sirve para que el núcleo resuelva la ruta.
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    let addr = s.local_addr().ok()?;
    let ip = addr.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return None;
    }
    Some(ip.to_string())
}

/// Todas las interfaces IPv4 que no sean el lazo local, con su dirección.
///
/// DE DÓNDE SALEN, y por qué no de `/proc/net/fib_trie`: ese fichero publica el
/// árbol de PREFIJOS del núcleo (0.0.0.0/0, 192.168.0.0/24, 127.0.0.0/8…) y **no
/// dice a qué interfaz pertenece cada dirección**. La primera versión de esto lo
/// parseaba como si los prefijos fueran interfaces, y la pantalla acabó enseñando
/// «127.0.0.0/31 1 0 0 127.0.0.1» como si fuera una tarjeta de red. Se vio en la
/// app real, no en una prueba.
///
/// La fuente buena es `ip -4 -o addr show`, que sí da nombre y dirección juntos y
/// es la salida que usa todo el mundo para comprobarlo a mano. Va por el módulo de
/// procesos, con su límite de tiempo: si `ip` no está o se cuelga, esto devuelve
/// una lista vacía y la interfaz lo dice, en vez de quedarse esperando.
pub fn interfaces() -> Vec<Interfaz> {
    // Las da `plataforma` (sysinfo), que las ve en los tres sistemas. Antes esto
    // parseaba `ip -4 -o addr show`: en macOS y Windows ese binario no existe, así
    // que la lista salía vacía y la app no podía decir con qué dirección se llega
    // desde otro equipo. El motivo por el que NO se lee `/proc/net/fib_trie` está
    // arriba, y sigue valiendo: son prefijos, no interfaces.
    let mut out: Vec<Interfaz> = crate::plataforma::red()
        .into_iter()
        .filter_map(|i| {
            let ip = i.ipv4?;
            // El lazo local no sirve para que te alcancen desde fuera.
            (!ip.starts_with("127.")).then_some(Interfaz { nombre: i.nombre, ip })
        })
        .collect();
    out.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    out
}

/* ── Arranque automático ──────────────────────────────────────────────────── */

// El arranque automático de la PROPIA app. Cada sistema tiene su mecanismo y no se
// puede fingir con el de otro:
//
//   Linux   → un `.desktop` en `~/.config/autostart` (XDG Autostart). Activar es
//             escribir el fichero; desactivar es borrarlo, y no queda nada más.
//   macOS   → un LaunchAgent (`~/Library/LaunchAgents/<Label>.plist`) con
//             `RunAtLoad`, que launchd lee al entrar en la sesión. El `Label` es la
//             identidad del trabajo y `launchctl` lo carga en la sesión de ahora.
//   Windows → un valor en `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, que
//             es la clave donde el propio sistema mira qué lanzar al entrar. Se
//             escribe con `reg` (no se añade ninguna dependencia nueva).
//
// El CONTENIDO de cada uno (el `.desktop`, el plist y el dato del registro) lo
// generan funciones PURAS: es texto, así que se puede comprobar en Linux sin estar
// en macOS ni en Windows. Lo que no se puede comprobar aquí se DICE con su motivo
// (campo `error`), nunca se contesta «desactivado» por no haber podido mirar.
//
// La lista de los programas de arranque AJENOS vive en `plataforma::autoarranque`;
// aquí solo está lo de Machinograph.

/// El identificador de Machinograph donde el mecanismo exige uno: el `Label` del
/// LaunchAgent de macOS. En Linux es el nombre del fichero y en Windows el nombre
/// del valor `Run`.
#[cfg(any(test, target_os = "macos"))]
pub const ID_APP: &str = "dev.machinograph.panel";

/// El nombre del valor que Machinograph escribe en la clave `Run` de Windows. Es también
/// la marca que permite borrar SOLO lo nuestro.
#[cfg(any(test, target_os = "windows"))]
pub const NOMBRE_RUN: &str = "Machinograph";

/// La clave `Run` del usuario. Solo se escribe aquí: lo de la máquina entera
/// (`HKLM`) es de otros y no se toca.
#[cfg(any(test, target_os = "windows"))]
pub const CLAVE_RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// El contenido del `.desktop` XDG que arranca Machinograph en Linux. Función pura: se
/// prueba en cualquier sistema comparando el texto.
#[cfg(any(test, target_os = "linux"))]
pub fn contenido_desktop(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Machinograph\n\
         Comment=Panel de hardware y servidores de IA locales\n\
         Exec={}\nIcon=machinograph\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    )
}

/// El contenido del LaunchAgent de macOS. Función pura (se prueba en Linux).
///
/// `RunAtLoad` es lo que hace que launchd lo lance al entrar en la sesión; sin esa
/// clave el plist sería un fichero decorativo. La ruta se escapa como XML: una
/// carpeta puede llamarse `A&B` y un plist mal escapado no lo leería nadie.
#[cfg(any(test, target_os = "macos"))]
pub fn contenido_plist(exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key><string>{}</string>\n\
         \t<key>ProgramArguments</key><array><string>{}</string></array>\n\
         \t<key>RunAtLoad</key><true/>\n\
         </dict>\n</plist>\n",
        ID_APP,
        escapar_xml(&exe.to_string_lossy())
    )
}

/// Escapa un texto para que sea un valor XML válido. El `&` va el PRIMERO: si se
/// cambiara después, volvería a escapar los `&` que introducen los demás.
#[cfg(any(test, target_os = "macos"))]
pub fn escapar_xml(texto: &str) -> String {
    texto
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// El dato que se escribe en el valor `Run` de Windows. Se entrecomilla siempre:
/// una ruta con espacios sin comillas la partiría el propio Windows al lanzarla.
#[cfg(any(test, target_os = "windows"))]
pub fn valor_run(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Los argumentos de `reg` para activar o quitar el valor `Run` de Machinograph.
#[cfg(any(test, target_os = "windows"))]
pub fn args_reg_run(activar: bool, exe: &Path) -> Vec<String> {
    if activar {
        vec![
            "add".into(),
            CLAVE_RUN.into(),
            "/v".into(),
            NOMBRE_RUN.into(),
            "/t".into(),
            "REG_SZ".into(),
            "/d".into(),
            valor_run(exe),
            "/f".into(),
        ]
    } else {
        vec!["delete".into(), CLAVE_RUN.into(), "/v".into(), NOMBRE_RUN.into(), "/f".into()]
    }
}

/// Saca el dato de Machinograph de la salida de `reg query`. Función pura (se prueba en
/// Linux), que es lo que permite no depender de `winreg` ni de una dependencia.
///
/// La salida real de `reg query` es una cabecera con la clave y una línea por valor
/// con columnas separadas por espacios: `    Nombre    REG_SZ    <dato>`. Se busca
/// la línea que EMPIEZA por el nombre (tras el sangrado) y se toma lo que sigue al
/// tipo; así un valor ajeno con un nombre que empiece igual no se confunde.
#[cfg(any(test, target_os = "windows"))]
pub fn parsear_run(salida: &str, nombre: &str) -> Option<String> {
    for linea in salida.lines() {
        let texto = linea.trim_start();
        let Some(resto) = texto.strip_prefix(nombre) else {
            continue;
        };
        // Tiene que seguir un separador: si no, «AI» casaría con «Machinograph».
        if resto.is_empty() || !resto.starts_with(char::is_whitespace) {
            continue;
        }
        let mut campos = resto.trim_start().splitn(2, char::is_whitespace);
        campos.next()?; // el tipo (REG_SZ, REG_EXPAND_SZ…)
        return Some(campos.next().unwrap_or("").trim().to_string());
    }
    None
}

/// ¿Este valor de `Run` lo escribió Machinograph?
///
/// La marca es el NOMBRE del valor, que es lo que se reserva Machinograph. Si el nombre
/// no es el suyo, es de otro y no se toca. Si lo es pero el dato apunta a un
/// binario distinto (la app se movió, o alguien puso ese nombre por su cuenta), se
/// compara el nombre del fichero: si no es un `machinograph`, no es nuestro.
#[cfg(any(test, target_os = "windows"))]
pub fn run_es_nuestro(nombre: &str, datos: &str, exe: &Path) -> bool {
    if nombre != NOMBRE_RUN {
        return false;
    }
    let limpio = datos.trim().trim_matches('"').replace('/', "\\");
    let mio = exe.to_string_lossy().replace('/', "\\");
    if limpio.eq_ignore_ascii_case(&mio) {
        return true;
    }
    let guardado = limpio.rsplit('\\').next().unwrap_or("");
    let binario = mio.rsplit('\\').next().unwrap_or("");
    !guardado.is_empty()
        && guardado.eq_ignore_ascii_case(binario)
        && binario.to_ascii_lowercase().starts_with("machinograph")
}

/// ¿El plist que hay en nuestra ruta lo escribió Machinograph? Se decide por el `Label`,
/// que es el identificador que reservamos. Función pura (se prueba en Linux).
#[cfg(any(test, target_os = "macos"))]
pub fn plist_es_nuestro(json: &str) -> bool {
    crate::plataforma::autoarranque::parsear_plist_json(json)
        .and_then(|o| o.label)
        .as_deref()
        == Some(ID_APP)
}

/// Si está activado, de dónde se sabe y qué comando se lanzaría. `None` en el
/// comando significa que el fichero o la clave existen pero no llevan comando.
#[derive(Debug, Clone, Serialize)]
pub struct Arranque {
    pub activado: bool,
    /// La ruta del fichero (Linux, macOS) o la clave del registro (Windows). Se
    /// enseña siempre: una casilla que dice «activado» sin decir DÓNDE es
    /// imposible de comprobar o deshacer a mano.
    pub fichero: String,
    pub comando: Option<String>,
    /// El motivo por el que NO se pudo comprobar el estado. Cuando trae texto,
    /// `activado` NO significa «desactivado»: significa «no se pudo leer», que es
    /// un dato distinto y no se puede callar. Si va vacío, el estado sí se leyó.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn arranque_estado() -> Arranque {
    estado_impl()
}

/// Activa o desactiva el arranque automático.
///
/// El comando que se escribe es la ruta del binario EN MARCHA (`current_exe`), no
/// una ruta inventada: si la app se ha movido, el arranque apuntaría a la nada.
/// Y se escribe a un temporal + `rename` para que un corte no deje el lanzador a
/// medias que el sistema no sepa leer.
pub fn arranque_configurar(activar: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("no se pudo saber la ruta del binario: {e}"))?;
    configurar_impl(activar, &exe)
}

/* Linux: XDG Autostart. */

#[cfg(target_os = "linux")]
fn ruta_app() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("autostart").join("machinograph.desktop"))
}

#[cfg(target_os = "linux")]
fn estado_impl() -> Arranque {
    let ruta = ruta_app();
    let fichero = ruta.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let contenido = ruta.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let comando = contenido.as_deref().and_then(|c| {
        c.lines()
            .find_map(|l| l.strip_prefix("Exec="))
            .map(|l| l.trim().to_string())
    });
    Arranque { activado: contenido.is_some(), fichero, comando, error: None }
}

#[cfg(target_os = "linux")]
fn configurar_impl(activar: bool, exe: &Path) -> Result<String, String> {
    let ruta = ruta_app().ok_or("no se pudo resolver la carpeta de configuración")?;
    if !activar {
        if ruta.is_file() {
            std::fs::remove_file(&ruta).map_err(|e| format!("no se pudo borrar {}: {e}", ruta.display()))?;
            return Ok(format!("Arranque automático desactivado (borrado {}).", ruta.display()));
        }
        return Ok("El arranque automático ya estaba desactivado.".into());
    }
    crate::plataforma::autoarranque::escribir_seguro(&ruta, &contenido_desktop(exe))?;
    Ok(format!("Arranque automático activado: {} lanzará {}", ruta.display(), exe.display()))
}

/* macOS: LaunchAgent + launchctl. */

#[cfg(target_os = "macos")]
fn ruta_app() -> Option<std::path::PathBuf> {
    Some(dirs::home_dir()?.join("Library").join("LaunchAgents").join(format!("{ID_APP}.plist")))
}

/// El uid del usuario, que launchd necesita para el dominio `gui/<uid>`. Si no se
/// puede saber, se devuelve vacío y quien lo use lo dice.
#[cfg(target_os = "macos")]
fn uid() -> String {
    crate::proceso::ejecutar("id", &["-u".into()], &[], std::time::Duration::from_secs(3))
        .map(|s| String::from_utf8_lossy(&s.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Lee un plist con `plutil` (la herramienta del propio macOS) y devuelve su JSON.
/// Se interpreta después con el parser puro de la capa de plataforma; así no hay
/// que interpretar XML a mano. Si `plutil` no está o el fichero no se puede leer,
/// devuelve el motivo: no se inventa un estado.
#[cfg(target_os = "macos")]
fn leer_plist_json(ruta: &Path) -> Result<String, String> {
    let salida = crate::proceso::ejecutar(
        "plutil",
        &["-convert".into(), "json".into(), "-o".into(), "-".into(), ruta.to_string_lossy().to_string()],
        &[],
        std::time::Duration::from_secs(5),
    )?;
    if !salida.status.success() {
        return Err(format!(
            "plutil no pudo leer {}: {}",
            ruta.display(),
            String::from_utf8_lossy(&salida.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&salida.stdout).to_string())
}

#[cfg(target_os = "macos")]
fn estado_impl() -> Arranque {
    let Some(ruta) = ruta_app() else {
        return Arranque {
            activado: false,
            fichero: String::new(),
            comando: None,
            error: Some("no se pudo resolver tu carpeta de LaunchAgents".into()),
        };
    };
    let fichero = ruta.to_string_lossy().to_string();
    if !ruta.is_file() {
        return Arranque { activado: false, fichero, comando: None, error: None };
    }
    let json = match leer_plist_json(&ruta) {
        Ok(j) => j,
        Err(e) => return Arranque { activado: false, fichero, comando: None, error: Some(e) },
    };
    match crate::plataforma::autoarranque::parsear_plist_json(&json) {
        Some(o) => Arranque {
            activado: o.run_at_load && !o.disabled,
            fichero,
            comando: o.programa,
            error: None,
        },
        None => Arranque {
            activado: false,
            fichero,
            comando: None,
            error: Some(format!("{} existe pero no tiene la forma de un LaunchAgent", ruta.display())),
        },
    }
}

#[cfg(target_os = "macos")]
fn configurar_impl(activar: bool, exe: &Path) -> Result<String, String> {
    let ruta = ruta_app().ok_or("no se pudo resolver tu carpeta de LaunchAgents")?;
    // Lo que hay ahora en nuestra ruta: si no lleva nuestro Label, no es nuestro.
    let es_nuestro = ruta
        .is_file()
        .then(|| leer_plist_json(&ruta).ok())
        .flatten()
        .as_deref()
        .map(plist_es_nuestro)
        .unwrap_or(false);
    if !activar {
        if !ruta.is_file() {
            return Ok("El arranque automático ya estaba desactivado.".into());
        }
        if !es_nuestro {
            return Err(format!(
                "Hay un fichero en {} que no es de Machinograph (no lleva el Label «{ID_APP}»); no se toca.",
                ruta.display()
            ));
        }
        let uid = uid();
        let destino = format!("gui/{uid}/{ID_APP}");
        let aviso = match crate::proceso::ejecutar(
            "launchctl",
            &["bootout".into(), destino.clone()],
            &[],
            std::time::Duration::from_secs(5),
        ) {
            Ok(s) if s.status.success() => String::new(),
            Ok(s) => format!(
                " No se pudo descargar de launchd ({}); hasta cerrar sesión puede seguir cargado: ejecuta `launchctl bootout {destino}`.",
                String::from_utf8_lossy(&s.stderr).trim()
            ),
            Err(e) => format!(
                " No se pudo ejecutar launchctl ({e}); hasta cerrar sesión puede seguir cargado: ejecuta `launchctl bootout {destino}`."
            ),
        };
        std::fs::remove_file(&ruta).map_err(|e| format!("no se pudo borrar {}: {e}", ruta.display()))?;
        return Ok(format!("Arranque automático desactivado (borrado {}).{aviso}", ruta.display()));
    }
    if ruta.is_file() && !es_nuestro {
        return Err(format!(
            "Ya hay un fichero en {} que no es de Machinograph (no lleva el Label «{ID_APP}»); no se sobrescribe.",
            ruta.display()
        ));
    }
    crate::plataforma::autoarranque::escribir_seguro(&ruta, &contenido_plist(exe))?;
    let uid = uid();
    let aviso = match crate::proceso::ejecutar(
        "launchctl",
        &["bootstrap".into(), format!("gui/{uid}"), ruta.to_string_lossy().to_string()],
        &[],
        std::time::Duration::from_secs(5),
    ) {
        Ok(s) if s.status.success() => String::new(),
        Ok(s) => format!(
            " No se pudo cargar ahora con launchctl ({}); se cargará al iniciar sesión: `launchctl bootstrap gui/{uid} {}`.",
            String::from_utf8_lossy(&s.stderr).trim(),
            ruta.display()
        ),
        Err(e) => format!(
            " No se pudo ejecutar launchctl ({e}); se cargará al iniciar sesión: `launchctl bootstrap gui/{uid} {}`.",
            ruta.display()
        ),
    };
    Ok(format!("Arranque automático activado: {} lanzará {}.{aviso}", ruta.display(), exe.display()))
}

/* Windows: valor en la clave `Run` del usuario, con `reg`. */

#[cfg(target_os = "windows")]
fn estado_impl() -> Arranque {
    let fichero = CLAVE_RUN.to_string();
    match crate::proceso::ejecutar("reg", &["query".into(), CLAVE_RUN.into()], &[], std::time::Duration::from_secs(5)) {
        Ok(s) if s.status.success() => {
            match parsear_run(&String::from_utf8_lossy(&s.stdout), NOMBRE_RUN) {
                Some(datos) => Arranque { activado: true, fichero, comando: Some(datos), error: None },
                None => Arranque { activado: false, fichero, comando: None, error: None },
            }
        }
        // Si `reg` no pudo con la clave, no se sabe si está activado: se dice.
        Ok(s) => Arranque {
            activado: false,
            fichero,
            comando: None,
            error: Some(format!(
                "`reg query {CLAVE_RUN}` no pudo leer la clave: {}",
                String::from_utf8_lossy(&s.stderr).trim()
            )),
        },
        Err(e) => Arranque {
            activado: false,
            fichero,
            comando: None,
            error: Some(format!("no se pudo ejecutar `reg` para leer el arranque: {e}")),
        },
    }
}

#[cfg(target_os = "windows")]
fn configurar_impl(activar: bool, exe: &Path) -> Result<String, String> {
    if activar {
        let s = crate::proceso::ejecutar("reg", &args_reg_run(true, exe), &[], std::time::Duration::from_secs(10))?;
        if !s.status.success() {
            return Err(format!("`reg add` falló: {}", String::from_utf8_lossy(&s.stderr).trim()));
        }
        // No basta con que el comando diga que fue bien: se RELEE, como en el resto
        // de la app.
        match estado_impl().comando {
            Some(c) if c == valor_run(exe) => {}
            _ => {
                return Err(format!(
                    "`reg add` dijo que fue bien, pero al releer la clave no aparece «{NOMBRE_RUN}»; revísalo con `reg query {CLAVE_RUN}`."
                ))
            }
        }
        Ok(format!("Arranque automático activado: Windows lanzará {} ({CLAVE_RUN}).", exe.display()))
    } else {
        let estado = estado_impl();
        if let Some(e) = estado.error {
            return Err(format!("no se pudo leer el arranque para desactivarlo: {e}"));
        }
        match estado.comando {
            None => Ok("El arranque automático ya estaba desactivado.".into()),
            Some(datos) if run_es_nuestro(NOMBRE_RUN, &datos, exe) => {
                let s =
                    crate::proceso::ejecutar("reg", &args_reg_run(false, exe), &[], std::time::Duration::from_secs(10))?;
                if !s.status.success() {
                    return Err(format!("`reg delete` falló: {}", String::from_utf8_lossy(&s.stderr).trim()));
                }
                Ok(format!("Arranque automático desactivado (quitado «{NOMBRE_RUN}» de {CLAVE_RUN})."))
            }
            // El valor tiene NUESTRO nombre pero apunta a otra cosa: no es nuestro.
            Some(datos) => Err(format!(
                "El valor «{NOMBRE_RUN}» de {CLAVE_RUN} apunta a «{datos}», que no es este Machinograph; no se toca."
            )),
        }
    }
}

/* Cualquier otro sistema: se dice, en vez de fingir. */

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn estado_impl() -> Arranque {
    Arranque {
        activado: false,
        fichero: String::new(),
        comando: None,
        error: Some(format!(
            "este sistema ({}) no tiene un mecanismo de arranque automático que Machinograph sepa usar",
            crate::plataforma::nombre_so()
        )),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn configurar_impl(_activar: bool, _exe: &Path) -> Result<String, String> {
    Err(format!(
        "este sistema ({}) no tiene un mecanismo de arranque automático que Machinograph sepa usar",
        crate::plataforma::nombre_so()
    ))
}

/* ── Carpetas de modelos ──────────────────────────────────────────────────── */

/// Una carpeta que el inventario recorre.
#[derive(Debug, Clone, Serialize)]
pub struct CarpetaModelos {
    pub ruta: String,
    pub familia: String,
    /// Si existe en el equipo AHORA. Una carpeta que no existe no es un error (no
    /// todo el mundo tiene ComfyUI), pero se dice.
    pub existe: bool,
}

/// Las carpetas que el inventario mira de verdad.
///
/// No es una lista aparte: se le pide a `inventario::raices()` (que es la que usa
/// el escaneo), para que lo que se enseña aquí y lo que se recorre no puedan
/// discrepar. Si se añade una carpeta por `MACHINOGRAPH_MODEL_DIRS`, aparece sola.
pub fn carpetas_modelos() -> Vec<CarpetaModelos> {
    crate::inventario::raices_publicas()
        .into_iter()
        .map(|(ruta, familia)| CarpetaModelos {
            existe: ruta.is_dir(),
            ruta: ruta.to_string_lossy().to_string(),
            familia,
        })
        .collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// El truco del socket UDP solo sirve si devuelve una dirección de verdad y no
    /// el lazo local: si devolviera 127.0.0.1, la URL que se enseña para conectar
    /// desde otro equipo no valdría.
    #[test]
    fn la_ip_local_no_es_el_lazo() {
        if let Some(ip) = ip_local() {
            assert_ne!(ip, "127.0.0.1");
            assert!(ip.parse::<std::net::Ipv4Addr>().is_ok(), "«{ip}» no es una IP");
        }
        // Sin red, `None`: no es un fallo, es que no hay dirección con la que ser
        // alcanzado desde fuera.
    }

    /// Las interfaces tienen que ser interfaces DE VERDAD: su nombre tiene que
    /// existir en `/sys/class/net`, su dirección tiene que ser una IPv4 y no puede
    /// ser el lazo local.
    ///
    /// La comprobación del nombre es la que caza el fallo que se vio en la app
    /// real: la primera versión leía `/proc/net/fib_trie` (que son PREFIJOS, no
    /// interfaces) y la pantalla enseñaba «127.0.0.0/31» como si fuera una tarjeta
    /// de red. Con esto, un nombre inventado no pasa.
    #[test]
    fn las_interfaces_son_interfaces_de_verdad() {
        let reales: Vec<String> = std::fs::read_dir("/sys/class/net")
            .map(|d| {
                d.flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        for i in interfaces() {
            assert!(
                reales.contains(&i.nombre),
                "«{}» no es una interfaz de esta máquina (las que hay: {reales:?})",
                i.nombre
            );
            let ip: std::net::Ipv4Addr = i.ip.parse().unwrap_or_else(|_| panic!("«{}» no es una IP", i.ip));
            assert!(!ip.is_loopback(), "el lazo local no sirve para que te alcancen");
            assert!(!ip.is_broadcast(), "una dirección de difusión no es de nadie");
        }
    }

    /// El estado del arranque automático se lee del fichero, sin tocar nada: si no
    /// hay fichero, está desactivado.
    #[test]
    fn el_arranque_automatico_se_lee_del_fichero() {
        let a = arranque_estado();
        assert!(!a.fichero.is_empty());
        assert_eq!(a.activado, std::path::Path::new(&a.fichero).is_file());
        // En Linux el estado se puede leer siempre (es un fichero del usuario): si
        // hubiera un error, es que algo raro pasa y hay que verlo, no taparlo.
        assert!(a.error.is_none(), "no se pudo comprobar el arranque: {:?}", a.error);
        if a.activado {
            assert!(
                a.comando.is_some(),
                "un .desktop de arranque sin `Exec=` no arranca nada"
            );
        }
    }

    /// El `.desktop` que se escribe en Linux tiene que ser el mismo que el parser
    /// de la capa de plataforma reconoce como activo, y llevar la ruta del binario.
    /// Es una función pura: se comprueba aquí sin tocar el arranque de nadie.
    #[test]
    fn el_desktop_apunta_al_binario_y_se_lee() {
        let exe = Path::new("/opt/ai hub/machinograph");
        let c = contenido_desktop(exe);
        assert!(c.starts_with("[Desktop Entry]\n"));
        assert!(c.contains("Exec=/opt/ai hub/machinograph\n"));
        assert!(c.contains("X-GNOME-Autostart-enabled=true\n"));
        let o = crate::plataforma::autoarranque::parsear_desktop(&c);
        assert!(crate::plataforma::autoarranque::activo_desktop(&o));
        assert_eq!(o.exec.as_deref(), Some("/opt/ai hub/machinograph"));
    }

    /// El plist de macOS lleva lo que launchd necesita para lanzarlo: el `Label`,
    /// `RunAtLoad` y el binario como argumento. Sin `RunAtLoad` el fichero se
    /// quedaría decorativo.
    #[test]
    fn el_plist_de_macos_lleva_lo_que_launchd_necesita() {
        let c = contenido_plist(Path::new("/Applications/Machinograph.app/Contents/MacOS/machinograph"));
        assert!(c.contains("<key>Label</key>") && c.contains(ID_APP));
        assert!(c.contains("<key>RunAtLoad</key><true/>"));
        assert!(c.contains("<key>ProgramArguments</key>"));
        assert!(c.contains("/Applications/Machinograph.app/Contents/MacOS/machinograph"));
    }

    /// Una ruta con `&` (que en XML abre una entidad) tiene que salir escapada, o
    /// `plutil`/launchd no leerían el plist.
    #[test]
    fn el_xml_de_una_ruta_rara_se_escapa() {
        let c = contenido_plist(Path::new("/Users/a&b/machinograph"));
        assert!(c.contains("/Users/a&amp;b/machinograph"), "{c}");
        assert!(!c.contains("a&b"));
    }

    /// La decisión de si el valor del registro es nuestro: solo se quita lo que
    /// escribió Machinograph. Un valor con otro nombre, o con nuestro nombre apuntando a
    /// otro binario, no se toca.
    #[test]
    fn la_decision_de_quien_puso_el_valor_de_run() {
        let exe = Path::new(r"C:\Program Files\Machinograph\machinograph.exe");
        // Nuestro valor, tal cual lo escribiríamos.
        assert!(run_es_nuestro(NOMBRE_RUN, &valor_run(exe), exe));
        // La app se movió: la ruta guardada es vieja pero el binario es el mismo.
        assert!(run_es_nuestro(NOMBRE_RUN, "\"D:\\otra\\machinograph.exe\"", exe));
        // El nombre es el nuestro pero el binario es de otra cosa: no se toca.
        assert!(!run_es_nuestro(NOMBRE_RUN, "\"C:\\otro\\evil.exe\"", exe));
        // Otro nombre no es asunto nuestro.
        assert!(!run_es_nuestro("Otro programa", "\"C:\\otro\\machinograph.exe\"", exe));
        // Y sin dato (valor vacío) tampoco.
        assert!(!run_es_nuestro(NOMBRE_RUN, "", exe));
    }

    /// El parser de `reg query` saca el dato de nuestra línea y no el de otro
    /// valor cuyo nombre empiece igual. Se prueba con una salida con la forma real
    /// de `reg query`.
    #[test]
    fn el_parser_de_reg_saca_el_valor_y_nada_mas() {
        let real = "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    OneDrive    REG_SZ    \"C:\\Users\\a\\OneDrive.exe\" /background\r\n    Machinograph    REG_SZ    \"C:\\Program Files\\Machinograph\\machinograph.exe\"\r\n    AI Otra Cosa    REG_SZ    C:\\otro.exe\r\n";
        assert_eq!(
            parsear_run(real, NOMBRE_RUN).as_deref(),
            Some("\"C:\\Program Files\\Machinograph\\machinograph.exe\"")
        );
        assert_eq!(parsear_run(real, "NoExiste"), None);
        // Un valor con la clave ausente: la salida no trae la línea y no se inventa.
        let error = "ERROR: The system was unable to find the specified registry key or value.\r\n";
        assert_eq!(parsear_run(error, NOMBRE_RUN), None);
    }

    /// Los argumentos de `reg` que se escriben y se borran: el valor lleva nuestro
    /// nombre y el binario entrecomillado.
    #[test]
    fn los_argumentos_de_reg_escriben_y_borran_lo_nuestro() {
        let exe = Path::new(r"C:\Program Files\Machinograph\machinograph.exe");
        let add = args_reg_run(true, exe);
        assert_eq!(add[0], "add");
        assert!(add.contains(&"/v".to_string()) && add.contains(&NOMBRE_RUN.to_string()));
        assert!(add.contains(&valor_run(exe)));
        let del = args_reg_run(false, exe);
        assert_eq!(del[0], "delete");
        assert!(del.contains(&NOMBRE_RUN.to_string()));
    }

    /// Un plist solo es nuestro si lleva nuestro `Label`: así no se borra ni se
    /// sobrescribe el fichero de otra cosa que estuviera en esa ruta.
    #[test]
    fn el_plist_solo_es_nuestro_con_nuestro_label() {
        let nuestro = format!(r#"{{"Label":"{ID_APP}","RunAtLoad":true}}"#);
        assert!(plist_es_nuestro(&nuestro));
        assert!(!plist_es_nuestro(r#"{"Label":"com.otro.app","RunAtLoad":true}"#));
        assert!(!plist_es_nuestro("no soy json"));
    }

    /// Y las carpetas de modelos son las del inventario: la misma lista que se
    /// recorre, para que no puedan discrepar.
    #[test]
    fn las_carpetas_son_las_del_inventario() {
        let carpetas = carpetas_modelos();
        assert!(!carpetas.is_empty(), "el inventario mira al menos una carpeta");
        assert!(
            carpetas.iter().any(|c| c.existe),
            "al menos una tiene que existir en este equipo"
        );
        assert!(carpetas.iter().all(|c| !c.ruta.is_empty() && !c.familia.is_empty()));
    }
}
