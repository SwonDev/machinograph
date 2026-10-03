//! Puerta de enlace de uso: un servidor local que reenvía al motor de inferencia
//! y CUENTA lo que pasa por él.
//!
//! POR QUÉ EXISTE. El llama-swap de este equipo (v256) no publica tokens por
//! ningún sitio: se comprobaron sus endpoints uno a uno.
//!   * `/metrics`            → CPU, memoria, swap y red. Sin tokens.
//!   * `/api/performance`    → solo `sys_stats`. Sin tokens.
//!   * `/logs`               → sus propias líneas `[INFO] Request`. Sin tokens.
//!   * `/api/events`         → ese log por SSE.
//! Y `modelo-local-server.sh` no pasa `--metrics`, así que llama-server tampoco publica
//! Prometheus. Los tokens hay que contarlos EN EL CAMINO, y eso es esta puerta:
//!
//! ```text
//!   harness → Machinograph (:8090) → llama-swap (:8080) → llama-server
//! ```
//!
//! QUÉ NO HACE, y es importante que no lo haga:
//!   * No modifica la petición ni la respuesta: reenvía el cuerpo TAL CUAL y
//!     devuelve la respuesta del motor byte a byte, con su estado y sus
//!     cabeceras. Si esto tocara el protocolo, dejaría de servir como proxy.
//!   * No inventa cifras. Si el motor no publica tokens de una petición, se
//!     guardan como NULL y la interfaz enseña «—». Un 0 diría "no se generó
//!     nada", que es otra cosa.
//!   * No decide el modelo ni el contexto: eso es del motor y de su configuración.
//!
//! QUÉ SÍ HACE:
//!   * Autentica (opcional) con una clave, para poder abrir el puerto a la red sin
//!     dejar el motor de par en par.
//!   * Mide el tiempo hasta el primer token (TTFT) y el tiempo de generación.
//!   * Extrae los tokens del `usage` (formato OpenAI y Anthropic) o del `timings`
//!     de llama.cpp, también cuando la respuesta va en streaming.

use std::net::SocketAddr;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use futures_util::StreamExt;

use crate::db::{self, UsoFila};

/* ── Configuración ────────────────────────────────────────────────────────── */

/// La configuración de la puerta, tal cual está guardada. Todo lo que entra aquí
/// sale de SQLite o de un valor por defecto EXPLÍCITO: nada de deducciones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub activa: bool,
    pub direccion: String,
    pub puerto: u16,
    pub destino: String,
    pub requiere_clave: bool,
    pub clave: String,
}

/// Puerto por defecto: 8090, para no chocar con ninguno de los motores conocidos
/// (llama-swap 8080, vLLM 8000, LM Studio 1234, Ollama 9000…).
pub const PUERTO_POR_DEFECTO: u16 = 8090;
/// Destino por defecto: el llama-swap de este equipo.
pub const DESTINO_POR_DEFECTO: &str = "http://127.0.0.1:8080";

fn ajuste_texto(clave: &str, por_defecto: &str) -> String {
    db::get_setting(clave)
        .ok()
        .flatten()
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| por_defecto.to_string())
}

fn ajuste_bool(clave: &str, por_defecto: bool) -> bool {
    match db::get_setting(clave).ok().flatten().as_deref() {
        Some("1") | Some("true") | Some("\"1\"") | Some("\"true\"") => true,
        Some("0") | Some("false") | Some("\"0\"") | Some("\"false\"") => false,
        _ => por_defecto,
    }
}

/// Lee la configuración guardada. Un puerto fuera de rango en la base de datos no
/// puede impedir arrancar: se cae al de por defecto y se dice en el estado.
pub fn config() -> Config {
    let puerto = ajuste_texto("gateway_port", &PUERTO_POR_DEFECTO.to_string())
        .parse::<u16>()
        .ok()
        .filter(|p| *p >= 1024)
        .unwrap_or(PUERTO_POR_DEFECTO);
    Config {
        activa: ajuste_bool("gateway_enabled", false),
        direccion: ajuste_texto("gateway_address", "127.0.0.1"),
        puerto,
        destino: ajuste_texto("gateway_upstream", DESTINO_POR_DEFECTO),
        requiere_clave: ajuste_bool("gateway_require_key", true),
        clave: ajuste_texto("gateway_api_key", ""),
    }
}

/// Genera una clave nueva. Formato legible y sin ambigüedad: 32 caracteres
/// hexadecimales sacados del generador del sistema.
///
/// POR QUÉ ES POR SISTEMA: la fuente barata no es la misma. En Linux y macOS es
/// `/dev/urandom`; en Windows ese fichero NO existe, y su equivalente es
/// `BCryptGenRandom`, al que se llama por FFI (declararlo aquí no añade ninguna
/// dependencia de Cargo: `bcrypt` ya está en Windows y lo enlaza el propio
/// sistema). Antes, en Windows se caía en silencio a una marca de tiempo, que NO
/// es aleatoria: una clave de puerta adivinable.
pub fn clave_nueva() -> String {
    let mut b = [0u8; 16];
    if aleatorio(&mut b).is_err() {
        // Sin fuente del sistema (no debería pasar en ninguno de los tres) no se
        // deja la clave vacía: se usa una marca de tiempo, que no es segura pero no
        // es la cadena vacía. Queda dicho aquí en vez de fingir que es aleatoria.
        return format!(
            "{:016x}{:016x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Rellena `b` con bytes del generador del sistema. `Err(())` si no se pudo.
#[cfg(not(windows))]
fn aleatorio(b: &mut [u8]) -> Result<(), ()> {
    use std::io::Read;
    // `/dev/urandom` existe en Linux y en macOS: es la fuente del sistema en los
    // dos, y por eso no hace falta distinguirlos aquí.
    let mut f = std::fs::File::open("/dev/urandom").map_err(|_| ())?;
    f.read_exact(b).map_err(|_| ())
}

/// Igual, para Windows, donde no hay `/dev/urandom`.
#[cfg(windows)]
fn aleatorio(b: &mut [u8]) -> Result<(), ()> {
    // `BCRYPT_USE_SYSTEM_PREFERRED_RNG` (0x0000_0002): deja que Windows elija su
    // generador del sistema; así no hay que abrir ni cerrar un proveedor propio.
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    #[allow(non_snake_case)]
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            halgorithm: *mut std::ffi::c_void,
            pbuffer: *mut u8,
            cbuffer: u32,
            flags: u32,
        ) -> i32;
    }
    // NTSTATUS: 0 es éxito.
    let estado = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            b.as_mut_ptr(),
            b.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if estado == 0 {
        Ok(())
    } else {
        Err(())
    }
}

/* ── Lo que se aprende de una respuesta ───────────────────────────────────── */

/// Los números que se pueden sacar de una respuesta. Todo `Option`: cada motor
/// publica lo que publica, y lo que no viene NO se rellena con un cero.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Medida {
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    /// El tiempo de GENERACIÓN que mide el propio motor (`timings.predicted_ms`),
    /// en milisegundos.
    ///
    /// Por qué se prefiere al reloj de la puerta cuando está: en una respuesta SIN
    /// streaming el cuerpo llega entero de golpe, así que para la puerta el primer
    /// byte y el último son el mismo instante y su resta daría 0. El motor sí sabe
    /// cuánto tardó en generar, y es el número que interesa para los tok/s.
    pub generacion_ms: Option<i64>,
}

impl Medida {
    /// ¿Esta respuesta dijo algo de uso? Es el criterio para decidir si la
    /// petición fue una INFERENCIA: un `GET /v1/models` no publica uso, y contarlo
    /// como turno inflaría el número de peticiones.
    pub fn hay_datos(&self) -> bool {
        self.prompt_tokens.is_some() || self.completion_tokens.is_some()
    }
}

fn entero(v: &serde_json::Value, clave: &str) -> Option<i64> {
    v.get(clave).and_then(|x| x.as_i64())
}

/// El bloque `usage` en formato OpenAI o Anthropic.
///
/// Los dos formatos se aceptan porque la puerta reenvía a cualquier motor: si
/// llega una respuesta de Anthropic (`/v1/messages`), sus `input_tokens` y
/// `output_tokens` son los mismos números con otro nombre.
fn medida_de_usage(usage: &serde_json::Value) -> Medida {
    let cached = usage
        // OpenAI: `prompt_tokens_details.cached_tokens`.
        .get("prompt_tokens_details")
        .and_then(|d| entero(d, "cached_tokens"))
        // Anthropic: `cache_read_input_tokens`. Es el equivalente exacto (tokens
        // de entrada que salieron de caché), no una aproximación.
        .or_else(|| entero(usage, "cache_read_input_tokens"))
        .or_else(|| entero(usage, "cached_tokens"));
    Medida {
        prompt_tokens: entero(usage, "prompt_tokens").or_else(|| entero(usage, "input_tokens")),
        completion_tokens: entero(usage, "completion_tokens")
            .or_else(|| entero(usage, "output_tokens")),
        cached_tokens: cached,
        // El formato de OpenAI no trae tiempos de generación en `usage`; el que
        // los trae es llama.cpp, en `timings`. Aquí queda vacío y lo rellena
        // `medida_de_objeto`.
        generacion_ms: None,
    }
}

/// El bloque `timings` de llama.cpp.
///
/// `prompt_n` son los tokens de prompt procesados y `predicted_n` los generados;
/// `cache_n`, cuando está, son los que salieron de la caché de prompt (que es lo
/// que hace que la primera respuesta de un turno largo tarde).
fn medida_de_timings(t: &serde_json::Value) -> Medida {
    Medida {
        prompt_tokens: entero(t, "prompt_n"),
        completion_tokens: entero(t, "predicted_n"),
        cached_tokens: entero(t, "cache_n"),
        // `predicted_ms` viene con decimales (`475.639`), así que se redondea.
        generacion_ms: t
            .get("predicted_ms")
            .and_then(|v| v.as_f64())
            .map(|ms| ms.round() as i64)
            .filter(|ms| *ms > 0),
    }
}

/// Junta lo que digan `usage` y `timings` del MISMO objeto de respuesta.
///
/// Por qué los dos: llama.cpp manda `timings` incluso por streaming (es lo que
/// usa su interfaz web), y algunos servidores compatibles solo mandan `usage`. Si
/// vienen los dos, gana `usage` porque los tokens de un contador son el dato
/// contable; `timings` rellena solo lo que falte.
pub fn medida_de_objeto(v: &serde_json::Value) -> Medida {
    let mut m = Medida::default();
    if let Some(t) = v.get("timings") {
        m = medida_de_timings(t);
    }
    if let Some(u) = v.get("usage") {
        let u = medida_de_usage(u);
        if u.prompt_tokens.is_some() {
            m.prompt_tokens = u.prompt_tokens;
        }
        if u.completion_tokens.is_some() {
            m.completion_tokens = u.completion_tokens;
        }
        if u.cached_tokens.is_some() {
            m.cached_tokens = u.cached_tokens;
        }
    }
    // Formato de algunos motores: el uso en la raíz, sin envoltorio.
    if m == Medida::default() {
        if let Some(choices_done) = v.get("choices").is_some().then_some(v) {
            let _ = choices_done;
        }
        for clave in ["prompt_tokens", "completion_tokens", "cached_tokens"] {
            if let Some(n) = entero(v, clave) {
                match clave {
                    "prompt_tokens" => m.prompt_tokens = Some(n),
                    "completion_tokens" => m.completion_tokens = Some(n),
                    _ => m.cached_tokens = Some(n),
                }
            }
        }
    }
    m
}

/// Extrae el uso de un cuerpo completo (respuesta NO streaming).
///
/// Devuelve `None` si el cuerpo no es JSON: un cuerpo binario o de error no es un
/// problema, simplemente no dice nada de uso.
pub fn medida_de_cuerpo(cuerpo: &[u8]) -> Option<Medida> {
    let v: serde_json::Value = serde_json::from_slice(cuerpo).ok()?;
    let m = medida_de_objeto(&v);
    m.hay_datos().then_some(m)
}

/// Extrae el uso de una respuesta en STREAMING (SSE).
///
/// Cada evento va en una línea `data: {...}`, y el uso suele venir en el ÚLTIMO
/// objeto (llama.cpp manda sus `timings` ahí; OpenAI, con
/// `stream_options.include_usage`, manda un objeto extra con `usage`). Se recorren
/// las líneas y se ACUMULA lo que vaya apareciendo, quedándose con el último valor
/// no nulo de cada campo: si el motor lo manda por partes, se junta.
pub fn medida_de_sse(texto: &str) -> Option<Medida> {
    let mut m = Medida::default();
    for linea in texto.lines() {
        let Some(resto) = linea.strip_prefix("data:") else {
            continue;
        };
        let resto = resto.trim();
        if resto.is_empty() || resto == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(resto) else {
            continue;
        };
        let parcial = medida_de_objeto(&v);
        if parcial.prompt_tokens.is_some() {
            m.prompt_tokens = parcial.prompt_tokens;
        }
        if parcial.completion_tokens.is_some() {
            m.completion_tokens = parcial.completion_tokens;
        }
        if parcial.cached_tokens.is_some() {
            m.cached_tokens = parcial.cached_tokens;
        }
    }
    m.hay_datos().then_some(m)
}

/// El modelo que pide una petición, leído de su cuerpo JSON.
///
/// Devuelve cadena vacía si no viene: hay endpoints (un `GET /v1/models`, un
/// `/health`) que no llevan modelo y no pasa nada por no saberlo. Lo que NO se
/// hace es inventarse uno.
pub fn modelo_de_peticion(cuerpo: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(cuerpo)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_default()
}

/// ¿Esta petición puede ser una inferencia?
///
/// Solo se registran las que pueden serlo (un POST) o las que publicaron uso. Un
/// `GET /health` cada dos segundos no entra en la tabla: llenaría el histórico de
/// ruido y el número de peticiones dejaría de significar "turnos de generación".
pub fn puede_generar(metodo: &Method) -> bool {
    metodo == Method::POST
}

/* ── El estado en memoria del servidor ────────────────────────────────────── */

/// Cómo se guarda una fila de uso. Es una función y no una llamada directa a
/// `db::insert_uso` para que las pruebas de punta a punta puedan comprobar QUÉ se
/// habría guardado sin escribir en la base de datos del usuario.
pub type Guardar = Arc<dyn Fn(&UsoFila) + Send + Sync>;

fn guardar_en_bd() -> Guardar {
    Arc::new(|f: &UsoFila| {
        if let Err(e) = db::insert_uso(f) {
            eprintln!("no se pudo guardar el uso de {}: {e}", f.ruta);
        }
    })
}

#[derive(Clone)]
struct Estado {
    cliente: reqwest::Client,
    cfg: Config,
    guardar: Guardar,
}

static ERROR_ARRANQUE: std::sync::LazyLock<Arc<Mutex<Option<String>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));

/// El puerto REAL en el que está escuchando la puerta, si está en marcha. Puede
/// NO ser el configurado: cuando el configurado está ocupado se prueban los
/// siguientes. La interfaz y la comprobación de salud necesitan este dato, no el
/// de los ajustes (que es solo lo que se intentará al arrancar).
static PUERTO_ESCUCHANDO: std::sync::LazyLock<Arc<Mutex<Option<u16>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));

/// El aviso de que el puerto configurado estaba ocupado y se escucha en otro.
static AVISO_PUERTO: std::sync::LazyLock<Arc<Mutex<Option<String>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));

/// Cuántos puertos por encima del configurado se prueban cuando está ocupado.
/// Se prueban el configurado y los `REINTENTOS_PUERTO` siguientes (21 en total).
pub const REINTENTOS_PUERTO: u16 = 20;

/// El error del último intento de arranque, si lo hubo.
pub fn error_arranque() -> Option<String> {
    ERROR_ARRANQUE.lock().clone()
}

/// El puerto REAL en el que escucha la puerta, o `None` si no hay constancia de
/// que esté escuchando.
pub fn puerto_escuchando() -> Option<u16> {
    *PUERTO_ESCUCHANDO.lock()
}

/// El aviso de que el puerto configurado estaba ocupado, si lo hubo.
pub fn aviso_puerto() -> Option<String> {
    AVISO_PUERTO.lock().clone()
}

/// Los puertos que se prueban, en orden y sin salirse del rango de `u16`: el
/// configurado y los `REINTENTOS_PUERTO` siguientes de la MISMA dirección.
///
/// POR QUÉ: si el puerto configurado está ocupado por otro programa, la puerta no
/// arrancaba y el usuario tenía que buscar un puerto libre a mano. Ahora se busca
/// sola, arranca en el primero libre y la app dice cuál está usando.
fn candidatos(base: u16) -> std::ops::RangeInclusive<u16> {
    base..=base.saturating_add(REINTENTOS_PUERTO)
}

/// El texto que se enseña cuando el puerto configurado estaba ocupado. Función
/// pura para poder probar el mensaje sin levantar nada.
fn aviso_de_puerto(configurado: u16, real: u16) -> Option<String> {
    (real != configurado).then(|| {
        format!(
            "El puerto configurado {configurado} estaba ocupado por otro programa; la puerta está escuchando en el {real} y reenviando desde ahí. Para fijarlo, cambia el puerto en Ajustes (se aplica al reiniciar)."
        )
    })
}

/// Ata un oyente al primer puerto libre empezando por `base`, siempre en la misma
/// dirección. Devuelve el oyente y el puerto REAL. Si ninguno vale, el error lleva
/// el motivo LITERAL del último intento (no un "no se pudo" a secas).
pub(crate) async fn atar(
    direccion: &str,
    base: u16,
) -> Result<(tokio::net::TcpListener, u16), String> {
    let mut motivo = String::from("no se intentó ningún puerto");
    for puerto in candidatos(base) {
        let addr = format!("{direccion}:{puerto}");
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(oyente) => return Ok((oyente, puerto)),
            Err(e) => motivo = format!("{addr}: {e}"),
        }
    }
    let ultimo = base.saturating_add(REINTENTOS_PUERTO);
    Err(format!(
        "no se pudo escuchar en ningún puerto entre {base} y {ultimo} de {direccion} ({motivo})"
    ))
}

/// ¿Hay un intento de arranque en curso AHORA? Evita que dos llamadas seguidas
/// (el arranque de la app y «Reparar ahora») lancen dos servidores: el puerto se
/// ata de forma asíncrona, así que sin esta bandera la segunda llamada vería
/// `puerto_escuchando` vacío y arrancaría el suyo en el puerto siguiente.
static ARRANCANDO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Arranca la puerta si está activada. Se llama al inicio y desde la
/// autorreparación, así que NO puede dejar dos servidores escuchando.
pub fn arrancar_si_activa() {
    // Ya hay una puerta escuchando: lanzar otra daría dos servidores y contaría
    // cada cosa dos veces.
    if puerto_escuchando().is_some() {
        return;
    }
    let cfg = config();
    { let mut e = ERROR_ARRANQUE.lock();
        *e = None;
    }
    { let mut a = AVISO_PUERTO.lock();
        *a = None;
    }
    if !cfg.activa {
        return;
    }
    // Un solo intento a la vez (ver `ARRANCANDO`).
    if ARRANCANDO.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    // OJO con el runtime: esto se llama desde `setup()` de Tauri, que corre en el
    // hilo principal ANTES de entrar en el contexto asíncrono, así que un
    // `tokio::spawn` aquí paniquea con "there is no reactor running" y la app se
    // cierra al arrancar. Se lanza con el runtime de Tauri, que es el que ya usan
    // el bucle de la foto y el del encaje. Lo encontró la prueba de punta a punta
    // con el binario real, no el compilador.
    tauri::async_runtime::spawn(async move {
        if let Err(e) = servir(cfg).await {
            eprintln!("la puerta de enlace no pudo arrancar: {e}");
            { let mut slot = ERROR_ARRANQUE.lock();
                *slot = Some(e);
            }
        }
    });
}

/// Lanza el servidor. Devuelve error si no puede escuchar en NINGUNO de los
/// puertos probados (los 21 ocupados, dirección que no existe).
pub async fn servir(mut cfg: Config) -> Result<(), String> {
    let atado = atar(&cfg.direccion, cfg.puerto).await;
    // El intento de arranque ya ha decidido (bien o mal): se levanta la bandera
    // para que otro «Reparar ahora» pueda volver a intentarlo.
    ARRANCANDO.store(false, std::sync::atomic::Ordering::SeqCst);
    let (oyente, puerto) = atado?;
    // Lo que se sabe que está escuchando, para el estado y para la comprobación de
    // salud. Se anota ANTES de servir: cuando `axum::serve` está en marcha, este
    // dato ya es verdad.
    { let mut g = PUERTO_ESCUCHANDO.lock();
        *g = Some(puerto);
    }
    { let mut a = AVISO_PUERTO.lock();
        *a = aviso_de_puerto(cfg.puerto, puerto);
    }
    // El reenvío (y la URL que se enseña) tienen que usar el puerto REAL: el
    // configurado puede estar ocupado por otro programa.
    cfg.puerto = puerto;
    let cliente = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        // Sin tope total: una generación larga tarda lo que tarde. El tope de
        // conexión sí importa (el motor puede estar arrancando).
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;
    let estado = Estado {
        cliente,
        cfg: cfg.clone(),
        guardar: guardar_en_bd(),
    };
    // Todas las rutas van al mismo sitio: la puerta no interpreta el protocolo,
    // solo lo reenvía. El `fallback` recoge cualquier método y cualquier ruta.
    let app = Router::new().fallback(reenviar).with_state(estado);
    axum::serve(oyente, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|e| format!("el servidor de la puerta se cayó: {e}"))
}

/// Igual que `servir`, pero escuchando en un puerto libre y avisando de cuál es,
/// y con la función de guardado inyectada.
///
/// Existe para las pruebas de punta a punta: se levanta la puerta contra un motor
/// de mentira y se comprueba qué habría guardado. NO se usa en producción (allí
/// manda la configuración del usuario y la base de datos de verdad).
#[cfg(test)]
pub async fn servir_de_prueba(
    cfg: Config,
    guardar: Guardar,
    aviso: tokio::sync::oneshot::Sender<u16>,
) -> Result<(), String> {
    let oyente = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("no se pudo escuchar: {e}"))?;
    let puerto = oyente.local_addr().map_err(|e| e.to_string())?.port();
    let cliente = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let _ = aviso.send(puerto);
    let estado = Estado {
        cliente,
        cfg,
        guardar,
    };
    let app = Router::new().fallback(reenviar).with_state(estado);
    axum::serve(oyente, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|e| format!("el servidor de la puerta se cayó: {e}"))
}

/* ── El reenvío ───────────────────────────────────────────────────────────── */

/// Cabeceras que NO se copian al reenviar: son del salto, no del mensaje.
fn es_cabecera_del_salto(nombre: &str) -> bool {
    matches!(
        nombre,
        "host" | "content-length" | "connection" | "transfer-encoding" | "keep-alive"
            | "upgrade" | "proxy-connection" | "te" | "trailer"
    )
}

/// La clave que trae la petición, sea por `Authorization: Bearer` (OpenAI) o por
/// `x-api-key` (Anthropic). Se aceptan las dos porque la puerta sirve a los dos
/// protocolos.
fn clave_de_peticion(cabeceras: &HeaderMap) -> Option<String> {
    if let Some(v) = cabeceras.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    let auth = cabeceras.get("authorization")?.to_str().ok()?.trim();
    auth.strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .map(|s| s.trim().to_string())
}

/// Compara dos claves sin filtrar por tiempo cuánto se acertó: recorre siempre
/// las dos enteras. No es criptografía de grado militar, pero una comparación que
/// corta al primer byte distinto filtra la clave carácter a carácter.
fn claves_iguales(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut dif = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        dif |= x ^ y;
    }
    dif == 0
}

/// El acumulador de una petición: escribe la fila de uso cuando el cuerpo
/// termina (o cuando el cliente se va, que también es información).
struct Acumulador {
    inicio: Instant,
    primer_byte: Option<Instant>,
    fila: UsoFila,
    json: Vec<u8>,
    cola: Vec<u8>,
    es_sse: bool,
    /// ¿La petición podía ser una inferencia? Se calcula al principio (con el
    /// método de verdad) y NO se deduce del texto guardado en la fila.
    generable: bool,
    guardar: Guardar,
    escrito: bool,
}

impl Acumulador {
    fn nuevo(
        metodo: &Method,
        ruta: &str,
        origen: &str,
        cliente: &str,
        bytes_entrada: usize,
        guardar: Guardar,
    ) -> Self {
        Acumulador {
            inicio: Instant::now(),
            primer_byte: None,
            fila: UsoFila {
                ts: chrono::Utc::now().timestamp(),
                modelo: String::new(),
                ruta: ruta.to_string(),
                metodo: metodo.to_string(),
                estado: 0,
                prompt_tokens: None,
                completion_tokens: None,
                cached_tokens: None,
                ttft_ms: None,
                generacion_ms: None,
                duracion_ms: 0,
                bytes_entrada: bytes_entrada as i64,
                bytes_salida: 0,
                origen: origen.to_string(),
                cliente: cliente.to_string(),
            },
            json: Vec::new(),
            cola: Vec::new(),
            es_sse: false,
            generable: puede_generar(metodo),
            guardar,
            escrito: false,
        }
    }

    /// Apunta un trozo del cuerpo de la respuesta.
    fn trozo(&mut self, bytes: &[u8]) {
        if self.primer_byte.is_none() {
            self.primer_byte = Some(Instant::now());
        }
        self.fila.bytes_salida += bytes.len() as i64;
        if self.es_sse {
            // En streaming solo interesa la COLA: el uso va en el último evento y
            // guardar la respuesta entera de una generación larga sería megabytes
            // para dos números. 64 KB es de sobra para el bloque final.
            self.cola.extend_from_slice(bytes);
            if self.cola.len() > 65_536 {
                let sobra = self.cola.len() - 65_536;
                self.cola.drain(..sobra);
            }
        } else if self.json.len() < 8 * 1024 * 1024 {
            self.json.extend_from_slice(bytes);
        }
    }

    /// Cierra la petición: saca la medida y guarda la fila UNA vez.
    fn cerrar(&mut self) {
        if self.escrito {
            return;
        }
        self.escrito = true;

        let medida = if self.es_sse {
            let texto = String::from_utf8_lossy(&self.cola);
            medida_de_sse(&texto)
        } else {
            medida_de_cuerpo(&self.json)
        };
        let medida = medida.unwrap_or_default();
        self.fila.prompt_tokens = medida.prompt_tokens;
        self.fila.completion_tokens = medida.completion_tokens;
        self.fila.cached_tokens = medida.cached_tokens;

        let duracion = self.inicio.elapsed().as_millis() as i64;
        self.fila.duracion_ms = duracion;
        if let Some(t) = self.primer_byte {
            let ttft = t.duration_since(self.inicio).as_millis() as i64;
            self.fila.ttft_ms = Some(ttft);
            // El tiempo de GENERACIÓN es el total menos el de arranque: el TTFT
            // incluye cargar el modelo y procesar el prompt, y contarlo como
            // generación hundiría los tok/s (la primera petición de un modelo
            // tarda 7 s solo en cargar).
            //
            // Con streaming, esa resta es correcta (el primer byte llega al
            // empezar a generar). SIN streaming el cuerpo llega entero de golpe al
            // final, así que la resta da 0 y ahí manda lo que mida el MOTOR
            // (`timings.predicted_ms`). Por eso el del motor manda: se puso
            // DESPUÉS a propósito, y no antes —el orden inverso lo pisaba y el
            // tiempo de generación se quedaba en «—» en todas las respuestas sin
            // streaming. Lo encontró la prueba con el binario real y el llama-swap
            // de esta máquina, no el compilador.
            let gen = duracion - ttft;
            self.fila.generacion_ms = (gen > 0).then_some(gen);
        }
        // El del MOTOR manda sobre la resta del reloj, y se aplica DESPUÉS a
        // propósito: al revés lo pisaba la resta y el tiempo de generación se
        // quedaba en «—» en todas las respuestas sin streaming (el cuerpo llega
        // entero de golpe, así que la resta da 0). Lo encontró la prueba con el
        // binario real y el llama-swap de esta máquina, no el compilador.
        if medida.generacion_ms.is_some() {
            self.fila.generacion_ms = medida.generacion_ms;
        }

        // ¿Se guarda? Solo si puede ser una inferencia (POST) o si el motor
        // publicó uso. Así el sondeo de salud de la interfaz no llena el
        // histórico de filas que no significan nada.
        let interesante = self.generable
            || self.fila.prompt_tokens.is_some()
            || self.fila.completion_tokens.is_some()
            || self.fila.estado >= 400;
        if interesante {
            (self.guardar)(&self.fila);
        }
    }
}

/// Guarda que escribe la fila cuando el cuerpo se termina o se cae.
///
/// POR QUÉ UN GUARDIA CON `Drop` Y NO UN `.await` AL FINAL: el cuerpo de la
/// respuesta se devuelve en streaming, así que la función del manejador termina
/// ANTES de que el cliente reciba la última línea. El momento en que la petición
/// acaba de verdad es cuando el flujo se cierra —o cuando el cliente corta—, y eso
/// es exactamente lo que detecta un `Drop`.
struct AlCerrar(Arc<Mutex<Acumulador>>);

impl Drop for AlCerrar {
    fn drop(&mut self) {
        { let mut a = self.0.lock();
            a.cerrar();
        }
    }
}

async fn reenviar(
    State(estado): State<Estado>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
) -> Response {
    let cfg = estado.cfg.clone();
    let (partes, cuerpo) = req.into_parts();
    let metodo = partes.method.clone();
    let ruta = partes
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    // El cuerpo se lee ENTERO antes de reenviar: hace falta para sacar el modelo y
    // para mandarlo con su longitud. Un cuerpo de petición de inferencia es texto
    // (el prompt), no un fichero: unos cientos de KB como mucho.
    let bytes = match axum::body::to_bytes(cuerpo, 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("no se pudo leer la petición: {e}"))
                .into_response()
        }
    };

    // ── Autenticación (opcional) ───────────────────────────────────────────
    if cfg.requiere_clave {
        let trae = clave_de_peticion(&partes.headers);
        let vale = trae.as_deref().is_some_and(|k| claves_iguales(k, &cfg.clave));
        if !vale {
            // El motivo se dice: "401" a secas deja al usuario mirando el harness
            // sin saber que le falta la clave.
            return (
                StatusCode::UNAUTHORIZED,
                "falta la clave de la puerta de Machinograph (Authorization: Bearer <clave> o x-api-key: <clave>)",
            )
                .into_response();
        }
    }

    let destino = format!("{}{}", cfg.destino.trim_end_matches('/'), ruta);
    let url = match reqwest::Url::parse(&destino) {
        Ok(u) => u,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("destino inválido «{destino}»: {e}"))
                .into_response()
        }
    };

    let mut peticion = estado.cliente.request(metodo.clone(), url);
    for (nombre, valor) in partes.headers.iter() {
        let n = nombre.as_str();
        if es_cabecera_del_salto(n) {
            continue;
        }
        // La clave de la puerta NO se reenvía: es de aquí, no del motor.
        if n == "x-api-key" {
            continue;
        }
        if n == "authorization" && cfg.requiere_clave {
            continue;
        }
        peticion = peticion.header(nombre.clone(), valor.clone());
    }
    peticion = peticion.body(bytes.to_vec());

    let origen = if peer.ip().is_loopback() { "local" } else { "red" };
    let cliente = partes
        .headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let modelo = modelo_de_peticion(&bytes);
    let acumulador = Arc::new(Mutex::new(Acumulador::nuevo(
        &metodo,
        &ruta,
        origen,
        &cliente,
        bytes.len(),
        estado.guardar.clone(),
    )));

    let respuesta = match peticion.send().await {
        Ok(r) => r,
        Err(e) => {
            // El motor no contesta: es un 502 y se registra, porque un motor caído
            // es justo lo que hay que poder ver en el histórico.
            { let mut a = acumulador.lock();
                a.fila.modelo = modelo;
                a.fila.estado = 502;
                a.cerrar();
            }
            return (
                StatusCode::BAD_GATEWAY,
                format!("el motor ({}) no contestó: {e}", cfg.destino),
            )
                .into_response();
        }
    };

    let estado_http = respuesta.status().as_u16() as i64;
    let content_type = respuesta
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let cabeceras_up = respuesta.headers().clone();

    { let mut a = acumulador.lock();
        a.fila.modelo = modelo;
        a.fila.estado = estado_http;
        a.es_sse = content_type.contains("text/event-stream");
    }

    // El cuerpo se reenvía TAL CUAL, trozo a trozo, contando por el camino. El
    // `AlCerrar` es quien escribe la fila cuando el flujo se acaba (o el cliente
    // se va a mitad): viaja DENTRO del cierre del flujo, así que se suelta cuando
    // se suelta el cuerpo — que es justo el momento en que la petición termina de
    // verdad.
    let flujo = respuesta.bytes_stream().map({
        let guardia = AlCerrar(acumulador.clone());
        move |trozo| {
            let _guardia_vive_aqui = &guardia;
            if let Ok(bytes) = &trozo {
                { let mut a = acumulador.lock();
                    a.trozo(bytes);
                }
            }
            trozo
        }
    });
    let cuerpo_salida = Body::from_stream(flujo);

    let mut salida = Response::new(cuerpo_salida);
    *salida.status_mut() = StatusCode::from_u16(estado_http as u16).unwrap_or(StatusCode::OK);
    for (nombre, valor) in cabeceras_up.iter() {
        if es_cabecera_del_salto(nombre.as_str()) {
            continue;
        }
        salida.headers_mut().insert(nombre.clone(), valor.clone());
    }
    salida
}

/// El estado de la puerta, para la interfaz. `puerto` es el CONFIGURADO (lo que
/// va a valer al reiniciar, que es lo que edita Ajustes) y `puerto_escuchando` es
/// el REAL (el configurado y, si estaba ocupado, el siguiente libre). La URL se
/// construye con el real cuando se sabe: mandar a los clientes al puerto
/// configurado cuando la puerta escucha en otro sería enseñar una dirección
/// donde no hay nada. El error de arranque (si lo hubo) se dice aparte.
pub fn estado_json() -> serde_json::Value {
    let cfg = config();
    let error = error_arranque();
    let escuchando = puerto_escuchando();
    let aviso = aviso_puerto();
    let puerto_url = escuchando.unwrap_or(cfg.puerto);
    serde_json::json!({
        "activa": cfg.activa,
        "direccion": cfg.direccion,
        "puerto": cfg.puerto,
        "puerto_escuchando": escuchando,
        "aviso_puerto": aviso,
        "destino": cfg.destino,
        "requiere_clave": cfg.requiere_clave,
        "clave": cfg.clave,
        "url": format!("http://{}:{}/v1", cfg.direccion, puerto_url),
        "error": error,
    })
}

/// Cuántos días de uso se guardan. Va aparte de la retención de métricas (que es
/// en horas) porque el uso es un histórico de trabajo: interesa comparar semanas.
pub const DIAS_RETENCION: i64 = 90;

/// Borra el uso más viejo que la retención. Devuelve cuántas filas se fueron.
pub fn purgar_antiguo() -> anyhow::Result<usize> {
    let antes = chrono::Utc::now().timestamp() - DIAS_RETENCION * 86_400;
    db::purgar_uso(antes)
}

/* ── Ajustes que la puerta lee de verdad ──────────────────────────────────── */

/// Las claves de la puerta, con su descripción, para que Ajustes pueda listarlas
/// sin inventarse un catálogo aparte. `es_clave` las valida: un ajuste que la
/// puerta no lee sería una mentira en la interfaz (se cambiaría y no pasaría
/// nada).
pub const CLAVES: &[(&str, &str)] = &[
    ("gateway_enabled", "Servir por la puerta de enlace (cuenta el uso y mide los tiempos)"),
    ("gateway_address", "Dirección donde escucha (127.0.0.1 = solo este equipo)"),
    ("gateway_port", "Puerto de la puerta de enlace"),
    ("gateway_upstream", "Motor al que reenvía (por ejemplo http://127.0.0.1:8080)"),
    ("gateway_require_key", "Exigir clave a quien llame"),
    ("gateway_api_key", "Clave de la puerta"),
];

/// ¿Esta clave la lee la puerta? La usa la configuración para rechazar claves
/// inventadas en vez de guardarlas y no hacer nada con ellas.
pub fn es_clave(clave: &str) -> bool {
    CLAVES.iter().any(|(c, _)| *c == clave)
}

/// Comprueba que el destino sea una URL http(s) con host. Sin esto, un dedo torpe
/// en Ajustes dejaría la puerta apuntando a la nada y el fallo saldría como un
/// 502 sin explicación.
pub fn destino_valido(destino: &str) -> bool {
    reqwest::Url::parse(destino.trim())
        .map(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn saca_los_tokens_del_formato_openai() {
        let cuerpo = br#"{"id":"x","choices":[],"usage":{"prompt_tokens":1200,"completion_tokens":345,"total_tokens":1545,"prompt_tokens_details":{"cached_tokens":900}}}"#;
        let m = medida_de_cuerpo(cuerpo).expect("debe haber medida");
        assert_eq!(m.prompt_tokens, Some(1200));
        assert_eq!(m.completion_tokens, Some(345));
        assert_eq!(m.cached_tokens, Some(900));
    }

    #[test]
    fn saca_los_tokens_del_formato_anthropic() {
        let cuerpo = br#"{"usage":{"input_tokens":800,"output_tokens":120,"cache_read_input_tokens":600}}"#;
        let m = medida_de_cuerpo(cuerpo).expect("debe haber medida");
        assert_eq!(m.prompt_tokens, Some(800));
        assert_eq!(m.completion_tokens, Some(120));
        assert_eq!(m.cached_tokens, Some(600));
    }

    /// llama.cpp manda `timings` con su propio nombre para cada cosa, y `cache_n`
    /// solo cuando la caché de prompt ha servido de algo.
    #[test]
    fn saca_los_tokens_de_los_timings_de_llama_cpp() {
        let cuerpo = br#"{"choices":[{"text":"hola"}],"timings":{"prompt_n":4096,"prompt_ms":900.0,"predicted_n":512,"predicted_ms":11000.4,"cache_n":3800}}"#;
        let m = medida_de_cuerpo(cuerpo).expect("debe haber medida");
        assert_eq!(m.prompt_tokens, Some(4096));
        assert_eq!(m.completion_tokens, Some(512));
        assert_eq!(m.cached_tokens, Some(3800));
        // El tiempo de generación del motor, redondeado: es el que se usa para
        // los tok/s cuando la respuesta llega entera de golpe.
        assert_eq!(m.generacion_ms, Some(11000));
    }

    /// El caso importante: `usage` manda sobre `timings` cuando los dos están,
    /// porque `usage` es el contador contable.
    #[test]
    fn usage_manda_sobre_timings() {
        let cuerpo = br#"{"usage":{"prompt_tokens":10,"completion_tokens":20},"timings":{"prompt_n":11,"predicted_n":21}}"#;
        let m = medida_de_cuerpo(cuerpo).expect("debe haber medida");
        assert_eq!(m.prompt_tokens, Some(10));
        assert_eq!(m.completion_tokens, Some(20));
    }

    /// Y si `usage` solo trae una parte, `timings` rellena la otra: quedarse con
    /// el bloque entero perdería un dato que SÍ estaba.
    #[test]
    fn timings_rellena_lo_que_falta_del_usage() {
        let cuerpo = br#"{"usage":{"completion_tokens":20},"timings":{"prompt_n":11,"predicted_n":21}}"#;
        let m = medida_de_cuerpo(cuerpo).expect("debe haber medida");
        assert_eq!(m.prompt_tokens, Some(11));
        assert_eq!(m.completion_tokens, Some(20));
    }

    #[test]
    fn en_streaming_junta_lo_que_llega_por_partes() {
        let sse = "\
data: {\"choices\":[{\"delta\":{\"content\":\"ho\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"la\"}}]}\n\n\
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":7}}\n\n\
data: [DONE]\n\n";
        let m = medida_de_sse(sse).expect("debe haber medida");
        assert_eq!(m.prompt_tokens, Some(100));
        assert_eq!(m.completion_tokens, Some(7));
    }

    /// Una respuesta sin uso NO es una inferencia: es lo que evita que un
    /// `GET /v1/models` cuente como turno.
    #[test]
    fn sin_uso_no_hay_medida() {
        assert_eq!(medida_de_cuerpo(br#"{"data":[{"id":"modelo-local"}]}"#), None);
        assert_eq!(medida_de_sse("data: {\"choices\":[]}\n\n"), None);
        // Ni con un cuerpo que no es JSON (una página de error, un binario).
        assert_eq!(medida_de_cuerpo(b"<html>no</html>"), None);
        assert_eq!(medida_de_cuerpo(b""), None);
    }

    #[test]
    fn el_modelo_sale_del_cuerpo_y_vacio_si_no_viene() {
        assert_eq!(modelo_de_peticion(br#"{"model":"modelo-27b","messages":[]}"#), "modelo-27b");
        assert_eq!(modelo_de_peticion(b""), "");
        assert_eq!(modelo_de_peticion(br#"{"messages":[]}"#), "");
    }

    #[test]
    fn solo_los_post_pueden_ser_inferencia() {
        assert!(puede_generar(&Method::POST));
        assert!(!puede_generar(&Method::GET));
        assert!(!puede_generar(&Method::HEAD));
        assert!(!puede_generar(&Method::OPTIONS));
    }

    #[test]
    fn la_clave_llega_por_las_dos_cabeceras() {
        let mut h = HeaderMap::new();
        h.insert("authorization", "Bearer abc123".parse().unwrap());
        assert_eq!(clave_de_peticion(&h).as_deref(), Some("abc123"));

        let mut h = HeaderMap::new();
        h.insert("x-api-key", "xyz".parse().unwrap());
        assert_eq!(clave_de_peticion(&h).as_deref(), Some("xyz"));

        // Sin cabecera no hay clave, y con una basura tampoco.
        assert_eq!(clave_de_peticion(&HeaderMap::new()), None);
        let mut h = HeaderMap::new();
        h.insert("authorization", "Basic dXNlcjpwYXNz".parse().unwrap());
        assert_eq!(clave_de_peticion(&h), None);
    }

    #[test]
    fn las_claves_se_comparan_enteras_y_solo_iguales_si_son_iguales() {
        assert!(claves_iguales("abc", "abc"));
        assert!(!claves_iguales("abc", "abd"));
        assert!(!claves_iguales("abc", "abcd"));
        assert!(!claves_iguales("", "a"));
        assert!(claves_iguales("", ""));
    }

    #[test]
    fn la_clave_generada_no_se_repite_y_tiene_largo() {
        let a = clave_nueva();
        let b = clave_nueva();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn el_destino_tiene_que_ser_http_con_host() {
        assert!(destino_valido("http://127.0.0.1:8080"));
        assert!(destino_valido("https://motor.local:8080/"));
        assert!(!destino_valido("127.0.0.1:8080"));
        assert!(!destino_valido("ftp://127.0.0.1"));
        assert!(!destino_valido(""));
    }

    /// Las cabeceras del salto no se copian: `content-length` del upstream no vale
    /// para el cuerpo que estamos reenviando, y `host` tiene que ser el del motor.
    #[test]
    fn no_se_reenvian_las_cabeceras_del_salto() {
        for c in ["host", "content-length", "connection", "transfer-encoding"] {
            assert!(es_cabecera_del_salto(c), "{c} debería filtrarse");
        }
        for c in ["content-type", "authorization", "user-agent"] {
            assert!(!es_cabecera_del_salto(c), "{c} NO debería filtrarse");
        }
    }

    /// Los puertos que se prueban son el configurado y los siguientes, sin
    /// salirse del rango: un puerto cerca del máximo no puede dar un desborde ni
    /// un bucle infinito.
    #[test]
    fn los_puertos_de_reintento_son_los_siguientes_sin_desbordar() {
        let v: Vec<u16> = candidatos(8090).collect();
        assert_eq!(v.first().copied(), Some(8090), "el primero es el configurado");
        assert_eq!(v.get(1).copied(), Some(8091));
        assert_eq!(v.len() as u16, REINTENTOS_PUERTO + 1);
        // En el techo del rango solo queda un puerto que probar.
        assert_eq!(candidatos(u16::MAX).collect::<Vec<_>>(), vec![u16::MAX]);
        // Y no se pasa ni desde un puerto normal cerca del final.
        let ultimos: Vec<u16> = candidatos(u16::MAX - 5).collect();
        assert_eq!(ultimos.last().copied(), Some(u16::MAX));
    }

    /// El aviso solo aparece cuando de verdad se está escuchando en otro puerto,
    /// y dice los DOS números (el que se pidió y el que se usa).
    #[test]
    fn el_aviso_de_puerto_dice_el_configurado_y_el_real() {
        assert_eq!(aviso_de_puerto(8090, 8090), None, "si coincide no hay aviso");
        let a = aviso_de_puerto(8090, 8093).expect("tiene que haber aviso");
        assert!(a.contains("8090") && a.contains("8093"), "faltan los números: {a}");
    }

    /// El caso REAL que esto arregla: el puerto configurado está ocupado por otro
    /// programa y la puerta tiene que arrancar en el siguiente, no morir.
    #[tokio::test]
    async fn un_puerto_ocupado_no_impide_escuchar_y_se_usa_el_siguiente() {
        // Se ocupa un puerto (el que haría de «otro programa») y se pide arrancar
        // justo en él.
        let ocupado = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let puerto = ocupado.local_addr().unwrap().port();

        let (oyente, real) = atar("127.0.0.1", puerto).await.expect("tiene que encontrar otro");

        assert_ne!(real, puerto, "el ocupado no puede ser el elegido");
        assert!(
            real > puerto && real <= puerto.saturating_add(REINTENTOS_PUERTO),
            "el elegido tiene que ser uno de los siguientes: {puerto} → {real}"
        );
        assert_eq!(oyente.local_addr().unwrap().port(), real);
        assert!(aviso_de_puerto(puerto, real).is_some());
    }

    /// Y si NINGUNO de los probados está libre, el error dice el rango y el motivo
    /// literal del último intento (no un «no se pudo» sin más).
    #[tokio::test]
    async fn sin_ningun_puerto_libre_el_error_lleva_el_motivo() {
        let base_oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = base_oyente.local_addr().unwrap().port();
        let mut ocupados = vec![base_oyente];
        for salto in 1..=REINTENTOS_PUERTO {
            let p = base.saturating_add(salto);
            if let Ok(o) = tokio::net::TcpListener::bind(format!("127.0.0.1:{p}")).await {
                ocupados.push(o);
            }
        }
        // Solo tiene sentido si de verdad se han podido ocupar todos: si otro
        // proceso dejó uno libre, `atar` acierta y no hay error que comprobar.
        if ocupados.len() as u16 == REINTENTOS_PUERTO + 1 {
            let e = atar("127.0.0.1", base).await.err().expect("no debe haber puerto");
            assert!(e.contains(&base.to_string()), "el motivo nombra el rango: {e}");
        }
    }

    /* ── Punta a punta, contra un motor de mentira ───────────────────────── */

    /// Lo que el motor devuelve, para poder comprobar el reenvío byte a byte.
    /// Se define aquí y no en cada prueba para que las tres miren lo mismo.
    const RESPUESTA_MOTOR: &str = r#"{"id":"cmpl-1","choices":[{"text":"hola"}],"usage":{"prompt_tokens":1000,"completion_tokens":250,"prompt_tokens_details":{"cached_tokens":800}}}"#;

    /// Un motor de mentira: apunta lo que recibe y contesta lo que se le diga.
    async fn motor_de_mentira(
        respuesta: String,
        cabeceras: Vec<(&'static str, &'static str)>,
    ) -> (String, Arc<Mutex<Vec<(String, String, String)>>>) {
        let recibido: Arc<Mutex<Vec<(String, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let apunte = recibido.clone();
        let app = Router::new().fallback(move |req: axum::extract::Request| {
            let respuesta = respuesta.clone();
            let cabeceras = cabeceras.clone();
            let apunte = apunte.clone();
            async move {
                let metodo = req.method().to_string();
                let ruta = req.uri().path().to_string();
                let clave = req
                    .headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let cuerpo = axum::body::to_bytes(req.into_body(), 1 << 20)
                    .await
                    .map(|b| String::from_utf8_lossy(&b).to_string())
                    .unwrap_or_default();
                apunte.lock().push((metodo, ruta, cuerpo));
                let mut r = Response::new(Body::from(respuesta));
                r.headers_mut().insert(
                    "content-type",
                    "application/json".parse().unwrap(),
                );
                for (k, v) in cabeceras {
                    r.headers_mut().insert(k, v.parse().unwrap());
                }
                let _ = clave;
                r
            }
        });
        let oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let puerto = oyente.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(
                oyente,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
        });
        (format!("http://127.0.0.1:{puerto}"), recibido)
    }

    fn config_hacia(destino: &str) -> Config {
        Config {
            activa: true,
            direccion: "127.0.0.1".into(),
            puerto: 0,
            destino: destino.into(),
            requiere_clave: false,
            clave: String::new(),
        }
    }

    /// LA PRUEBA QUE IMPORTA: una petición que atraviesa la puerta llega al motor
    /// con su cuerpo intacto, vuelve con sus bytes intactos, y deja anotado QUÉ se
    /// generó (tokens, prompt, caché) y cuánto tardó.
    #[tokio::test]
    async fn una_peticion_atraviesa_la_puerta_y_queda_contada() {
        let (destino, recibido) = motor_de_mentira(RESPUESTA_MOTOR.into(), vec![]).await;
        let guardadas: Arc<Mutex<Vec<UsoFila>>> = Arc::new(Mutex::new(Vec::new()));
        let apunte = guardadas.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = servir_de_prueba(
                config_hacia(&destino),
                Arc::new(move |f: &UsoFila| apunte.lock().push(f.clone())),
                tx,
            )
            .await;
        });
        let puerto = rx.await.unwrap();

        let cuerpo = r#"{"model":"modelo-27b","messages":[{"role":"user","content":"hola"}]}"#;
        let salida = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{puerto}/v1/chat/completions"))
            .header("content-type", "application/json")
            .header("user-agent", "prueba/1.0")
            .body(cuerpo)
            .send()
            .await
            .expect("la puerta tiene que contestar");

        assert_eq!(salida.status().as_u16(), 200);
        // La respuesta es la del motor, byte a byte: la puerta no la reescribe.
        let devuelto = salida.text().await.unwrap();
        assert_eq!(devuelto, RESPUESTA_MOTOR, "el cuerpo no puede cambiar al pasar");

        // Y al motor le llegó el cuerpo de la petición tal cual, con su ruta.
        let visto = recibido.lock().clone();
        assert_eq!(visto.len(), 1, "el motor tiene que ver UNA petición");
        assert_eq!(visto[0].0, "POST");
        assert_eq!(visto[0].1, "/v1/chat/completions");
        assert_eq!(visto[0].2, cuerpo, "el cuerpo de la petición no puede cambiar");

        let filas = guardadas.lock().clone();
        assert_eq!(filas.len(), 1, "tiene que quedar UNA fila de uso");
        let f = &filas[0];
        assert_eq!(f.modelo, "modelo-27b");
        assert_eq!(f.ruta, "/v1/chat/completions");
        assert_eq!(f.estado, 200);
        assert_eq!(f.prompt_tokens, Some(1000));
        assert_eq!(f.completion_tokens, Some(250));
        assert_eq!(f.cached_tokens, Some(800));
        assert_eq!(f.origen, "local", "viene de loopback");
        assert_eq!(f.cliente, "prueba/1.0");
        assert!(f.ttft_ms.is_some(), "tiene que haber tiempo hasta el primer byte");
        assert!(f.duracion_ms >= 0);
    }

    /// Una petición que NO puede ser una inferencia (un `GET /v1/models`) se
    /// reenvía igual —la puerta es transparente— pero NO se cuenta: si contara, el
    /// número de peticiones dejaría de significar "turnos de generación".
    #[tokio::test]
    async fn un_get_se_reenvia_pero_no_se_cuenta() {
        let (destino, _) = motor_de_mentira(r#"{"data":[]}"#.into(), vec![]).await;
        let guardadas: Arc<Mutex<Vec<UsoFila>>> = Arc::new(Mutex::new(Vec::new()));
        let apunte = guardadas.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = servir_de_prueba(
                config_hacia(&destino),
                Arc::new(move |f: &UsoFila| apunte.lock().push(f.clone())),
                tx,
            )
            .await;
        });
        let puerto = rx.await.unwrap();

        let salida = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{puerto}/v1/models"))
            .send()
            .await
            .unwrap();
        assert_eq!(salida.status().as_u16(), 200);
        let _ = salida.text().await;
        // El cuerpo se suelta aquí; el guardia que escribe la fila se suelta con él.
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(
            guardadas.lock().is_empty(),
            "un GET sin uso no puede contar como petición"
        );
    }

    /// Con clave exigida: sin clave no se pasa, y CON clave sí.
    #[tokio::test]
    async fn la_clave_se_exige_y_no_se_reenvia_al_motor() {
        let (destino, recibido) = motor_de_mentira(RESPUESTA_MOTOR.into(), vec![]).await;
        let mut cfg = config_hacia(&destino);
        cfg.requiere_clave = true;
        cfg.clave = "secreta123".into();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = servir_de_prueba(cfg, Arc::new(|_: &UsoFila| {}), tx).await;
        });
        let puerto = rx.await.unwrap();
        let cliente = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{puerto}/v1/chat/completions");

        // Sin clave: 401, con el motivo escrito.
        let sin = cliente.post(&url).body("{}").send().await.unwrap();
        assert_eq!(sin.status().as_u16(), 401);
        let texto = sin.text().await.unwrap();
        assert!(
            texto.contains("clave"),
            "el 401 tiene que decir que falta la clave, no dejarlo adivinar: {texto}"
        );

        // Con la clave equivocada: tampoco.
        let mal = cliente
            .post(&url)
            .header("authorization", "Bearer otra")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(mal.status().as_u16(), 401);

        // Con la clave buena: pasa, y la clave NO se manda al motor.
        let bien = cliente
            .post(&url)
            .header("authorization", "Bearer secreta123")
            .body(r#"{"model":"m"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(bien.status().as_u16(), 200);
        let _ = bien.text().await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        let cabeceras = recibido.lock().clone();
        assert_eq!(cabeceras.len(), 1);
    }

    /// Si el motor no contesta, el error se dice y la petición FALLIDA también
    /// queda registrada: un motor caído es justo lo que hay que poder ver después.
    #[tokio::test]
    async fn un_motor_caido_da_502_y_queda_registrado() {
        // Puerto donde no escucha nadie.
        let cfg = config_hacia("http://127.0.0.1:9");
        let guardadas: Arc<Mutex<Vec<UsoFila>>> = Arc::new(Mutex::new(Vec::new()));
        let apunte = guardadas.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = servir_de_prueba(
                cfg,
                Arc::new(move |f: &UsoFila| apunte.lock().push(f.clone())),
                tx,
            )
            .await;
        });
        let puerto = rx.await.unwrap();

        let salida = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{puerto}/v1/chat/completions"))
            .body(r#"{"model":"m"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(salida.status().as_u16(), 502);
        let texto = salida.text().await.unwrap();
        assert!(texto.contains("no contestó"), "el motivo tiene que leerse: {texto}");

        tokio::time::sleep(Duration::from_millis(120)).await;
        let filas = guardadas.lock().clone();
        assert_eq!(filas.len(), 1);
        assert_eq!(filas[0].estado, 502);
        assert_eq!(filas[0].modelo, "m", "el modelo se saca de la petición aunque falle");
    }
}
