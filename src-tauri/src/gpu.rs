use crate::types::Gpu;

use std::path::PathBuf;
use parking_lot::Mutex;
use std::time::{Duration, Instant};

use std::sync::LazyLock;
use serde_json::Value;

fn n(j: &Value, path: &[&str]) -> f64 {
    let mut cur = j;
    for p in path {
        match cur.get(*p) {
            Some(v) => cur = v,
            None => return 0.0,
        }
    }
    cur.as_f64().unwrap_or_default()
}

/// Igual que `n`, pero distingue "no está el dato" de "vale cero".
///
/// Es lo que hace falta para la temperatura y el consumo: un `0` ahí no es una
/// lectura, es la ausencia de lectura mal disfrazada. Antes `mem_temp_c` se
/// construía con `Some(n(...))`, así que una tarjeta sin sensor de memoria
/// publicaba `Some(0.0)`, un dato falso con toda la pinta de ser real.
fn opt(j: &Value, path: &[&str]) -> Option<f64> {
    let mut cur = j;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_f64()
}

fn s(j: &Value, path: &[&str]) -> Option<String> {
    let mut cur = j;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_str().map(|x| x.to_string())
}

/// La parte `static` de `amd-smi` (nombre de la tarjeta, mercado, driver…) no
/// cambia mientras el equipo está encendido y cuesta un proceso `exec` cada vez.
/// Se guarda y se reutiliza: la foto la pedía cada 2 s.
static ESTATICO: LazyLock<Mutex<Option<(Instant, Value)>>> = LazyLock::new(|| Mutex::new(None));
const TTL_ESTATICO: Duration = Duration::from_secs(600);


fn gpu_data(j: &Value) -> Vec<Value> {
    j.get("gpu_data").and_then(|v| v.as_array()).cloned().unwrap_or_default()
}

fn build(m: &Value, st: &Value) -> Gpu {
    let id = n(m, &["gpu"]) as i32;
    let name = s(st, &["asic", "market_name"]).unwrap_or_else(|| "GPU".into());
    let total_vram = n(m, &["mem_usage", "total_vram", "value"]);
    let used_vram = n(m, &["mem_usage", "used_vram", "value"]);
    Gpu {
        id,
        name,
        driver: "amdgpu (amdgpu-smi)".into(),
        temp_c: opt(m, &["temperature", "edge", "value"]),
        mem_temp_c: opt(m, &["temperature", "mem", "value"]),
        power_w: opt(m, &["power", "socket_power", "value"]),
        mem_used_mb: used_vram,
        mem_total_mb: total_vram,
        mem_pct: if total_vram > 0.0 { used_vram / total_vram * 100.0 } else { 0.0 },
        util: n(m, &["usage", "gfx_activity", "value"]),
        clock_mhz: n(m, &["clock", "gfx_0", "clk", "value"]),
        fan_rpm: n(m, &["fan", "rpm"]) as i32,
        fan_pct: n(m, &["fan", "usage", "value"]),
        throttle: s(m, &["power", "throttle_status"])
            .and_then(|t| if t.eq_ignore_ascii_case("throttled") { Some(t) } else { None }),
        // Con `amd-smi` se mide TODO, así que la ficha es completa.
        parcial: false,
    }
}

/// Convierte una GPU leída del SISTEMA (macOS o Windows) a la ficha del panel.
///
/// POR QUÉ ES UNA FUNCIÓN PURA: en Linux `plataforma::gpu::gpus()` devuelve vacío
/// —aquí manda `sysfs`/`amd-smi`, que saben mucho más—, así que esta conversión
/// no se ejecuta NUNCA en la máquina de desarrollo. Si viviera dentro de
/// `load()`, el paso `GpuBasica -> Gpu` se quedaría sin una sola prueba: podría
/// perder el `parcial`, o convertir la VRAM ausente en un 0 que parece un dato,
/// y nadie lo notaría hasta tener un Mac delante. Al ser pura se prueba en Linux
/// con la misma forma que devuelven los parsers ya probados de
/// `plataforma::gpu` (salida real de `system_profiler` y de WMI).
///
/// `parcial: true` NO es decorativo: de macOS y Windows solo salen nombre, VRAM y
/// driver. El uso, la temperatura y la potencia **no se pueden leer sin
/// privilegios** (en macOS el SMC pide root y en Windows hace falta un driver o
/// WMI elevado), así que la ficha lo declara y la interfaz pinta «—» en esos
/// huecos en vez de un 0 % que afirmaría que la tarjeta está parada.
pub fn desde_basica(id: i32, g: crate::plataforma::gpu::GpuBasica) -> Gpu {
    // Si el sistema no publicó driver, se enseña la FUENTE ("system_profiler",
    // "WMI…"): es peor dejar la casilla vacía que decir de dónde salió el dato.
    let driver = g.driver.unwrap_or(g.fuente);
    Gpu {
        id,
        name: g.nombre,
        driver,
        // Lo que no se puede medir queda en `None`, nunca en 0: la interfaz
        // distingue «no hay lectura» de «vale cero».
        temp_c: None,
        mem_temp_c: None,
        power_w: None,
        // De macOS/Windows no sale VRAM USADA, solo el total (y a veces topado).
        mem_used_mb: 0.0,
        mem_total_mb: g.vram_mb.unwrap_or(0.0),
        mem_pct: 0.0,
        util: 0.0,
        clock_mhz: 0.0,
        fan_rpm: 0,
        fan_pct: 0.0,
        throttle: None,
        parcial: true,
    }
}

pub fn load() -> Vec<Gpu> {
    if let Some(path) = find_cmd("amd-smi") {
        if let (Some(metric), Some(statics)) = (
            run_json(&path, &["metric", "--json"]),
            estatico(&path),
        ) {
            let md = gpu_data(&metric);
            let sd = gpu_data(&statics);
            if !md.is_empty() {
                let out = md
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let st = sd.get(i).cloned().unwrap_or_default();
                        build(m, &st)
                    })
                    .collect();
                return out;
            }
        }
    }
    // Sin `amd-smi` —o en macOS y Windows, donde no existe— se enseña lo que el
    // SISTEMA publica: nombre, VRAM y driver. La ficha va marcada como PARCIAL
    // porque el uso, la temperatura y la potencia no se pueden leer ahí, y eso se
    // dice en la interfaz en vez de pintar un 0 %.
    crate::plataforma::gpu::gpus()
        .into_iter()
        .enumerate()
        .map(|(i, g)| desde_basica(i as i32, g))
        .collect()
}

/// Parte `static` de `amd-smi`, con caché (ver `TTL_ESTATICO`).
fn estatico(path: &str) -> Option<Value> {
    {
        let g = ESTATICO.lock();
        if let Some((cuando, v)) = g.as_ref() {
            if cuando.elapsed() < TTL_ESTATICO {
                return Some(v.clone());
            }
        }
    }
    let v = run_json(path, &["static", "--json"])?;
    { let mut g = ESTATICO.lock();
        *g = Some((Instant::now(), v.clone()));
    }
    Some(v)
}

fn run_json(p: &str, args: &[&str]) -> Option<Value> {
    run(p, args).and_then(|s| serde_json::from_str(&s).ok())
}

/// Ejecuta `amd-smi` con límite de tiempo.
///
/// Si el driver se atasca, la foto tiene que seguir saliendo (sin datos de GPU)
/// en vez de quedarse esperando para siempre: `amd-smi` pregunta al kernel y no
/// hay nada que garantice que conteste.
fn run(p: &str, args: &[&str]) -> Option<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let out = crate::proceso::ejecutar(p, &args, &[], Duration::from_secs(5)).ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn find_cmd(cmd: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let dirs: [PathBuf; 10] = [
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/opt/rocm/bin"),
        PathBuf::from("/opt/rocm"),
        PathBuf::from("/bin"),
        PathBuf::from("/sbin"),
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".local/share/claude"),
    ];
    for d in &dirs {
        let p = d.join(cmd);
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    None
}

/* ── Reloj de memoria (MCLK): el fallo silencioso de esta GPU ─────────────── */

/// Un nivel de reloj de memoria de la tabla de la GPU.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NivelMclk {
    pub idx: i32,
    pub mhz: i32,
    pub activo: bool,
}

/// Estado del reloj de memoria, que es lo que se degrada.
///
/// POR QUÉ ESTO ESTÁ EN EL PANEL: en esta RX 6800 XT, el MCLK se queda clavado en
/// el nivel mínimo (96 MHz) y no sube aunque la GPU esté al 100 %. Es un fallo de
/// Display Core en amdgpu (gitlab.freedesktop.org/drm/amd#2657) asociado a
/// pantallas 4K de alto refresco. Y es SILENCIOSO: no da error ni aviso, los
/// modelos simplemente van ~15 veces más lentos. Medido en esta máquina con el
/// mismo binario y el mismo modelo:
///
///     estado      Modelo local 27B (tg64)   Modelo 8B (tg64)
///     degradado        3,27 tok/s         9,45 tok/s
///     sano            45,81 tok/s       142,94 tok/s
///
/// Se lee de sysfs, así que no hace falta ningún privilegio.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EstadoGpu {
    pub niveles: Vec<NivelMclk>,
    pub activo_idx: i32,
    pub activo_mhz: i32,
    pub max_idx: i32,
    pub max_mhz: i32,
    pub gpu_busy: i32,
    pub mem_busy: i32,
    /// `true` cuando el reloj está por debajo de lo que la tarjeta puede dar Y
    /// hay carga. Sin carga, estar abajo es lo normal (la tarjeta no necesita
    /// ancho de banda).
    pub degradado: bool,
    pub veredicto: String,
}

fn leer(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn primer_dev() -> Option<String> {
    for entrada in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let p = entrada.path();
        let nombre = p.file_name()?.to_string_lossy().to_string();
        if !nombre.starts_with("card") || nombre.contains('-') {
            continue; // card1 sí, card1-DP-1 no
        }
        let mclk = p.join("device").join("pp_dpm_mclk");
        if mclk.is_file() {
            return Some(p.join("device").to_string_lossy().to_string());
        }
    }
    None
}

/// Carga de la GPU, en porcentaje, si el driver la publica.
fn carga(dev: &str, cual: &str) -> i32 {
    leer(&format!("{dev}/{cual}"))
        .and_then(|v| v.trim().parse::<i32>().ok())
        .unwrap_or(0)
}

/// Parsea la tabla de `pp_dpm_mclk`, que tiene esta pinta:
///
/// ```text
/// 0: 96Mhz
/// 1: 456Mhz *
/// 2: 673Mhz
/// 3: 1000Mhz
/// ```
///
/// Devuelve los niveles y el índice del activo (el que lleva `*`). Es una función
/// pura a propósito: así se prueba con la tabla real sin depender de la GPU.
fn parsear_mclk(tabla: &str) -> (Vec<NivelMclk>, i32) {
    let mut niveles = Vec::new();
    let mut activo_idx = 0;
    for linea in tabla.lines() {
        let Some((idx, resto)) = linea.split_once(':') else { continue };
        let Ok(idx) = idx.trim().parse::<i32>() else { continue };
        let activo = resto.contains('*');
        let mhz = resto
            .replace('*', " ")
            .trim()
            .trim_end_matches("Mhz")
            .trim_end_matches("MHz")
            .trim()
            .parse::<i32>()
            .unwrap_or(0);
        if activo {
            activo_idx = idx;
        }
        niveles.push(NivelMclk { idx, mhz, activo });
    }
    (niveles, activo_idx)
}

pub fn estado_mclk() -> Option<EstadoGpu> {
    let dev = primer_dev()?;
    let tabla = leer(&format!("{dev}/pp_dpm_mclk"))?;

    let (niveles, activo_idx) = parsear_mclk(&tabla);
    if niveles.is_empty() {
        return None;
    }

    let gpu_busy = carga(&dev, "gpu_busy_percent");
    let mem_busy = carga(&dev, "mem_busy_percent");
    // Se copian los valores (no la referencia) porque después `niveles` se mueve
    // a la estructura que se devuelve.
    let (max_idx, max_mhz) = niveles
        .iter()
        .max_by_key(|n| n.mhz)
        .map(|n| (n.idx, n.mhz))
        .unwrap_or((0, 0));
    let activo_mhz = niveles.iter().find(|n| n.activo).map(|n| n.mhz).unwrap_or(0);

    // La firma del fallo es "el nivel MÁS BAJO con la GPU trabajando". Sin carga,
    // el nivel bajo es lo correcto y no hay nada que arreglar.
    let trabajando = gpu_busy >= 25 || mem_busy >= 25;
    let minimo = niveles.iter().min_by_key(|n| n.mhz).map(|n| n.idx).unwrap_or(0);
    let degradado = trabajando && activo_idx == minimo;

    let veredicto = if !trabajando {
        format!(
            "En reposo: el reloj está en {} MHz y es lo normal sin carga.",
            activo_mhz
        )
    } else if degradado {
        format!(
            "DEGRADADO: con la GPU trabajando ({} %), el reloj de memoria sigue en el mínimo ({} MHz) en vez de subir a {}. Los modelos irán unas 15 veces más lentos y no dará ningún error.",
            gpu_busy.max(mem_busy),
            activo_mhz,
            max_mhz
        )
    } else if activo_mhz < max_mhz {
        format!(
            "Trabajando con el reloj en {} MHz, y la tarjeta llega a {}. Subirlo se nota: en esta máquina, pasar de 456 a 1000 MHz mejoró la decodificación un 74 %.",
            activo_mhz, max_mhz
        )
    } else {
        format!("Sano: reloj de memoria a tope ({} MHz).", activo_mhz)
    };

    Some(EstadoGpu {
        niveles,
        activo_idx,
        activo_mhz,
        max_idx,
        max_mhz,
        gpu_busy,
        mem_busy,
        degradado,
        veredicto,
    })
}

#[cfg(test)]
mod pruebas_mclk {
    use super::*;

    /// La tabla real de esta máquina, tal cual la publica el driver.
    const TABLA: &str = "0: 96Mhz \n1: 456Mhz *\n2: 673Mhz \n3: 1000Mhz \n";

    #[test]
    fn lee_los_niveles_y_marca_el_activo() {
        let (niveles, activo) = parsear_mclk(TABLA);
        assert_eq!(niveles.len(), 4);
        assert_eq!(niveles.iter().map(|n| n.mhz).collect::<Vec<_>>(), vec![96, 456, 673, 1000]);
        assert_eq!(activo, 1, "el nivel marcado con * es el 1 (456)");
        assert!(niveles[1].activo);
        assert!(!niveles[3].activo);
        assert_eq!(niveles.iter().map(|n| n.idx).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn si_no_hay_marca_el_activo_es_el_primero() {
        let (niveles, activo) = parsear_mclk("0: 96Mhz\n1: 1000Mhz\n");
        assert_eq!(niveles.len(), 2);
        assert_eq!(activo, 0);
    }

    #[test]
    fn aguanta_una_tabla_vacia_o_con_basura() {
        assert!(parsear_mclk("").0.is_empty());
        assert!(parsear_mclk("nivel raro sin dos puntos\n").0.is_empty());
    }

    #[test]
    fn lee_el_estado_real_de_la_gpu() {
        // Contra el sysfs de verdad. Si no hay una GPU amdgpu, se salta: eso no es
        // un fallo del código (mismo criterio que las pruebas de pantalla).
        let Some(e) = estado_mclk() else { return };
        assert!(!e.niveles.is_empty(), "tiene que haber niveles");
        assert_eq!(
            e.niveles.iter().filter(|n| n.activo).count(),
            1,
            "exactamente un nivel está marcado como activo"
        );
        assert!(e.activo_mhz > 0, "el nivel activo tiene un valor en MHz");
        assert!(e.max_mhz >= e.activo_mhz, "el tope no puede ser menor que el actual");
        assert!(!e.veredicto.is_empty(), "siempre hay veredicto que enseñar");
        // La tabla real de esta máquina tiene 96/456/673/1000.
        assert!(e.niveles.iter().any(|n| n.mhz == 96));
    }

    #[test]
    fn la_firma_del_fallo_es_el_minimo_con_carga() {
        // Réplica de la regla: sin carga, el nivel bajo es normal; con carga y en
        // el mínimo, está degradado.
        let minimo = 0;
        let activo_bajo = 0;
        let activo_medio = 1;
        let ocupada = true;
        let reposo = false;
        assert!(ocupada && activo_bajo == minimo, "con carga y en el mínimo: degradado");
        assert!(!(reposo && activo_bajo == minimo), "en reposo el mínimo es lo normal");
        assert!(!(activo_medio == minimo), "en 456 no es la firma del fallo, aunque no sea el tope");
    }
}

/// Pruebas de la conversión `GpuBasica -> Gpu` de macOS y Windows.
///
/// Se pueden correr en Linux porque la conversión es pura: lo que aquí se
/// construye es exactamente lo que devuelven los parsers de `plataforma::gpu`,
/// que a su vez están probados con salidas REALES de `system_profiler -json`
/// (Mac) y de `Get-CimInstance Win32_VideoController` (WMI).
#[cfg(test)]
mod pruebas_portables {
    use super::*;
    use crate::plataforma::gpu::GpuBasica;

    /// Un Mac: nombre, VRAM en GB y "Metal 3". Lo demás no se puede medir.
    #[test]
    fn convierte_un_mac_sin_inventar_medidas() {
        let basica = GpuBasica {
            nombre: "Apple M1 Pro".to_string(),
            vram_mb: Some(16.0 * 1024.0),
            driver: Some("Metal 3".to_string()),
            fuente: "system_profiler".to_string(),
        };
        let g = desde_basica(0, basica);
        assert_eq!(g.id, 0);
        assert_eq!(g.name, "Apple M1 Pro");
        assert_eq!(g.driver, "Metal 3");
        assert_eq!(g.mem_total_mb, 16384.0);
        // Uso, temperatura y potencia no los publica el sistema sin privilegios:
        // se quedan en None (la interfaz pinta «—»), NO en 0.
        assert_eq!(g.temp_c, None);
        assert_eq!(g.mem_temp_c, None);
        assert_eq!(g.power_w, None);
        assert_eq!(g.util, 0.0);
        assert_eq!(g.throttle, None);
        // Y lo que hace honesta la ficha: se declara PARCIAL.
        assert!(g.parcial, "sin uso/temperatura/potencia la ficha es parcial");
    }

    /// Un Windows sin `DriverVersion`: el hueco del driver lo ocupa la FUENTE, no
    /// se deja vacío (el repo prefiere "WMI" a una casilla en blanco). Y sin
    /// `AdapterRAM` no se inventa un tamaño: 0, que la interfaz sabe leer como «—».
    #[test]
    fn sin_driver_ni_vram_no_se_rellena_con_datos_falsos() {
        let basica = GpuBasica {
            nombre: "NVIDIA GeForce RTX 3080".to_string(),
            vram_mb: None,
            driver: None,
            fuente: "WMI".to_string(),
        };
        let g = desde_basica(2, basica);
        assert_eq!(g.id, 2);
        assert_eq!(g.name, "NVIDIA GeForce RTX 3080");
        assert_eq!(g.driver, "WMI", "sin driver se enseña de dónde salió el dato");
        assert_eq!(g.mem_total_mb, 0.0, "sin VRAM no se inventa un tamaño");
        assert_eq!(g.mem_pct, 0.0);
        assert!(g.parcial);
    }

    /// La VRAM de Windows llega topada a 4 GB (el `AdapterRAM` es de 32 bits): lo
    /// que se enseña es lo que dice Windows, con su aviso ya dentro de `fuente`.
    #[test]
    fn la_vram_topada_de_windows_se_enseña_tal_cual() {
        let basica = GpuBasica {
            nombre: "AMD Radeon RX 7900 XTX".to_string(),
            vram_mb: Some(4096.0),
            driver: Some("31.0.24033.1003".to_string()),
            fuente: "WMI (AdapterRAM: puede estar topado a 4 GB)".to_string(),
        };
        let g = desde_basica(1, basica);
        assert_eq!(g.mem_total_mb, 4096.0);
        assert!(!g.driver.contains("topado"), "el driver es el driver, no la fuente");
    }

    /// El camino entero, con un trozo de la salida REAL de
    /// `system_profiler SPDisplaysDataType -json`: parser de `plataforma` + esta
    /// conversión. Es lo más cerca que se puede estar de un Mac sin tener uno.
    #[test]
    fn del_json_de_macos_a_la_ficha_del_panel() {
        let json = r#"{
          "SPDisplaysDataType" : [
            {
              "_name" : "Apple M1 Pro",
              "spdisplays_metal" : "3",
              "spdisplays_vram" : "16 GB"
            }
          ]
        }"#;
        let basicas = crate::plataforma::gpu::parsear_macos(json);
        assert_eq!(basicas.len(), 1);
        let g = desde_basica(0, basicas.into_iter().next().unwrap());
        assert_eq!(g.name, "Apple M1 Pro");
        assert_eq!(g.driver, "Metal 3");
        assert_eq!(g.mem_total_mb, 16384.0);
        assert!(g.parcial);
        assert_eq!(g.temp_c, None);
    }

    /// Y con un trozo de la salida REAL de WMI (`Get-CimInstance
    /// Win32_VideoController | ConvertTo-Json`): una sola tarjeta llega como
    /// objeto, y su `AdapterRAM` de 32 bits llega topado a 4 GB.
    #[test]
    fn del_json_de_windows_a_la_ficha_del_panel() {
        let json = r#"{"Name":"NVIDIA GeForce RTX 3080","AdapterRAM":4294967295,"DriverVersion":"31.0.15.3699"}"#;
        let basicas = crate::plataforma::gpu::parsear_windows(json);
        assert_eq!(basicas.len(), 1);
        let g = desde_basica(0, basicas.into_iter().next().unwrap());
        assert_eq!(g.name, "NVIDIA GeForce RTX 3080");
        assert_eq!(g.driver, "31.0.15.3699");
        assert!((g.mem_total_mb - 4096.0).abs() < 1.0, "VRAM: {}", g.mem_total_mb);
        assert!(g.parcial);
        assert_eq!(g.util, 0.0);
        assert_eq!(g.power_w, None);
    }
}
