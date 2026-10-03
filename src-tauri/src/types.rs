use chrono::Utc;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub ts: i64,
    pub uptime_secs: i64,
    pub boot: i64,
    pub system: System,
    pub gpu: Vec<Gpu>,
    pub display: Vec<DisplayOutput>,
    pub servers: Vec<Server>,
    pub ai_procs: Vec<AiProc>,
    pub disk: DiskUsage,
    /// TODO lo que el equipo expone por `hwmon`, más caudales de disco y red y la
    /// frecuencia por núcleo. Va DENTRO de la foto porque es una lectura de sysfs
    /// (barata y sin privilegios): así la sección Hardware está al día sin pedir
    /// nada aparte, y no puede discrepar de lo que enseña Inicio.
    pub hardware: crate::sensores::Hardware,
    pub note: String,
    /// Totales de modelos de TODO tipo (no solo los .gguf de ~/models).
    pub inventario: crate::inventario::Totales,
    /// El sistema operativo del equipo: `linux`, `macos`, `windows` u `otro`.
    ///
    /// Va en la foto porque la interfaz TIENE que saberlo: hay datos que un sistema
    /// publica y otro no (los ventiladores y voltajes de la placa son de Linux), y
    /// la sección correspondiente lo dice en vez de quedarse vacía sin explicación.
    pub so: String,
    /// El nombre del sistema para leerlo ("Linux", "macOS", "Windows").
    pub so_nombre: String,
    /// Motivo por el que la base de datos no está disponible, si lo está
    /// fallando. `null` cuando todo va bien.
    ///
    /// POR QUÉ: abrir la BD es fallible (disco lleno, permisos cambiados, fichero
    /// corrupto) y el error tiene que LLEGAR a la pantalla. Antes se abría con
    /// `expect` y, como en release el perfil es `panic = "abort"`, un fallo cerraba
    /// el proceso sin mensaje. Ahora la app sigue viva y publica aquí el motivo,
    /// que es lo que puede enseñar la interfaz.
    pub db_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct System {
    pub cpu_pct: f64,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub cores: i32,
    pub mem: Mem,
    pub swap: Mem,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Mem {
    pub total_mb: f64,
    pub used_mb: f64,
    pub free_mb: f64,
    pub avail_mb: f64,
    pub pct: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiskUsage {
    pub total_gb: f64,
    pub used_gb: f64,
    pub free_gb: f64,
    pub pct: f64,
    /// Punto de montaje medido. Hace falta decirlo: en un equipo con varios
    /// discos, "474 GB" sin decir de dónde salen no significa nada.
    pub mount: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gpu {
    pub id: i32,
    pub name: String,
    pub driver: String,
    /// `None` cuando el driver no publica la temperatura. Se distingue de `0`,
    /// que sería una lectura falsa: antes la aplicación no lo publicaba y se
    /// pintaba un `0 °C` como si fuera un dato.
    pub temp_c: Option<f64>,
    pub mem_temp_c: Option<f64>,
    /// `None` cuando el driver no publica el consumo (mismo motivo que `temp_c`).
    pub power_w: Option<f64>,
    pub mem_used_mb: f64,
    pub mem_total_mb: f64,
    pub mem_pct: f64,
    pub util: f64,
    pub clock_mhz: f64,
    pub fan_rpm: i32,
    pub fan_pct: f64,
    pub throttle: Option<String>,
    /// Solo se conoce el nombre, la VRAM y el driver.
    ///
    /// Pasa en macOS y Windows, donde el uso, la temperatura y la potencia no se
    /// pueden leer sin privilegios. Con esto en `true` la interfaz enseña «—» en
    /// esos huecos en vez de un `0 %` que afirmaría que la tarjeta está parada.
    pub parcial: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisplayOutput {
    pub name: String,
    pub status: String,
    pub connected: bool,
    pub primary: bool,
    pub w: i32,
    pub h: i32,
    pub hz: f64,
    pub offset_x: i32,
    pub offset_y: i32,
    pub modes: Vec<Mode>,
    pub current_flags: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Mode {
    pub w: i32,
    pub h: i32,
    pub hz: f64,
    pub flags: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub port: u16,
    pub state: String, // "active" | "stopped"
    pub process_active: bool,
    /// Versión del motor, si su API la publica en claro. Solo se rellena donde se
    /// ha COMPROBADO el endpoint (llama-swap lo publica en `/api/version`); para
    /// el resto se deja `None` en vez de inventarse una.
    pub version: Option<String>,
    pub pid: Option<i32>,
    pub proc_uptime_secs: Option<i64>,
    pub models: Vec<ServerModel>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerModel {
    pub id: String,
    pub label: String,
    pub state: String, // loaded | unloaded
    pub quant: Option<String>,
    pub size_mb: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiProc {
    pub pid: i32,
    pub name: String,
    pub cmd: String,
    pub cpu_pct: f64,
    pub mem_mb: f64,
    pub uptime_secs: i64,
    pub tag: String,
}

impl Snapshot {
    pub async fn build() -> Self {
        // El trabajo pesado BLOQUEA y va a hilos del pool de bloqueo, no al
        // runtime asíncrono: leer /proc y /sys y lanzar binarios externos (amd-smi,
        // df, kscreen-doctor) dentro del propio bucle asíncrono dejaba un worker de
        // tokio ocupado para siempre con el intervalo mínimo y, de paso, ensuciaba
        // la medida de `dt` entre fotos que usa el cálculo de CPU por proceso.
        //
        // Y van EN PARALELO entre sí: encadenarlos sumaba todas las esperas.
        let (system, gpu, display, ai_procs, inventario, servers, hardware) = tokio::join!(
            tokio::task::spawn_blocking(crate::system::build),
            tokio::task::spawn_blocking(crate::gpu::load),
            tokio::task::spawn_blocking(crate::display::query_para_foto),
            tokio::task::spawn_blocking(crate::scan::ai_procs),
            tokio::task::spawn_blocking(crate::inventario::totales_cacheados),
            crate::servers::build(),
            // Los sensores también bloquean (recorren /sys/class/hwmon y leen
            // /proc): al pool de bloqueo, como el resto.
            tokio::task::spawn_blocking(crate::sensores::leer),
        );
        // Si una tarea de bloqueo no llegara a devolver nada (cancelación al
        // cerrar), se usa el valor neutro: en release un pánico aborta el proceso,
        // así que esto no puede tapar un fallo real.
        let (system, disk, uptime_secs) = system.unwrap_or_default();
        let gpu = gpu.unwrap_or_default();
        let display = display.unwrap_or_else(|_| Ok(Vec::new())).unwrap_or_default();
        let ai_procs = ai_procs.unwrap_or_default();
        let inventario = inventario.unwrap_or_default();
        let hardware = hardware.unwrap_or_default();
        let ts = Utc::now().timestamp();
        let boot = ts - uptime_secs;
        Self {
            ts,
            uptime_secs,
            boot,
            system,
            gpu,
            display,
            servers,
            ai_procs,
            disk,
            note: crate::display::nota(),
            inventario,
            hardware,
            so: crate::plataforma::so().to_string(),
            so_nombre: crate::plataforma::nombre_so().to_string(),
            db_error: crate::db::ultimo_error(),
        }
    }
}
