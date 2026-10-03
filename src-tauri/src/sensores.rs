//! Sensores del equipo: TODO lo que la máquina expone por `/sys/class/hwmon`,
//! más las medidas que faltaban de caudal (disco y red) y de frecuencia por
//! núcleo.
//!
//! POR QUÉ UN MÓDULO APARTE. `system.rs` lee lo que `/proc` da (CPU, memoria,
//! disco) y `gpu.rs` lee la tarjeta. Pero en este equipo hay mucho más: la placa
//! `nct6683` publica 8 ventiladores con su nombre ("CPU Fan", "Pump Fan", "System
//! Fan #1"…) y 14 voltajes (CPU Vcore, DRAM, +12V…), la CPU un segundo sensor
//! (`Tccd1`, el del CCD), cada NVMe su temperatura, la WiFi la suya, y el
//! contador de energía de AMD permite despejar la potencia del paquete de CPU.
//! Nada de eso se estaba enseñando.
//!
//! TRES REGLAS, y las tres vienen de fallos vistos en otros sitios:
//!
//! 1. **Un 0 no es una medida.** En esta placa hay cinco temperaturas que leen
//!    exactamente 0 (VRM MOS, PCH, CPU Socket, PCIe x1, M2_1): esos sensores NO
//!    están conectados. Enseñar "0 °C" afirmaría que algo está a cero grados. Se
//!    descartan y se dice cuántos se han descartado.
//! 2. **Cada cifra dice de DÓNDE sale.** El sensor lleva su chip y su etiqueta de
//!    sysfs, y no se traduce el nombre a algo más bonito: "CPU Fan" es el nombre
//!    que da la placa, y así se puede comprobar con `sensors`.
//! 3. **Lo que no está, no se pinta.** Sin ventiladores, sin NVMe o sin batería,
//!    la sección correspondiente no aparece.
//!
//! Y una medida que necesita DOS lecturas: la potencia de la CPU se despeja del
//! contador de energía de `zenergy` (microjulios acumulados) dividiendo por el
//! tiempo entre lecturas. La primera vuelta no tiene dato anterior, así que
//! devuelve `None` y punto: inventarse un vatio para llenar la casilla sería
//! justo lo que este programa no hace.
//!
//! QUÉ PASA EN macOS Y WINDOWS: la parte de `hwmon` (ventiladores, voltajes,
//! potencia de CPU) es una interfaz del kernel de Linux y allí **no existe**, así
//! que este mismo código simplemente no encuentra nada (`/sys/class/hwmon` no
//! está) y devuelve la lista vacía. La sección enseña las temperaturas que sí da
//! el sistema (vía `plataforma::temperaturas`, que usa lo que publique cada uno) y
//! dice POR QUÉ no hay nada más, en vez de poner ceros: no es que el programa no
//! sepa leerlo, es que esos sensores solo se exponen a un programa con
//! privilegios (en macOS los da la API privada del SMC con root; en Windows hacen
//! falta un driver o WMI elevado). La frecuencia y los caudales SÍ salen en los
//! tres sistemas, porque los da `plataforma`.
//!
//! Y la potencia de la CPU se distingue con `cpu_potencia_fuente`: `None` ahí
//! significa «este sistema no publica el contador» (no «espera a la siguiente
//! lectura»), para que la interfaz no afirme algo falso.

use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::sync::LazyLock;
use std::time::Instant;

use serde::Serialize;

/// De qué clase es una medida. Decide la unidad que se enseña y cómo se lee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Clase {
    Temperatura,
    Ventilador,
    Voltaje,
    Potencia,
    Corriente,
    Energia,
}

impl Clase {
    /// El sufijo del fichero en sysfs y la unidad en la que se enseña.
    fn de_nombre(fichero: &str) -> Option<(Clase, &'static str, f64)> {
        // El factor convierte lo que guarda sysfs a la unidad de salida: milésimas
        // de grado a grados, milivoltios a voltios, microvatios a vatios.
        if fichero.starts_with("temp") {
            Some((Clase::Temperatura, "°C", 0.001))
        } else if fichero.starts_with("fan") {
            Some((Clase::Ventilador, "rpm", 1.0))
        } else if fichero.starts_with("in") {
            Some((Clase::Voltaje, "V", 0.001))
        } else if fichero.starts_with("power") {
            Some((Clase::Potencia, "W", 0.000001))
        } else if fichero.starts_with("curr") {
            Some((Clase::Corriente, "A", 0.001))
        } else if fichero.starts_with("energy") {
            Some((Clase::Energia, "J", 0.000001))
        } else {
            None
        }
    }

}

/// Una medida de un sensor

/// Una medida de un sensor, con su procedencia.
#[derive(Debug, Clone, Serialize)]
pub struct Sensor {
    /// El chip que lo publica, tal cual lo dice sysfs (`nct6683`, `k10temp`…).
    pub chip: String,
    /// Un nombre legible para el chip, cuando se sabe qué es. Si no, el suyo.
    pub chip_legible: String,
    /// El nombre que da el propio chip ("CPU Fan", "Tctl", "+12V") o, si no lo
    /// da, el identificador del sensor (`fan4`, `temp7`). Nunca se traduce.
    pub etiqueta: String,
    pub clase: Clase,
    pub valor: f64,
    pub unidad: String,
    /// Umbral de aviso del chip, si lo publica (en la misma unidad).
    pub max: Option<f64>,
    /// Umbral crítico del chip, si lo publica.
    pub critico: Option<f64>,
    /// De dónde sale el número: la ruta sysfs. Va en el `title` de la interfaz,
    /// para poder comprobarlo con `cat`.
    pub fuente: String,
}

/// Temperaturas de los discos, leídas de `hwmon` (NVMe) y de `smartctl` si está.
#[derive(Debug, Clone, Serialize)]
pub struct TempDisco {
    pub nombre: String,
    /// `°C`, o `None` si no se pudo leer (un disco que no informa, o smartctl sin
    /// permisos).
    pub temp_c: Option<f64>,
    /// De dónde sale: "hwmon (nvme)" o "smartctl". Es lo que permite distinguir
    /// una medida de un «no se pudo».
    pub fuente: String,
}

/// Caudal de un disco: bytes por segundo de lectura y escritura.
#[derive(Debug, Clone, Serialize)]
pub struct CaudalDisco {
    pub nombre: String,
    pub leer_b_s: f64,
    pub escribir_b_s: f64,
}

/// Caudal de una interfaz de red.
#[derive(Debug, Clone, Serialize)]
pub struct CaudalRed {
    pub nombre: String,
    pub rx_b_s: f64,
    pub tx_b_s: f64,
    /// Se está usando ahora (tiene tráfico o está levantada). Sirve para no
    /// enseñar las cinco interfaces virtuales de Docker cuando no hacen nada.
    pub activa: bool,
}

/// Frecuencia de la CPU: el reparto entre núcleos, que es lo que explica un
/// "72%" de escalado del procesador.
#[derive(Debug, Clone, Serialize, Default)]
pub struct FrecuenciaCpu {
    pub actual_mhz: Option<f64>,
    pub media_mhz: Option<f64>,
    pub min_mhz: Option<f64>,
    pub max_mhz: Option<f64>,
    /// Cuántos núcleos se han podido leer.
    pub nucleos: usize,
}

/// Un chip de sensores con sus medidas.
#[derive(Debug, Clone, Serialize)]
pub struct GrupoSensores {
    pub chip: String,
    pub chip_legible: String,
    /// El driver del kernel que lo publica, sacado del enlace `device` del hwmon
    /// (por ejemplo `nct6687` o `nct6775`). Es lo que permite entender por qué la
    /// misma placa aparece dos veces con números distintos.
    pub driver: Option<String>,
    pub items: Vec<Sensor>,
    pub descartados: usize,
    /// Tiene ventiladores y TODOS leen 0 mientras otro chip del equipo sí ve
    /// ventiladores girando. Es la firma del chip mal enlazado: esta placa la
    /// publican `nct6687` (que lee los 5 ventiladores que giran, con su nombre) y
    /// `nct6775` (que lee 0 en los 7 suyos y no pone nombre a los voltajes). Se
    /// enseña como aviso en vez de esconder sus filas: los datos son suyos y el
    /// usuario decide.
    pub ventiladores_a_cero: bool,
}

/// Todo lo del equipo que no ven `system.rs` ni `gpu.rs`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Hardware {
    pub grupos: Vec<GrupoSensores>,
    pub descartados: usize,
    pub cpu_potencia_w: Option<f64>,
    /// De dónde sale `cpu_potencia_w`, o `None` si este sistema NO publica ningún
    /// contador de energía de la CPU.
    ///
    /// POR QUÉ HACE FALTA UN CAMPO APARTE: `cpu_potencia_w` vale `None` en DOS
    /// casos que significan cosas distintas y que la interfaz no puede confundir:
    /// (1) es la primera lectura y todavía no hay delta que dividir, y (2) este
    /// sistema no tiene contador de energía. Sin este campo, la tarjeta de CPU
    /// decía «se mide en la siguiente lectura» en macOS y Windows PARA SIEMPRE,
    /// que es falso: ahí no hay contador que esperar. macOS lo tiene en el SMC,
    /// que pide root, y Windows en ACPI/WMI elevado, así que sin privilegios no
    /// hay nada que leer y se dice, en vez de esperar un dato que no va a llegar.
    pub cpu_potencia_fuente: Option<String>,
    pub cpu_frecuencia: FrecuenciaCpu,
    pub discos_temp: Vec<TempDisco>,
    pub discos_caudal: Vec<CaudalDisco>,
    pub red: Vec<CaudalRed>,
}

/* ── Nombres legibles de los chips ────────────────────────────────────────── */

/// Nombres de los chips que se conocen, para no enseñar `k10temp` a secas.
///
/// Solo se traducen los que se han COMPROBADO en esta máquina (leyendo su
/// `name` y sus etiquetas). Un chip desconocido se enseña con su nombre de sysfs:
/// es feo, pero es verdad, y se puede buscar.
fn chip_legible(nombre: &str) -> String {
    let conocido = match nombre {
        "k10temp" => "CPU (k10temp)",
        "nct6683" => "Placa base (nct6683)",
        "nct6798" => "Placa base (nct6798)",
        "amdgpu" => "GPU (amdgpu)",
        "nvme" => "NVMe",
        "iwlwifi_1" => "WiFi (iwlwifi)",
        "zenergy" => "Energía de la CPU (zenergy)",
        "ucsi_source_psy_0_00081" => "USB-C (alimentación)",
        _ => return nombre.to_string(),
    };
    conocido.to_string()
}

/// ¿Este sensor está desconectado?
///
/// El criterio se midió en esta placa: los sensores no conectados del `nct6683`
/// leen EXACTAMENTE 0 (VRM MOS, PCH, CPU Socket, PCIe x1, M2_1), y el chip
/// fantasma `nct6798` publica temperaturas imposibles (-62 °C). Un 0 exacto en
/// una temperatura o un voltaje es "no hay nada ahí"; en un ventilador, no: un
/// ventilador a 0 rpm es un ventilador parado, y eso SÍ es un dato.
fn desconectado(clase: Clase, valor: f64) -> bool {
    match clase {
        // 0 °C exactos no existen en un equipo encendido, y una temperatura
        // negativa en un superIO es un sensor fantasma (se midió -62 °C en el
        // nct6798 de esta placa, que no es un chip real).
        Clase::Temperatura => valor == 0.0 || valor < -10.0,
        // Un voltaje de 0 no es un riel a cero voltios: es un sensor sin nada.
        Clase::Voltaje => valor == 0.0,
        // Una corriente de 0 A exactos en un chip de sensores es un canal sin
        // nada: el `ucsi_source_psy` de este equipo publica `curr1 = 0` con
        // `max = 0`, que no es una medida de nada.
        Clase::Corriente => valor == 0.0,
        // Un ventilador NUNCA se descarta: 0 rpm es un ventilador parado, y eso
        // es un dato (esta placa tiene cuatro canales sin ventilador, y se ven).
        Clase::Ventilador => false,
        // Potencia y energía tampoco: 0 W es "no consume", que es un dato real.
        Clase::Potencia | Clase::Energia => false,
    }
}

/// ¿Este umbral sirve para algo?
///
/// POR QUÉ NO SE ENSEÑA CUALQUIER `_max`: el `nct6683` de esta placa publica como
/// límite el valor de AHORA (`temp1_input = 40000` y `temp1_max = 40000`), y el
/// `amdgpu` no publica `max` pero sí `crit`. Un «40 de 40 °C» no informa: parece
/// que está al límite cuando no lo está. Se queda solo el umbral que está POR
/// ENCIMA de la lectura, que es lo que un límite significa.
fn umbral_util(umbral: Option<f64>, valor: f64) -> Option<f64> {
    umbral.filter(|u| *u > valor)
}

/* ── Lectura de sysfs ─────────────────────────────────────────────────────── */

fn leer_num(p: &Path) -> Option<f64> {
    std::fs::read_to_string(p).ok()?.trim().parse::<f64>().ok()
}

fn leer_texto(p: &Path) -> Option<String> {
    let s = std::fs::read_to_string(p).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn hwmons() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir("/sys/class/hwmon")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("name").is_file())
        .collect();
    // Orden estable por nombre de chip: sin esto, el orden lo decide el sistema de
    // ficheros y las secciones bailan entre lecturas.
    out.sort();
    out
}

/// Lee TODOS los sensores de un chip.
///
/// El orden de los `_input` es el de sysfs (temp1, temp2, …, fan1, …): así la
/// interfaz no reordena nada y el usuario puede comparar con `sensors`.
fn sensores_de_chip(dir: &Path, chip: &str, legible: &str) -> (Vec<Sensor>, usize) {
    let mut items = Vec::new();
    let mut descartados = 0usize;

    let Ok(entradas) = std::fs::read_dir(dir) else {
        return (items, descartados);
    };
    let mut ficheros: Vec<String> = entradas
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with("_input"))
        .collect();
    ficheros.sort();

    for f in ficheros {
        let base = f.trim_end_matches("_input").to_string();
        let Some((clase, unidad, factor)) = Clase::de_nombre(&base) else {
            continue;
        };
        // `power1_average` es el que interesa cuando existe; aquí se recorre
        // cualquier `X_input` y el nombre ya dice cuál es.
        let Some(bruto) = leer_num(&dir.join(&f)) else {
            continue;
        };
        let valor = bruto * factor;
        if desconectado(clase, valor) {
            descartados += 1;
            continue;
        }
        // Los contadores de energía son ACUMULADOS (van por 1,4 millones de
        // julios en este equipo): no son una medida que enseñar, son la fuente de
        // la potencia de CPU que se calcula aparte. Se quedan fuera de la lista.
        if clase == Clase::Energia {
            continue;
        }
        let etiqueta = leer_texto(&dir.join(format!("{base}_label"))).unwrap_or_else(|| base.clone());
        items.push(Sensor {
            chip: chip.to_string(),
            chip_legible: legible.to_string(),
            etiqueta,
            clase,
            valor,
            unidad: unidad.to_string(),
            max: umbral_util(leer_num(&dir.join(format!("{base}_max"))).map(|v| v * factor), valor),
            critico: umbral_util(
                leer_num(&dir.join(format!("{base}_crit"))).map(|v| v * factor),
                valor,
            ),
            fuente: dir.join(&f).to_string_lossy().to_string(),
        });
    }

    (items, descartados)
}

/* ── Potencia de la CPU por contadores de energía ─────────────────────────── */

/// Última lectura del contador de energía: (microjulios, instante).
static ULTIMA_ENERGIA: LazyLock<Mutex<Option<(f64, Instant)>>> = LazyLock::new(|| Mutex::new(None));

/// Busca el contador de energía del PAQUETE de CPU.
///
/// En este equipo (`AMD Ryzen 7 5800X`) el chip `zenergy` publica un contador por
/// núcleo (`Ecore000`…) y uno por zócalo (`Esocket0`), que es el que suma el
/// paquete entero. Se prefiere ese; si no está, se cae al `package-0` de RAPL,
/// que en AMD también existe.
fn contador_energia() -> Option<(PathBuf, String)> {
    for dir in hwmons() {
        let chip = leer_texto(&dir.join("name"))?;
        if chip != "zenergy" {
            continue;
        }
        // Cualquier `energyN_input` cuya etiqueta empiece por `Esocket` sirve, y
        // también `Psocket`. Se recorren en orden y se coge el primero que cuadre.
        let Ok(entradas) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut candidatos: Vec<String> = entradas
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("energy") && n.ends_with("_input"))
            .collect();
        candidatos.sort();
        for f in candidatos {
            let base = f.trim_end_matches("_input");
            let etiqueta = leer_texto(&dir.join(format!("{base}_label"))).unwrap_or_default();
            if etiqueta.starts_with("Esocket") || etiqueta.starts_with("Psocket") {
                return Some((dir.join(&f), etiqueta));
            }
        }
    }
    // RAPL como alternativa (mismo dato, otro sitio).
    let rapl = PathBuf::from("/sys/class/powercap/intel-rapl:0/energy_uj");
    if rapl.is_file() {
        return Some((rapl, "RAPL package-0".to_string()));
    }
    None
}

/// Potencia de la CPU en vatios, por diferencia entre dos lecturas del contador.
///
/// Recibe la ruta del contador ya resuelta (ver `contador_energia`): así `leer()`
/// recorre `/sys/class/hwmon` una sola vez por foto en vez de dos.
///
/// Devuelve `None` la PRIMERA vez (no hay con qué comparar) y también si el
/// contador se ha reiniciado (el valor nuevo es menor que el viejo, que pasa al
/// cruzar el máximo de `max_energy_range_uj`): en los dos casos, inventarse un
/// número sería mentir. Y si el sistema no tiene contador (macOS y Windows), ni
/// se llama: `cpu_potencia_fuente` lo dice para que la interfaz no prometa una
/// lectura que no va a llegar.
fn potencia_cpu_w(ruta: &Path) -> Option<f64> {
    let energia_uj = leer_num(ruta)?;
    let ahora = Instant::now();
    let mut guardia = ULTIMA_ENERGIA.lock();
    let anterior = guardia.replace((energia_uj, ahora));
    let (prev_uj, prev_t) = anterior?;
    let segundos = ahora.duration_since(prev_t).as_secs_f64();
    // Menos de 100 ms entre lecturas da un ruido enorme al dividir; el contador
    // tiene una resolución que no aguanta ese ritmo.
    if segundos < 0.1 {
        return None;
    }
    let delta_uj = energia_uj - prev_uj;
    if delta_uj < 0.0 {
        return None;
    }
    Some((delta_uj / 1_000_000.0) / segundos)
}

/* ── Frecuencia por núcleo ────────────────────────────────────────────────── */

/// La frecuencia la da `plataforma` (sysinfo), que la lee en los tres sistemas.
///
/// Antes esto recorría `/sys/devices/system/cpu/cpuN/cpufreq/scaling_cur_freq`, que
/// solo existe en Linux: en macOS y Windows la tarjeta de CPU se quedaba sin
/// frecuencia y decía «el kernel no publica cpufreq», que era falso: lo que pasaba
/// es que se estaba mirando donde no era.
fn frecuencia_cpu() -> FrecuenciaCpu {
    let Some(f) = crate::plataforma::frecuencia_cpu_mhz() else {
        // Este sistema (o esta máquina virtual) no la publica: se dice, no se
        // rellena con un número.
        return FrecuenciaCpu::default();
    };
    FrecuenciaCpu {
        actual_mhz: Some(f.primera),
        media_mhz: Some(f.media),
        min_mhz: Some(f.min),
        max_mhz: Some(f.max),
        nucleos: f.nucleos,
    }
}

/* ── Temperatura de los discos ────────────────────────────────────────────── */

/// El nombre del dispositivo al que pertenece un `hwmon` de tipo `nvme`.
///
/// Se resuelve por el enlace `device` del hwmon: `/sys/class/hwmon/hwmon0/device`
/// apunta al controlador NVMe, y de ahí se coge el nombre (`nvme0`). Si no se
/// puede resolver, se devuelve el nombre del hwmon: mejor un nombre feo que un
/// dato sin dueño.
/// El driver del kernel que publica este hwmon, del enlace `device`.
///
/// `/sys/class/hwmon/hwmon3/device -> ../../../nct6687.2592` da `nct6687`: el
/// nombre del dispositivo de plataforma, que es el driver que lo ha creado.
fn driver_de_hwmon(dir: &Path) -> Option<String> {
    let real = std::fs::canonicalize(dir.join("device")).ok()?;
    let nombre = real.file_name()?.to_str()?.to_string();
    // El sufijo es el número de dispositivo de plataforma (`nct6687.2592`).
    Some(nombre.split('.').next().unwrap_or(&nombre).to_string())
}

fn dispositivo_de_hwmon(dir: &Path, indice: &str) -> String {
    let enlace = dir.join("device");
    if let Ok(real) = std::fs::canonicalize(&enlace) {
        if let Some(nombre) = real.file_name().and_then(|n| n.to_str()) {
            // El enlace apunta al PCIe del controlador (`0000:01:00.0`), así que el
            // nombre del disco hay que buscarlo entre los `/dev/nvme*`.
            if nombre.contains(':') {
                if let Some(dev) = nvme_por_pci(nombre) {
                    return dev;
                }
            }
            return nombre.to_string();
        }
    }
    format!("hwmon{indice}")
}

/// Qué `/dev/nvmeN` corresponde a una dirección PCIe.
fn nvme_por_pci(pci: &str) -> Option<String> {
    for i in 0..16 {
        let enlace = PathBuf::from(format!("/sys/class/nvme/nvme{i}/device"));
        let Ok(real) = std::fs::canonicalize(&enlace) else { continue };
        if real.to_string_lossy().ends_with(pci) {
            return Some(format!("nvme{i}"));
        }
    }
    None
}

/// Temperaturas de los discos. Los NVMe las publican por `hwmon`; los SATA, solo
/// por SMART, y ahí se prueba `smartctl` SIN sudo (si no deja, se dice que no se
/// pudo en vez de inventarse un número).
fn discos_temperatura() -> Vec<TempDisco> {
    let mut out = Vec::new();
    for dir in hwmons() {
        let Some(chip) = leer_texto(&dir.join("name")) else { continue };
        if chip != "nvme" {
            continue;
        }
        let indice = dir.file_name().and_then(|n| n.to_str()).unwrap_or("hwmon").to_string();
        let nombre = dispositivo_de_hwmon(&dir, indice.trim_start_matches("hwmon"));
        // El sensor `Composite` es la temperatura que publica la especificación
        // NVMe como "la del disco"; los otros (sensor 1, 2) son del controlador.
        let mut puesto = false;
        let mut lecturas: Vec<String> = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("temp") && n.ends_with("_input"))
            .collect();
        lecturas.sort();
        for f in lecturas {
            let base = f.trim_end_matches("_input");
            let etiqueta = leer_texto(&dir.join(format!("{base}_label"))).unwrap_or_default();
            let Some(v) = leer_num(&dir.join(&f)) else { continue };
            if etiqueta == "Composite" && !puesto {
                out.push(TempDisco {
                    nombre: nombre.clone(),
                    temp_c: Some(v * 0.001),
                    fuente: "hwmon (nvme, Composite)".into(),
                });
                puesto = true;
            } else if puesto && etiqueta != "Composite" {
                // Los sensores extra van como filas propias, con su nombre, para
                // no perderlos: en un NVMe de este equipo hay más de uno.
                out.push(TempDisco {
                    nombre: format!("{nombre} · {etiqueta}"),
                    temp_c: Some(v * 0.001),
                    fuente: "hwmon (nvme)".into(),
                });
            }
        }
        if !puesto {
            out.push(TempDisco {
                nombre,
                temp_c: None,
                fuente: "hwmon (nvme): no publica el sensor Composite".into(),
            });
        }
    }
    out
}

/* ── Caudal de disco y de red ─────────────────────────────────────────────── */

/// Convierte los contadores en caudales (bytes por segundo).
///
/// Los contadores y el tiempo entre lecturas los da `plataforma::caudales()`, que
/// es la única capa que refresca discos y red (y por eso puede saber cuánto ha
/// pasado). Antes esto leía `/proc/diskstats` (sectores de 512 B, un formato de
/// Linux) y `/proc/net/dev`, así que fuera de Linux no había caudales.
///
/// `None` la primera vuelta: sin intervalo no hay velocidad que calcular, y un
/// «0 B/s» parecería «no hay tráfico», que es otra cosa.
fn caudales() -> Option<(Vec<CaudalDisco>, Vec<CaudalRed>)> {
    let (segundos, discos, red) = crate::plataforma::caudales();
    if segundos < 0.1 {
        return None;
    }

    let mut out_discos: Vec<CaudalDisco> = discos
        .into_iter()
        .filter(|(nombre, _, _)| !nombre.is_empty())
        .map(|(nombre, leer, escribir)| CaudalDisco {
            nombre,
            leer_b_s: leer as f64 / segundos,
            escribir_b_s: escribir as f64 / segundos,
        })
        .collect();
    out_discos.sort_by(|a, b| {
        let ta = a.leer_b_s + a.escribir_b_s;
        let tb = b.leer_b_s + b.escribir_b_s;
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out_red: Vec<CaudalRed> = red
        .into_iter()
        .filter(|(nombre, _, _)| {
            // El lazo local no es tráfico de red: enseñarlo confunde y no se puede
            // actuar sobre él.
            nombre != "lo"
        })
        .map(|(nombre, rx, tx)| CaudalRed {
            nombre,
            rx_b_s: rx as f64 / segundos,
            tx_b_s: tx as f64 / segundos,
            activa: rx > 0 || tx > 0,
        })
        .collect();
    out_red.sort_by(|a, b| {
        let ta = a.rx_b_s + a.tx_b_s;
        let tb = b.rx_b_s + b.tx_b_s;
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });

    Some((out_discos, out_red))
}

/* ── La lectura completa ──────────────────────────────────────────────────── */

/// Todo lo del equipo que no cuentan `system.rs` ni `gpu.rs`.
///
/// Es una lectura de sysfs (sin privilegios y sin lanzar nada), así que cabe en
/// cada foto. Lo único que necesita dos vueltas es la potencia de la CPU y los
/// caudales, que devuelven `None` la primera vez.
pub fn leer() -> Hardware {
    let mut grupos = Vec::new();
    let mut descartados_total = 0usize;

    for dir in hwmons() {
        let Some(chip) = leer_texto(&dir.join("name")) else { continue };
        // La GPU ya tiene su propia tarjeta (temperaturas, relojes, potencia,
        // ventilador y el reloj de memoria), con más detalle que los cinco
        // sensores sueltos de su `hwmon`: repetirla aquí sería ruido.
        if chip == "amdgpu" {
            continue;
        }
        let legible = chip_legible(&chip);
        let (items, descartados) = sensores_de_chip(&dir, &chip, &legible);
        descartados_total += descartados;
        // Un chip sin ninguna medida se calla: no hay nada que enseñar de él.
        if items.is_empty() && descartados == 0 {
            continue;
        }
        grupos.push(GrupoSensores {
            driver: driver_de_hwmon(&dir),
            chip,
            chip_legible: legible,
            items,
            descartados,
            ventiladores_a_cero: false,
        });
    }

    // Si no se ha encontrado ningún chip, se enseña lo que publica el SISTEMA, en
    // el mismo formato. Cubre dos casos distintos y los dos son reales: macOS y
    // Windows, donde no hay `hwmon` (es una interfaz del kernel de Linux), y un
    // Linux sin acceso a `/sys` (un contenedor). Sin esto, la sección saldría
    // vacía y parecería que el programa no sabe leer la máquina.
    if grupos.is_empty() {
        let mut items = Vec::new();
        let mut descartados = 0usize;
        for (etiqueta, valor, max, critico) in crate::plataforma::temperaturas() {
            if desconectado(Clase::Temperatura, valor) {
                descartados += 1;
                continue;
            }
            items.push(Sensor {
                chip: "sistema".to_string(),
                chip_legible: "Sensores del sistema".to_string(),
                etiqueta,
                clase: Clase::Temperatura,
                valor,
                unidad: "°C".to_string(),
                max: umbral_util(max, valor),
                critico,
                fuente: "sistema (sysinfo)".to_string(),
            });
        }
        descartados_total += descartados;
        if !items.is_empty() {
            grupos.push(GrupoSensores {
                chip: "sistema".to_string(),
                chip_legible: "Sensores del sistema".to_string(),
                driver: None,
                items,
                descartados,
                ventiladores_a_cero: false,
            });
        }
    }

    // ¿Hay algún ventilador girando en el equipo? Sirve para distinguir un chip
    // que no lee esta placa (todos sus ventiladores a 0 cuando otros sí giran) de
    // un equipo sin ventiladores, donde 0 en todos es normal.
    let algun_ventilador_girando = grupos
        .iter()
        .flat_map(|g| g.items.iter())
        .any(|s| s.clase == Clase::Ventilador && s.valor > 0.0);
    for g in grupos.iter_mut() {
        let ventiladores: Vec<&Sensor> = g
            .items
            .iter()
            .filter(|s| s.clase == Clase::Ventilador)
            .collect();
        g.ventiladores_a_cero = !ventiladores.is_empty()
            && ventiladores.iter().all(|s| s.valor == 0.0)
            && algun_ventilador_girando;
    }

    let (discos_caudal, red) = caudales().unwrap_or_default();

    // El contador de energía se busca UNA sola vez por foto: `contador_energia`
    // recorre `/sys/class/hwmon`, y no hay motivo para hacerlo dos veces. Se
    // pregunta antes de construir la estructura porque `potencia_cpu_w` necesita
    // la ruta y `cpu_potencia_fuente` necesita la etiqueta.
    let contador = contador_energia();
    let potencia = contador.as_ref().and_then(|(ruta, _)| potencia_cpu_w(ruta));

    Hardware {
        grupos,
        descartados: descartados_total,
        cpu_potencia_w: potencia,
        cpu_potencia_fuente: contador.map(|(_, etiqueta)| etiqueta),
        cpu_frecuencia: frecuencia_cpu(),
        discos_temp: discos_temperatura(),
        discos_caudal,
        red,
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// La regla que más importa: en esta placa hay cinco temperaturas que leen 0
    /// exactos porque el sensor no está conectado. Un «0 °C» afirmaría que algo
    /// está a cero grados, así que se descartan.
    #[test]
    fn un_cero_no_es_una_temperatura() {
        assert!(desconectado(Clase::Temperatura, 0.0));
        assert!(!desconectado(Clase::Temperatura, 40.0));
        // Y un sensor fantasma con temperatura imposible tampoco: el nct6798 de
        // esta placa publica -62 °C en un temp que no existe.
        assert!(desconectado(Clase::Temperatura, -62.0));
        assert!(!desconectado(Clase::Temperatura, -5.0));
    }

    /// Un ventilador a 0 rpm SÍ es un dato (está parado): no se descarta.
    #[test]
    fn un_ventilador_parado_es_un_dato() {
        assert!(!desconectado(Clase::Ventilador, 0.0));
        assert!(!desconectado(Clase::Ventilador, 1608.0));
    }

    /// Un riel de 0 V no es un riel apagado: es un sensor sin nada enchufado.
    #[test]
    fn un_voltaje_de_cero_no_es_un_riel() {
        assert!(desconectado(Clase::Voltaje, 0.0));
        assert!(!desconectado(Clase::Voltaje, 1.34));
        assert!(!desconectado(Clase::Voltaje, 12.1));
    }

    /// Las unidades de sysfs son milésimas y millonésimas, y confundirlas da
    /// números mil veces más grandes. Se comprueba el factor de cada clase.
    #[test]
    fn las_unidades_se_convierten_como_tocan() {
        let (c, u, f) = Clase::de_nombre("temp1").unwrap();
        assert_eq!((c, u), (Clase::Temperatura, "°C"));
        assert_eq!(44850.0 * f, 44.85);

        let (c, u, f) = Clase::de_nombre("fan1").unwrap();
        assert_eq!((c, u), (Clase::Ventilador, "rpm"));
        assert_eq!(1608.0 * f, 1608.0);

        let (c, u, f) = Clase::de_nombre("in4").unwrap();
        assert_eq!((c, u), (Clase::Voltaje, "V"));
        assert_eq!(1830.0 * f, 1.83);

        let (c, u, f) = Clase::de_nombre("power1").unwrap();
        assert_eq!((c, u), (Clase::Potencia, "W"));
        assert_eq!(32_000_000.0 * f, 32.0);

        // Algo que no es un sensor (por ejemplo `update_interval`) no se cuela.
        assert!(Clase::de_nombre("update_interval").is_none());
    }

    /// En esta máquina, la lectura tiene que traer lo que se comprobó a mano: los
    /// nombres de ventilador de la placa, el voltaje de la DRAM y las temperaturas
    /// de los NVMe. Si algún día no los trae, esta prueba lo dice.
    #[test]
    fn en_esta_maquina_salen_los_sensores_que_se_comprobaron() {
        let h = leer();
        let todas: Vec<&Sensor> = h.grupos.iter().flat_map(|g| g.items.iter()).collect();
        if todas.is_empty() {
            // Un contenedor sin /sys/class/hwmon no es un fallo del código.
            return;
        }
        let etiquetas: Vec<&str> = todas.iter().map(|s| s.etiqueta.as_str()).collect();
        let hay_placa = h.grupos.iter().any(|g| g.chip == "nct6683" || g.chip == "nct6798");
        if hay_placa {
            assert!(
                etiquetas.iter().any(|e| e.contains("CPU Fan")),
                "el nct6683 de esta placa publica «CPU Fan»: {etiquetas:?}"
            );
        }
        let hay_cpu = h.grupos.iter().any(|g| g.chip == "k10temp");
        if hay_cpu {
            assert!(
                etiquetas.iter().any(|e| *e == "Tctl" || *e == "Tccd1"),
                "k10temp publica Tctl y Tccd1: {etiquetas:?}"
            );
        }
        // Y ninguna temperatura de las que se enseñan puede ser un 0: si lo fuera,
        // el filtro de «desconectado» estaría roto.
        for s in todas.iter().filter(|s| s.clase == Clase::Temperatura) {
            assert!(s.valor != 0.0, "{} salió con 0 y no debería", s.etiqueta);
        }
    }

    /// La potencia de la CPU necesita DOS lecturas: la primera no puede devolver
    /// un número. Se comprueba que la primera es `None` y que la segunda (tras un
    /// respiro) da algo razonable, o `None` si la máquina no expone contadores.
    #[test]
    fn la_potencia_de_la_cpu_necesita_dos_lecturas() {
        // Se fuerza un estado limpio: otra prueba puede haber dejado una lectura.
        {
            let mut g = ULTIMA_ENERGIA.lock();
            *g = None;
        }
        // macOS y Windows no tienen contador de energía: allí `leer()` deja
        // `cpu_potencia_fuente` en `None` y la interfaz dice que no se puede medir
        // en vez de prometer una lectura que no va a llegar.
        let Some((ruta, etiqueta)) = contador_energia() else {
            return; // esta máquina no expone contadores
        };
        assert!(
            !etiqueta.is_empty(),
            "el contador tiene que decir de dónde sale (zenergy, RAPL…)"
        );
        let primera = potencia_cpu_w(&ruta);
        assert_eq!(primera, None, "la primera lectura no tiene con qué comparar");
        std::thread::sleep(std::time::Duration::from_millis(250));
        let segunda = potencia_cpu_w(&ruta);
        if let Some(w) = segunda {
            assert!(
                (0.0..500.0).contains(&w),
                "una potencia de CPU de {w} W no es creíble"
            );
        }
    }

    /// Los contadores de caudal también necesitan dos vueltas, y la primera no
    /// puede inventarse una velocidad.
    #[test]
    fn los_caudales_necesitan_dos_lecturas() {
        // El intervalo lo lleva `plataforma`, que es quien refresca los contadores
        // (y por eso el delta no se lo puede comer nadie por el camino). La
        // primera vuelta de esta prueba puede no tener intervalo y devolver `None`;
        // lo que sí se exige es que con intervalo haya caudal y sea creíble.
        let _ = caudales();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let (discos, red) = caudales().expect("con intervalo sí hay delta");
        // En cualquier equipo hay al menos un disco y una interfaz que no sea `lo`.
        assert!(!discos.is_empty(), "tiene que haber algún disco");
        assert!(
            red.iter().all(|i| i.nombre != "lo"),
            "el lazo local no es tráfico de red y no se enseña"
        );
        for d in &discos {
            assert!(d.leer_b_s >= 0.0 && d.escribir_b_s >= 0.0);
        }
    }

    /// La frecuencia por núcleo: se leen todos los que exponga el kernel y la
    /// media tiene que caer entre el mínimo y el máximo.
    #[test]
    fn la_frecuencia_por_nucleo_cuadra() {
        let f = frecuencia_cpu();
        if f.nucleos == 0 {
            return; // sin cpufreq no hay nada que comprobar
        }
        let media = f.media_mhz.unwrap();
        assert!(media >= f.min_mhz.unwrap() && media <= f.max_mhz.unwrap());
        assert!(f.max_mhz.unwrap() > 0.0);
    }
}
