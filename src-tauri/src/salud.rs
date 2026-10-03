//! Autorreparación: lo que la app se arregla a sí misma, y cómo lo cuenta.
//!
//! POR QUÉ EXISTE. Todas las piezas de Machinograph pueden romperse por cosas que NO son
//! culpa del usuario y que, hasta ahora, se quedaban en un aviso: el puerto de la
//! puerta ocupado por otro programa, la base de datos del histórico corrupta, una
//! entrada de arranque que apunta a un binario que ya no está, un fichero de
//! configuración de un cliente que quedó a medias. Ninguna de esas cosas necesita
//! que el usuario diagnostique nada: la app sabe lo que había y sabe cómo
//! volverlo a poner.
//!
//! La regla de esta casa, aplicada a cada comprobación:
//!   * **Nada de datos del usuario se tira.** La base rota se APARTA con su fecha
//!     (`data.db.corrupta-…`), y el fichero de configuración roto se restaura
//!     desde su copia, que antes de pisarlo guarda lo que había.
//!   * **Lo que no se pudo arreglar se dice con su motivo**, nunca con un «bien»
//!     falso.
//!   * **Nada inventado**: cada fila sale de leer el estado real (el puerto que
//!     está escuchando, el `quick_check`, la entrada de arranque, el fichero).
//!
//! Todo esto corre solo al arrancar, en segundo plano, y también a mano (el
//! comando `salud:revisar` y `machinograph --cli salud`). Lo que se repara queda en el
//! historial de acciones (`db::insert_action`).
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use crate::{copias, db, entorno, gateway};

/* ── Lo que se devuelve ───────────────────────────────────────────────────── */

/// El estado de una comprobación de autorreparación. `Reparado` es un estado con
/// contenido: cuando se ve, abajo va QUÉ se hizo exactamente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoSalud {
    /// Estaba bien (o no había nada que mirar).
    Correcto,
    /// Estaba roto y se ha arreglado.
    Reparado,
    /// Estaba roto y NO se ha podido arreglar; el motivo va en el detalle.
    NoSePudo,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComprobacionSalud {
    /// Identificador estable: `gateway-puerto`, `base-datos`, `arranque`,
    /// `config:<ruta>`. Es lo que evita anotar dos veces la misma reparación.
    pub id: String,
    pub titulo: String,
    pub estado: EstadoSalud,
    /// Qué se ha encontrado y, si se reparó, qué se hizo (con las rutas).
    pub detalle: String,
    /// Cuando no se pudo arreglar: qué haría falta. `None` si no hay nada que hacer.
    pub como_arreglarlo: Option<String>,
}

impl ComprobacionSalud {
    fn nueva(
        id: &str,
        titulo: &str,
        estado: EstadoSalud,
        detalle: String,
        como_arreglarlo: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            titulo: titulo.into(),
            estado,
            detalle,
            como_arreglarlo,
        }
    }
}

/// El resultado de una pasada completa, con el resumen de una línea que se enseña
/// arriba de la tarjeta.
#[derive(Debug, Clone, Serialize)]
pub struct Revision {
    pub resumen: String,
    pub reparadas: usize,
    pub sin_arreglar: usize,
    pub comprobaciones: Vec<ComprobacionSalud>,
}

impl Revision {
    fn nueva(comprobaciones: Vec<ComprobacionSalud>) -> Self {
        let reparadas = comprobaciones.iter().filter(|c| c.estado == EstadoSalud::Reparado).count();
        let sin_arreglar = comprobaciones.iter().filter(|c| c.estado == EstadoSalud::NoSePudo).count();
        let resumen = if reparadas == 0 && sin_arreglar == 0 {
            "Todo correcto: no había nada que reparar.".to_string()
        } else {
            match (reparadas, sin_arreglar) {
                (r, 0) => format!("{} reparada{}.", r, if r == 1 { "" } else { "s" }),
                (0, s) => format!("{s} sin arreglar."),
                (r, s) => format!(
                    "{} reparada{} / {s} sin arreglar.",
                    r,
                    if r == 1 { "" } else { "s" }
                ),
            }
        };
        Self { resumen, reparadas, sin_arreglar, comprobaciones }
    }
}

/* ── La pasada completa ───────────────────────────────────────────────────── */

/// Revisa (y, si `reparar`, arregla) las cuatro cosas. Es asíncrona porque la
/// comprobación de la puerta tiene que darle un momento a que el servidor ate el
/// puerto antes de decir que no escucha.
pub async fn revisar(reparar: bool) -> Revision {
    let mut out = Vec::new();
    out.extend(base_de_datos());
    out.extend(puerta_de_enlace(reparar).await);
    out.extend(arranque_automatico(reparar));
    out.extend(ficheros_de_configuracion(reparar));
    if reparar {
        registrar(&out);
    }
    Revision::nueva(out)
}

/* ── 1. La base de datos del histórico ────────────────────────────────────── */

/// La base no se repara aquí: se repara al ABRIRLA (`db::connect_reparando` hace
/// el `quick_check` y aparta la rota). Esta comprobación solo la fuerza a abrir y
/// cuenta lo que pasó, para que la reparación se vea en vez de ocurrir en silencio.
fn base_de_datos() -> Vec<ComprobacionSalud> {
    let titulo = "Base de datos del histórico";
    match db::asegurar() {
        Ok(()) => {
            let reparacion = db::reparacion_bd();
            let (estado, detalle) = match reparacion {
                Some(aviso) => (EstadoSalud::Reparado, aviso),
                None => (
                    EstadoSalud::Correcto,
                    format!(
                        "{} está abierta y pasa la comprobación de integridad (quick_check).",
                        db::ruta_actual().display()
                    ),
                ),
            };
            vec![ComprobacionSalud::nueva("base-datos", titulo, estado, detalle, None)]
        }
        Err(motivo) => vec![ComprobacionSalud::nueva(
            "base-datos",
            titulo,
            EstadoSalud::NoSePudo,
            format!("La base de datos no quedó utilizable: {motivo}"),
            Some(format!(
                "Comprueba que se pueda escribir en la carpeta de {} y vuelve a lanzar «Reparar ahora». La app sigue funcionando, pero sin histórico.",
                db::ruta_actual().display()
            )),
        )],
    }
}

/* ── 2. El puerto de la puerta de enlace ──────────────────────────────────── */

/// El puerto real en el que escucha la puerta. Si el configurado está ocupado, la
/// puerta ya arranca sola en el siguiente (`gateway::atar`); esto lo CUENTA y, si
/// no hay nada escuchando, vuelve a intentarlo.
async fn puerta_de_enlace(reparar: bool) -> Vec<ComprobacionSalud> {
    let titulo = "Puerta de enlace (puerto)";
    let cfg = gateway::config();

    if !cfg.activa {
        return vec![ComprobacionSalud::nueva(
            "gateway-puerto",
            titulo,
            EstadoSalud::Correcto,
            format!(
                "La puerta está apagada en los ajustes: no hay nada escuchando en el {} ni nada que reparar. Si la enciendes, un puerto ocupado ya no la deja muerta: se prueban los {} siguientes.",
                cfg.puerto,
                gateway::REINTENTOS_PUERTO
            ),
            None,
        )];
    }

    // Un arranque en curso tarda un instante en atar el puerto: se le da ese
    // instante antes de decidir que falló.
    if reparar && gateway::puerto_escuchando().is_none() {
        esperar_puerto(Duration::from_millis(700)).await;
    }
    // Sigue sin escuchar y se pidió reparar: se vuelve a intentar. Si ya escucha,
    // `arrancar_si_activa` no hace nada (no deja dos servidores).
    if reparar && gateway::puerto_escuchando().is_none() {
        gateway::arrancar_si_activa();
        esperar_puerto(Duration::from_millis(1500)).await;
    }

    let Some(real) = gateway::puerto_escuchando() else {
        let motivo = gateway::error_arranque()
            .unwrap_or_else(|| "no hay constancia de que haya llegado a escuchar".to_string());
        return vec![ComprobacionSalud::nueva(
            "gateway-puerto",
            titulo,
            EstadoSalud::NoSePudo,
            format!("La puerta está activada pero no escucha en {}:{}. {motivo}", cfg.direccion, cfg.puerto),
            Some(format!(
                "Libera alguno de los puertos entre {} y {} (o cambia el puerto en Ajustes) y vuelve a lanzar «Reparar ahora».",
                cfg.puerto,
                cfg.puerto.saturating_add(gateway::REINTENTOS_PUERTO)
            )),
        )];
    };

    if real == cfg.puerto {
        return vec![ComprobacionSalud::nueva(
            "gateway-puerto",
            titulo,
            EstadoSalud::Correcto,
            format!("Escuchando en {}:{real}, el puerto configurado.", cfg.direccion),
            None,
        )];
    }
    let aviso = gateway::aviso_puerto().unwrap_or_else(|| {
        format!("El puerto configurado {} estaba ocupado; la puerta escucha en el {real}.", cfg.puerto)
    });
    let url = format!("http://{}:{real}/v1", cfg.direccion);
    vec![ComprobacionSalud::nueva(
        "gateway-puerto",
        titulo,
        EstadoSalud::Reparado,
        format!("{aviso} La puerta está en marcha en {url} y reenvía desde ahí (los clientes tienen que apuntar a esa URL)."),
        None,
    )]
}

/// Espera (con tope) a que la puerta anote su puerto, sin bloquear la ventana:
/// son esperas cortas y en un hilo asíncrono.
async fn esperar_puerto(tope: Duration) {
    let inicio = Instant::now();
    while inicio.elapsed() < tope {
        if gateway::puerto_escuchando().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/* ── 3. El arranque automático de la propia app ───────────────────────────── */

/// Ajuste donde se recuerda lo que el usuario pidió sobre el arranque automático.
///
/// Hace falta porque el estado que se lee del sistema solo dice si la entrada ESTÁ,
/// no si el usuario la quería: sin esta memoria, una entrada que desaparece sería
/// indistinguible de un usuario que nunca la activó.
pub const AJUSTE_PEDIDO: &str = "arranque_automatico_pedido";

/// Lo que se sabe del arranque automático: lo que el usuario pidió y cómo está la
/// entrada del sistema AHORA. Es lo que decide `hay_que_reparar`.
#[derive(Debug, Clone)]
pub struct EstadoArranque {
    /// El usuario lo tenía activado (ajuste guardado, o una entrada que ya existía).
    pub pedido: bool,
    pub activado: bool,
    /// El comando que lanzaría el sistema, tal cual lo lee `entorno`.
    pub comando: Option<String>,
}

/// ¿Hay que volver a escribir el arranque automático?
///
/// PURA a propósito: esta es la DECISIÓN, y se prueba sin tocar el sistema. Tres
/// casos, en este orden:
///   * el usuario NO lo pidió → `false`, jamás se activa por su cuenta;
///   * lo pidió y la entrada no está → `true`;
///   * la entrada está pero apunta a un binario que ya no existe (o no se puede
///     leer) → `true`, se reescribe apuntando al binario en marcha.
pub fn hay_que_reparar(estado: &EstadoArranque, exe: &Path) -> bool {
    if !estado.pedido {
        return false;
    }
    if !estado.activado {
        return true;
    }
    match estado.comando.as_deref().and_then(ruta_del_comando) {
        // El comando apunta a un binario que ya no está donde decía.
        Some(p) => !p.exists() && p != exe,
        // Sin comando legible no se puede afirmar que la entrada valga: se reescribe.
        None => true,
    }
}

/// Saca la ruta del binario de un comando guardado por el sistema: el primer
/// token, quitando comillas (Windows guarda `"C:\…\machinograph.exe"`) y los argumentos.
/// `None` si no hay nada que parezca una ruta ABSOLUTA (una ruta relativa no se
/// puede comprobar desde aquí, así que no se da por buena).
pub fn ruta_del_comando(comando: &str) -> Option<PathBuf> {
    let c = comando.trim();
    if c.is_empty() {
        return None;
    }
    let primero = if let Some(resto) = c.strip_prefix('"') {
        resto.split('"').next().unwrap_or(resto)
    } else {
        c.split_whitespace().next().unwrap_or(c)
    };
    let p = PathBuf::from(primero);
    p.is_absolute().then_some(p)
}

/// La intención del usuario. Si no hay ajuste (una instalación anterior a este
/// ajuste) se adopta lo que ya hubiera: una entrada activa es la prueba de que el
/// usuario la pidió. Sin ajuste y sin entrada, no se toca nada.
fn intencion(estado: &entorno::Arranque) -> bool {
    match db::get_setting(AJUSTE_PEDIDO).ok().flatten().as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => estado.activado,
    }
}

fn arranque_automatico(reparar: bool) -> Vec<ComprobacionSalud> {
    let titulo = "Arranque automático de Machinograph";
    let estado = entorno::arranque_estado();

    // Un estado que no se pudo leer no se juzga: se dice y no se toca nada.
    if let Some(e) = &estado.error {
        return vec![ComprobacionSalud::nueva(
            "arranque",
            titulo,
            EstadoSalud::NoSePudo,
            format!("No se pudo leer el arranque automático: {e}"),
            Some("Se puede activar o desactivar a mano en Ajustes; hasta entonces no se toca.".into()),
        )];
    }

    let pedido = intencion(&estado);
    let actual = EstadoArranque {
        pedido,
        activado: estado.activado,
        comando: estado.comando.clone(),
    };
    let Ok(exe) = std::env::current_exe() else {
        return vec![ComprobacionSalud::nueva(
            "arranque",
            titulo,
            EstadoSalud::NoSePudo,
            "No se pudo saber la ruta del binario en marcha, así que no se puede comprobar si la entrada de arranque apunta a él.".into(),
            Some("Vuelve a activar el arranque automático en Ajustes.".into()),
        )];
    };

    if !hay_que_reparar(&actual, &exe) {
        let detalle = if !pedido {
            format!("El usuario no lo pidió: no se toca nada ({}).", estado.fichero)
        } else {
            format!("Activado y apuntando a {}.", estado.comando.as_deref().unwrap_or(&estado.fichero))
        };
        return vec![ComprobacionSalud::nueva("arranque", titulo, EstadoSalud::Correcto, detalle, None)];
    }

    // Se pidió arrancar el arranque automático (nunca mejor dicho): hay que
    // reescribir la entrada. En modo consulta se dice lo que se haría.
    if !reparar {
        return vec![ComprobacionSalud::nueva(
            "arranque",
            titulo,
            EstadoSalud::NoSePudo,
            format!(
                "El usuario lo tenía activado, pero la entrada ({}) ya no está o apunta a un binario que no existe. No se ha tocado nada: se pidió solo comprobar.",
                estado.fichero
            ),
            Some("Lanza la reparación («Reparar ahora» en Diagnóstico, o `machinograph --cli salud --reparar`).".into()),
        )];
    }

    match entorno::arranque_configurar(true) {
        Ok(mensaje) => vec![ComprobacionSalud::nueva(
            "arranque",
            titulo,
            EstadoSalud::Reparado,
            format!("El usuario lo tenía activado y la entrada ({}) no estaba o apuntaba a un binario que ya no existe. Se ha vuelto a escribir: {mensaje}", estado.fichero),
            None,
        )],
        Err(e) => vec![ComprobacionSalud::nueva(
            "arranque",
            titulo,
            EstadoSalud::NoSePudo,
            format!("La entrada de arranque ({}) no estaba o estaba rota y no se pudo reescribir: {e}", estado.fichero),
            Some("Se puede volver a activar a mano en Ajustes (la casilla de arranque automático).".into()),
        )],
    }
}

/* ── 4. Los ficheros de configuración que ha escrito la app ───────────────── */

/// Los ficheros de configuración de clientes que Machinograph ha escrito, según el
/// índice de copias (`copias.rs`). Solo se puede revisar lo que uno mismo escribió,
/// y la copia es la prueba de que lo escribió.
///
/// El criterio de «es una configuración» sale del CONTENIDO de la última copia
/// —literalmente lo que la app escribió—, no del nombre: las configuraciones de
/// `conexiones.rs` son JSON con `providers`, y un lanzador `.desktop` no lo es, así
/// que de este último se ocupa la comprobación de arranque y aquí no se toca.
fn ficheros_de_configuracion(reparar: bool) -> Vec<ComprobacionSalud> {
    let rutas = match db::rutas_con_copia() {
        Ok(v) => v,
        Err(e) => {
            return vec![ComprobacionSalud::nueva(
                "config",
                "Configuración escrita por Machinograph",
                EstadoSalud::NoSePudo,
                format!("No se pudo leer el índice de copias para saber qué ficheros escribió Machinograph: {e}"),
                Some("Sin ese índice no se toca ningún fichero ajeno.".into()),
            )]
        }
    };

    let mut out = Vec::new();
    for ruta in rutas {
        let copia = db::copia_mas_reciente(&ruta).ok().flatten();
        if !es_configuracion_escrita(Path::new(&ruta), copia.as_ref()) {
            continue;
        }
        out.push(revisar_fichero(&ruta, copia, reparar));
    }

    if out.is_empty() {
        out.push(ComprobacionSalud::nueva(
            "config",
            "Configuración escrita por Machinograph",
            EstadoSalud::Correcto,
            "Todavía no ha escrito ningún fichero de configuración de cliente (los escribe al conectar uno, como el `models.json` de gentle-shell o de pi).".into(),
            None,
        ));
    }
    out
}

/// ¿Este fichero es una configuración de cliente que escribió Machinograph? Se decide
/// por la última copia; si no hay copia legible, por el nombre y solo cuando es
/// concluyente (`.json`).
fn es_configuracion_escrita(ruta: &Path, copia: Option<&db::CopiaRow>) -> bool {
    if let Some(c) = copia {
        if let Ok(texto) = std::fs::read_to_string(&c.ruta_copia) {
            return es_json_de_objeto(&texto);
        }
    }
    ruta.extension().map(|e| e.eq_ignore_ascii_case("json")).unwrap_or(false)
}

fn es_json_de_objeto(texto: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(texto)
        .map(|v| v.is_object())
        .unwrap_or(false)
}

/// Qué le pasa al fichero para no poder darlo por bueno. `None` = está bien.
///
/// El criterio es el que se usó al escribirlo: estos ficheros son un OBJETO JSON
/// (`conexiones.rs` escribe `providers`). Un fichero que no se puede leer, que
/// quedó vacío o que ya no parsea como objeto es exactamente lo que hay que
/// restaurar desde su copia.
fn problema_de_fichero(p: &Path) -> Option<String> {
    let texto = match std::fs::read_to_string(p) {
        Ok(t) => t,
        Err(e) => return Some(format!("No se puede leer {}: {e}.", p.display())),
    };
    if texto.trim().is_empty() {
        return Some(format!(
            "{} está vacío: ya no tiene la configuración que escribió Machinograph.",
            p.display()
        ));
    }
    if !es_json_de_objeto(&texto) {
        return Some(format!(
            "{} ya no parsea como el objeto JSON que escribió Machinograph.",
            p.display()
        ));
    }
    None
}

fn revisar_fichero(ruta: &str, copia: Option<db::CopiaRow>, reparar: bool) -> ComprobacionSalud {
    let p = Path::new(ruta);
    let id = format!("config:{ruta}");
    let nombre = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| ruta.to_string());
    let titulo = format!("Configuración escrita por Machinograph · {nombre}");

    let Some(problema) = problema_de_fichero(p) else {
        return ComprobacionSalud::nueva(
            &id,
            &titulo,
            EstadoSalud::Correcto,
            format!("{} se puede leer y tiene el formato que escribió Machinograph.", p.display()),
            None,
        );
    };

    let Some(copia) = copia else {
        return ComprobacionSalud::nueva(
            &id,
            &titulo,
            EstadoSalud::NoSePudo,
            format!("{problema} No hay ninguna copia de la que restaurarlo."),
            Some("Vuelve a aplicarlo desde Conexiones: Machinograph lo escribirá de nuevo (y guardará su copia).".into()),
        );
    };
    if !copia.existe {
        return ComprobacionSalud::nueva(
            &id,
            &titulo,
            EstadoSalud::NoSePudo,
            format!(
                "{problema} La copia más reciente está anotada en {} pero ya no está en el disco, así que no se puede restaurar.",
                copia.ruta_copia
            ),
            Some("Vuelve a aplicarlo desde Conexiones.".into()),
        );
    }

    if !reparar {
        return ComprobacionSalud::nueva(
            &id,
            &titulo,
            EstadoSalud::NoSePudo,
            format!(
                "{problema} Se restauraría desde la copia de {} (del {}), pero se pidió solo comprobar.",
                copia.ruta_copia,
                fecha_de(copia.ts)
            ),
            Some("Lanza la reparación («Reparar ahora» en Diagnóstico, o `machinograph --cli salud --reparar`).".into()),
        );
    }

    match copias::restaurar(copia.id) {
        Ok(mensaje) => ComprobacionSalud::nueva(
            &id,
            &titulo,
            EstadoSalud::Reparado,
            format!("{problema} {mensaje}"),
            None,
        ),
        Err(e) => ComprobacionSalud::nueva(
            &id,
            &titulo,
            EstadoSalud::NoSePudo,
            format!("{problema} No se pudo restaurar desde {}: {e}", copia.ruta_copia),
            Some("Se puede restaurar a mano desde el Centro de recuperación (Mantenimiento).".into()),
        ),
    }
}

/// Una fecha legible para los textos: las rutas y las fechas son lo que hace
/// comprobable una reparación.
fn fecha_de(ts: i64) -> String {
    match chrono::DateTime::from_timestamp(ts, 0) {
        Some(d) => d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
        None => format!("ts {ts}"),
    }
}

/* ── Contarlo ─────────────────────────────────────────────────────────────── */

/// Lo ya anotado en el historial, para no repetir la misma reparación en cada
/// pasada (el arranque y el botón pueden coincidir, y la pasada se puede lanzar
/// tantas veces como se quiera).
static YA_ANOTADO: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Anota en el historial de acciones lo que se reparó y lo que no se pudo. Las
/// filas correctas no se anotan: el historial es de ACCIONES, y «no había nada que
/// hacer» no es una acción.
fn registrar(comprobaciones: &[ComprobacionSalud]) {
    for c in comprobaciones {
        if c.estado == EstadoSalud::Correcto {
            continue;
        }
        if !YA_ANOTADO.lock().insert(c.id.clone()) {
            continue;
        }
        let ok = c.estado == EstadoSalud::Reparado;
        let mensaje = format!("{}: {}", c.titulo, c.detalle);
        if let Err(e) = db::insert_action("salud", &c.id, ok, &mensaje) {
            eprintln!("no se pudo anotar la reparación «{}» en el historial: {e}", c.id);
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Una ruta absoluta VÁLIDA en el sistema que corre.
    ///
    /// `ruta_del_comando` solo acepta rutas absolutas (`Path::is_absolute()`), y
    /// `/usr/bin/...` NO lo es en Windows (allí lo es `C:\...`). El ayudante pone
    /// la raíz de cada sistema para que la misma prueba valga en los tres.
    fn abs(p: &str) -> String {
        if cfg!(windows) {
            format!("C:/{}", p.trim_start_matches('/'))
        } else {
            format!("/{}", p.trim_start_matches('/'))
        }
    }

    fn estado(pedido: bool, activado: bool, comando: Option<&str>) -> EstadoArranque {
        EstadoArranque {
            pedido,
            activado,
            comando: comando.map(|c| c.to_string()),
        }
    }

    /// La regla que da título a esta casa: NUNCA se activa por su cuenta lo que el
    /// usuario no pidió. Aunque no haya entrada, si no lo pidió, no se toca.
    #[test]
    fn no_se_repara_lo_que_el_usuario_no_pidio() {
        let exe = Path::new("/opt/machinograph/machinograph");
        for comando in [None, Some("/opt/machinograph/machinograph"), Some("/no/existe/machinograph")] {
            assert!(
                !hay_que_reparar(&estado(false, false, comando), exe),
                "sin pedido no se toca nada (comando {comando:?})"
            );
        }
    }

    /// Si lo pidió y la entrada desapareció, hay que volver a escribirla.
    #[test]
    fn se_repara_la_entrada_que_desaparecio() {
        let exe = Path::new("/opt/machinograph/machinograph");
        assert!(hay_que_reparar(&estado(true, false, None), exe));
    }

    /// Si lo pidió y la entrada está, pero apunta a un binario que ya no existe
    /// (mover o desinstalar la app), se reescribe con el binario en marcha.
    #[test]
    fn se_repara_la_entrada_que_apunta_a_un_binario_que_no_esta() {
        let exe = Path::new("/opt/machinograph/machinograph");
        assert!(hay_que_reparar(&estado(true, true, Some("/ruta/que/ya/no/existe")), exe));
        // Una entrada ilegible (sin comando) tampoco se da por buena.
        assert!(hay_que_reparar(&estado(true, true, None), exe));
        // Y una ruta relativa no se puede comprobar: no se da por buena.
        assert!(hay_que_reparar(&estado(true, true, Some("machinograph")), exe));
    }

    /// Y si está bien, no se toca: reescribir lo que ya vale sería un cambio a
    /// espaldas del usuario.
    #[test]
    fn no_se_toca_una_entrada_que_apunta_al_binario_en_marcha() {
        let exe = std::env::current_exe().expect("la prueba corre en un binario");
        let ruta = exe.to_string_lossy().to_string();
        assert!(!hay_que_reparar(&estado(true, true, Some(&ruta)), &exe));
        // Con comillas y argumentos (formato de Windows) también se entiende.
        let con_comillas = format!("\"{ruta}\" --oculto");
        assert!(!hay_que_reparar(&estado(true, true, Some(&con_comillas)), &exe));
    }

    /// La ruta del comando se saca sin comillas y sin argumentos, y solo si es
    /// absoluta (una relativa no se puede comprobar desde aquí).
    #[test]
    fn la_ruta_del_comando_se_entiende_con_o_sin_comillas_y_con_argumentos() {
        let exe = abs("/usr/bin/machinograph");
        assert_eq!(ruta_del_comando(&exe), Some(PathBuf::from(&exe)));
        assert_eq!(
            ruta_del_comando(&format!("  {exe} --silencioso  ")),
            Some(PathBuf::from(&exe))
        );
        assert_eq!(
            ruta_del_comando(&format!("\"{exe}\" --silencioso")),
            Some(PathBuf::from(&exe))
        );
        assert_eq!(ruta_del_comando("machinograph"), None, "una ruta relativa no vale");
        assert_eq!(ruta_del_comando(""), None);
    }

    /// El formato de configuración se decide por el CONTENIDO de la copia (lo que
    /// la app escribió), no por el nombre: así un `.desktop` no entra aquí.
    #[test]
    fn solo_cuentan_las_configuraciones_json_que_escribio_la_app() {
        let base = std::env::temp_dir().join(format!("machinograph-salud-prueba-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let json = base.join("models.json.bak-1");
        std::fs::write(&json, br#"{"providers":{}}"#).unwrap();
        let desktop = base.join("machinograph.desktop.bak-1");
        std::fs::write(&desktop, "[Desktop Entry]\nExec=/usr/bin/machinograph\n").unwrap();

        let fila_de = |ruta: &Path| {
            let copia = base.join(format!("copia-{}", ruta.file_name().unwrap().to_string_lossy()));
            db::CopiaRow {
                id: 1,
                ts: 0,
                ruta_original: ruta.to_string_lossy().to_string(),
                ruta_copia: copia.to_string_lossy().to_string(),
                bytes: 0,
                motivo: "prueba".into(),
                existe: true,
            }
        };
        let fila = fila_de(&json);
        std::fs::copy(&json, &fila.ruta_copia).unwrap();
        assert!(
            es_configuracion_escrita(&json, Some(&fila)),
            "un JSON con providers sí es una configuración escrita"
        );

        let fila_desktop = fila_de(&desktop);
        std::fs::copy(&desktop, &fila_desktop.ruta_copia).unwrap();
        assert!(
            !es_configuracion_escrita(Path::new("/home/x/.config/autostart/machinograph.desktop"), Some(&fila_desktop)),
            "un lanzador .desktop no es una configuración de cliente"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// La comprobación de un fichero: correcto si es un objeto JSON; problema si
    /// está vacío o si ya no parsea. Es lo que decide si se restaura.
    #[test]
    fn un_fichero_ilegible_o_a_medias_da_problema() {
        let base = std::env::temp_dir().join(format!("machinograph-salud-problema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let bueno = base.join("bueno.json");
        std::fs::write(&bueno, br#"{"providers":{"x":1}}"#).unwrap();
        assert_eq!(problema_de_fichero(&bueno), None);

        let vacio = base.join("vacio.json");
        std::fs::write(&vacio, b"   \n").unwrap();
        let p = problema_de_fichero(&vacio).expect("vacío tiene que dar problema");
        assert!(p.contains("vacío"), "{p}");

        let roto = base.join("roto.json");
        std::fs::write(&roto, br#"{"providers":{"x":"#).unwrap();
        let p = problema_de_fichero(&roto).expect("a medias tiene que dar problema");
        assert!(p.contains("no parsea"), "{p}");

        let ausente = base.join("no-esta.json");
        assert!(problema_de_fichero(&ausente).is_some(), "si no se puede leer, es problema");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Ida y vuelta de verdad con un fichero temporal y su copia: se copia (queda
    /// anotada), se rompe el original a medias y la comprobación lo restaura desde
    /// la copia dejando el contenido igual que estaba. Y lo roto NO se pierde:
    /// antes de pisarlo se guarda, como en `copias.rs`.
    ///
    /// Usa la base de datos real (el mismo patrón que las pruebas de `copias.rs`) y
    /// recoge lo que ensucia; si la base no está disponible, lo dice y no finge.
    #[test]
    fn un_fichero_roto_se_restaura_desde_su_copia_en_ida_y_vuelta() {
        let dir = std::env::temp_dir().join(format!("machinograph-salud-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("models.json");
        let bueno: &[u8] = br#"{"providers":{"gentle-shell":{"api":"http://127.0.0.1:8090/v1"}}}"#;
        std::fs::write(&f, bueno).unwrap();

        // La copia con fecha y anotada en el índice: es lo que deja `conexiones.rs`
        // al escribir en el fichero de un cliente.
        if let Err(e) = copias::copia_con_motivo(&f, "prueba de ida y vuelta") {
            assert!(e.contains("no se pudo anotar"), "{e}");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let copia = db::copia_mas_reciente(&f.to_string_lossy())
            .unwrap()
            .expect("la copia tiene que estar anotada");

        // El fichero se rompe: es justo el caso que tiene que cazar el arranque.
        std::fs::write(&f, br#"{"providers":{"gentle-shell":"#).unwrap();
        assert!(problema_de_fichero(&f).is_some(), "roto tiene que dar problema");

        let fila = revisar_fichero(&f.to_string_lossy(), Some(copia), true);
        assert_eq!(fila.estado, EstadoSalud::Reparado, "{}", fila.detalle);
        assert_eq!(
            std::fs::read(&f).unwrap(),
            bueno,
            "el fichero tiene que quedar como la copia"
        );
        let copias_en_disco = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak-"))
            .count();
        assert!(
            copias_en_disco >= 2,
            "tiene que quedar la copia original Y la previa a restaurar: {copias_en_disco}"
        );

        // Se recoge lo que esta prueba ha dejado en la base de datos de verdad.
        let original = f.to_string_lossy().to_string();
        for c in db::copias(1000).unwrap() {
            if c.ruta_original == original {
                let _ = db::borrar_copia(c.id);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// El resumen de una línea: «todo correcto» cuando no hay nada, y los conteos
    /// cuando sí. En ningún caso un «bien» falso.
    #[test]
    fn el_resumen_cuenta_lo_reparado_y_lo_que_no_se_pudo() {
        let c = |estado| ComprobacionSalud::nueva("x", "t", estado, "d".into(), None);
        let todo = Revision::nueva(vec![c(EstadoSalud::Correcto), c(EstadoSalud::Correcto)]);
        assert!(todo.resumen.contains("Todo correcto"), "{}", todo.resumen);
        assert_eq!((todo.reparadas, todo.sin_arreglar), (0, 0));

        let mezcla = Revision::nueva(vec![
            c(EstadoSalud::Reparado),
            c(EstadoSalud::Reparado),
            c(EstadoSalud::NoSePudo),
        ]);
        assert_eq!((mezcla.reparadas, mezcla.sin_arreglar), (2, 1));
        assert!(mezcla.resumen.contains('2') && mezcla.resumen.contains('1'), "{}", mezcla.resumen);

        let una = Revision::nueva(vec![c(EstadoSalud::Reparado)]);
        assert!(una.resumen.contains("1 reparada"), "{}", una.resumen);
    }
}
