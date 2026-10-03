mod types;
mod system;
mod gpu;
mod inventario;
// Almacenamiento y optimización: el analizador de disco, la limpieza de basura,
// el arranque automático y la papelera que comparten. `papelera` va aparte porque
// la usan tanto el inventario de modelos como el analizador.
mod almacen;
// Lo que se puede hacer por el equipo sin abrir nada: comprobar actualizaciones y
// la limpieza programada.
mod actualizar;
// El modo línea de comandos (`machinograph --cli`): la misma información que enseña el
// panel, para un script o una tarea programada, sin abrir ninguna ventana.
mod cli;
mod copias;
mod limpieza;
// Las bases SQLite de las aplicaciones (catálogo `databases.json` de Kudu, MIT):
// se MIDEN con PRAGMA en solo lectura y, si el usuario lo pide, se compactan.
mod bases;
mod programar;
// Indicadores de compromiso: qué se ejecuta solo en este equipo y qué hay en los
// sitios donde se esconde la persistencia. Local, sin firmas y sin bajar nada.
mod seguridad;
// Exclusiones del usuario: lo que no se mide ni se borra (y por qué). Las respetan
// el analizador de disco, la limpieza y el borrado.
mod exclusiones;
// La capa que resuelve las diferencias entre sistemas (Linux, macOS, Windows):
// discos, red, memoria, procesos, papelera y autoarranque. Es lo que permite que
// el resto del backend no sepa en qué sistema corre.
mod plataforma;
mod llmfit;
mod diagnostico;
mod display;
mod conexiones;
mod db;
// Histórico del uso de disco: una medida por carpeta y día, y la comparación
// entre dos medidas (lo que Kudu llama «storage history and growth
// comparisons»). La comparación es una función pura y el guardado vive en `db`.
mod historial;
mod actions;
mod proceso;
mod tray;
mod perf;
mod gateway;
// Autorreparación de lo que se le rompe a la propia app (puerta, base de datos,
// arranque automático, ficheros de configuración que ella escribió) y cómo lo
// cuenta. Ver `salud.rs`.
mod salud;
mod sensores;
mod entorno;
mod descarga;
// Autoinstalación y autorreparación de lo que la app necesita para funcionar
// entera (llmfit para descargar modelos, llama.cpp para medir y encajar, y la
// detección honesta de lo que no se puede instalar sola). Ver `provision.rs`.
mod provision;
mod memoria;
mod scan;
mod servers;

use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[tauri::command(rename = "snapshot:now")]
async fn snapshot_now() -> Result<types::Snapshot, String> {
    Ok(types::Snapshot::build().await)
}

#[tauri::command(rename = "servers:list")]
async fn servers_list() -> Result<Vec<db::ServerRow>, String> {
    db::servers().map_err(|e| e.to_string())
}

#[tauri::command(rename = "servers:update")]
async fn servers_update(args: Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let cmd = args.get("cmd").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let enabled = args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    db::update_server(&id, &cmd, enabled).map_err(|e| e.to_string())?;
    Ok("ok".into())
}

#[tauri::command(rename = "servers:add")]
async fn servers_add(args: Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| id.clone());
    let kind = args.get("kind").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let port = args.get("port").and_then(|v| v.as_u64()).unwrap_or_default() as u16;
    let enabled = args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    db::add_server(&id, &name, &kind, port, enabled).map_err(|e| e.to_string())?;
    Ok("ok".into())
}

#[tauri::command(rename = "servers:remove")]
async fn servers_remove(args: Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    db::remove_server(&id).map_err(|e| e.to_string())?;
    Ok("ok".into())
}

#[tauri::command(rename = "settings:get")]
async fn settings_get() -> Result<Value, String> {
    db::settings_all().map_err(|e| e.to_string())
}

#[tauri::command(rename = "settings:set")]
async fn settings_set(args: Value) -> Result<String, String> {
    let key = args.get("key").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let value = args.get("value").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    db::set_setting(&key, &value).map_err(|e| e.to_string())?;
    Ok("ok".into())
}

#[tauri::command(rename = "metrics:recent")]
async fn metrics_recent(args: Value) -> Result<Vec<db::MetricRow>, String> {
    let since = args.get("since").and_then(|v| v.as_i64()).unwrap_or(0);
    db::metrics(since).map_err(|e| e.to_string())
}

#[tauri::command(rename = "actions:recent")]
async fn actions_recent(args: Value) -> Result<Vec<db::ActionRow>, String> {
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(100);
    db::actions(limit).map_err(|e| e.to_string())
}

#[tauri::command(rename = "updates:recent")]
async fn updates_recent(args: Value) -> Result<Vec<db::UpdateRow>, String> {
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(100);
    db::updates(limit).map_err(|e| e.to_string())
}

/// Runtimes de llama.cpp disponibles (dónde está `llama-fit-params` y
/// `llama-bench`). Hace falta decirlo en la interfaz: para medir y calcular
/// encaje hay que elegir el binario, y los modelos ternarios solo los lee el fork.
#[tauri::command(rename = "perf:tools")]
async fn perf_tools() -> Result<Vec<perf::Runtime>, String> {
    Ok(perf::runtimes())
}

#[tauri::command(rename = "benchmarks:recent")]
async fn benchmarks_recent(args: Value) -> Result<Vec<db::BenchRow>, String> {
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(100);
    db::benchmarks(limit).map_err(|e| e.to_string())
}

/// Estado del reloj de memoria de la GPU (el fallo silencioso de esta tarjeta).
/// Lectura de sysfs: no necesita privilegios ni cargar ningún modelo.
/// Chequeo de salud del entorno: qué está bien y qué no, con qué hacer en cada
/// caso. Todo son lecturas locales (nada de GPU ni privilegios).
#[tauri::command(rename = "diagnostico:comprobar")]
async fn diagnostico_comprobar() -> Result<Vec<diagnostico::Comprobacion>, String> {
    Ok(diagnostico::comprobar())
}

/// Autorreparación: revisa lo que la app puede arreglarse a sí misma y, con
/// `reparar: true`, lo arregla. Devuelve una fila por comprobación con su estado
/// (correcto / reparado / no se pudo) y, si se reparó, qué se hizo.
///
/// Se llama al arrancar (en segundo plano) y desde el botón «Reparar ahora»; sin
/// `reparar` solo informa de lo que hay (y de lo que habría que hacer).
#[tauri::command(rename = "salud:revisar")]
async fn salud_revisar(args: Value) -> Result<salud::Revision, String> {
    let reparar = args.get("reparar").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(salud::revisar(reparar).await)
}

/// Clientes de IA de este equipo: dónde está su configuración y si ya apuntan a
/// un endpoint local. Es solo lectura.
#[tauri::command(rename = "conexiones:clientes")]
async fn conexiones_clientes() -> Result<Vec<conexiones::Cliente>, String> {
    Ok(conexiones::clientes())
}

/// El fichero final como quedaría, para revisarlo ANTES de escribirlo. NO
/// escribe nada: lo que devuelve es exactamente lo que escribiría `conexiones:aplicar`.
#[tauri::command(rename = "conexiones:propuesta")]
async fn conexiones_propuesta(args: Value) -> Result<conexiones::Propuesta, String> {
    let (cliente, id, nombre, endpoint, api, modelos) = args_conexion(&args);
    conexiones::propuesta(cliente, id, nombre, endpoint, api, &modelos)
}

/// Escribe el proveedor en la configuración de un cliente que admita escritura.
///
/// Es la única parte de la app que escribe en un fichero de OTRO programa, así
/// que va con red: copia de seguridad con fecha al lado del original, escritura
/// atómica, permisos copiados del original (ese fichero lleva una clave de API),
/// verificación releyendo y restauración automática si no cuadra. Los detalles
/// están en `conexiones.rs`.
#[tauri::command(rename = "conexiones:aplicar")]
async fn conexiones_aplicar(args: Value) -> Result<conexiones::Aplicado, String> {
    let (cliente, id, nombre, endpoint, api, modelos) = args_conexion(&args);
    conexiones::aplicar(cliente, id, nombre, endpoint, api, &modelos)
}

/// Los argumentos que comparten `conexiones:propuesta` y `conexiones:aplicar`.
/// Van en un solo sitio para que no puedan discrepar: lo que se revisa tiene que
/// ser exactamente lo que se escribe.
fn args_conexion(args: &Value) -> (&str, &str, &str, &str, &str, Vec<String>) {
    let cliente = args.get("cliente").and_then(|v| v.as_str()).unwrap_or("gentle-shell");
    let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("local");
    let nombre = args.get("nombre").and_then(|v| v.as_str()).unwrap_or("Local");
    let endpoint = args.get("endpoint").and_then(|v| v.as_str()).unwrap_or("");
    let api = args.get("api").and_then(|v| v.as_str()).unwrap_or("openai-completions");
    let modelos: Vec<String> = args
        .get("modelos")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    (cliente, id, nombre, endpoint, api, modelos)
}

/// Plan de hardware para un modelo a un contexto: memoria necesaria y qué cabe
/// esperar en cada vía (GPU, con capas en CPU, solo CPU). Estimación de llmfit.
#[tauri::command(rename = "llmfit:plan")]
async fn llmfit_plan(args: Value) -> Result<llmfit::Plan, String> {
    let modelo = args.get("modelo").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    if modelo.is_empty() {
        return Err("Falta el modelo".into());
    }
    let contexto = args.get("context").and_then(|v| v.as_i64()).unwrap_or(32768);
    let quant = args.get("quant").and_then(|v| v.as_str()).map(str::to_string);
    llmfit::plan(&modelo, contexto, quant.as_deref()).await
}

/// Cuántas sesiones simultáneas aguanta el equipo con un modelo, a cada contexto.
#[tauri::command(rename = "llmfit:concurrencia")]
async fn llmfit_concurrencia(args: Value) -> Result<llmfit::Concurrencia, String> {
    let modelo = args.get("modelo").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    if modelo.is_empty() {
        return Err("Falta el modelo".into());
    }
    llmfit::concurrencia(&modelo).await
}

/// Últimas líneas del log del motor (llama-swap). Es una lectura bajo demanda:
/// el visor las refresca cuando el usuario quiere, no en cada foto.
#[tauri::command(rename = "swap:logs")]
async fn swap_logs(args: Value) -> Result<Vec<String>, String> {
    let puerto = args.get("port").and_then(|v| v.as_u64()).unwrap_or(8080) as u16;
    let lineas = args.get("lineas").and_then(|v| v.as_u64()).unwrap_or(300) as usize;
    servers::llama_swap_logs(puerto, lineas).await
}

/// Encajes conocidos de cada modelo, con la fecha del cálculo. Se calculan solos
/// en segundo plano; esto solo los lee.
#[tauri::command(rename = "fits:listar")]
async fn fits_listar() -> Result<Vec<db::FitRow>, String> {
    db::fits().map_err(|e| e.to_string())
}

#[tauri::command(rename = "gpu:mclk")]
async fn gpu_mclk() -> Result<Option<gpu::EstadoGpu>, String> {
    Ok(gpu::estado_mclk())
}

/// Inventario de modelos de TODO tipo que hay en el sistema, con totales por
/// tipo. Es una lectura del sistema de ficheros: no carga nada ni necesita
/// privilegios.
#[tauri::command(rename = "inventario:listar")]
async fn inventario_listar() -> Result<Value, String> {
    let modelos = inventario::inventario();
    let resumen = inventario::resumen(&modelos);
    Ok(serde_json::json!({ "modelos": modelos, "resumen": resumen }))
}

/// Estado de la integración con llmfit (si está instalado, su versión y ruta).
#[tauri::command(rename = "llmfit:estado")]
async fn llmfit_estado() -> Result<llmfit::Estado, String> {
    Ok(llmfit::estado().await)
}

/// Perfil de hardware según llmfit.
#[tauri::command(rename = "llmfit:sistema")]
async fn llmfit_sistema() -> Result<llmfit::Sistema, String> {
    llmfit::sistema().await
}

/// Modelos que le encajan a esta máquina, según llmfit. Acepta los mismos
/// filtros que su CLI: caso de uso, encaje mínimo y capacidad.
#[tauri::command(rename = "llmfit:recomendar")]
async fn llmfit_recomendar(args: Value) -> Result<Value, String> {
    let limite = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(40);
    let caso = args.get("useCase").and_then(|v| v.as_str()).map(str::to_string);
    let encaje = args.get("minFit").and_then(|v| v.as_str()).map(str::to_string);
    let capacidad = args.get("capability").and_then(|v| v.as_str()).map(str::to_string);
    let con_comando = args.get("conComando").and_then(|v| v.as_bool()).unwrap_or(false);

    let (sistema, modelos) = llmfit::recomendar(
        limite,
        caso.as_deref(),
        encaje.as_deref(),
        capacidad.as_deref(),
        con_comando,
    )
    .await?;
    Ok(serde_json::json!({ "sistema": sistema, "modelos": modelos }))
}

/// Tamaño de ventana en píxeles FÍSICOS para que el viewport CSS sea el del
/// diseño (1280x800), con la escala de la pantalla que sea.
///
/// Separado de `main` para poder probarlo: este cálculo es justo el que estaba
/// mal (se creía lógico y era físico), así que tiene prueba propia.
fn tamano_ventana(escala: f64) -> (f64, f64) {
    let e = normalizar_escala(escala);
    ((1280.0 * e).round(), (800.0 * e).round())
}

/// Una escala imposible (0, negativa, NaN o infinito) no puede dar una ventana
/// de tamaño cero: se cae a 1 y la app sigue abriéndose.
fn normalizar_escala(escala: f64) -> f64 {
    if escala.is_finite() && escala > 0.0 { escala } else { 1.0 }
}

/**
 * Corrige el tamaño de la ventana a partir de lo que mide la INTERFAZ.
 *
 * Para qué: el backend no sabe cuántos píxeles CSS tiene la webview. En Wayland
 * `scale_factor` acierta, pero en X11 reporta 1 mientras WebKit pinta a
 * `Xft.dpi/96` (1,4499 en este equipo), así que la ventana salía con un viewport
 * de 882x551 en vez de 1280x800. La interfaz SÍ lo sabe (`window.innerWidth`,
 * `devicePixelRatio`), y con esos dos números se despeja cuántos píxeles físicos
 * hacen falta para llegar al mínimo del diseño:
 *
 *     físicos_por_css = ancho_físico / ancho_css
 *     tamaño_nuevo    = 1280 x 800 CSS  x  físicos_por_css
 *
 * NO se aplica si el viewport ya llega al mínimo: solo se corrige cuando el
 * usuario se encontraría una ventana por debajo de lo que el diseño promete, y
 * una sola vez (lo pide la interfaz al arrancar). Si el usuario encoge la
 * ventana a mano después, no se le discute.
 */
fn correccion_ventana(
    fisico: (f64, f64),
    css: (f64, f64),
) -> Option<(f64, f64)> {
    let (fw, fh) = fisico;
    let (cw, ch) = css;
    // Sin medidas no se toca nada: con un 0 aquí saldría una ventana de tamaño
    // absurdo.
    if cw <= 0.0 || ch <= 0.0 || fw <= 0.0 || fh <= 0.0 {
        return None;
    }
    if cw >= 960.0 && ch >= 640.0 {
        return None;
    }
    let por_css = (fw / cw).max(fh / ch);
    Some(((1280.0 * por_css).round(), (800.0 * por_css).round()))
}

#[tauri::command(rename = "ventana:ajustar")]
async fn ventana_ajustar(app: tauri::AppHandle, args: Value) -> Result<String, String> {
    let css_ancho = args.get("ancho_css").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let css_alto = args.get("alto_css").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let ventana = app
        .get_webview_window("main")
        .ok_or_else(|| "no hay ventana principal".to_string())?;
    let fisico = ventana
        .inner_size()
        .map(|s| (s.width as f64, s.height as f64))
        .map_err(|e| e.to_string())?;
    let Some((ancho, alto)) = correccion_ventana(fisico, (css_ancho, css_alto)) else {
        return Ok("sin cambios".into());
    };
    ventana
        .set_size(tauri::PhysicalSize::new(ancho, alto))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "ventana ajustada a {ancho}x{alto} físicos (viewport CSS era {css_ancho}x{css_alto})"
    ))
}

/// Mínimo de la ventana, también en píxeles físicos: 960x640 CSS, que es el
/// mínimo que `DESIGN.md` da por bueno y con el que el arnés de interfaz prueba
/// que no hay desplazamiento horizontal de página.
fn tamano_minimo_ventana(escala: f64) -> (f64, f64) {
    let e = normalizar_escala(escala);
    ((960.0 * e).round(), (640.0 * e).round())
}


/* ── Memoria de la GPU ────────────────────────────────────────────────────── */

/// Qué modelos están servidos y CÓMO, con los pesos medidos del disco.
///
/// Se le pasa la GPU para poder dar también la VRAM total en uso: con eso, la
/// interfaz puede decir cuánto de la VRAM son pesos y cuánto es el resto (caché
/// KV, sobrecarga del motor y demás programas).
#[tauri::command(rename = "memoria:cargados")]
async fn memoria_cargados() -> Result<Value, String> {
    let cfg = gateway::config();
    // El proxy es al que hay que preguntar por los modelos servidos: es el que
    // sabe qué tiene arrancado, y su puerto es el del destino de la puerta.
    let (host, puerto) = url_a_host_puerto(&cfg.destino).unwrap_or(("127.0.0.1".into(), 8080));
    let gpu = crate::gpu::load().into_iter().next();
    let vram = gpu.map(|g| ((g.mem_used_mb / 1024.0), (g.mem_total_mb / 1024.0)));
    let m = memoria::cargados(&host, puerto, vram).await;
    Ok(serde_json::to_value(m).map_err(|e| e.to_string())?)
}

/// Parte una URL de destino en (host, puerto). Devuelve `None` si no se puede.
fn url_a_host_puerto(url: &str) -> Option<(String, u16)> {
    let sin_esquema = url.split("://").nth(1)?;
    let autoridad = sin_esquema.split('/').next()?;
    match autoridad.split_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse::<u16>().ok()?)),
        None => Some((autoridad.to_string(), 80)),
    }
}

/* ── Descargas ────────────────────────────────────────────────────────────── */

#[tauri::command(rename = "descarga:estado")]
async fn descarga_estado() -> Result<Value, String> {
    Ok(serde_json::to_value(descarga::estado()).map_err(|e| e.to_string())?)
}

#[tauri::command(rename = "descarga:iniciar")]
async fn descarga_iniciar(app: AppHandle, args: Value) -> Result<String, String> {
    let modelo = args
        .get("modelo")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let quant = args.get("quant").and_then(|v| v.as_str()).map(str::to_string);
    descarga::iniciar(app, modelo.clone(), quant)?;
    Ok(format!("Descarga de {modelo} lanzada. El progreso va en su panel."))
}

#[tauri::command(rename = "descarga:cancelar")]
async fn descarga_cancelar() -> Result<String, String> {
    descarga::cancelar()
}

/* ── Provisionamiento: instalar, verificar y reparar lo que la app necesita ── */

/// Lo que está instalado, lo que falta y lo que no se puede instalar sola.
///
/// Comprueba cada binario EJECUTÁNDOLO, así que tarda unas decenas de ms. La
/// tarjeta lo pide al abrirse y cuando termina una instalación.
#[tauri::command(rename = "provision:estado")]
async fn provision_estado() -> Result<Value, String> {
    Ok(serde_json::json!({
        "herramientas": provision::estados(),
        "auto_provision": provision::auto_activo(),
        "en_curso": provision::en_curso(),
    }))
}

/// Lanza la instalación de lo que falte (o esté roto). Con `forzar`, reinstala
/// aunque funcione. Devuelve enseguida: el progreso va por `ai:provision`.
#[tauri::command(rename = "provision:instalar")]
async fn provision_instalar(app: AppHandle, args: Value) -> Result<String, String> {
    let ids: Vec<String> = args
        .get("herramientas")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let forzar = args.get("forzar").and_then(|v| v.as_bool()).unwrap_or(false);
    provision::instalar(Some(app), ids, forzar)
}

/// «Comprobar ahora»: revisa y repara solo lo que esté roto o falte.
#[tauri::command(rename = "provision:reparar")]
async fn provision_reparar(app: AppHandle) -> Result<String, String> {
    provision::reparar(Some(app))
}

#[tauri::command(rename = "provision:cancelar")]
async fn provision_cancelar() -> Result<String, String> {
    provision::cancelar()
}

/// Lee o cambia el ajuste de instalación automática (por defecto, activada).
#[tauri::command(rename = "provision:auto")]
async fn provision_auto(args: Value) -> Result<bool, String> {
    match args.get("activo").and_then(|v| v.as_bool()) {
        Some(v) => provision::fijar_auto(v),
        None => Ok(provision::auto_activo()),
    }
}

/* ── Entorno: red, arranque automático y carpetas de modelos ──────────────── */

#[tauri::command(rename = "entorno:red")]
async fn entorno_red() -> Result<Value, String> {
    Ok(serde_json::json!({
        "ip": entorno::ip_local(),
        "interfaces": entorno::interfaces(),
    }))
}

#[tauri::command(rename = "entorno:arranque")]
async fn entorno_arranque() -> Result<Value, String> {
    Ok(serde_json::to_value(entorno::arranque_estado()).map_err(|e| e.to_string())?)
}

/// Activa o desactiva el arranque automático. Pide `activar` explícito: sin él no
/// se toca nada, para que una llamada incompleta no cambie el arranque del equipo.
///
/// Además se RECUERDA la intención del usuario (`salud::AJUSTE_PEDIDO`): sin esa
/// memoria, una entrada de arranque que desaparece sería indistinguible de un
/// usuario que nunca la activó, y la autorreparación no podría volver a escribirla
/// sin arriesgarse a activar algo que nadie pidió.
#[tauri::command(rename = "entorno:arranque-configurar")]
async fn entorno_arranque_configurar(args: Value) -> Result<String, String> {
    let activar = args
        .get("activar")
        .and_then(|v| v.as_bool())
        .ok_or("falta «activar» (true o false)")?;
    let mensaje = entorno::arranque_configurar(activar)?;
    // Solo se recuerda si de verdad quedó escrito: si falló, la intención no cambia
    // (y lo que haya que arreglar lo dirá la comprobación de salud).
    if let Err(e) = db::set_setting(salud::AJUSTE_PEDIDO, if activar { "1" } else { "0" }) {
        // No se puede callar el fracaso de guardar la intención, pero el arranque
        // YA se ha escrito: se dice para que no parezca que fue del todo bien.
        return Ok(format!(
            "{mensaje} (Ojo: no se pudo recordar tu elección en los ajustes ({e}); si reinicias, Machinograph podría no volver a escribir el arranque solo.)"
        ));
    }
    Ok(mensaje)
}

#[tauri::command(rename = "entorno:carpetas")]
async fn entorno_carpetas() -> Result<Value, String> {
    Ok(serde_json::json!({ "carpetas": entorno::carpetas_modelos() }))
}

/* ── La puerta de enlace de uso ───────────────────────────────────────────── */

#[tauri::command(rename = "gateway:estado")]
async fn gateway_estado() -> Result<Value, String> {
    Ok(gateway::estado_json())
}

/// Cambia la configuración y REARRANCA la puerta con la nueva.
///
/// Se validan las tres cosas que pueden dejarla inservible antes de guardar nada:
/// el puerto, la dirección y el destino. Un puerto 0 o un destino sin `http://`
/// dejarían la puerta muerta y el fallo aparecería como un 502 sin explicación.
#[tauri::command(rename = "gateway:configurar")]
async fn gateway_configurar(args: Value) -> Result<String, String> {
    let mut cambios: Vec<(&str, String)> = Vec::new();

    if let Some(v) = args.get("activa").and_then(|v| v.as_bool()) {
        cambios.push(("gateway_enabled", if v { "1".into() } else { "0".into() }));
    }
    if let Some(v) = args.get("requiere_clave").and_then(|v| v.as_bool()) {
        cambios.push(("gateway_require_key", if v { "1".into() } else { "0".into() }));
    }
    if let Some(destino) = args.get("destino").and_then(|v| v.as_str()) {
        let d = destino.trim();
        if !gateway::destino_valido(d) {
            return Err(format!(
                "el destino tiene que ser una URL http o https con host (por ejemplo http://127.0.0.1:8080), y llegó «{d}»"
            ));
        }
        cambios.push(("gateway_upstream", d.to_string()));
    }
    if let Some(puerto) = args.get("puerto").and_then(|v| v.as_u64()) {
        if !(1024..=65535).contains(&puerto) {
            return Err(format!("el puerto {puerto} no vale: tiene que estar entre 1024 y 65535"));
        }
        cambios.push(("gateway_port", puerto.to_string()));
    }
    if let Some(dir) = args.get("direccion").and_then(|v| v.as_str()) {
        let d = dir.trim();
        if d.parse::<std::net::IpAddr>().is_err() {
            return Err(format!(
                "«{d}» no es una dirección IP. Para escuchar en todas, usa 0.0.0.0"
            ));
        }
        cambios.push(("gateway_address", d.to_string()));
    }

    for (clave, valor) in &cambios {
        // Y solo se guarda lo que la puerta LEE de verdad: una clave que no
        // consulta nadie sería un ajuste que se guarda y no hace nada (la
        // interfaz diría que sí y sería mentira).
        if !gateway::es_clave(clave) {
            return Err(format!("«{clave}» no es un ajuste de la puerta de enlace"));
        }
        db::set_setting(clave, valor).map_err(|e| e.to_string())?;
    }
    // La clave se genera la PRIMERA vez que se activa y no hay ninguna: sin esto,
    // activar «exigir clave» con la clave vacía dejaría la puerta rechazándolo
    // TODO, incluido el propio usuario.
    let cfg = gateway::config();
    if cfg.activa && cfg.clave.is_empty() {
        db::set_setting("gateway_api_key", &gateway::clave_nueva()).map_err(|e| e.to_string())?;
    }
    Ok("Configuración guardada. La puerta se reinicia para aplicarla.".into())
}

#[tauri::command(rename = "gateway:regenerar-clave")]
async fn gateway_regenerar_clave() -> Result<String, String> {
    let clave = gateway::clave_nueva();
    db::set_setting("gateway_api_key", &clave).map_err(|e| e.to_string())?;
    Ok(format!(
        "Clave nueva generada. Hay que reiniciar la app para que la puerta la use (y volver a apuntarla en los clientes)."
    ))
}

/* ── Uso ──────────────────────────────────────────────────────────────────── */

/// El resumen de uso de un periodo.
///
/// `periodo` es "hoy" (desde la medianoche LOCAL) o "todo". Se calcula la
/// medianoche local con la zona del equipo: si se usara UTC, el trabajo de después
/// de las 23:00 aparecería en el día siguiente y el resumen mentiría.
#[tauri::command(rename = "uso:resumen")]
async fn uso_resumen(args: Value) -> Result<Value, String> {
    use chrono::{Local, TimeZone};
    let periodo = args.get("periodo").and_then(|v| v.as_str()).unwrap_or("hoy");
    let modelo = args
        .get("modelo")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|m| !m.is_empty());
    let desde = if periodo == "todo" {
        0
    } else {
        let hoy = Local::now().date_naive();
        Local
            .from_local_datetime(&hoy.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .map(|d| d.timestamp())
            .unwrap_or(0)
    };

    let resumen = db::uso_resumen(desde, modelo).map_err(|e| e.to_string())?;
    let por_modelo = db::uso_por_modelo(desde).map_err(|e| e.to_string())?;
    // La gráfica siempre abarca 14 días, no el periodo elegido: en "hoy" una
    // gráfica de un solo día no dice nada.
    let desde_grafica = desde.min(chrono::Utc::now().timestamp() - 14 * 86_400);
    let diario = db::uso_diario(desde_grafica, modelo).map_err(|e| e.to_string())?;
    let recientes = db::uso_reciente(modelo, 50).map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "periodo": periodo,
        "desde": desde,
        "resumen": resumen,
        "por_modelo": por_modelo,
        "diario": diario,
        "recientes": recientes,
        "retencion_dias": gateway::DIAS_RETENCION,
        "config": gateway::estado_json(),
    }))
}

/* ── Almacenamiento: analizador de disco ──────────────────────────────────── */

/// La carpeta que se analiza: la que pida la interfaz o, si no, el home.
fn raiz_analizada(args: &Value) -> Result<String, String> {
    match args
        .get("raiz")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(r) => Ok(r.to_string()),
        None => dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .ok_or_else(|| "sin carpeta que analizar".to_string()),
    }
}

fn max_hijos(args: &Value, por_defecto: u64) -> usize {
    args.get("max_hijos")
        .and_then(|v| v.as_u64())
        .unwrap_or(por_defecto)
        .clamp(1, 5000) as usize
}

/// Los hijos directos de una carpeta con su tamaño recursivo, de mayor a menor.
///
/// Recorrer el árbol entero es trabajo de disco puro (segundos, no milisegundos),
/// así que va a un hilo bloqueante propio: si no, la consulta retendría un hilo del
/// runtime asíncrono mientras lee el disco.
#[tauri::command(rename = "almacen:arbol")]
async fn almacen_arbol(args: Value) -> Result<almacen::Arbol, String> {
    let raiz = raiz_analizada(&args)?;
    let max = max_hijos(&args, 400);
    tauri::async_runtime::spawn_blocking(move || {
        // Las exclusiones del usuario se cargan UNA vez por recorrido, aquí dentro
        // (hilo bloqueante): recorrer el disco no puede ir en el runtime asíncrono.
        let filtro = almacen::Filtro::reales();
        let a = almacen::arbol_con(&raiz, max, &filtro)?;
        // Guardar la medida es GRATIS: ya está hecha, así que es el momento de
        // anotarla para poder comparar el crecimiento mañana. Si falla (disco
        // lleno, base bloqueada) NO se tumba el análisis: se dice por stderr y el
        // árbol se devuelve igual; perder la medida no es perder el análisis.
        let inst = historial::Instantanea::desde_arbol(&a, db::now_ts());
        if let Err(e) = db::guardar_instantanea(&inst) {
            eprintln!("no se pudo guardar la medida de {raiz}: {e}");
        }
        Ok(a)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Los ficheros más grandes de un árbol, de mayor a menor.
#[tauri::command(rename = "almacen:grandes")]
async fn almacen_grandes(args: Value) -> Result<Vec<almacen::Fichero>, String> {
    let raiz = raiz_analizada(&args)?;
    let limite = args.get("limite").and_then(|v| v.as_u64()).unwrap_or(50).clamp(1, 1000) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let filtro = almacen::Filtro::reales();
        almacen::grandes_con(&raiz, limite, &filtro)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Busca por nombre (sin distinguir mayúsculas) dentro de un árbol.
#[tauri::command(rename = "almacen:buscar")]
async fn almacen_buscar(args: Value) -> Result<Vec<almacen::Coincidencia>, String> {
    let raiz = raiz_analizada(&args)?;
    let consulta = args.get("consulta").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let limite = args.get("limite").and_then(|v| v.as_u64()).unwrap_or(200).clamp(1, 1000) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let filtro = almacen::Filtro::reales();
        almacen::buscar_con(&raiz, &consulta, limite, &filtro)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ficheros repetidos por CONTENIDO dentro de un árbol.
///
/// Es la lectura más cara de la aplicación (hay que leer los ficheros), así que va
/// bajo demanda y con un mínimo de tamaño: los miles de ficheros pequeños que se
/// repiten solos no liberan espacio y llenarían la lista de ruido.
#[tauri::command(rename = "almacen:duplicados")]
async fn almacen_duplicados(args: Value) -> Result<Vec<almacen::Duplicado>, String> {
    let raiz = raiz_analizada(&args)?;
    let min = args.get("min_bytes").and_then(|v| v.as_u64()).unwrap_or(1024 * 1024);
    let limite = args.get("limite").and_then(|v| v.as_u64()).unwrap_or(200).clamp(1, 5000) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let filtro = almacen::Filtro::reales();
        almacen::duplicados_con(&raiz, min, limite, &filtro)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Carpetas que no contienen ningún fichero (ni en su subárbol).
#[tauri::command(rename = "almacen:vacias")]
async fn almacen_vacias(args: Value) -> Result<Vec<String>, String> {
    let raiz = raiz_analizada(&args)?;
    let limite = args.get("limite").and_then(|v| v.as_u64()).unwrap_or(500).clamp(1, 5000) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let filtro = almacen::Filtro::reales();
        almacen::vacias_con(&raiz, limite, &filtro)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Enlaces simbólicos que apuntan a algo que ya no está.
#[tauri::command(rename = "almacen:enlaces")]
async fn almacen_enlaces(args: Value) -> Result<Vec<almacen::EnlaceRoto>, String> {
    let raiz = raiz_analizada(&args)?;
    let limite = args.get("limite").and_then(|v| v.as_u64()).unwrap_or(500).clamp(1, 5000) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let filtro = almacen::Filtro::reales();
        almacen::enlaces_rotos_con(&raiz, limite, &filtro)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// El histórico de uso de disco de una carpeta: las medidas guardadas y, si hay
/// al menos dos, el crecimiento.
///
/// `dias` pide comparar contra una medida de hace AL MENOS esos días (Inicio usa
/// 7 para «la última semana»); sin él se comparan las dos últimas, que es lo que
/// enseña Almacenamiento. La comparación es pura (`historial::comparar`), así que
/// esto solo lee la base y devuelve el resultado tal cual.
#[tauri::command(rename = "almacen:historial")]
async fn almacen_historial(args: Value) -> Result<Value, String> {
    let raiz = raiz_analizada(&args)?;
    let dias = args.get("dias").and_then(|v| v.as_i64()).filter(|d| *d > 0);
    let limite = args.get("limite").and_then(|v| v.as_u64()).unwrap_or(30).clamp(1, 400) as usize;
    tauri::async_runtime::spawn_blocking(move || -> Result<Value, String> {
        // Leer la base es rápido, pero no se hace en el runtime asíncrono: la
        // conexión es única y está detrás de un candado compartido con la foto.
        let instantaneas = db::instantaneas(&raiz, limite).map_err(|e| e.to_string())?;
        let crecimiento = match dias {
            Some(d) => db::crecimiento_en(&raiz, d).map_err(|e| e.to_string())?,
            None => db::crecimiento(&raiz).map_err(|e| e.to_string())?,
        };
        Ok(serde_json::json!({
            "ruta": raiz,
            // El ajuste decide si se mide sola una vez al día; mirar el histórico
            // de lo que el usuario analiza a mano funciona siempre.
            "activo": db::setting_int("historial_activo", 1) != 0,
            "umbral_gb": db::setting_int("historial_umbral_gb", 5),
            "retencion_dias": db::HISTORIAL_DIAS,
            "dias": dias,
            "instantaneas": instantaneas,
            "crecimiento": crecimiento,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Cuánto hay en la papelera del sistema.
///
/// Es de las cifras que más espacio explican: lo que se manda a la papelera NO
/// libera espacio hasta que se vacía, y mucha gente no lo sabe. `null` cuando el
/// sistema no deja contarlo.
#[tauri::command(rename = "papelera:estado")]
async fn papelera_estado() -> Result<Value, String> {
    let r = plataforma::rutas();
    match plataforma::papelera::resumen() {
        Some((elementos, bytes)) => Ok(serde_json::json!({
            "elementos": elementos,
            "bytes": bytes,
            "ruta": r.papelera().to_string_lossy(),
        })),
        None => Ok(Value::Null),
    }
}

/// Las copias de seguridad de los ficheros que la app ha tocado.
///
/// Es el Centro de recuperación de Kudu: la copia vive al lado del original (que
/// es donde uno la busca) y la base de datos guarda el índice para poder
/// enseñarlas y restaurarlas.
#[tauri::command(rename = "copias:listar")]
async fn copias_listar() -> Result<Vec<db::CopiaRow>, String> {
    copias::listar(200)
}

/// Qué está desactualizado en este equipo, según la herramienta de cada sistema.
///
/// Es una lectura que ejecuta las comprobaciones de fuera (rpm-ostree, flatpak,
/// brew, winget…), así que va BAJO DEMANDA y con su límite de tiempo: cada una
/// puede tardar y algunas consultan su repositorio.
#[tauri::command(rename = "actualizar:comprobar")]
async fn actualizar_comprobar() -> Result<Vec<actualizar::Fuente>, String> {
    tauri::async_runtime::spawn_blocking(actualizar::comprobar)
        .await
        .map_err(|e| e.to_string())
}

/// La limpieza programada tal como está configurada.
#[tauri::command(rename = "programar:leer")]
async fn programar_leer() -> Result<db::Programacion, String> {
    db::programacion().map_err(|e| e.to_string())
}

/// Guarda la limpieza programada. Se valida aquí (hora 0-23, minuto 0-59) para que
/// no se pueda dejar una hora imposible que no se ejecutaría nunca.
#[tauri::command(rename = "programar:guardar")]
async fn programar_guardar(args: Value) -> Result<String, String> {
    let p = db::Programacion {
        activa: args.get("activa").and_then(|v| v.as_bool()).unwrap_or(false),
        hora: args.get("hora").and_then(|v| v.as_u64()).unwrap_or(3).min(23) as u32,
        minuto: args.get("minuto").and_then(|v| v.as_u64()).unwrap_or(30).min(59) as u32,
        categorias: args
            .get("categorias")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        ultima: None,
    };
    db::guardar_programacion(&p).map_err(|e| e.to_string())?;
    Ok(if p.activa {
        format!(
            "Limpieza programada a las {:02}:{:02}. Mide y avisa; NO borra nada: eso lo decides tú.",
            p.hora, p.minuto
        )
    } else {
        "Limpieza programada desactivada.".into()
    })
}

/// Las recetas para que la limpieza se haga con la app cerrada, en este sistema.
#[tauri::command(rename = "programar:recetas")]
async fn programar_recetas() -> Result<Vec<programar::Receta>, String> {
    let p = db::programacion().map_err(|e| e.to_string())?;
    Ok(programar::recetas(&p))
}

/// Las exclusiones del usuario: lo que no se mide ni se borra.
///
/// Se devuelven las GUARDADAS (el texto que escribió el usuario) y las VIGENTES
/// (ese texto ya resuelto a una ruta), porque la interfaz tiene que poder enseñar
/// qué significa `${HOME}/VMs` sin que el usuario lo adivine.
#[tauri::command(rename = "exclusiones:listar")]
async fn exclusiones_listar() -> Result<Value, String> {
    let guardadas = db::exclusiones_listar().map_err(|e| e.to_string())?;
    let vigentes = exclusiones::vigentes();
    Ok(serde_json::json!({
        "guardadas": guardadas
            .iter()
            .map(|(patron, ts)| serde_json::json!({ "patron": patron, "ts": ts }))
            .collect::<Vec<_>>(),
        "vigentes": vigentes
            .iter()
            .map(|v| {
                serde_json::json!({
                    "patron": v.patron,
                    "descripcion": v.descripcion(),
                    "resuelta": v.base.as_ref().map(|b| b.to_string_lossy().to_string()),
                })
            })
            .collect::<Vec<_>>(),
    }))
}

#[tauri::command(rename = "exclusiones:anadir")]
async fn exclusiones_anadir(args: Value) -> Result<String, String> {
    let patron = args.get("patron").and_then(|v| v.as_str()).ok_or("falta el patrón")?;
    exclusiones::anadir(patron)
}

#[tauri::command(rename = "exclusiones:quitar")]
async fn exclusiones_quitar(args: Value) -> Result<String, String> {
    let patron = args.get("patron").and_then(|v| v.as_str()).ok_or("falta el patrón")?;
    exclusiones::quitar(patron)
}

/// ¿Esta ruta está excluida, y por qué patrón? Sirve para poder decirlo en la
/// interfaz en vez de que un total encoga sin explicación.
#[tauri::command(rename = "exclusiones:comprobar")]
async fn exclusiones_comprobar(args: Value) -> Result<Value, String> {
    let ruta = args.get("ruta").and_then(|v| v.as_str()).ok_or("falta la ruta")?;
    // Las vigentes se guardan en una variable ANTES de buscar: si se pasara
    // `&exclusiones::vigentes()` directamente, el vector temporal se liberaría al
    // final de la expresión y la exclusión devuelta apuntaría a memoria muerta.
    let vigentes = exclusiones::vigentes();
    let v = exclusiones::excluida_con(ruta, &vigentes, exclusiones::sin_distinguir_caja());
    Ok(serde_json::json!({
        "excluida": v.is_some(),
        "patron": v.map(|x| x.patron.clone()),
    }))
}

/// Indicadores de compromiso: lo que se ejecuta solo y dónde se escondería algo.
///
/// NO es un antivirus (no mira dentro de los binarios ni tiene firmas) y TODO es
/// local: no se sube nada ni se bajan reglas. Es un chequeo de los sitios donde
/// vive la persistencia, con la prueba de cada hallazgo.
#[tauri::command(rename = "seguridad:revisar")]
async fn seguridad_revisar() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let hallazgos = seguridad::revisar();
        let resumen = seguridad::resumen(&hallazgos);
        serde_json::json!({ "hallazgos": hallazgos, "resumen": resumen })
    })
    .await
    .map_err(|e| e.to_string())
}

/// Uso de cada punto de montaje real (los pseudo-sistemas se descartan).
#[tauri::command(rename = "almacen:montajes")]
async fn almacen_montajes() -> Result<Vec<almacen::Montaje>, String> {
    tauri::async_runtime::spawn_blocking(almacen::montajes)
        .await
        .map_err(|e| e.to_string())
}

/* ── Limpieza: qué se puede borrar sin miedo ──────────────────────────────── */

/// Categorías del catálogo de limpieza, con su nombre para la interfaz.
#[tauri::command(rename = "limpieza:categorias")]
async fn limpieza_categorias() -> Result<Value, String> {
    Ok(serde_json::json!(
        limpieza::CATEGORIAS
            .iter()
            .map(|(id, nombre)| serde_json::json!({ "id": id, "nombre": nombre }))
            .collect::<Vec<_>>()
    ))
}

/// Mide lo que se puede limpiar. Recorre las cachés enteras: hilo bloqueante.
#[tauri::command(rename = "limpieza:escanear")]
async fn limpieza_escanear(args: Value) -> Result<limpieza::Escaneo, String> {
    let categorias = args
        .get("categorias")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>());
    tauri::async_runtime::spawn_blocking(move || limpieza::escanear(categorias))
        .await
        .map_err(|e| e.to_string())?
}

/* ── Bases SQLite de las aplicaciones ─────────────────────────────────────── */

/// Mide las bases SQLite del catálogo de Kudu: cuánto ocupan, cuánto devolvería
/// un `VACUUM` (`PRAGMA freelist_count × page_size`) y si otra aplicación las
/// tiene abiertas. Solo LEE: la medida no escribe nada.
#[tauri::command(rename = "bases:listar")]
async fn bases_listar() -> Result<bases::Listado, String> {
    tauri::async_runtime::spawn_blocking(bases::listar)
        .await
        .map_err(|e| e.to_string())
}

/// Compacta (`VACUUM`) las bases que pida la interfaz; sin lista, las que tengan
/// algo que recuperar. Es una acción EXPLÍCITA (botón, en dos pasos) porque
/// reescribe ficheros del usuario: antes de tocar una base se prueba su bloqueo de
/// escritura y, si la tiene otra aplicación, no se toca y se dice qué proceso la
/// bloquea. `VACUUM` es atómico y no borra filas.
#[tauri::command(rename = "bases:compactar")]
async fn bases_compactar(args: Value) -> Result<bases::Compactacion, String> {
    let peticiones: Vec<bases::Peticion> = match args.get("bases") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
        _ => Vec::new(),
    };
    let informe = tauri::async_runtime::spawn_blocking(move || bases::compactar_lote(&peticiones))
        .await
        .map_err(|e| e.to_string())?;
    // Queda en el registro de acciones: un VACUUM toca ficheros del usuario y
    // tiene que poder consultarse después (cuánto se recuperó y en qué bases).
    let detalle = informe
        .resultados
        .iter()
        .map(|r| r.ruta.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if let Err(e) = db::insert_action("bases:compactar", &detalle, informe.compactadas > 0, &informe.mensaje) {
        eprintln!("no se pudo anotar la compactación: {e}");
    }
    Ok(informe)
}

/// Programas que arrancan solos con la sesión (XDG Autostart).
#[tauri::command(rename = "arranque:listar")]
async fn arranque_listar() -> Result<Vec<plataforma::autoarranque::Entrada>, String> {
    tauri::async_runtime::spawn_blocking(plataforma::autoarranque::listar)
        .await
        .map_err(|e| e.to_string())
}

fn main() -> tauri::Result<()> {
    // Modo línea de comandos: se comprueba ANTES de construir nada de Tauri, así
    // que no se abre ventana, no se crea la bandeja y no se queda ningún proceso
    // vivo. `--cli` es lo único que activa esta vía.
    if cli::invocado() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::process::exit(cli::ejecutar(&args));
    }

    tauri::Builder::default()
        .setup(|app: &mut tauri::App<tauri::Wry>| {
            // ── El tamaño de la ventana, en píxeles FÍSICOS ────────────────────
            //
            // POR QUÉ NO BASTA `tauri.conf.json`: el tamaño se mide en píxeles
            // FÍSICOS, y el diseño (`DESIGN.md`) habla de píxeles CSS. Con la
            // pantalla de este equipo (3840x2160 con escala 1,45) una ventana
            // «de 1280x800» daba un viewport CSS de 882x551, por debajo del
            // mínimo de 960x640 que el diseño da por bueno. Se midió con la app
            // real (`window.innerWidth` = 882, `devicePixelRatio` = 1,4499).
            //
            // Y no se puede uno fiar del factor que reporta el entorno: en
            // Wayland dice 2 (correcto) pero en X11 dice 1 mientras WebKit pinta
            // a 1,45, porque la escala fraccionaria de X11 va por `Xft.dpi`
            // (139 en este equipo) y Tauri no la ve. Por eso esto es solo el
            // primer ajuste: la corrección de verdad la pide la interfaz, que es
            // la única que sabe cuántos píxeles CSS tiene de verdad
            // (`ventana:ajustar`).
            if let Some(ventana) = app.get_webview_window("main") {
                let escala = ventana.scale_factor().unwrap_or(1.0);
                let (ancho, alto) = tamano_ventana(escala);
                let _ = ventana.set_size(tauri::PhysicalSize::new(ancho, alto));
                let (min_ancho, min_alto) = tamano_minimo_ventana(escala);
                let _ = ventana.set_min_size(Some(tauri::PhysicalSize::new(min_ancho, min_alto)));
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Bucle de fotos. El intervalo se RELEE en cada vuelta, así que
                // cambiarlo en Ajustes surte efecto sin reiniciar la aplicación
                // (por eso es `sleep` y no un `interval` fijo).
                loop {
                    let ms = db::setting_int("snapshot_interval_ms", 2000);
                    tokio::time::sleep(Duration::from_millis(ms as u64)).await;
                    let s = types::Snapshot::build().await;
                    // La bandeja se refresca con la MISMA foto: así el menú no
                    // puede decir que hay un modelo cargado cuando ya no lo hay.
                    tray::actualizar(&s);
                    let _ = handle.emit("ai:snapshot", &s);
                    // El error NO se descarta: si no se puede guardar la métrica
                    // (disco lleno, BD bloqueada), se dice. Además queda anotado en
                    // `db::ultimo_error`, así que la foto siguiente lo publica en
                    // `Snapshot.db_error` y la interfaz puede avisar en vez de
                    // perder métricas en silencio.
                    if let Err(e) = db::insert_metric(
                        s.system.cpu_pct,
                        s.system.mem.pct,
                        s.disk.pct,
                        s.gpu.first().map(|g| g.mem_used_mb),
                        s.gpu.first().map(|g| g.mem_total_mb),
                        s.gpu.first().and_then(|g| g.temp_c),
                        s.gpu.first().and_then(|g| g.power_w),
                    ) {
                        eprintln!("no se pudo guardar la métrica: {e}");
                    }
                }
            });

            // ── Encaje AUTOMÁTICO ──────────────────────────────────────────────
            // Al arrancar, y luego cada 10 minutos, se calcula el encaje de cada
            // modelo local con el planificador nativo de llama.cpp. Tarda ~0,4 s
            // por modelo y no toca la GPU, así que la interfaz puede enseñar "así
            // se serviría" sin que el usuario pulse nada. Se prueban los runtimes
            // hasta dar con uno que sepa leer el modelo (los ternarios solo los
            // lee el fork), que es la parte autodetectable.
            //
            // Se guarda también el FALLO con su motivo: así la interfaz distingue
            // "todavía no se ha calculado" de "ningún binario sabe leerlo".
            tauri::async_runtime::spawn(async move {
                loop {
                    for ruta in perf::modelos_encajables() {
                        let para_hilo = ruta.clone();
                        let res = tokio::task::spawn_blocking(move || {
                            perf::fit_de_modelo(&para_hilo, None, true, None, None)
                        })
                        .await;
                        match res {
                            Ok(Ok(f)) => {
                                if let Err(e) = db::insert_fit(&f) {
                                    eprintln!("no se pudo guardar el encaje de {ruta}: {e}");
                                }
                            }
                            Ok(Err(e)) => {
                                // El FALLO también se guarda, para que la interfaz
                                // distinga "no calculado" de "no se puede leer".
                                if let Err(e2) = db::insert_fit_error(&ruta, &e) {
                                    eprintln!("no se pudo guardar el fallo del encaje de {ruta}: {e2}");
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(600)).await;
                }
            });

            // ── La puerta de enlace de uso ─────────────────────────────────────
            // Arranca si está activada en los ajustes. Si el puerto está ocupado,
            // se prueban los siguientes puertos de la misma dirección (ver
            // `gateway::atar`): antes esto dejaba la puerta muerta y el usuario
            // tenía que buscar un puerto libre a mano.
            gateway::arrancar_si_activa();

            // ── Autorreparación ────────────────────────────────────────────────
            //
            // En SEGUNDO PLANO y sin bloquear la ventana: se revisa lo que la app
            // puede arreglarse a sí misma (el puerto de la puerta, la base del
            // histórico, su propia entrada de arranque y los ficheros de
            // configuración que ella escribió) y lo que se repare queda en el
            // historial de acciones. La tarjeta de Diagnóstico lo enseña con el
            // botón «Reparar ahora».
            let salud_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Un respiro para que la puerta tenga su oportunidad de atar el
                // puerto antes de que la comprobación diga que no escucha.
                tokio::time::sleep(Duration::from_millis(400)).await;
                let revision = salud::revisar(true).await;
                for c in &revision.comprobaciones {
                    if c.estado == salud::EstadoSalud::Reparado {
                        let _ = salud_handle.emit(
                            "ai:action",
                            serde_json::Value::String(format!(
                                "Autorreparación · {}: {}",
                                c.titulo, c.detalle
                            )),
                        );
                    }
                }
            });

            // ── Purga del uso viejo ────────────────────────────────────────────
            // Una vez al día: el histórico de uso no crece sin fin (90 días).
            tauri::async_runtime::spawn(async move {
                loop {
                    if let Err(e) = gateway::purgar_antiguo() {
                        eprintln!("no se pudo purgar el uso viejo: {e}");
                    }
                    tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
                }
            });

            // ── La medida diaria del disco ─────────────────────────────────────
            //
            // Una vez al día (lo decide `historial::diaria`, que se acuerda del día
            // en la base) se mide el hogar y la carpeta de modelos con el
            // presupuesto del analizador y se guarda la instantánea: eso es lo que
            // permite decir «has crecido X GB desde la semana pasada» con medidas
            // reales. Va en `spawn_blocking` porque recorre el disco y puede tardar
            // hasta los 45 s del presupuesto. Se comprueba cada 30 min (mirar si
            // toca es barato); se puede apagar en Ajustes (`historial_activo`).
            tauri::async_runtime::spawn(async move {
                // Un respiro para no competir con el arranque (inventario, encajes,
                // autorreparación): cuando llegue aquí, la ventana ya está viva.
                tokio::time::sleep(Duration::from_secs(90)).await;
                loop {
                    let r = tokio::task::spawn_blocking(historial::diaria).await.unwrap_or(None);
                    if let Some(m) = r {
                        eprintln!("{m}");
                    }
                    tokio::time::sleep(Duration::from_secs(1800)).await;
                }
            });

            // ── La limpieza programada ─────────────────────────────────────────
            //
            // Se mira cada 30 s (no cada 2 s: la programación es por minuto) y, si
            // toca, MIDE la basura y lo deja anotado en el registro de acciones.
            // NO borra nada: un borrado a las tres de la mañana que nadie ha mirado
            // es justo lo que este programa no hace. El aviso llega a la interfaz
            // por el evento que ya existe (`ai:action`).
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let handle = app_handle.clone();
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let handle = handle.clone();
                    let msg = tokio::task::spawn_blocking(programar::revisar)
                        .await
                        .unwrap_or(None);
                    if let Some(m) = msg {
                        let _ = db::insert_action("programar:limpieza", "", true, &m);
                        let _ = handle.emit("ai:action", serde_json::Value::String(m));
                    }
                }
            });

            // ── Lo que la app necesita para funcionar entera ───────────────────
            //
            // Al arrancar se revisa si falta o está roto algo de lo que la app usa
            // (llmfit para descargar modelos, llama.cpp para medir y encajar) y, si
            // el ajuste `auto_provision` está activo (por defecto SÍ), se instala
            // sola. Va en segundo plano y con su aviso: descargar es una acción de
            // red y el usuario tiene que verla y poder cancelarla. Lo que no se
            // puede instalar sola (amd-smi viene con ROCm y necesita root) solo se
            // DETECTA: se dice con su motivo y su comando, sin fingir nada.
            let provision_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || provision::arranque(Some(provision_handle))).await;
            });

            if let Err(e) = tray::setup(app) {
                eprintln!("tray init failed: {e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ventana_ajustar,
            memoria_cargados,
            descarga_estado,
            descarga_iniciar,
            descarga_cancelar,
            provision_estado,
            provision_instalar,
            provision_reparar,
            provision_cancelar,
            provision_auto,
            entorno_red,
            entorno_arranque,
            entorno_arranque_configurar,
            entorno_carpetas,
            gateway_estado,
            gateway_configurar,
            gateway_regenerar_clave,
            uso_resumen,
            snapshot_now,
            actions::run,
            servers_list,
            servers_update,
            servers_add,
            servers_remove,
            settings_get,
            settings_set,
            metrics_recent,
            actions_recent,
            updates_recent,
            perf_tools,
            benchmarks_recent,
            gpu_mclk,
            llmfit_estado,
            llmfit_sistema,
            llmfit_recomendar,
            inventario_listar,
            fits_listar,
            swap_logs,
            llmfit_plan,
            llmfit_concurrencia,
            diagnostico_comprobar,
            salud_revisar,
            conexiones_clientes,
            conexiones_aplicar,
            conexiones_propuesta,
            almacen_arbol,
            almacen_grandes,
            almacen_buscar,
            almacen_montajes,
            limpieza_categorias,
            limpieza_escanear,
            bases_listar,
            bases_compactar,
            arranque_listar,
            almacen_duplicados,
            almacen_vacias,
            almacen_enlaces,
            almacen_historial,
            papelera_estado,
            copias_listar,
            actualizar_comprobar,
            programar_leer,
            programar_guardar,
            programar_recetas,
            seguridad_revisar,
            exclusiones_listar,
            exclusiones_anadir,
            exclusiones_quitar,
            exclusiones_comprobar
        ])
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod pruebas_ventana {
    use super::{correccion_ventana, tamano_minimo_ventana, tamano_ventana};

    /// El error que esto evita, medido con la app real: con la ventana puesta a
    /// 1280x800 en `tauri.conf.json`, en esta pantalla (escala 1,4499) el
    /// viewport CSS salía de 882x551 y el diseño se rompía por abajo del mínimo.
    #[test]
    fn el_viewport_css_es_el_del_diseno_con_cualquier_escala() {
        for escala in [1.0, 1.25, 1.4499, 1.5, 2.0] {
            let (ancho, alto) = tamano_ventana(escala);
            let css_ancho = ancho / escala;
            let css_alto = alto / escala;
            assert!(
                (css_ancho - 1280.0).abs() < 0.5 && (css_alto - 800.0).abs() < 0.5,
                "con escala {escala} el viewport CSS sería {css_ancho}x{css_alto}, no 1280x800"
            );
        }
    }

    /// Y el mínimo declarado no puede quedarse por debajo del que el diseño da
    /// por bueno: el arnés de interfaz comprueba 960x640, así que la ventana
    /// tiene que poder llegar ahí.
    #[test]
    fn el_minimo_da_al_menos_960x640_css() {
        for escala in [1.0, 1.25, 1.4499, 1.5, 2.0] {
            let (ancho, alto) = tamano_minimo_ventana(escala);
            assert!(
                ancho / escala >= 960.0 - 0.5 && alto / escala >= 640.0 - 0.5,
                "con escala {escala} el mínimo sería {}x{} CSS",
                ancho / escala,
                alto / escala
            );
        }
    }

    /// Una escala imposible (0, negativa o NaN) no puede dar una ventana de
    /// tamaño cero: se cae a 1.0 y se sigue abriendo.
    #[test]
    fn una_escala_absurda_no_deja_la_ventana_a_cero() {
        for escala in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            let (ancho, alto) = tamano_ventana(escala);
            assert_eq!((ancho, alto), (1280.0, 800.0), "escala {escala}");
        }
    }

    /// El caso que se midió: X11 con `Xft.dpi: 139` deja la ventana en 1280x800
    /// FÍSICOS, que son 882x551 CSS. La corrección tiene que pedir esa
    /// proporción (1280/882 = 1,4512) para llegar a los 1280x800 CSS.
    #[test]
    fn la_correccion_despeja_los_pixeles_fisicos_por_css() {
        let (ancho, alto) = correccion_ventana((1280.0, 800.0), (882.0, 551.0)).expect("debe corregir");
        // por_css = max(1280/882, 800/551) = 1,45190 (el alto es el que manda)
        // 1280 x 1,45190 = 1858,4 -> 1858   800 x 1,45190 = 1161,5 -> 1162
        assert_eq!(ancho.round(), 1858.0, "ancho físico pedido");
        assert_eq!(alto.round(), 1162.0, "alto físico pedido");
    }

    /// Si el viewport YA llega al mínimo del diseño, no se toca nada: redimensionar
    /// una ventana que estaba bien sería un salto visible al arrancar.
    #[test]
    fn no_se_corrige_una_ventana_que_ya_cumple() {
        assert_eq!(correccion_ventana((2560.0, 1600.0), (1280.0, 800.0)), None);
        assert_eq!(correccion_ventana((1920.0, 1280.0), (960.0, 640.0)), None);
    }

    /// Sin medidas creíbles (un 0 porque el comando llegó a medias) no se toca
    /// nada: con un 0 ahí saldría una ventana de tamaño absurdo o un bucle.
    #[test]
    fn sin_medidas_no_hay_correccion() {
        for caso in [(0.0, 0.0), (1280.0, 800.0)] {
            for css in [(0.0, 0.0), (0.0, 500.0), (900.0, 0.0), (-10.0, -10.0)] {
                assert_eq!(
                    correccion_ventana(caso, css),
                    None,
                    "físico {caso:?} css {css:?}"
                );
            }
        }
    }
}
