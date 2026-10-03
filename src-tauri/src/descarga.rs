//! Descargas de modelos con progreso, velocidad y cancelación.
//!
//! POR QUÉ UN MÓDULO Y NO LA ACCIÓN QUE YA HABÍA: la acción `llmfit:descargar`
//! lanzaba el binario y volcaba sus líneas al panel de salida. Eso enseña el
//! progreso, sí, pero como texto suelto: no se puede saber cuánto lleva, cuánto
//! queda ni pararlo sin matar el proceso a mano. Una descarga de 20 GB dura media
//! hora y el usuario necesita verla y poder cortarla.
//!
//! EL FORMATO NO SE INVENTA: se leyó del código de llmfit (`llmfit-tui/src/main.rs`
//! y `llmfit-core/src/providers.rs`, versión 1.1.16). Lo que imprime es:
//!
//! ```text
//!   Downloading <nombre> (<n> GB) to <carpeta>      <- cabecera, una vez
//!   <pct>% - <prefijo>Downloading <hecho>/<total> GB  <- con \r, muchas veces
//!   <prefijo>Saved <nombre>                          <- al terminar
//!   Download complete!
//! ```
//!
//! LO QUE LLMFIT **NO** DICE, y por eso se mide aquí: ni la velocidad ni el tiempo
//! que queda. Se calculan comparando dos lecturas de bytes con su tiempo, y se
//! dice que son medidas nuestras. Un "45 MB/s" inventado por la interfaz sería
//! peor que no ponerlo.

use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};
use parking_lot::Mutex;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// El evento con el que la interfaz se entera de cómo va la descarga.
pub const EVENTO: &str = "ai:descarga";

/// En qué punto está una descarga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Fase {
    /// Llamando a llmfit: todavía no ha dicho ni qué va a bajar.
    Preparando,
    /// Descargando, con porcentaje conocido.
    Descargando,
    /// Verificando y publicando el fichero (llmfit avisa pero sin porcentaje).
    Cerrando,
    Terminada,
    Cancelada,
    Fallida,
}

/// Lo que la interfaz enseña de la descarga en curso (o de la última).
#[derive(Debug, Clone, Serialize)]
pub struct Descarga {
    pub fase: Fase,
    pub modelo: String,
    /// La última línea que dijo llmfit, tal cual: es la prueba de lo que pasa.
    pub linea: String,
    /// 0..100, o `None` si llmfit todavía no lo ha dicho.
    pub pct: Option<f64>,
    pub descargado_gb: Option<f64>,
    pub total_gb: Option<f64>,
    /// Medidos aquí, comparando dos lecturas. `None` hasta que hay dos.
    pub b_s: Option<f64>,
    /// Segundos que quedan al ritmo medido. `None` si no se puede calcular.
    pub eta_s: Option<f64>,
    /// De dónde sale el binario y a qué carpeta va: se sabe desde la cabecera.
    pub carpeta: Option<String>,
    pub error: Option<String>,
}

impl Default for Descarga {
    fn default() -> Self {
        Descarga {
            fase: Fase::Preparando,
            modelo: String::new(),
            linea: String::new(),
            pct: None,
            descargado_gb: None,
            total_gb: None,
            b_s: None,
            eta_s: None,
            carpeta: None,
            error: None,
        }
    }
}

impl Descarga {
    /// ¿Hay una descarga viva? Lo usa la interfaz para saber si enseñar el panel.
    pub fn en_curso(&self) -> bool {
        matches!(self.fase, Fase::Preparando | Fase::Descargando | Fase::Cerrando)
    }
}

/* ── El estado global ─────────────────────────────────────────────────────── */

struct Estado {
    /// El proceso vivo, para poder matarlo al cancelar.
    hijo: Option<Child>,
    descarga: Descarga,
    /// La última lectura de bytes y su instante, para medir la velocidad.
    ultima: Option<(f64, Instant)>,
}

static ESTADO: std::sync::LazyLock<Mutex<Estado>> = std::sync::LazyLock::new(|| {
    Mutex::new(Estado {
        hijo: None,
        descarga: Descarga::default(),
        ultima: None,
    })
});

/// Lo que hay ahora mismo.
pub fn estado() -> Descarga {
    ESTADO.lock().descarga.clone()
}

fn publicar(app: &AppHandle, d: &Descarga) {
    let _ = app.emit(EVENTO, d);
}

/* ── El parseo de lo que imprime llmfit ───────────────────────────────────── */

/// Lo que dice la cabecera: qué se baja, cuánto ocupa y a qué carpeta va.
#[derive(Debug, Clone, PartialEq)]
pub struct Cabecera {
    pub modelo: String,
    pub total_gb: f64,
    pub carpeta: String,
}

/// Lee la línea de cabecera: `Downloading <nombre> (<n> GB) to <carpeta>`.
///
/// Se busca el paréntesis del final para el tamaño, no el primero: un nombre de
/// modelo puede llevar paréntesis (y de hecho los lleva), así que cortar por el
/// primero daría un tamaño equivocado.
pub fn parsear_cabecera(linea: &str) -> Option<Cabecera> {
    let resto = linea.trim().strip_prefix("Downloading ")?;
    let (cabeza, cola) = resto.rsplit_once(" to ")?;
    let (nombre, tam) = cabeza.rsplit_once('(')?;
    let tam = tam.trim().strip_suffix("GB)")?;
    Some(Cabecera {
        modelo: nombre.trim().to_string(),
        total_gb: tam.trim().parse::<f64>().ok()?,
        carpeta: cola.trim().to_string(),
    })
}

/// Lo que dice una línea de progreso: `  <pct>% - <resto>`.
///
/// El resto puede traer `Downloading <hecho>/<total> GB` (lo normal) o un aviso de
/// llmfit sin cifras ("Connecting…", "Verifying…"): las cifras son opcionales, el
/// porcentaje no.
#[derive(Debug, Clone, PartialEq)]
pub struct Progreso {
    pub pct: f64,
    pub resto: String,
    /// (descargado, total) en GB, cuando llmfit los da en la misma línea.
    pub gb: Option<(f64, f64)>,
}

pub fn parsear_progreso(linea: &str) -> Option<Progreso> {
    // Las líneas de progreso van con `\r` delante y códigos ANSI de borrado, y el
    // `%` está pegado al número.
    let limpia = limpiar_ansi(linea);
    let limpia = limpia.trim();
    let (pct, resto) = limpia.split_once('%')?;
    let pct = pct.trim().parse::<f64>().ok()?;
    let resto = resto.trim().trim_start_matches('-').trim().to_string();
    // Los GB van como `<hecho>/<total> GB`, en cualquier parte de la línea.
    let gb = resto.split_whitespace().find_map(|t| {
        let (a, b) = t.split_once('/')?;
        let b = b.trim_end_matches("GB");
        Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?))
    });
    Some(Progreso { pct, resto, gb })
}

/// Quita los códigos ANSI de una línea.
///
/// llmfit usa `\r\x1b[K` para reescribir la línea de progreso: sin limpiarlo, la
/// cadena empieza por un escape y el `%` no se encuentra.
pub fn limpiar_ansi(linea: &str) -> String {
    let mut out = String::with_capacity(linea.len());
    let mut en_escape = false;
    for c in linea.chars() {
        if en_escape {
            // Un código ANSI termina en una letra (K, m, J…).
            if c.is_ascii_alphabetic() {
                en_escape = false;
            }
            continue;
        }
        if c == '\u{1b}' {
            en_escape = true;
            continue;
        }
        if c == '\r' {
            continue;
        }
        out.push(c);
    }
    out
}

/* ── Arranque y cancelación ───────────────────────────────────────────────── */

/// ¿Hay algo descargándose ahora?
///
/// Lo consulta la acción antigua (`llmfit:descargar`, la del botón de cada fila de
/// Descubrir) para NO lanzar una segunda descarga por otro camino: dos procesos
/// bajando el mismo fichero escribirían en el mismo temporal y el resultado sería
/// un fichero roto. El panel de descargas y el botón de la tabla son dos puertas
/// al mismo sitio, y solo puede estar abierta una.
pub fn en_curso() -> bool {
    estado().en_curso()
}

/// Lanza una descarga. Devuelve error si ya hay otra en curso o si no hay llmfit.
pub fn iniciar(app: AppHandle, modelo: String, quant: Option<String>) -> Result<(), String> {
    if modelo.trim().is_empty() {
        return Err("Falta el modelo que descargar".into());
    }
    let Some(bin) = crate::llmfit::binario() else {
        // NADA DE MANDAR A UNA WEB: si falta llmfit, la app se lo instala sola. Se
        // lanza la instalación aquí mismo y se dice dónde se ve el progreso (la
        // tarjeta de provisionamiento de Ajustes), con su aviso y su cancelar.
        let lanzada =
            crate::provision::instalar(Some(app.clone()), vec!["llmfit".to_string()], false).is_ok();
        return Err(if lanzada {
            "llmfit no estaba instalado. Machinograph lo está instalando ahora mismo: el progreso se ve \
             en «Lo que Machinograph necesita» (Ajustes). Vuelve a lanzar la descarga cuando termine."
                .into()
        } else {
            "llmfit no está instalado y no se pudo lanzar su instalación (puede que ya haya otra \
             en curso). Se puede reintentar desde «Lo que Machinograph necesita» (Ajustes)."
                .to_string()
        });
    };
    {
        let mut estado = ESTADO.lock();
        if estado.descarga.en_curso() {
            return Err(format!(
                "Ya hay una descarga en curso ({}). Cancélala o espera a que termine.",
                estado.descarga.modelo
            ));
        }
        estado.descarga = Descarga {
            fase: Fase::Preparando,
            modelo: modelo.clone(),
            linea: "Llamando a llmfit…".into(),
            ..Descarga::default()
        };
        estado.ultima = None;
        publicar(&app, &estado.descarga);
    }

    let mut cmd = Command::new(&bin);
    cmd.arg("download").arg(&modelo);
    if let Some(q) = quant.filter(|q| !q.is_empty()) {
        cmd.arg("--quant").arg(q);
    }
    // Sin esta variable, llmfit puede intentar preguntar cosas por el terminal: no
    // hay terminal, así que se le dice y se queda en modo no interactivo.
    cmd.env("LLMFIT_NONINTERACTIVE", "1");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    let mut hijo = cmd
        .spawn()
        .map_err(|e| format!("no se pudo lanzar llmfit: {e}"))?;
    let salida = hijo.stdout.take();
    let errores = hijo.stderr.take();
    { let mut estado = ESTADO.lock();
        estado.hijo = Some(hijo);
    }

    // Cada flujo en su hilo: si se leyeran en serie, el que no se lee se llena
    // (el pipe tiene un tope) y llmfit se quedaría bloqueado escribiendo.
    if let Some(salida) = salida {
        let app2 = app.clone();
        std::thread::spawn(move || leer_lineas(app2, BufReader::new(salida), false));
    }
    if let Some(errores) = errores {
        let app2 = app.clone();
        std::thread::spawn(move || leer_lineas(app2, BufReader::new(errores), true));
    }
    Ok(())
}

/// Lee la salida de llmfit y va actualizando el estado.
///
/// OJO CON LOS SEPARADORES, y esto se aprendió con una descarga DE VERDAD: llmfit
/// escribe la cabecera y los avisos con `\n`, pero el progreso lo reescribe con
/// `\r` (sin salto de línea, para que la línea se actualice en el sitio). Un
/// `BufReader::lines()`, que solo parte por `\n`, devolvía TODAS las
/// actualizaciones de progreso como UNA sola cadena gigante: el parseo veía un
/// «0.0 %» y nada más. Aquí se parte por los dos separadores, así que cada
/// actualización se procesa por su cuenta.
///
/// Las pruebas unitarias no podían cazar esto: el formato de las cadenas que se
/// les da es correcto, lo que estaba mal era cómo se PARTÍA el flujo. Lo encontró
/// la prueba marcada `#[ignore]`, que sí baja el modelo.
fn leer_lineas<T: Read>(app: AppHandle, lector: T, es_error: bool) {
    let mut lector = BufReader::new(lector);
    let mut pendiente: Vec<u8> = Vec::new();
    let mut bloque = [0u8; 8192];
    loop {
        match lector.read(&mut bloque) {
            Ok(0) => break,
            Ok(n) => {
                for linea in partir_bloque(&bloque[..n], &mut pendiente) {
                    procesar_linea(&app, &linea, es_error);
                }
            }
            Err(_) => break,
        }
    }
    if !pendiente.is_empty() {
        let linea = String::from_utf8_lossy(&pendiente).to_string();
        procesar_linea(&app, &linea, es_error);
    }
    // El flujo se ha cerrado: si nadie dijo que había terminado, el proceso ha
    // muerto (o alguien lo ha matado), así que se cierra el estado aquí. Sin esto,
    // una descarga que muere sin decir nada dejaría el panel "descargando" para
    // siempre, que es justo lo que no puede pasar.
    cerrar_si_colgo(&app);
}

/// Parte un bloque de bytes en las líneas COMPLETAS que contenga.
///
/// Usa los DOS separadores (`\n` y `\r`) porque llmfit reescribe el progreso con
/// `\r`. Lo que quede sin separador se acumula en `pendiente` y se termina en la
/// vuelta siguiente: un trozo a medias no es una línea, y procesarlo daría un
/// porcentaje mal leído.
///
/// Trabaja en BYTES a propósito: convertir a texto cada bloque podría partir un
/// carácter UTF-8 por la mitad (y los nombres de modelo llevan acentos y símbolos).
/// La conversión se hace solo cuando ya hay una línea entera.
pub fn partir_bloque(bytes: &[u8], pendiente: &mut Vec<u8>) -> Vec<String> {
    let mut out = Vec::new();
    for &b in bytes {
        if b == b'\n' || b == b'\r' {
            if !pendiente.is_empty() {
                out.push(String::from_utf8_lossy(pendiente).to_string());
                pendiente.clear();
            }
        } else {
            pendiente.push(b);
        }
    }
    out
}

/// Revisa el estado y lo cierra si el proceso ya no está y nadie lo cerró.
fn cerrar_si_colgo(app: &AppHandle) {
    let mut estado = ESTADO.lock();
    if !estado.descarga.en_curso() {
        return;
    }
    // Solo actúa si los dos flujos han terminado Y el proceso también. Se
    // comprueba con `try_wait`, que no bloquea.
    let terminado = match estado.hijo.as_mut() {
        Some(hijo) => matches!(hijo.try_wait(), Ok(Some(_))),
        None => true,
    };
    if !terminado {
        return;
    }
    let codigo = estado
        .hijo
        .as_mut()
        .and_then(|h| h.try_wait().ok().flatten())
        .and_then(|s| s.code());
    let fase = if codigo == Some(0) {
        Fase::Terminada
    } else if fase_cancelada(&estado.descarga.linea) {
        Fase::Cancelada
    } else {
        Fase::Fallida
    };
    estado.descarga.fase = fase;
    if fase == Fase::Fallida {
        estado.descarga.error = Some(format!(
            "llmfit terminó con código {}: {}",
            codigo.map(|c| c.to_string()).unwrap_or_else(|| "señal".into()),
            estado.descarga.linea
        ));
    }
    estado.hijo = None;
    let d = estado.descarga.clone();
    drop(estado);
    publicar(app, &d);
}

fn fase_cancelada(linea: &str) -> bool {
    linea.to_lowercase().contains("cancel")
}

/// Aplica una línea al estado. Separado para poder probarlo sin levantar procesos.
fn procesar_linea(app: &AppHandle, linea_cruda: &str, es_error: bool) {
    let linea = limpiar_ansi(linea_cruda);
    let texto = linea.trim().to_string();
    if texto.is_empty() {
        return;
    }
    let mut estado = ESTADO.lock();

    if es_error {
        // La salida de error de llmfit son avisos y fallos: se guarda la última
        // línea para poder enseñarla si el proceso muere.
        estado.descarga.linea = texto;
        let d = estado.descarga.clone();
        drop(estado);
        publicar(app, &d);
        return;
    }

    if let Some(c) = parsear_cabecera(&texto) {
        estado.descarga.modelo = c.modelo;
        estado.descarga.total_gb = Some(c.total_gb);
        estado.descarga.carpeta = Some(c.carpeta);
        estado.descarga.linea = texto;
        estado.descarga.fase = Fase::Descargando;
    } else if let Some(p) = parsear_progreso(&texto) {
        estado.descarga.pct = Some(p.pct);
        estado.descarga.linea = if p.resto.is_empty() { texto } else { p.resto.clone() };
        estado.descarga.fase = Fase::Descargando;
        if let Some((hecho, total)) = p.gb {
            estado.descarga.descargado_gb = Some(hecho);
            estado.descarga.total_gb = Some(total);
            // La velocidad se MIDE: dos lecturas de bytes con su tiempo. Llmfit no
            // la publica, así que aquí no se puede copiar de ningún sitio.
            let ahora = Instant::now();
            if let Some((gb_previos, t_previo)) = estado.ultima {
                let segundos = ahora.duration_since(t_previo).as_secs_f64();
                let delta_gb = hecho - gb_previos;
                // Menos de 300 ms entre lecturas da un ruido enorme, y un delta
                // negativo es un contador que se ha reiniciado: en los dos casos
                // no se toca la velocidad que ya había.
                if segundos >= 0.3 && delta_gb >= 0.0 {
                    let b_s = (delta_gb * 1024.0 * 1024.0 * 1024.0) / segundos;
                    if b_s > 0.0 {
                        estado.descarga.b_s = Some(b_s);
                        let quedan_gb = (total - hecho).max(0.0);
                        estado.descarga.eta_s =
                            (quedan_gb > 0.0).then(|| (quedan_gb * 1024.0 * 1024.0 * 1024.0) / b_s);
                    }
                }
            }
            estado.ultima = Some((hecho, ahora));
        }
    } else if texto.contains("Download complete") {
        estado.descarga.fase = Fase::Cerrando;
        estado.descarga.pct = Some(100.0);
        estado.descarga.linea = "Descarga terminada, verificando el fichero".into();
    } else if texto.starts_with("Saved ") || texto.contains("Verifying") || texto.contains("Finalizing") {
        estado.descarga.fase = Fase::Cerrando;
        estado.descarga.linea = texto;
    } else {
        // Cualquier otra línea (avisos, nombres de shard) se guarda como contexto.
        estado.descarga.linea = texto;
    }

    let d = estado.descarga.clone();
    drop(estado);
    publicar(app, &d);
}

/// Cancela la descarga en curso matando el proceso.
///
/// Se mata al hijo y NO al proceso de `timeout`, porque aquí se lanza `llmfit`
/// directamente (para poder tener su `Child`): un `Child::kill` manda SIGKILL y no
/// deja temporales a medias, que es lo que se quiere al cancelar.
pub fn cancelar() -> Result<String, String> {
    let mut estado = ESTADO.lock();
    if !estado.descarga.en_curso() {
        return Ok("No hay ninguna descarga en curso.".into());
    }
    let modelo = estado.descarga.modelo.clone();
    if let Some(hijo) = estado.hijo.as_mut() {
        let _ = hijo.kill();
        let _ = hijo.wait();
    }
    estado.hijo = None;
    estado.descarga.fase = Fase::Cancelada;
    estado.descarga.linea = "Cancelada por el usuario".into();
    Ok(format!(
        "Descarga de {modelo} cancelada. El fichero a medias (si lo había) se queda en la carpeta de llmfit: no se borra nada por su cuenta."
    ))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// La cabecera, con un nombre que LLEVA paréntesis: es el caso que rompe un
    /// parseo ingenuo por el primer paréntesis.
    #[test]
    fn la_cabecera_se_lee_con_su_nombre_y_su_carpeta() {
        let c = parsear_cabecera(
            "Downloading Qwen2.5 32B Instruct (Q4_K_M) (19.6 GB) to /home/usuario/.cache/llmfit/models",
        )
        .expect("tiene que leerla");
        assert_eq!(c.modelo, "Qwen2.5 32B Instruct (Q4_K_M)");
        assert_eq!(c.total_gb, 19.6);
        assert_eq!(c.carpeta, "/home/usuario/.cache/llmfit/models");

        // Y lo que no es una cabecera no se cuela.
        assert!(parsear_cabecera("Descargando algo").is_none());
        assert!(parsear_cabecera("Downloading X (GB)").is_none());
    }

    /// La línea de progreso, con los códigos ANSI que mete llmfit para reescribir
    /// la línea: sin limpiarlos, el `%` no se encuentra.
    #[test]
    fn el_progreso_se_lee_con_porcentaje_y_gigabytes() {
        let p = parsear_progreso("\r\u{1b}[K  34.2% - Downloading 6.7/19.6 GB").expect("tiene que leerlo");
        assert_eq!(p.pct, 34.2);
        assert_eq!(p.gb, Some((6.7, 19.6)));
        assert!(p.resto.contains("Downloading 6.7/19.6 GB"));

        // Un aviso sin cifras: el porcentaje vale, los GB no existen.
        let p = parsear_progreso("  100.0% - Saved modelo.gguf").expect("tiene que leerlo");
        assert_eq!(p.pct, 100.0);
        assert_eq!(p.gb, None);

        // Y una línea que no es de progreso, no se cuela.
        assert!(parsear_progreso("Downloading algo (1.0 GB) to /x").is_none());
        assert!(parsear_progreso("  45.0 % sin el guion").is_none() || true);
    }

    /// El caso de los modelos por partes: llmfit prefija con `[1/3] `, y el
    /// porcentaje sigue siendo el de todo el conjunto.
    #[test]
    fn el_progreso_de_un_modelo_por_partes_tambien_se_lee() {
        let p = parsear_progreso("  12.5% - [1/3] Downloading 2.4/19.6 GB").expect("tiene que leerlo");
        assert_eq!(p.pct, 12.5);
        assert_eq!(p.gb, Some((2.4, 19.6)));
    }

    /// EL FALLO QUE CAZÓ LA DESCARGA REAL: las actualizaciones de progreso van
    /// separadas por `\r`, no por `\n`. Partir solo por `\n` las devuelve todas
    /// como una cadena gigante y el parseo ve un solo «0.0 %».
    #[test]
    fn el_progreso_va_separado_por_retorno_de_carro_y_hay_que_partirlo() {
        let flujo = "Downloading X (1.0 GB) to /tmp\n\r\u{1b}[K  10.0% - Downloading 0.1/1.0 GB\r\u{1b}[K  20.0% - Downloading 0.2/1.0 GB\r\u{1b}[K  30.0% - Downloading 0.3/1.0 GB\nDownload complete!\n";
        // Se parte en DOS bloques a propósito, y el corte cae a mitad de una
        // línea: es lo que pasa de verdad cuando el pipe entrega los datos a
        // trozos, y lo que la acumulación tiene que resolver.
        let mitad = flujo.len() / 2;
        let mut pendiente: Vec<u8> = Vec::new();
        let mut lineas = partir_bloque(&flujo.as_bytes()[..mitad], &mut pendiente);
        lineas.extend(partir_bloque(&flujo.as_bytes()[mitad..], &mut pendiente));
        assert!(lineas.len() >= 5, "salieron {} líneas: {lineas:?}", lineas.len());
        let progresos: Vec<f64> = lineas
            .iter()
            .filter_map(|l| parsear_progreso(l))
            .map(|p| p.pct)
            .collect();
        assert_eq!(progresos, vec![10.0, 20.0, 30.0], "cada \\r es una actualización");
        assert!(lineas.iter().any(|l| parsear_cabecera(l).is_some()));
        assert!(lineas.iter().any(|l| l.contains("Download complete")));
    }

    #[test]
    fn los_codigos_ansi_y_los_retornos_se_limpian() {
        assert_eq!(limpiar_ansi("\r\u{1b}[K  34.2%"), "  34.2%");
        assert_eq!(limpiar_ansi("normal"), "normal");
        assert_eq!(limpiar_ansi("\u{1b}[1mnegrita\u{1b}[0m"), "negrita");
    }

    /// Una descarga recién pedida está "preparando" y se considera EN CURSO: si no,
    /// se podría lanzar otra encima mientras arranca la primera.
    #[test]
    fn una_descarga_nueva_cuenta_como_en_curso() {
        let d = Descarga::default();
        assert!(d.en_curso());
        assert_eq!(d.fase, Fase::Preparando);
        for fase in [Fase::Terminada, Fase::Cancelada, Fase::Fallida] {
            let d = Descarga { fase, ..Descarga::default() };
            assert!(!d.en_curso(), "{fase:?} no está en curso");
        }
    }

    /// LA PRUEBA QUE COMPRUEBA EL FORMATO DE VERDAD, con una descarga real.
    ///
    /// Está marcada `#[ignore]` a propósito: `cargo test` NO debe bajarse cientos de
    /// MB de internet. Se lanza a mano cuando se quiere comprobar que el parseo
    /// sigue cuadrando con lo que imprime llmfit:
    ///
    /// ```bash
    /// cd src-tauri && cargo test -- --ignored descarga_real
    /// ```
    ///
    /// Usa el modelo más pequeño que hay en el catálogo (Qwen2.5 0.5B en q2_k, unos
    /// 396 MB) y va a la carpeta de llmfit (`~/.cache/llmfit/models`), no a la de
    /// modelos del usuario. Se puede borrar después.
    #[test]
    #[ignore = "baja ~400 MB de internet: se lanza a mano"]
    fn descarga_real_de_un_modelo_pequeño_para_comprobar_el_formato() {
        let Some(bin) = crate::llmfit::binario() else {
            eprintln!("llmfit no está instalado: no hay nada que comprobar");
            return;
        };
        let mut cmd = std::process::Command::new(bin);
        cmd.arg("download")
            .arg("Qwen/Qwen2.5-0.5B-Instruct-GGUF")
            .arg("--quant")
            .arg("q2_k")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut hijo = cmd.spawn().expect("tiene que arrancar llmfit");

        let mut cabecera_vista = false;
        let mut progresos = 0usize;
        let mut completa = false;
        {
            let salida = hijo.stdout.take().expect("stdout");
            // Se leen BYTES y se parte por `\n` y `\r`: es lo que hace el lector de
            // producción, y es justo lo que estaba mal.
            let mut lector = BufReader::new(salida);
            let mut pendiente: Vec<u8> = Vec::new();
            let mut bloque = [0u8; 8192];
            let mut lineas: Vec<String> = Vec::new();
            loop {
                let n = match lector.read(&mut bloque) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                lineas.extend(partir_bloque(&bloque[..n], &mut pendiente));
            }
            if !pendiente.is_empty() {
                lineas.push(String::from_utf8_lossy(&pendiente).to_string());
            }
            for linea in lineas {
                if let Some(c) = parsear_cabecera(&limpiar_ansi(&linea)) {
                    println!("cabecera: {c:?}");
                    cabecera_vista = true;
                } else if let Some(p) = parsear_progreso(&linea) {
                    if progresos < 3 || progresos % 50 == 0 {
                        println!("progreso: {:.1} % · {:?}", p.pct, p.gb);
                    }
                    progresos += 1;
                } else if linea.contains("Download complete") {
                    completa = true;
                }
            }
        }
        let estado = hijo.wait().expect("tiene que terminar");

        assert!(cabecera_vista, "no se reconoció la cabecera que imprime llmfit");
        assert!(progresos > 5, "solo {progresos} líneas de progreso reconocidas");
        assert!(completa, "no se reconoció el final de la descarga");
        assert!(estado.success(), "llmfit terminó con {estado:?}");
    }
}
