//! Programas que arrancan solos con la sesión, en los tres sistemas.
//!
//! Cada sistema tiene SU mecanismo, y ninguno se puede fingir con el de otro:
//!
//! | Sistema | Dónde vive | Cómo se desactiva |
//! | --- | --- | --- |
//! | Linux | `~/.config/autostart` y `/etc/xdg/autostart` (`.desktop`) | un fichero de usuario con `Hidden=true` que TAPA al del sistema, o `Hidden=true` en el propio fichero si es tuyo |
//! | macOS | `~/Library/LaunchAgents` y `/Library/LaunchAgents` (`.plist`) | `launchctl disable gui/<uid>/<label>` (la base de datos de overrides de launchd: **no se toca el fichero**) |
//! | Windows | `HKCU\...\CurrentVersion\Run` y `%APPDATA%\...\Startup` | se quita el valor de `Run` y se guarda aparte, para poder devolverlo tal cual |
//!
//! Reglas que se cumplen en los tres:
//!
//! 1. **Lo del sistema no se toca.** En Linux se tapa con un fichero propio que
//!    lleva la marca `# machinograph` (y solo se borra si la lleva). En macOS se usa la
//!    base de overrides de launchd, que es reversible. En Windows se escribe
//!    solo en `HKCU`.
//! 2. **Lo que se desactiva se puede volver a activar**, y se dice cómo.
//! 3. **Nada de red**: aquí no sale nada del equipo.
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Marca de los ficheros que crea Machinograph. Es lo que permite borrarlos después sin
/// miedo: si no la llevan, no son nuestros.
// Solo la implementación de Linux escribe ficheros de arranque (macOS usa launchd
// y Windows, el registro): fuera de Linux no la usa nadie, así que el aviso de
// código muerto se silencia A CONCIENCIA.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const MARCA: &str = "# machinograph";

#[derive(Debug, Clone, Serialize)]
pub struct Entrada {
    pub id: String,
    pub nombre: String,
    pub exec: String,
    pub comentario: Option<String>,
    pub ruta: String,
    /// "usuario" (se puede cambiar) o "sistema" (se tapa/bloquea, no se edita).
    pub origen: String,
    pub activo: bool,
    /// Quien lo escribió no quiere que se enseñe (`NoDisplay=true` / `Hidden`).
    pub oculta: bool,
}

/* ── Entrada pública ──────────────────────────────────────────────────────── */

pub fn listar() -> Vec<Entrada> {
    let mut v = listar_impl();
    v.sort_by(|a, b| {
        b.activo
            .cmp(&a.activo)
            .then_with(|| a.nombre.to_lowercase().cmp(&b.nombre.to_lowercase()))
    });
    v
}

pub fn activar(id: &str, activo: bool) -> Result<String, String> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") || id.starts_with('.') {
        return Err(format!("identificador de arranque no válido: {id}"));
    }
    activar_impl(id, activo)
}

/* ── Linux: XDG Autostart ─────────────────────────────────────────────────── */

#[cfg(target_os = "linux")]
fn dirs_arranque() -> Vec<(PathBuf, &'static str)> {
    let mut v: Vec<(PathBuf, &'static str)> = vec![(PathBuf::from("/etc/xdg/autostart"), "sistema")];
    if let Some(cfg) = dirs::config_dir() {
        v.push((cfg.join("autostart"), "usuario"));
    }
    v
}

#[cfg(target_os = "linux")]
fn listar_impl() -> Vec<Entrada> {
    let mut mapa: Vec<(String, Entrada)> = Vec::new();
    for (dir, origen) in dirs_arranque() {
        for f in ficheros_con(&dir, "desktop") {
            let Some(id) = f.file_stem().map(|s| s.to_string_lossy().to_string()) else {
                continue;
            };
            let Ok(contenido) = std::fs::read_to_string(&f) else {
                continue;
            };
            // Nuestro override no es una entrada: solo cambia el estado de la del
            // sistema que tapa.
            if contenido.starts_with(MARCA) {
                let o = parsear_desktop(&contenido);
                if let Some((_, e)) = mapa.iter_mut().find(|(k, _)| k == &id) {
                    e.activo = activo_desktop(&o);
                }
                continue;
            }
            let o = parsear_desktop(&contenido);
            let entrada = Entrada {
                id: id.clone(),
                nombre: o.nombre.clone().unwrap_or_else(|| id.clone()),
                exec: o.exec.clone().unwrap_or_default(),
                comentario: o.comentario.clone(),
                ruta: f.to_string_lossy().to_string(),
                origen: origen.to_string(),
                activo: activo_desktop(&o),
                oculta: o.no_display,
            };
            match mapa.iter_mut().find(|(k, _)| k == &id) {
                Some((_, e)) => *e = entrada,
                None => mapa.push((id, entrada)),
            }
        }
    }
    mapa.into_iter().map(|(_, e)| e).collect()
}

#[cfg(target_os = "linux")]
fn activar_impl(id: &str, activo: bool) -> Result<String, String> {
    let cfg = dirs::config_dir().ok_or("sin carpeta de configuración")?;
    let user_file = cfg.join("autostart").join(format!("{id}.desktop"));
    let sistema = Path::new("/etc/xdg/autostart").join(format!("{id}.desktop"));
    let es_nuestro = std::fs::read_to_string(&user_file)
        .map(|c| c.starts_with(MARCA))
        .unwrap_or(false);

    if user_file.exists() && es_nuestro {
        if activo {
            std::fs::remove_file(&user_file)
                .map_err(|e| format!("no se pudo quitar el bloqueo del arranque: {e}"))?;
            return Ok(nombre_de(&sistema).unwrap_or_else(|| id.to_string())
                + ": vuelve a arrancar con la sesión.");
        }
        escribir_seguro(&user_file, &override_hidden(&sistema, true))?;
        return Ok(nombre_de(&sistema).unwrap_or_else(|| id.to_string()) + ": desactivado (se puede reactivar).");
    }

    if user_file.exists() {
        let contenido = std::fs::read_to_string(&user_file)
            .map_err(|e| format!("no se pudo leer {}: {e}", user_file.display()))?;
        let nuevo = poner_hidden_desktop(&contenido, !activo);
        escribir_seguro(&user_file, &nuevo)?;
        let nombre = parsear_desktop(&nuevo).nombre.unwrap_or_else(|| id.to_string());
        return Ok(format!(
            "{nombre}: {}. El original quedó con copia al lado.",
            if activo { "activa otra vez" } else { "desactivada" }
        ));
    }

    if sistema.exists() {
        if activo {
            return Ok(nombre_de(&sistema).unwrap_or_else(|| id.to_string()) + ": ya arranca con la sesión.");
        }
        escribir_seguro(&user_file, &override_hidden(&sistema, true))?;
        return Ok(nombre_de(&sistema).unwrap_or_else(|| id.to_string())
            + ": desactivado con un bloqueo en tu carpeta (el del sistema no se toca).");
    }
    Err(format!("no existe ninguna entrada de arranque con ese nombre ({id})"))
}

/* ── Parsers y escritura de `.desktop` (puros: se prueban en cualquier sistema) ── */

#[derive(Default, Debug, Clone, PartialEq)]
// Solo lo produce y lo consume el parser de `.desktop` de Linux (aquí se compila
// y se prueba); en macOS y Windows no hay quien lo llame.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct OpcionesDesktop {
    pub nombre: Option<String>,
    pub exec: Option<String>,
    pub comentario: Option<String>,
    pub oculto: bool,
    pub gnome: Option<String>,
    pub no_display: bool,
}

/// Lee las claves que importan de la sección `[Desktop Entry]`.
///
/// Se ignoran las claves localizadas (`Name[es]`): se quiere el nombre base, que
/// existe siempre, y no depender del idioma de la sesión para el mismo dato.
// Solo lo usa la implementación de Linux (aquí se compila y se prueba): en macOS
// y Windows el arranque se lee de launchd y del registro.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parsear_desktop(contenido: &str) -> OpcionesDesktop {
    let mut o = OpcionesDesktop::default();
    let mut en_desktop = false;
    for linea in contenido.lines() {
        let l = linea.trim();
        if l.starts_with('[') {
            en_desktop = l == "[Desktop Entry]";
            continue;
        }
        if !en_desktop {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" if o.nombre.is_none() => o.nombre = Some(v.trim().to_string()),
            "Exec" if o.exec.is_none() => o.exec = Some(v.trim().to_string()),
            "Comment" if o.comentario.is_none() => o.comentario = Some(v.trim().to_string()),
            "Hidden" => o.oculto = v.trim().eq_ignore_ascii_case("true"),
            "X-GNOME-Autostart-enabled" => o.gnome = Some(v.trim().to_ascii_lowercase()),
            "NoDisplay" => o.no_display = v.trim().eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    o
}

// Solo lo usa la implementación de Linux (aquí se compila y se prueba): en macOS
// y Windows el estado lo dan launchd y el registro.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn activo_desktop(o: &OpcionesDesktop) -> bool {
    !o.oculto && o.gnome.as_deref() != Some("false")
}

/// Cambia la línea `Hidden=` de un `.desktop`, conservando todo lo demás.
// Solo lo usa la implementación de Linux (aquí se compila y se prueba): macOS
// desactiva con launchd y Windows, quitando el valor del registro.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn poner_hidden_desktop(contenido: &str, valor: bool) -> String {
    let mut lineas: Vec<String> = contenido.lines().map(str::to_string).collect();
    let mut en_desktop = false;
    let mut puesta = false;
    for l in lineas.iter_mut() {
        let t = l.trim();
        if t.starts_with('[') {
            en_desktop = t == "[Desktop Entry]";
            continue;
        }
        if en_desktop && !puesta {
            if let Some((k, _)) = t.split_once('=') {
                if k.trim().eq_ignore_ascii_case("Hidden") {
                    *l = format!("Hidden={valor}");
                    puesta = true;
                }
            }
        }
    }
    if !puesta {
        // Justo debajo de la cabecera de sección: el orden de claves no importa
        // en un .desktop, así que no hay que buscar el final de la sección.
        match lineas.iter().position(|l| l.trim() == "[Desktop Entry]") {
            Some(i) => lineas.insert(i + 1, format!("Hidden={valor}")),
            None => {
                lineas.insert(0, "[Desktop Entry]".to_string());
                lineas.insert(1, format!("Hidden={valor}"));
            }
        }
    }
    let mut s = lineas.join("\n");
    s.push('\n');
    s
}

/// El fichero de bloqueo que se deja en la carpeta de usuario para tapar una
/// entrada del sistema.
// Solo lo usa la implementación de Linux (aquí se compila y se prueba): el tapado
// con un fichero propio es la forma de XDG Autostart, no de macOS ni Windows.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn override_hidden(original: &Path, oculto: bool) -> String {
    let o = std::fs::read_to_string(original)
        .map(|c| parsear_desktop(&c))
        .unwrap_or_default();
    let mut s = String::from(MARCA);
    s.push('\n');
    s.push_str("[Desktop Entry]\nType=Application\n");
    if let Some(n) = o.nombre {
        s.push_str(&format!("Name={n}\n"));
    }
    if let Some(c) = o.comentario {
        s.push_str(&format!("Comment={c}\n"));
    }
    s.push_str(&format!("Hidden={oculto}\n"));
    if let Some(e) = o.exec {
        s.push_str(&format!("Exec={e}\n"));
    }
    s
}

#[cfg(target_os = "linux")]
fn nombre_de(p: &Path) -> Option<String> {
    let c = std::fs::read_to_string(p).ok()?;
    parsear_desktop(&c).nombre
}

/// Escribe con copia de seguridad y `rename` atómico (un corte no deja el
/// arranque a medias).
// Solo lo usa la implementación de Linux (aquí se compila y se prueba): es la que
// escribe ficheros `.desktop`; macOS y Windows no escriben ficheros aquí.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn escribir_seguro(destino: &Path, contenido: &str) -> Result<(), String> {
    if let Some(padre) = destino.parent() {
        std::fs::create_dir_all(padre).map_err(|e| format!("no se pudo crear {padre:?}: {e}"))?;
    }
    if destino.exists() {
        crate::copias::copia_de_seguridad(destino)?;
    }
    let temporal = PathBuf::from(format!("{}.machinograph-tmp", destino.to_string_lossy()));
    std::fs::write(&temporal, contenido).map_err(|e| format!("no se pudo escribir: {e}"))?;
    std::fs::rename(&temporal, destino).map_err(|e| format!("no se pudo reemplazar: {e}"))?;
    Ok(())
}

// Lo usan las implementaciones de Linux y macOS (aquí se compila y se prueba): en
// Windows el arranque vive en el registro y en la carpeta Startup, que se leen sin
// esta ayuda.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn ficheros_con(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(it) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = it
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == ext).unwrap_or(false))
        .collect();
    v.sort();
    v
}

/* ── macOS: LaunchAgents ──────────────────────────────────────────────────── */

/// Lo que se lee de un `launchd` job. Se obtiene con `plutil -convert json`, que
/// es la herramienta del propio macOS: así no hay que meter un parser de plist ni
/// interpretar XML a mano.
#[derive(Debug, Clone, PartialEq)]
// Se usa solo en macOS (aquí se compila y se prueba): en Linux y Windows no hay
// quien lo llame, así que el aviso de código muerto se silencia A CONCIENCIA.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct OpcionesPlist {
    pub label: Option<String>,
    pub programa: Option<String>,
    pub run_at_load: bool,
    pub disabled: bool,
}

/// Parser PURO del JSON que devuelve `plutil`. Se prueba en cualquier sistema.
// Se usa solo en macOS (aquí se compila y se prueba): en Linux y Windows no hay
// quien lo llame, así que el aviso de código muerto se silencia A CONCIENCIA.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parsear_plist_json(json: &str) -> Option<OpcionesPlist> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let obj = v.as_object()?;
    let programa = obj
        .get("ProgramArguments")
        .and_then(|a| a.as_array())
        .and_then(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().first().map(|s| s.to_string()))
        .or_else(|| obj.get("Program").and_then(|p| p.as_str()).map(|s| s.to_string()));
    Some(OpcionesPlist {
        label: obj.get("Label").and_then(|l| l.as_str()).map(|s| s.to_string()),
        programa,
        run_at_load: obj.get("RunAtLoad").and_then(|b| b.as_bool()).unwrap_or(false),
        disabled: obj.get("Disabled").and_then(|b| b.as_bool()).unwrap_or(false),
    })
}

#[cfg(target_os = "macos")]
fn dirs_launchd() -> Vec<(PathBuf, &'static str)> {
    let mut v: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push((home.join("Library").join("LaunchAgents"), "usuario"));
    }
    v.push((PathBuf::from("/Library/LaunchAgents"), "sistema"));
    v.push((PathBuf::from("/System/Library/LaunchAgents"), "sistema"));
    v
}

#[cfg(target_os = "macos")]
fn listar_impl() -> Vec<Entrada> {
    let mut v: Vec<Entrada> = Vec::new();
    let mut vistos: Vec<String> = Vec::new();
    for (dir, origen) in dirs_launchd() {
        for f in ficheros_con(&dir, "plist") {
            let id = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if id.is_empty() || vistos.contains(&id) {
                continue;
            }
            vistos.push(id.clone());
            let json = crate::proceso::ejecutar(
                "plutil",
                &["-convert".into(), "json".into(), "-o".into(), "-".into(), f.to_string_lossy().to_string()],
                &[],
                std::time::Duration::from_secs(5),
            )
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
            let o = parsear_plist_json(&json).unwrap_or(OpcionesPlist {
                label: Some(id.clone()),
                programa: None,
                run_at_load: true,
                disabled: false,
            });
            v.push(Entrada {
                id: id.clone(),
                nombre: o.label.clone().unwrap_or_else(|| id.clone()),
                exec: o.programa.clone().unwrap_or_default(),
                comentario: None,
                ruta: f.to_string_lossy().to_string(),
                origen: origen.to_string(),
                // Un LaunchAgent solo arranca con la sesión si pide `RunAtLoad` y
                // no está desactivado.
                activo: o.run_at_load && !o.disabled,
                oculta: false,
            });
        }
    }
    v
}

#[cfg(target_os = "macos")]
fn activar_impl(id: &str, activo: bool) -> Result<String, String> {
    // Se usa la base de overrides de launchd (`launchctl enable/disable`), que es
    // REVERSIBLE y no reescribe el fichero de nadie. El dominio `gui/<uid>` es el
    // de la sesión gráfica del usuario.
    let uid = String::from_utf8_lossy(
        &crate::proceso::ejecutar("id", &["-u".into()], &[], std::time::Duration::from_secs(3))
            .map_err(|e| format!("no se pudo saber tu uid: {e}"))?
            .stdout,
    )
    .trim()
    .to_string();
    if uid.is_empty() {
        return Err("no se pudo saber tu uid para hablar con launchd".into());
    }
    let verbo = if activo { "enable" } else { "disable" };
    let dominio = format!("gui/{uid}/{id}");
    let salida = crate::proceso::ejecutar(
        "launchctl",
        &[verbo.into(), dominio.clone()],
        &[],
        std::time::Duration::from_secs(10),
    )?;
    if !salida.status.success() {
        return Err(format!(
            "launchctl {verbo} {dominio} falló: {}",
            String::from_utf8_lossy(&salida.stderr).trim()
        ));
    }
    // Y se comprueba releyendo: el mismo criterio que en el resto de la app (no
    // basta con que el comando diga que fue bien).
    let ahora = listar().into_iter().find(|e| e.id == id);
    Ok(format!(
        "{id}: {}.{}",
        if activo { "vuelve a arrancar con la sesión" } else { "bloqueado en launchd (se puede reactivar)" },
        match ahora {
            Some(e) if e.activo == activo => " Comprobado releyendo.".to_string(),
            Some(e) => format!(" OJO: sigue {}.", if e.activo { "activo" } else { "inactivo" }),
            None => " Ya no aparece en la lista.".to_string(),
        }
    ))
}

/* ── Windows: claves Run ──────────────────────────────────────────────────── */

#[cfg(target_os = "windows")]
fn listar_impl() -> Vec<Entrada> {
    let mut v: Vec<Entrada> = Vec::new();
    // HKCU (usuario) y HKLM (sistema, solo lectura). El valor es «nombre → comando».
    for (raiz, origen) in [
        ("HKEY_CURRENT_USER", "usuario"),
        ("HKEY_LOCAL_MACHINE", "sistema"),
    ] {
        if let Ok(clave) = abrir_run(raiz) {
            for (nombre, cmd) in valores(&clave) {
                let id = limpiar_id(&nombre);
                v.push(Entrada {
                    id,
                    nombre,
                    exec: cmd,
                    comentario: None,
                    ruta: format!("{raiz}\\...\\CurrentVersion\\Run"),
                    origen: origen.to_string(),
                    activo: true,
                    oculta: false,
                });
            }
        }
    }
    v
}

#[cfg(target_os = "windows")]
fn activar_impl(id: &str, activo: bool) -> Result<String, String> {
    // Lo que se desactiva se GUARDA aparte para poder devolverlo tal cual: borrar
    // el valor y perder su comando sería irreversible.
    let run = abrir_run("HKEY_CURRENT_USER")?;
    let guardados = abrir_guardados()?;
    if activo {
        let algunos = valores(&guardados);
        let Some((nombre, cmd)) = algunos.into_iter().find(|(n, _)| limpiar_id(n) == id) else {
            return Err(format!("{id} no está desactivado por Machinograph"));
        };
        run.set_value(&nombre, &cmd).map_err(|e| format!("no se pudo reactivar: {e}"))?;
        let _ = guardados.delete_value(&nombre);
        Ok(format!("{nombre}: vuelve a arrancar con la sesión."))
    } else {
        let Some((nombre, cmd)) = valores(&run).into_iter().find(|(n, _)| limpiar_id(n) == id) else {
            return Err(format!("{id} no está en el arranque de tu usuario"));
        };
        guardados.set_value(&nombre, &cmd).map_err(|e| format!("no se pudo guardar para reactivar: {e}"))?;
        run.delete_value(&nombre).map_err(|e| format!("no se pudo quitar del arranque: {e}"))?;
        Ok(format!("{nombre}: desactivado (guardado para poder reactivarlo)."))
    }
}

/// Un identificador estable a partir del nombre del valor (que puede llevar
/// espacios, acentos y paréntesis).
#[cfg(target_os = "windows")]
fn limpiar_id(nombre: &str) -> String {
    nombre
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(target_os = "windows")]
fn abrir_run(raiz: &str) -> Result<winreg::RegKey, String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
    let (raiz, acceso) = match raiz {
        "HKEY_LOCAL_MACHINE" => (HKEY_LOCAL_MACHINE, KEY_READ),
        _ => (HKEY_CURRENT_USER, KEY_READ | KEY_WRITE),
    };
    winreg::RegKey::predef(raiz)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", acceso)
        .map_err(|e| format!("no se pudo abrir la clave de arranque: {e}"))
}

#[cfg(target_os = "windows")]
fn abrir_guardados() -> Result<winreg::RegKey, String> {
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    hkcu.create_subkey(r"Software\Machinograph\ArranqueDesactivado")
        .map(|(k, _)| k)
        .map_err(|e| format!("no se pudo abrir el almacén de desactivados: {e}"))
}

/// Los valores de una clave `Run`, como `(nombre, comando)`.
///
/// CÓMO SE DECODIFICA (esto lo pilló el cruce a Windows, no Linux): en `winreg`
/// 0.56 un valor es crudo (`RegValue { bytes, vtype }`) y no tiene `as_string()`;
/// hay que decodificarlo por su TIPO. Y solo se aceptan los tipos de TEXTO: un
/// `DWORD` o un binario no son una línea de arranque, así que se SALTAN en vez de
/// enseñar su representación cruda como si fuera un comando.
#[cfg(target_os = "windows")]
fn valores(clave: &winreg::RegKey) -> Vec<(String, String)> {
    use winreg::enums::{REG_EXPAND_SZ, REG_MULTI_SZ, REG_SZ};
    use winreg::types::FromRegValue;
    clave
        .enum_values()
        .flatten()
        .filter_map(|(n, v)| match v.vtype {
            REG_SZ | REG_EXPAND_SZ | REG_MULTI_SZ => {
                String::from_reg_value(&v).ok().map(|s| (n, s))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lee_las_claves_que_importan() {
        let c = "\
[Desktop Entry]
Type=Application
Name=Dispositivos de almacenamiento
Comment=Monta USB al conectar
Exec=kded5
X-GNOME-Autostart-enabled=true
NoDisplay=false
";
        let o = parsear_desktop(c);
        assert_eq!(o.nombre.as_deref(), Some("Dispositivos de almacenamiento"));
        assert_eq!(o.exec.as_deref(), Some("kded5"));
        assert!(activo_desktop(&o));
        assert!(!o.no_display);
    }

    #[test]
    fn hidden_y_gnome_desactivan() {
        let o = parsear_desktop("[Desktop Entry]\nName=X\nHidden=true\n");
        assert!(!activo_desktop(&o));
        let o2 = parsear_desktop("[Desktop Entry]\nName=X\nX-GNOME-Autostart-enabled=false\n");
        assert!(!activo_desktop(&o2));
        // Una sección que no es la de escritorio no cuenta.
        let o3 = parsear_desktop("[Desktop Action X]\nHidden=true\n");
        assert!(activo_desktop(&o3));
    }

    #[test]
    fn cambiar_hidden_conserva_el_resto_del_fichero() {
        let original = "\
[Desktop Entry]
Type=Application
# un comentario que no se puede perder
Name=Algo
Exec=/usr/bin/algo --flag
Icon=algo
";
        let nuevo = poner_hidden_desktop(original, true);
        assert!(nuevo.contains("Hidden=true"));
        assert!(nuevo.contains("# un comentario que no se puede perder"));
        assert!(nuevo.contains("Exec=/usr/bin/algo --flag"));
        assert!(nuevo.contains("Icon=algo"));
        // Y con Hidden ya puesto, se CAMBIA en el sitio (no se duplica).
        let nuevo2 = poner_hidden_desktop(&nuevo, false);
        assert_eq!(nuevo2.matches("Hidden=").count(), 1, "{nuevo2}");
        assert!(nuevo2.contains("Hidden=false"));
        assert!(nuevo2.contains("Name=Algo"));
    }

    #[test]
    fn el_override_lleva_la_marca_de_ai_hub() {
        let s = override_hidden(Path::new("/etc/xdg/autostart/no-existe.desktop"), true);
        assert!(s.starts_with(MARCA), "{s}");
        assert!(s.contains("Hidden=true"));
    }

    #[test]
    fn rechaza_identificadores_con_ruta() {
        for malo in ["../evil", "a/b", ".oculto", ""] {
            assert!(activar(malo, false).is_err(), "{malo} debería rechazarse");
        }
    }

    #[test]
    fn el_parser_de_plists_de_macos_lee_lo_que_hace_falta() {
        // Salida REAL de `plutil -convert json -o -` (documentada por Apple).
        let json = r#"{"Label":"com.ejemplo.respaldo","ProgramArguments":["/usr/local/bin/backup","--daily"],"RunAtLoad":true,"Disabled":false}"#;
        let o = parsear_plist_json(json).unwrap();
        assert_eq!(o.label.as_deref(), Some("com.ejemplo.respaldo"));
        assert_eq!(o.programa.as_deref(), Some("/usr/local/bin/backup"));
        assert!(o.run_at_load);
        assert!(!o.disabled);

        let json2 = r#"{"Label":"x","Program":"/bin/x","Disabled":true}"#;
        let o2 = parsear_plist_json(json2).unwrap();
        assert_eq!(o2.programa.as_deref(), Some("/bin/x"));
        assert!(!o2.run_at_load);
        assert!(o2.disabled);

        // Un JSON que no es un job no revienta: devuelve None.
        assert!(parsear_plist_json("no soy json").is_none());
        assert!(parsear_plist_json("[1,2,3]").is_none());
    }

    #[test]
    fn la_lista_de_este_equipo_se_lee_sin_romperse() {
        let lista = listar();
        for e in &lista {
            assert!(!e.id.is_empty());
            assert!(!e.nombre.is_empty());
            assert!(["usuario", "sistema"].contains(&e.origen.as_str()));
        }
    }
}
