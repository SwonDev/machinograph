//! Capa de plataforma: **todo** lo que cambia de un sistema operativo a otro.
//!
//! POR QUÉ EXISTE: el panel nació en Linux y leía `/proc`, `/sys`, `df` y el
//! formato de `.desktop`, así que en macOS y Windows no arrancaba. En vez de
//! repartir `#[cfg(target_os)]` por quince módulos (donde es imposible saber qué
//! falta), todo eso vive aquí: los módulos de arriba piden «dame los discos» y no
//! saben de qué sistema son.
//!
//! Cuatro reglas que se aplican SOLO aquí:
//!
//! 1. **Nada de leer `/proc` ni `/sys` a pelo.** Se usa `sysinfo`, que ya resuelve
//!    cada sistema (Linux, macOS y Windows) y trae lo mismo en las tres: discos,
//!    red, memoria, CPU, procesos y temperaturas donde el sistema las exponga.
//! 2. **Lo que un sistema no expone se devuelve vacío y con nota**, nunca
//!    inventado: en macOS no hay `hwmon`, así que no hay ventiladores que enseñar
//!    y se dice, en vez de poner ceros.
//! 3. **Nada sale del equipo.** Esta capa no abre conexiones: es la regla del
//!    proyecto (local-first). Lo que necesite red (descargar un modelo, por
//!    ejemplo) lo pide el usuario desde su módulo y se ve en la interfaz.
//! 4. **Una sola lectura por foto.** Los sondeos caros (procesos, discos) se
//!    cachean aquí con su instante, porque varias secciones piden lo mismo.

pub mod autoarranque;
pub mod gpu;
pub mod pantalla;
pub mod papelera;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::sync::LazyLock;
use std::time::Instant;
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

/* ── Qué sistema es ───────────────────────────────────────────────────────── */

/// El sistema operativo, tal como lo enseña la interfaz.
///
/// Se publica en la foto (`Snapshot.so`) para que la vista pueda decir «esto no
/// existe en tu sistema» en vez de enseñar una sección vacía sin explicación.
pub fn so() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "otro"
    }
}

/// El nombre del sistema para enseñarlo (no el identificador técnico).
pub fn nombre_so() -> &'static str {
    match so() {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        otro => otro,
    }
}

/* ── Rutas estándar ───────────────────────────────────────────────────────── */

/// Las carpetas del sistema, resueltas por el propio sistema operativo.
///
/// Es el único sitio que decide dónde vive cada cosa: en Linux `~/.config` y
/// `~/.cache`; en macOS `~/Library/Application Support` y `~/Library/Caches`; en
/// Windows `%APPDATA%` y `%LOCALAPPDATA%`. El resto del programa no vuelve a
/// componer esa ruta a mano.
#[derive(Debug, Clone)]
pub struct Rutas {
    pub home: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub datos: PathBuf,
    pub temp: PathBuf,
}

impl Rutas {
    /// Dónde vive la papelera de este sistema (para poder ENSEÑARLA; mover a la
    /// papelera lo hace `plataforma::papelera` con la implementación de cada uno).
    pub fn papelera(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home.join(".Trash")
        } else {
            // Linux (freedesktop) y Windows («Papelera de reciclaje») no viven en
            // una carpeta del home: en Windows es un directorio oculto por volumen
            // y en Linux está en `share`. Se devuelve la del home, que es la única
            // que se puede nombrar sin preguntar al sistema.
            self.datos.join("Trash")
        }
    }
}

pub fn rutas() -> Rutas {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    Rutas {
        cache: dirs::cache_dir().unwrap_or_else(|| home.join(".cache")),
        config: dirs::config_dir().unwrap_or_else(|| home.join(".config")),
        datos: dirs::data_dir().unwrap_or_else(|| home.join(".local").join("share")),
        temp: std::env::temp_dir(),
        home,
    }
}

/* ── Discos ───────────────────────────────────────────────────────────────── */

/// Expande las variables que pueden llevar las rutas escritas a mano.
///
/// `dirs` ya traduce cada una al sitio de su sistema (XDG en Linux, `~/Library` en
/// macOS, `%APPDATA%`/`%LOCALAPPDATA%` en Windows), así que una ruta escrita con
/// `${LOCALAPPDATA}` apunta al sitio correcto sin saber dónde está en cada máquina.
///
/// Lo usan el catálogo de limpieza Y las exclusiones del usuario: es la MISMA
/// lista de variables, y por eso vive aquí y no dentro de uno de los dos.
pub fn expandir_plantilla(plantilla: &str) -> String {
    let r = rutas();
    let local = dirs::data_local_dir().unwrap_or_else(|| r.datos.clone());
    let roaming = dirs::config_dir().unwrap_or_else(|| r.config.clone());
    plantilla
        .replace("${HOME}", &r.home.to_string_lossy())
        .replace("${CACHE}", &r.cache.to_string_lossy())
        .replace("${CONFIG}", &r.config.to_string_lossy())
        .replace("${LOCAL_SHARE}", &r.datos.to_string_lossy())
        .replace("${LOCALAPPDATA}", &local.to_string_lossy())
        .replace("${APPDATA}", &roaming.to_string_lossy())
        .replace("${TEMP}", &r.temp.to_string_lossy())
        .replace("${LIBRARY}", &r.home.join("Library").to_string_lossy())
        // La carpeta de Windows. En otro sistema no existe y la regla que la use
        // simplemente no encuentra nada (que es lo correcto: no hay nada que medir).
        .replace(
            "${WINDIR}",
            &std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string()),
        )
        // OJO CON EL ORDEN: `${PROGRAMFILES_X86}` va ANTES que `${PROGRAMFILES}`, o
        // el segundo se come el principio del primero y deja un `_X86}` suelto.
        .replace(
            "${PROGRAMFILES_X86}",
            &std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string()),
        )
        .replace(
            "${PROGRAMFILES}",
            &std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string()),
        )
        .replace(
            "${PROGRAMDATA}",
            &std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string()),
        )
        .replace(
            "${SYSTEMDRIVE}",
            &std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string()),
        )
}

/// Un disco montado, con su uso. Unidades: **bytes** (la interfaz decide cómo se
/// escriben; el backend no redondea).
#[derive(Debug, Clone)]
pub struct Disco {
    pub punto: String,
    pub nombre: String,
    /// El sistema de ficheros (`btrfs`, `ext4`, `apfs`, `NTFS`).
    pub fs: String,
    pub total: u64,
    pub usado: u64,
    pub libre: u64,
    pub uso_pct: f64,
    pub extraible: bool,
}

/// Sistemas de ficheros que NO son un disco del usuario: se descartan para no
/// llenar la tabla de filas que no dicen nada (y en Linux son decenas).
fn es_pseudo(tipo: &str, punto: &str) -> bool {
    const PSEUDO: &[&str] = &[
        "tmpfs", "devtmpfs", "devpts", "sysfs", "proc", "cgroup", "cgroup2", "securityfs",
        "efivarfs", "squashfs", "overlay", "ramfs", "autofs", "fusectl", "configfs", "debugfs",
        "tracefs", "bpf", "pstore", "hugetlbfs", "mqueue", "nsfs", "binfmt_misc", "rpc_pipefs",
        "selinuxfs", "fuseblk", "fuse.portal", "fuse.gvfsd-fuse", "cifs", "nfs", "nfs4",
        "smbfs", "sshfs", "davfs", "exfat_allow",
    ];
    if PSEUDO.contains(&tipo) {
        return true;
    }
    // Montajes del sistema que no son "donde se llena el disco": en un macOS hay
    // decenas de puntos `/System/Volumes/*` y en Linux `/run/*`.
    punto == "/run" || punto.starts_with("/run/") || punto.starts_with("/sys/") || punto.starts_with("/proc/")
}

static DISCOS: LazyLock<Mutex<Disks>> = LazyLock::new(|| Mutex::new(Disks::new_with_refreshed_list()));
/// Si ya se refrescó alguna vez. Un `Disks` recién creado con
/// `new_with_refreshed_list` cuenta como refrescado; esto es para no refrescar
/// dos veces seguidas cuando lo primero que pasa es que alguien pide la lista.
static DISCOS_LISTOS: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(true));
/// Instante del último refresco de discos Y red: es lo que convierte los
/// contadores en caudales.
static ULTIMO_REFRESCO: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));

fn refrescar_discos() {
    let mut guard = DISCOS.lock();
    guard.refresh(true);
    *DISCOS_LISTOS.lock() = true;
}

/// La lista de discos con su uso.
///
/// NO refresca por su cuenta si ya lo ha hecho el refresco de la foto: los
/// contadores de lectura y escritura de `sysinfo` son «desde el último refresco»,
/// así que si esta función refrescara por su lado, se comería el caudal que iba a
/// leer la foto y aparecería un 0 B/s de mentira.
pub fn discos() -> Vec<Disco> {
    if !*DISCOS_LISTOS.lock() {
        refrescar_discos();
    }
    let guard = DISCOS.lock();
    let mut v: Vec<Disco> = guard
        .list()
        .iter()
        .filter(|d| {
            let tipo = d.file_system().to_string_lossy().to_string();
            let punto = d.mount_point().to_string_lossy().to_string();
            !es_pseudo(&tipo, &punto)
        })
        .map(|d| {
            let total = d.total_space();
            let libre = d.available_space();
            let usado = total.saturating_sub(libre);
            Disco {
                punto: d.mount_point().to_string_lossy().to_string(),
                nombre: d.name().to_string_lossy().to_string(),
                fs: d.file_system().to_string_lossy().to_string(),
                total,
                usado,
                libre,
                uso_pct: if total > 0 {
                    usado as f64 * 100.0 / total as f64
                } else {
                    0.0
                },
                extraible: d.is_removable(),
            }
        })
        .collect();
    v.sort_by(|a, b| b.usado.cmp(&a.usado));
    v
}

/// Quita el prefijo `\\?\` que `canonicalize` añade en Windows (y devuelve el
/// `\\servidor\recurso` de las rutas de red, que llegan como `\\?\UNC\...`).
///
/// POR QUÉ: `sysinfo` publica los puntos de montaje como `C:\`, y el camino
/// canónico empieza por `\\?\C:\`. Sin esto, `canon.starts_with(C:\)` nunca cuadra
/// y `disco_de` no encontraba ningún disco en Windows. Fuera de Windows es la
/// identidad (no toca nada).
fn sin_prefijo_verbatim(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(s) = p.to_str() {
            if let Some(resto) = s.strip_prefix(r"\\?\UNC\") {
                return PathBuf::from(format!(r"\\{resto}"));
            }
            if let Some(resto) = s.strip_prefix(r"\\?\") {
                return PathBuf::from(resto);
            }
        }
    }
    p
}

/// El disco donde vive una ruta: el punto de montaje **más largo** que sea prefijo
/// suyo (en un sistema con `/` y `/home` en discos distintos, el bueno es el
/// segundo).
pub fn disco_de(ruta: &Path) -> Option<Disco> {
    let canon = sin_prefijo_verbatim(ruta.canonicalize().unwrap_or_else(|_| ruta.to_path_buf()));
    discos()
        .into_iter()
        .filter(|d| canon.starts_with(&d.punto))
        .max_by_key(|d| d.punto.len())
}

/// Bytes leídos y escritos desde el refresco anterior, por disco. Es una
/// DIFERENCIA, no un total.
fn caudal_discos() -> Vec<(String, u64, u64)> {
    let guard = DISCOS.lock();
    guard
        .list()
        .iter()
        .map(|d| {
            let u = d.usage();
            (
                d.name().to_string_lossy().to_string(),
                u.read_bytes,
                u.written_bytes,
            )
        })
        .collect()
}

/// Bytes movidos por disco y por red desde la llamada ANTERIOR, con el tiempo
/// transcurrido: `(segundos, discos, red)`.
///
/// El tiempo lo mide ESTA capa porque es la única que refresca, y sin él los
/// contadores no son un caudal. La primera vuelta devuelve 0 segundos y quien lo
/// use tiene que decir «—»: un 0 B/s ahí parecería «no hay tráfico», que es
/// distinto de «todavía no se puede saber».
///
/// **Es el ÚNICO sitio que refresca discos y red.** Si lo hiciera también
/// `discos()`, el delta se consumiría antes de que lo leyera la foto y los
/// caudales saldrían a cero.
pub fn caudales() -> (f64, Vec<(String, u64, u64)>, Vec<(String, u64, u64)>) {
    let segundos = {
        let mut g = ULTIMO_REFRESCO.lock();
        let s = g.map(|t: Instant| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        *g = Some(Instant::now());
        s
    };
    refrescar_discos();
    let discos = caudal_discos();
    let red = {
        let mut guard = REDES.lock();
        guard.refresh(false);
        guard
            .list()
            .iter()
            .map(|(n, d)| (n.clone(), d.received(), d.transmitted()))
            .collect()
    };
    (segundos, discos, red)
}

/* ── Red ──────────────────────────────────────────────────────────────────── */

/// Una interfaz de red con el tráfico del último intervalo.
#[derive(Debug, Clone)]
pub struct Interfaz {
    pub nombre: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// La IPv4 de esa interfaz, si tiene. Es lo que hace falta para decir con qué
    /// dirección se llega a la app desde otro equipo.
    pub ipv4: Option<String>,
}

static REDES: LazyLock<Mutex<Networks>> = LazyLock::new(|| Mutex::new(Networks::new_with_refreshed_list()));
/// Interfaces que YA han movido algo desde que arrancó el programa: una interfaz
/// sin tráfico en un intervalo no es una interfaz desconectada, y ocultarla por
/// eso la haría parpadear en la tabla.
static REDES_VISTAS: LazyLock<Mutex<HashMap<String, bool>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Las interfaces de red con lo que han movido desde el último refresco de la
/// foto. NO refresca (lo hace `caudales()`): si refrescara aquí, ese tráfico se
/// perdería para el caudal que enseña Hardware.
pub fn red() -> Vec<Interfaz> {
    let guard = REDES.lock();
    let mut vistas = REDES_VISTAS.lock();
    let mut v: Vec<Interfaz> = guard
        .list()
        .iter()
        .map(|(nombre, d)| {
            let rx = d.received();
            let tx = d.transmitted();
            if rx > 0 || tx > 0 {
                vistas.insert(nombre.clone(), true);
            }
            let ipv4 = d.ip_networks().iter().find_map(|ip| match ip.addr {
                std::net::IpAddr::V4(v4) => Some(v4.to_string()),
                std::net::IpAddr::V6(_) => None,
            });
            let _ = vistas.entry(nombre.clone()).or_insert(false);
            Interfaz {
                nombre: nombre.clone(),
                rx_bytes: rx,
                tx_bytes: tx,
                ipv4,
            }
        })
        .collect();
    // La que más movió, primero: es la que se está mirando.
    v.sort_by(|a, b| (b.rx_bytes + b.tx_bytes).cmp(&(a.rx_bytes + a.tx_bytes)));
    v
}

/* ── Sistema: CPU, memoria y swap ─────────────────────────────────────────── */

/// Lo que se enseña de la máquina, en las unidades que usa la foto (MB para
/// memoria, MHz para frecuencia).
#[derive(Debug, Clone, Default)]
pub struct Resumen {
    pub cpu_pct: f64,
    pub cores: i32,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub mem_total_mb: f64,
    pub mem_usado_mb: f64,
    pub mem_libre_mb: f64,
    pub mem_disp_mb: f64,
    pub mem_pct: f64,
    pub swap_total_mb: f64,
    pub swap_usado_mb: f64,
    pub swap_libre_mb: f64,
    pub swap_pct: f64,
}

/// Un solo `System` para todo el proceso: el porcentaje de CPU de `sysinfo` es una
/// DIFERENCIA entre dos refrescos, así que hace falta el mismo objeto entre
/// llamadas (crear uno nuevo cada vez daría siempre 0).
static SISTEMA: LazyLock<Mutex<System>> = LazyLock::new(|| Mutex::new(System::new()));

/// Refresca y devuelve el resumen. Se llama una vez por foto.
pub fn resumen() -> Resumen {
    let mut s = SISTEMA.lock();
    s.refresh_cpu_all();
    s.refresh_memory();

    let mb = |bytes: u64| bytes as f64 / 1024.0 / 1024.0;
    let total = s.total_memory();
    let disp = s.available_memory();
    let libre = s.free_memory();
    let usado = total.saturating_sub(disp);
    let st = s.total_swap();
    let sl = s.free_swap();
    let su = st.saturating_sub(sl);
    let carga = System::load_average();

    Resumen {
        cpu_pct: s.global_cpu_usage() as f64,
        cores: s.cpus().len() as i32,
        load1: carga.one,
        load5: carga.five,
        load15: carga.fifteen,
        mem_total_mb: mb(total),
        mem_usado_mb: mb(usado),
        mem_libre_mb: mb(libre),
        mem_disp_mb: mb(disp),
        mem_pct: if total > 0 {
            usado as f64 * 100.0 / total as f64
        } else {
            0.0
        },
        swap_total_mb: mb(st),
        swap_usado_mb: mb(su),
        swap_libre_mb: mb(sl),
        swap_pct: if st > 0 {
            su as f64 * 100.0 / st as f64
        } else {
            0.0
        },
    }
}

pub fn uptime() -> i64 {
    System::uptime() as i64
}

/// Frecuencias de CPU, en MHz, por núcleo.
///
/// `None` cuando el sistema no las publica (pasa en algunas máquinas virtuales y
/// donde el contador de frecuencia no es legible): quien las enseñe tiene que decir
/// «—» o «el sistema no publica la frecuencia», nunca inventarse un número.
#[derive(Debug, Clone)]
pub struct Frecuencias {
    pub min: f64,
    pub max: f64,
    pub media: f64,
    pub nucleos: usize,
    /// La del primer núcleo. Es una medida real (el «ahora» de ese núcleo); lo que
    /// NO se hace es inventarse un «actual del equipo» promediando por detrás.
    pub primera: f64,
}

pub fn frecuencia_cpu_mhz() -> Option<Frecuencias> {
    let mut s = SISTEMA.lock();
    s.refresh_cpu_frequency();
    let frec: Vec<f64> = s
        .cpus()
        .iter()
        .map(|c| c.frequency() as f64)
        .filter(|f| *f > 0.0)
        .collect();
    if frec.is_empty() {
        return None;
    }
    Some(Frecuencias {
        min: frec.iter().cloned().fold(f64::MAX, f64::min),
        max: frec.iter().cloned().fold(0.0, f64::max),
        media: frec.iter().sum::<f64>() / frec.len() as f64,
        nucleos: frec.len(),
        primera: frec[0],
    })
}

/// Temperaturas que publica el propio sistema (`sysinfo::Components`), con su
/// etiqueta y sus umbrales.
///
/// Es lo ÚNICO de los sensores que se puede leer en los tres sistemas; en Linux,
/// además, está `sensores.rs` con ventiladores, voltajes y potencia.
pub fn temperaturas() -> Vec<(String, f64, Option<f64>, Option<f64>)> {
    let c = sysinfo::Components::new_with_refreshed_list();
    c.list()
        .iter()
        .filter_map(|x| x.temperature().map(|t| (x.label().to_string(), t as f64, x.max().map(|v| v as f64), x.critical().map(|v| v as f64))))
        .collect()
}

/* ── Procesos ─────────────────────────────────────────────────────────────── */

/// Un proceso, con lo que la interfaz necesita para decidir si es de IA.
#[derive(Debug, Clone)]
pub struct Proceso {
    pub pid: i32,
    pub nombre: String,
    pub cmd: String,
    pub memoria_mb: f64,
    pub cpu_pct: f64,
    /// Segundos que lleva vivo.
    pub uptime_secs: i64,
}

/// Todos los procesos, refrescados de una vez.
///
/// Antes esto leía `/proc` a mano (y contaba mal el RSS: leía `statm`, que son
/// PÁGINAS, y lo dividía entre 1024 como si fueran kB). `sysinfo` ya devuelve
/// bytes en los tres sistemas.
pub fn procesos() -> Vec<Proceso> {
    let mut s = SISTEMA.lock();
    s.refresh_processes(ProcessesToUpdate::All, true);
    s.processes()
        .values()
        .map(|p| Proceso {
            pid: p.pid().as_u32() as i32,
            nombre: p.name().to_string_lossy().to_string(),
            // La línea de comandos COMPLETA, que es lo que usan los patrones de
            // `scan.rs` para reconocer un motor (`--model`, `llama-server`…).
            cmd: p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" "),
            memoria_mb: p.memory() as f64 / 1024.0 / 1024.0,
            cpu_pct: p.cpu_usage() as f64,
            uptime_secs: p.run_time() as i64,
        })
        .collect()
}

/// Mata un proceso por su PID. Devuelve `false` si ya no existe o no se pudo.
pub fn matar(pid: i32) -> bool {
    let mut s = SISTEMA.lock();
    let pid = sysinfo::Pid::from_u32(pid as u32);
    // Hay que tener el proceso refrescado para poder señalarlo.
    s.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
    s.process(pid).map(|p| p.kill()).unwrap_or(false)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn dice_que_sistema_es() {
        assert!(["linux", "macos", "windows", "otro"].contains(&so()));
        assert!(!nombre_so().is_empty());
    }

    #[test]
    fn las_rutas_del_sistema_existen_o_son_componibles() {
        let r = rutas();
        // El home SIEMPRE existe; cache/config/datos los crea la app si no están.
        assert!(r.home.is_dir(), "{:?}", r.home);
        assert!(r.temp.is_dir(), "{:?}", r.temp);
        assert!(r.cache.is_absolute());
        assert!(r.config.is_absolute());
    }

    #[test]
    fn los_discos_no_traen_pseudo_sistemas_y_suman_bien() {
        let d = discos();
        // En cualquier equipo real hay al menos un disco del usuario.
        assert!(!d.is_empty(), "ningún disco: {d:?}");
        for x in &d {
            assert!(x.total > 0, "{x:?}");
            assert!(x.usado <= x.total, "{x:?}");
            assert!(!es_pseudo(&x.fs, &x.punto), "se coló un pseudo-sistema: {x:?}");
            let esperado = x.usado as f64 * 100.0 / x.total as f64;
            assert!((x.uso_pct - esperado).abs() < 0.01, "{x:?}");
        }
    }

    #[test]
    fn el_disco_de_una_ruta_es_el_mas_especifico() {
        let home = rutas().home;
        let d = disco_de(&home).expect("el home está en algún disco");
        // Se compara contra el camino CANÓNICO: en esta máquina `/home` es un
        // enlace a `/var/home`, y el disco que se enseña es el de la ruta real
        // (por eso `disco_de` canonicaliza antes de comparar). Se quita el prefijo
        // `\\?\` de Windows igual que hace `disco_de`, o la comparación del test
        // fallaría aunque el código estuviera bien.
        let real = sin_prefijo_verbatim(home.canonicalize().unwrap_or(home.clone()));
        assert!(
            real.starts_with(&d.punto),
            "{:?} (real {:?}) no está en {:?}",
            home,
            real,
            d.punto
        );
    }

    #[test]
    fn el_resumen_tiene_datos_coherentes() {
        let r = resumen();
        assert!(r.cores >= 1, "{r:?}");
        assert!(r.mem_total_mb > 0.0, "{r:?}");
        assert!(r.mem_usado_mb <= r.mem_total_mb, "{r:?}");
        assert!((0.0..=100.0).contains(&r.mem_pct), "{r:?}");
        assert!(uptime() > 0);
    }

    #[test]
    fn ve_procesos_y_encuentra_el_suyo() {
        let ps = procesos();
        assert!(ps.len() > 1, "solo {} procesos", ps.len());
        // El propio proceso de la prueba tiene que estar: si no, la lectura está
        // mirando a otro sitio.
        let yo = std::process::id() as i32;
        let mio = ps.iter().find(|p| p.pid == yo);
        assert!(mio.is_some(), "no me veo entre {} procesos", ps.len());
        assert!(!mio.unwrap().nombre.is_empty());
    }

    #[test]
    fn matar_un_pid_que_no_existe_no_tumba_nada() {
        // Un PID altísimo no existe: tiene que devolver false, no entrar en pánico.
        assert!(!matar(2_147_483_000));
    }
}
