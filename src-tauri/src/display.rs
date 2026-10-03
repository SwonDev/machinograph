//! Pantallas: consulta de salidas/modos y cambios de resolución y estado.
//!
//! OJO con el backend, que es la trampa de este módulo: `xrandr` **solo** vale
//! en una sesión X11. En Wayland, XWayland expone una pantalla sintética, así
//! que `xrandr` devuelve datos que no son los del monitor de verdad (en KDE con
//! escala fraccionaria: un solo modo, todas las frecuencias iguales e ignorando
//! la escala) y, peor, sus cambios no afectan a la pantalla real: el comando
//! termina con éxito y no pasa absolutamente nada. Es decir, la interfaz decía
//! "aplicado" sin haber aplicado nada.
//!
//! Por eso la herramienta se elige según la sesión:
//!   * KDE Plasma en Wayland -> `kscreen-doctor` (habla por D-Bus con KWin, es
//!     el que manda de verdad)
//!   * X11                    -> `xrandr`
//!   * otro Wayland           -> no se inventa nada: se devuelve un error
//!     explicando que esta sesión no está soportada (soportar wlroots exigiría
//!     `wlr-randr`, que aquí no se puede probar, y código sin probar no entra).
// En macOS y Windows este fichero solo usa la lectura por plataforma: los parsers
// de `kscreen-doctor` y `xrandr` se quedan compilados (para que el código sea el
// mismo en los tres sistemas y no se bifurque el fichero) pero sin uso, así que el
// aviso de código muerto se silencia A CONCIENCIA aquí y no en cada función.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::types::{DisplayOutput, Mode};
use std::sync::LazyLock;
use std::process::Command;
use parking_lot::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Backend {
    Kde,
    Xrandr,
}

impl Backend {
    fn nombre(self) -> &'static str {
        match self {
            Backend::Kde => "kscreen-doctor",
            Backend::Xrandr => "xrandr",
        }
    }
}

fn existe(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

/// Backend de esta sesión. La sesión no cambia mientras la app está abierta, así
/// que se decide una sola vez (esto también evita lanzar `which` cada 2 s).
static BACKEND: LazyLock<Result<Backend, String>> = LazyLock::new(|| {
    let es_wayland = std::env::var("XDG_SESSION_TYPE")
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some();

    if es_wayland {
        let kde = std::env::var("XDG_CURRENT_DESKTOP")
            .map(|v| v.to_uppercase().contains("KDE"))
            .unwrap_or(false)
            || std::env::var_os("KDE_FULL_SESSION").is_some();
        if kde && existe("kscreen-doctor") {
            return Ok(Backend::Kde);
        }
        return Err(
            "Sesión Wayland sin backend soportado: se necesita kscreen-doctor (KDE) para leer \
             y cambiar las pantallas; con xrandr los datos serían de XWayland, no del monitor."
                .to_string(),
        );
    }

    if existe("xrandr") {
        Ok(Backend::Xrandr)
    } else {
        Err("No se encuentra xrandr (sesión X11 sin xrandr instalado).".to_string())
    }
});

/// En macOS y Windows no hay backend para CAMBIAR el modo, y se dice con el motivo
/// de verdad (no «no encuentro xrandr», que en un Mac no significa nada).
#[cfg(target_os = "linux")]
fn backend() -> Result<Backend, String> {
    match &*BACKEND {
        Ok(b) => Ok(*b),
        Err(e) => Err(e.clone()),
    }
}

#[cfg(not(target_os = "linux"))]
fn backend() -> Result<Backend, String> {
    Err(crate::plataforma::pantalla::motivo_sin_cambio())
}

pub fn query() -> Result<Vec<DisplayOutput>, String> {
    query_impl()
}

#[cfg(target_os = "linux")]
fn query_impl() -> Result<Vec<DisplayOutput>, String> {
    match backend()? {
        Backend::Kde => query_kde(),
        Backend::Xrandr => query_xrandr(),
    }
}

/// Convierte una salida leída del SISTEMA (macOS o Windows) al tipo del panel.
///
/// PURA A PROPÓSITO: en Linux `query_impl` no pasa por aquí (allí mandan
/// `kscreen-doctor` y `xrandr`), así que sin esta función la conversión de la
/// lectura por plataforma se quedaría sin una sola prueba hasta tener un Mac o un
/// Windows delante. Se prueba abajo con la misma forma que devuelven los parsers
/// de `plataforma::pantalla` (ya probados con salidas reales de `system_profiler`
/// y de WMI).
// Se usa solo fuera de Linux (aquí se compila y se prueba): en Linux no hay quien
// la llame, así que el aviso de código muerto se silencia A CONCIENCIA.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn desde_salida(s: crate::plataforma::pantalla::SalidaBasica) -> DisplayOutput {
    DisplayOutput {
        name: s.nombre,
        // Un ancho de 0 no es una pantalla encendida: `plataforma::pantalla` lo
        // publica así cuando el adaptador existe pero no tiene pantalla, y la
        // interfaz lo pinta como «—» en vez de inventarse una resolución.
        status: if s.ancho > 0 { "enabled".to_string() } else { "unknown".to_string() },
        connected: s.ancho > 0,
        primary: s.principal,
        w: s.ancho,
        h: s.alto,
        hz: s.hz,
        offset_x: 0,
        offset_y: 0,
        // Los modos disponibles no los publica ninguna de las dos herramientas:
        // se devuelve la lista vacía y la interfaz dice que no se pueden listar.
        modes: Vec::new(),
        current_flags: String::new(),
    }
}

/// En macOS y Windows se lee lo que publica el sistema (ver
/// `plataforma::pantalla`), pero NO se puede cambiar el modo: ninguno de los dos
/// expone una forma soportada de hacerlo desde fuera de su API nativa. Se dice en
/// la nota de la sección, y las acciones de cambiar modo fallan con ese motivo en
/// vez de fingir que lo han hecho.
#[cfg(not(target_os = "linux"))]
fn query_impl() -> Result<Vec<DisplayOutput>, String> {
    Ok(crate::plataforma::pantalla::salidas()
        .into_iter()
        .map(desde_salida)
        .collect())
}

/* ── Lectura para la FOTO (con caché) ─────────────────────────────────────── */

/// Caché de la lista de salidas para la foto.
///
/// POR QUÉ: `query()` lanza `kscreen-doctor` (o `xrandr`) cada vez, y la foto la
/// pedía cada 2 s sin que nada hubiera cambiado. Se reutiliza unos segundos, y se
/// invalida en cuanto el usuario aplica un modo o conmuta una salida, así que la
/// vista de Pantallas nunca enseña algo viejo después de una acción. Las acciones
/// siguen leyendo con `query()`, que no pasa por aquí y siempre está fresco.
static CACHE: LazyLock<Mutex<Option<(Instant, Result<Vec<DisplayOutput>, String>)>>> =
    LazyLock::new(|| Mutex::new(None));
const TTL: Duration = Duration::from_secs(5);

pub fn query_para_foto() -> Result<Vec<DisplayOutput>, String> {
    {
        let g = CACHE.lock();
        if let Some((cuando, v)) = g.as_ref() {
            if cuando.elapsed() < TTL {
                return v.clone();
            }
        }
    }
    let r = query();
    let mut g = CACHE.lock();
    *g = Some((Instant::now(), r.clone()));
    r
}

/// Tira la caché de la foto: se llama SIEMPRE que una acción toca la
/// configuración de pantalla, haya salido bien o mal (si salió a medias, el
/// estado también ha dejado de valer).
pub fn invalidar_cache() {
    { let mut g = CACHE.lock();
        *g = None;
    }
}

/* ── KDE (kscreen-doctor) ─────────────────────────────────────────────────── */

/// Quita las secuencias de escape ANSI: `kscreen-doctor` colorea su salida y
/// los códigos se comen los prefijos que hay que reconocer (`Modes:`, `Scale:`…).
fn sin_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.next() == Some('[') {
                // Secuencia CSI: termina en una letra.
                for c2 in it.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Separa la parte numérica de una marca de modo: `3840x2160@144.01*!` ->
/// (`3840x2160@144.01`, `*!`). `*` = modo actual, `!` = preferido.
fn partir_flags(raw: &str) -> (&str, String) {
    let idx = raw
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'x' || c == '@'))
        .unwrap_or(raw.len());
    let (num, flags) = raw.split_at(idx);
    (num, flags.to_string())
}

fn query_kde() -> Result<Vec<DisplayOutput>, String> {
    let out = crate::proceso::ejecutar(
        "kscreen-doctor",
        &["-o".to_string()],
        &[],
        Duration::from_secs(5),
    )?;
    if !out.status.success() {
        return Err(format!(
            "kscreen-doctor -o falló: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse_kde(&String::from_utf8_lossy(&out.stdout)))
}

/// Parsea la salida de `kscreen-doctor -o`.
///
/// Es una función pura a propósito: así se puede probar con una salida real
/// guardada, sin depender de tener una sesión de KDE delante.
fn parse_kde(texto: &str) -> Vec<DisplayOutput> {
    let texto = sin_ansi(texto);
    let mut salidas: Vec<DisplayOutput> = Vec::new();
    let mut cur: Option<DisplayOutput> = None;

    for linea in texto.lines() {
        let l = linea.trim();
        if l.is_empty() {
            continue;
        }

        if let Some(resto) = l.strip_prefix("Output:") {
            if let Some(d) = cur.take() {
                salidas.push(d);
            }
            // "Output: 1 DP-1 <uuid>": el nombre es el segundo campo.
            let partes: Vec<&str> = resto.split_whitespace().collect();
            cur = Some(DisplayOutput {
                name: partes.get(1).copied().unwrap_or("").to_string(),
                status: "disabled".into(),
                connected: false,
                primary: false,
                w: 0,
                h: 0,
                hz: 0.0,
                offset_x: 0,
                offset_y: 0,
                modes: Vec::new(),
                current_flags: String::new(),
            });
            continue;
        }

        let Some(d) = cur.as_mut() else { continue };

        if l == "enabled" {
            d.status = "enabled".into();
        } else if l == "disabled" {
            d.status = "disabled".into();
        } else if l == "connected" {
            d.connected = true;
        } else if l == "disconnected" {
            d.connected = false;
        } else if let Some(v) = l.strip_prefix("priority ") {
            // En KDE la prioridad 1 es la pantalla principal.
            d.primary = v.trim() == "1";
        } else if let Some(v) = l.strip_prefix("Modes:") {
            for tok in v.split_whitespace() {
                // "1:3840x2160@144.01*!"
                let Some((_, marca)) = tok.split_once(':') else {
                    continue;
                };
                let (num, flags) = partir_flags(marca);
                let Some((w, h)) = parse_wh(num.split('@').next().unwrap_or(num)) else {
                    continue;
                };
                let hz = num
                    .split_once('@')
                    .and_then(|(_, h)| h.parse().ok())
                    .unwrap_or_default();
                if flags.contains('*') {
                    d.w = w;
                    d.h = h;
                    d.hz = hz;
                    d.current_flags = flags.clone();
                }
                d.modes.push(Mode { w, h, hz, flags });
            }
        } else if let Some(v) = l.strip_prefix("Geometry:") {
            // "0,0 2649x1490" -> posición y tamaño LÓGICO (ya con la escala).
            if let Some(pos) = v.split_whitespace().next() {
                if let Some((x, y)) = pos.split_once(',') {
                    d.offset_x = x.parse().unwrap_or_default();
                    d.offset_y = y.parse().unwrap_or_default();
                }
            }
        }
    }
    if let Some(d) = cur {
        salidas.push(d);
    }

    // Solo las conectadas: una salida desconectada no tiene nada que contar y
    // llenaría la vista de tarjetas vacías.
    salidas.retain(|d| d.connected);
    salidas
}

/// Ejecuta `kscreen-doctor` y decide si ha funcionado.
///
/// OJO, que aquí está la trampa: `kscreen-doctor` **siempre termina con código
/// 0**, incluso cuando no hace nada. Sus fallos son mensajes de texto que además
/// salen por la SALIDA ESTÁNDAR, no por la de error:
///
///     $ kscreen-doctor output.__NO_EXISTE__.mode.9999x9999 ; echo $?
///     Output with name or uuid __NO_EXISTE__ not found.
///     0
///
/// Por eso no basta con mirar el código de salida (que era lo que se hacía: se
/// cantaba "aplicado" sin haber aplicado nada). Se miran las dos salidas y, si
/// hay rastro de fallo, se devuelve error. Quien llama ADEMÁS comprueba el
/// efecto real sobre la configuración.
fn kde_orden(args: &[String]) -> Result<String, String> {
    let out = Command::new("kscreen-doctor")
        .args(args)
        .output()
        .map_err(|e| format!("kscreen-doctor: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let respuesta = format!("{stdout} {stderr}");

    // Mensajes de fallo de kscreen-doctor (no están traducidos, así que son
    // estables). La lista cubre los cuatro que puede dar.
    const FALLOS: &[&str] = &[
        "Unable to parse arguments",
        "not found",
        "Failed to",
        "failed to",
    ];
    if FALLOS.iter().any(|f| respuesta.contains(f)) {
        return Err(format!(
            "kscreen-doctor no aplicó el cambio: {}",
            respuesta.trim()
        ));
    }
    if !out.status.success() {
        return Err(format!(
            "kscreen-doctor terminó con error: {}",
            if respuesta.trim().is_empty() { "sin mensaje" } else { respuesta.trim() }
        ));
    }
    Ok(respuesta.trim().to_string())
}

/// Número de modo que corresponde a (w, h, hz) según `kscreen-doctor -o`.
///
/// Se pide por NÚMERO y no como "WxH@HZ" por dos motivos comprobados:
/// `WxH@144.01` no se puede parsear (kscreen-doctor quiere la frecuencia sin
/// decimales) y redondearla sería un error en cuanto aparezca un modo a 59,94.
/// El número que encabeza cada modo de la lista (`1:3840x2160@144.01`) es
/// inequívoco.
fn indice_modo(nombre: &str, w: i32, h: i32, hz: f64) -> Option<u32> {
    let out = Command::new("kscreen-doctor").arg("-o").output().ok()?;
    let texto = sin_ansi(&String::from_utf8_lossy(&out.stdout));
    let mut dentro = false;
    for linea in texto.lines() {
        let l = linea.trim();
        if let Some(resto) = l.strip_prefix("Output:") {
            // "Output: 1 DP-1 <uuid>": el nombre es el segundo campo.
            let partes: Vec<&str> = resto.split_whitespace().collect();
            dentro = partes.get(1).copied() == Some(nombre);
            continue;
        }
        if !dentro {
            continue;
        }
        let Some(v) = l.strip_prefix("Modes:") else {
            continue;
        };
        for tok in v.split_whitespace() {
            let (indice, marca) = tok.split_once(':')?;
            let (num, _) = partir_flags(marca);
            let Some((mw, mh)) = parse_wh(num.split('@').next().unwrap_or(num)) else {
                continue;
            };
            let mhz: f64 = num
                .split_once('@')
                .and_then(|(_, x)| x.parse().ok())
                .unwrap_or_default();
            if mw == w && mh == h && (mhz - hz).abs() < 0.05 {
                return indice.parse().ok();
            }
        }
    }
    None
}

/// ¿Está la salida ahora mismo en ese modo? Se comprueba DESPUÉS de pedir el
/// cambio, porque `kscreen-doctor` puede responder que sí sin haber hecho nada.
fn modo_aplicado(nombre: &str, w: i32, h: i32, hz: f64) -> bool {
    query_kde()
        .ok()
        .and_then(|salidas| salidas.into_iter().find(|o| o.name == nombre))
        .map(|o| o.w == w && o.h == h && (o.hz - hz).abs() < 0.05)
        .unwrap_or(false)
}

/// ¿Está la salida activada o desactivada?
fn salida_activada(nombre: &str) -> Option<bool> {
    let out = Command::new("kscreen-doctor").arg("-o").output().ok()?;
    let texto = sin_ansi(&String::from_utf8_lossy(&out.stdout));
    let mut dentro = false;
    let mut activada = None;
    for linea in texto.lines() {
        let l = linea.trim();
        if let Some(resto) = l.strip_prefix("Output:") {
            let partes: Vec<&str> = resto.split_whitespace().collect();
            dentro = partes.get(1).copied() == Some(nombre);
            continue;
        }
        if !dentro {
            continue;
        }
        if l == "enabled" {
            activada = Some(true);
        } else if l == "disabled" {
            activada = Some(false);
        }
    }
    activada
}

/* ── X11 (xrandr) ─────────────────────────────────────────────────────────── */

fn query_xrandr() -> Result<Vec<DisplayOutput>, String> {
    let out = crate::proceso::ejecutar(
        "xrandr",
        &["--query".to_string()],
        &[],
        Duration::from_secs(5),
    )
    .map_err(|e| format!("xrandr: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "xrandr --query falló: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse_xrandr(&String::from_utf8_lossy(&out.stdout)))
}

/// Parsea la salida de `xrandr --query`. Pura, por el mismo motivo que `parse_kde`.
fn parse_xrandr(texto: &str) -> Vec<DisplayOutput> {
    let mut outputs: Vec<DisplayOutput> = Vec::new();
    let mut cur: Option<DisplayOutput> = None;

    for line in texto.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');

        if !indented {
            if let Some(d) = cur.take() {
                outputs.push(d);
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 && (parts[1] == "connected" || parts[1] == "disconnected") {
                let (w, h, ox, oy) = parse_geom(parts.get(3).copied().unwrap_or(""));
                cur = Some(DisplayOutput {
                    name: parts[0].to_string(),
                    status: parts[1].to_string(),
                    connected: parts[1] == "connected",
                    primary: parts.get(2) == Some(&"primary"),
                    w,
                    h,
                    hz: 0.0,
                    offset_x: ox,
                    offset_y: oy,
                    modes: Vec::new(),
                    current_flags: String::new(),
                });
            }
        } else if let Some(d) = cur.as_mut() {
            if d.connected {
                let tokens: Vec<&str> = trimmed.split_whitespace().collect();
                if tokens.len() >= 2 {
                    if let Some((w, h)) = parse_wh(tokens[0]) {
                        let (hz, flags) = split_hz_flags(tokens[1]);
                        d.modes.push(Mode {
                            w,
                            h,
                            hz,
                            flags: flags.clone(),
                        });
                        if flags.contains('*') {
                            d.w = w;
                            d.h = h;
                            d.hz = hz;
                            d.current_flags = flags;
                        }
                    }
                }
            }
        }
    }
    if let Some(d) = cur {
        outputs.push(d);
    }
    outputs
}

/// Frecuencia para enseñarla: 144 en vez de 144.01, pero 59.94 tal cual.
fn num_hz(hz: f64) -> String {
    if (hz - hz.round()).abs() < 0.01 {
        format!("{}", hz.round() as i64)
    } else {
        format!("{hz:.2}")
    }
}

fn parse_wh(s: &str) -> Option<(i32, i32)> {
    let (a, b) = s.split_once('x')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// Separa la frecuencia de los indicadores de un token de xrandr: `60.00*+` ->
/// (`60.00`, `*+`). `*` = actual, `+` = preferida.
fn split_hz_flags(raw: &str) -> (f64, String) {
    let idx = raw
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(raw.len());
    let (hz_str, flags) = raw.split_at(idx);
    (hz_str.parse().unwrap_or_default(), flags.to_string())
}

fn parse_geom(s: &str) -> (i32, i32, i32, i32) {
    let (wh, rest) = s.split_once('+').unwrap_or((s, ""));
    let (w, h) = parse_wh(wh).unwrap_or((0, 0));
    let parts: Vec<&str> = rest.split('+').collect();
    let (ox, oy) = match parts.len() {
        2 => (
            parts[0].parse().unwrap_or_default(),
            parts[1].parse().unwrap_or_default(),
        ),
        1 => (parts[0].parse().unwrap_or_default(), 0),
        _ => (0, 0),
    };
    (w, h, ox, oy)
}

/* ── Acciones ─────────────────────────────────────────────────────────────── */

pub fn apply(output: &str, w: i32, h: i32, hz: f64) -> Result<String, String> {
    // Lo primero: la caché de la foto deja de valer en cuanto se toca la
    // configuración de pantalla.
    invalidar_cache();
    let b = backend()?;
    if output.is_empty() || w <= 0 || h <= 0 {
        return Err("Falta la pantalla o el modo que hay que aplicar.".into());
    }

    match b {
        Backend::Kde => {
            let orden = match indice_modo(output, w, h, hz) {
                Some(n) => format!("output.{output}.mode.{n}"),
                // Sin número (por ejemplo si acaba de cambiar la lista de
                // modos), se intenta por resolución; el `hz` va sin decimales
                // porque con ellos no lo sabe parsear.
                None if hz > 0.0 => format!("output.{output}.mode.{w}x{h}@{}", hz.round() as i64),
                None => format!("output.{output}.mode.{w}x{h}"),
            };
            kde_orden(&[orden])?;
            // Y ahora lo importante: comprobar que ha surtido efecto.
            if !modo_aplicado(output, w, h, hz) {
                return Err(format!(
                    "kscreen-doctor aceptó la orden pero {output} no ha cambiado de modo."
                ));
            }
            Ok(format!("Modo {w}×{h}@{} Hz aplicado a {output}", num_hz(hz)))
        }
        Backend::Xrandr => {
            let out = Command::new("xrandr")
                .args([
                    "--output",
                    output,
                    "--mode",
                    &format!("{w}x{h}"),
                    "--refresh",
                    &format!("{hz}"),
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(format!("Modo {w}×{h}@{hz} Hz aplicado a {output}"))
            } else {
                Err(format!(
                    "xrandr falló: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }
    }
}

/// Reaplica a cada salida conectada el modo que ya tiene.
///
/// Es la acción que arregla el caso típico: tras un suspender/despertar, la
/// pantalla se queda en 60 Hz o no despierta. Volver a pedir el mismo modo la
/// despierta sin cambiarle la configuración.
pub fn reapply() -> Result<String, String> {
    invalidar_cache();
    let outs = query()?;
    let conectadas: Vec<DisplayOutput> = outs.into_iter().filter(|o| o.connected).collect();
    if conectadas.is_empty() {
        return Err("No hay salidas conectadas a las que reaplicar el modo.".into());
    }
    let mut msgs = Vec::new();
    for o in conectadas {
        msgs.push(apply(&o.name, o.w, o.h, o.hz)?);
    }
    Ok(msgs.join(" · "))
}

pub fn toggle(output: &str, on: bool) -> Result<String, String> {
    invalidar_cache();
    let b = backend()?;
    if output.is_empty() {
        return Err("Falta indicar qué pantalla conmutar.".into());
    }

    match b {
        Backend::Kde => {
            kde_orden(&[format!(
                "output.{output}.{}",
                if on { "enable" } else { "disable" }
            )])?;
            // Igual que en `apply`: no vale con que el comando no se queje.
            if salida_activada(output) != Some(on) {
                return Err(format!(
                    "kscreen-doctor aceptó la orden pero {output} no ha cambiado de estado."
                ));
            }
            Ok(format!(
                "{output} {}",
                if on { "activada" } else { "desactivada" }
            ))
        }
        Backend::Xrandr => {
            let word = if on { "on" } else { "off" };
            let out = Command::new("xrandr")
                .args(["--output", output, word])
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(format!("{output} {word}"))
            } else {
                Err(format!(
                    "xrandr falló: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }
    }
}

/// Una línea para que la interfaz pueda decir de dónde salen los datos de
/// pantalla. Si no hay backend, devuelve el motivo: es justo lo que hay que
/// enseñar cuando la lista de salidas sale vacía.
pub fn nota() -> String {
    nota_impl()
}

#[cfg(target_os = "linux")]
fn nota_impl() -> String {
    match backend() {
        Ok(b) => format!("Pantallas leídas con {}.", b.nombre()),
        Err(e) => e,
    }
}

/// Fuera de Linux sí se leen las pantallas (con la herramienta del sistema), pero
/// el MODO no se puede cambiar: la nota lo dice, que es lo que necesita saber
/// quien mire la sección.
#[cfg(not(target_os = "linux"))]
fn nota_impl() -> String {
    crate::plataforma::pantalla::motivo_sin_cambio()
}


/* ── Pruebas ──────────────────────────────────────────────────────────────────
   Los parsers son funciones puras justamente para poder probarlos con salidas
   REALES capturadas de esta máquina, sin depender de tener una sesión gráfica
   (ni KDE, ni X11) delante. Las dos muestras de abajo no son inventadas: son la
   salida literal de `kscreen-doctor -o` y `xrandr --query` en una sesión de KDE
   con una pantalla 3840x2160 a 144 Hz.
   ──────────────────────────────────────────────────────────────────────────── */
#[cfg(test)]
mod pruebas {
    use super::*;

    const SALIDA_KDE: &str = r#"
Output: 1 DP-1 e231859b-2821-4c77-bc48-c4c569c56e5c
	enabled
	connected
	priority 1
	DisplayPort
	replication source:0
	Modes:  1:3840x2160@144.01*!  2:3840x2160@60.00  3:3840x2160@120.00  4:2560x1440@144.00  5:2560x1440@120.00  6:2560x1440@60.00  7:1920x1080@144.00  8:1920x1080@119.96  9:1920x1080@120.00  10:1920x1080@119.88  11:1920x1080@60.00  12:1920x1080@60.00  13:1920x1080@59.94  14:1280x1024@75.03  15:1280x720@119.86  16:1280x720@120.00  17:1280x720@119.88  18:1280x720@100.00  19:1280x720@99.88  20:1280x720@60.00  21:1280x720@60.00  22:1280x720@59.94  23:1024x768@75.03  24:1024x768@70.07  25:1024x768@60.00  26:800x600@75.00  27:800x600@72.19  28:800x600@60.32  29:720x576@50.00  30:720x480@60.00  31:720x480@59.94  32:640x480@75.00  33:640x480@72.81  34:640x480@66.67  35:640x480@60.00  36:640x480@59.94  37:1600x1200@59.87  38:1600x1200@143.89  39:1280x1024@59.90  40:1280x1024@143.92  41:1024x768@143.87  42:2560x1600@59.99  43:2560x1600@144.00  44:1920x1200@59.88  45:1920x1200@143.89  46:1280x800@59.81  47:1280x800@143.84  48:3200x1800@59.96  49:3200x1800@143.94  50:2880x1620@59.96  51:2880x1620@143.95  52:1600x900@59.95  53:1600x900@143.93  54:1368x768@59.88  55:1368x768@143.77  56:1280x720@143.85
	Custom modes: None
	Geometry: 0,0 2649x1490
	Scale: 1.45
	Rotation: 1
	Overscan: 0
	Vrr: Never
	RgbRange: Automatic
	HDR: disabled
	Wide Color Gamut: disabled
	ICC profile: none
	Color profile source: sRGB
	Color power preference: prefer efficiency and performance
	Brightness control: supported, set to 19% and dimming to 100%
	DDC/CI: allowed
	Color resolution: 8 bits per color, range: [8; 16] bits per color
	Allow EDR: unsupported
	Sharpness control: unsupported
	Automatic brightness: unsupported
	Auto Rotate Policy: incapable
	Adaptive backlight modulation: unsupported
"#;

    const SALIDA_XRANDR: &str = r#"
Screen 0: minimum 16 x 16, current 3840 x 2160, maximum 32767 x 32767
DP-1 connected primary 3840x2160+0+0 (normal left inverted right x axis y axis) 698mm x 392mm
   3840x2160    143.94*+
   2048x1536    143.94
   1920x1440    143.90
   1600x1200    143.89
   1440x1080    143.80
   1400x1050    143.89
   1280x1024    143.92
   1280x960     143.86
   1152x864     143.92
   1024x768     143.87
   800x600      143.83
   640x480      143.85
   320x240      142.05
   2560x1600    144.00
   1920x1200    143.89
   1680x1050    143.88
   1440x900     143.86
   1280x800     143.84
   1152x720     143.77
   960x600      143.72
   928x580      143.50
   800x500      143.68
   768x480      143.69
   720x480      143.85
   640x400      143.37
   320x200      141.40
   3200x1800    143.94
   2880x1620    143.95
   2560x1440    143.91
   2048x1152    143.88
   1920x1080    143.88
   1600x900     143.93
   1368x768     143.77
   1280x720     143.85
   1024x576     143.91
   864x486      143.63
   720x400      143.88
   640x350      143.57
"#;

    #[test]
    fn quita_los_codigos_de_color() {
        // `kscreen-doctor` colorea la salida; si no se limpiara, los prefijos
        // ("Modes:", "Geometry:") no se reconocerían nunca.
        assert_eq!(sin_ansi("\u{1b}[01;32mOutput:\u{1b}[0;0m 1 DP-1"), "Output: 1 DP-1");
        assert_eq!(sin_ansi("sin codigos"), "sin codigos");
    }

    #[test]
    fn lee_kde_una_salida_conectada() {
        let salidas = parse_kde(SALIDA_KDE);
        assert_eq!(salidas.len(), 1, "se esperaba una sola salida conectada");
        let o = &salidas[0];
        assert_eq!(o.name, "DP-1");
        assert!(o.connected);
        assert!(o.primary, "la prioridad 1 es la principal");
        assert_eq!(o.status, "enabled");
        // El modo en uso es el marcado con `*`, no la geometría (que va en
        // píxeles lógicos por la escala fraccionaria).
        assert_eq!((o.w, o.h), (3840, 2160));
        assert!((o.hz - 144.01).abs() < 0.01, "hz = {}", o.hz);
        assert!(o.current_flags.contains('*'));
        assert!(o.modes.len() > 50, "modos = {}", o.modes.len());
        assert!(o.modes.iter().any(|m| m.flags.contains('!')));
    }

    #[test]
    fn lee_xrandr_una_salida_conectada() {
        let salidas = parse_xrandr(SALIDA_XRANDR);
        assert_eq!(salidas.len(), 1);
        let o = &salidas[0];
        assert_eq!(o.name, "DP-1");
        assert!(o.connected && o.primary);
        assert_eq!((o.w, o.h), (3840, 2160));
        assert!((o.hz - 143.94).abs() < 0.01);
        assert!(o.modes.len() > 15);
    }

    #[test]
    fn descarta_las_salidas_desconectadas() {
        let muestra = "Output: 1 DP-1 abc\n\tenabled\n\tdisconnected\n\tModes: 1:1920x1080@60*\n";
        assert!(parse_kde(muestra).is_empty());
    }

    #[test]
    fn aguanta_lineas_que_no_vienen_a_cuentas() {
        let muestra = "ruido suelto\nOutput: 1 HDMI-1 xyz\n\tenabled\n\tconnected\n\
             \tCustom modes: None\n\tModes: 1:1280x720@60.00* 2:640x480@59.94\n\
             \tGeometry: 100,0 1280x720\n";
        let salidas = parse_kde(muestra);
        assert_eq!(salidas.len(), 1);
        assert_eq!((salidas[0].w, salidas[0].hz), (1280, 60.0));
        assert_eq!((salidas[0].offset_x, salidas[0].offset_y), (100, 0));
        assert_eq!(salidas[0].modes.len(), 2);
    }

    #[test]
    fn aplicar_sobre_una_salida_inexistente_da_error() {
        // Prueba el camino de ERROR de una acción real (habla con el servidor
        // gráfico de verdad) SIN tocar la configuración de pantalla de nadie:
        // se apunta a una salida que no existe, así que la única respuesta
        // posible es un error. Si el backend de la sesión no está disponible,
        // tampoco hay nada que probar.
        if backend().is_err() {
            return;
        }
        let r = apply("__SALIDA_QUE_NO_EXISTE__", 9999, 9999, 0.0);
        assert!(r.is_err(), "se esperaba un error, pero salió: {r:?}");

        let t = toggle("__SALIDA_QUE_NO_EXISTE__", false);
        assert!(t.is_err(), "se esperaba un error, pero salió: {t:?}");
    }

    #[test]
    fn resuelve_el_numero_de_modo_de_la_pantalla_actual() {
        // Cubre el camino bueno de `apply` SIN cambiar la pantalla de nadie: se
        // resuelve el número del modo que YA está puesto y se comprueba que la
        // verificación de "aplicado" lo ve. Lo único que no se ejecuta aquí es
        // la llamada a kscreen-doctor que lo cambia, a propósito.
        if backend() != Ok(Backend::Kde) {
            return;
        }
        let Ok(salidas) = query_kde() else { return };
        let Some(o) = salidas.first() else { return };

        let n = indice_modo(&o.name, o.w, o.h, o.hz)
            .unwrap_or_else(|| panic!("no se resolvió el modo de {},{}", o.w, o.h));
        assert!(n >= 1, "número de modo raro: {n}");
        assert!(
            modo_aplicado(&o.name, o.w, o.h, o.hz),
            "no se ve como aplicado el modo que está puesto"
        );
        assert!(salida_activada(&o.name).is_some(), "no se lee si está activada");

        // Y un modo que no existe no debe resolver a ningún número.
        assert_eq!(indice_modo(&o.name, 9999, 9999, 30.0), None);
    }

    #[test]
    fn sin_pantalla_indicada_la_accion_no_se_ejecuta() {
        // Un `output` vacío es lo que mandaba el botón "Conmutar salida" que
        // había antes, y acababa en un error del backend. Que se detecte antes
        // de llamar a nada.
        assert!(apply("", 1920, 1080, 60.0).is_err());
        assert!(toggle("", true).is_err());
    }

    #[test]
    fn separa_frecuencia_e_indicadores() {
        assert_eq!(split_hz_flags("143.94*+"), (143.94, "*+".to_string()));
        assert_eq!(split_hz_flags("60.00"), (60.0, String::new()));
        assert_eq!(partir_flags("3840x2160@144.01*!"), ("3840x2160@144.01", "*!".to_string()));
        assert_eq!(partir_flags("1280x720@60.00"), ("1280x720@60.00", String::new()));
    }

    #[test]
    fn lee_tamano_y_posicion() {
        assert_eq!(parse_wh("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_wh("vaya"), None);
        assert_eq!(parse_geom("3840x2160+0+0"), (3840, 2160, 0, 0));
        assert_eq!(parse_geom("1280x720+1920+540"), (1280, 720, 1920, 540));
    }

    /// La conversión de una salida de macOS/Windows al tipo del panel. Se prueba
    /// desde Linux porque es pura, y con trozos de salida REALES: el JSON de
    /// `system_profiler -json` y el de WMI, los mismos que ya prueban los parsers
    /// de `plataforma::pantalla`.
    #[test]
    fn convierte_las_salidas_del_sistema_al_tipo_del_panel() {
        let mac = r#"{
          "SPDisplaysDataType" : [
            {
              "_name" : "Apple M1 Pro",
              "spdisplays_ndrvs" : [
                {
                  "_name" : "Color LCD",
                  "spdisplays_main" : "spdisplays_yes",
                  "spdisplays_resolution" : "3024 x 1964 @ 120.00Hz"
                }
              ]
            }
          ]
        }"#;
        let salidas = crate::plataforma::pantalla::parsear_macos(mac);
        assert_eq!(salidas.len(), 1);
        let o = desde_salida(salidas.into_iter().next().unwrap());
        assert_eq!(o.name, "Color LCD");
        assert!(o.connected && o.primary);
        assert_eq!(o.status, "enabled");
        assert_eq!((o.w, o.h), (3024, 1964));
        assert!((o.hz - 120.0).abs() < 0.01);
        // Ni `system_profiler` ni WMI publican la lista de modos: va vacía y la
        // interfaz dice que no se pueden listar (no se inventa ninguno).
        assert!(o.modes.is_empty());
        assert!(o.current_flags.is_empty());

        // Un adaptador sin pantalla conectada (Windows publica `null`, que el
        // parser convierte en 0): no está «enabled» y no se inventa un modo.
        let win = r#"[{"Name":"Intel UHD Graphics 630","CurrentHorizontalResolution":3840,"CurrentVerticalResolution":2160,"CurrentRefreshRate":60},{"Name":"NVIDIA GeForce RTX 3070","CurrentHorizontalResolution":null,"CurrentVerticalResolution":null,"CurrentRefreshRate":null}]"#;
        let salidas = crate::plataforma::pantalla::parsear_windows(win);
        assert_eq!(salidas.len(), 2);
        let sin_pantalla = desde_salida(salidas.into_iter().nth(1).unwrap());
        assert!(!sin_pantalla.connected);
        assert_eq!(sin_pantalla.status, "unknown");
        assert_eq!(sin_pantalla.w, 0);
    }
}
