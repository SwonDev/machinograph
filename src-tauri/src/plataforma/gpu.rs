//! La tarjeta gráfica en macOS y Windows.
//!
//! En Linux la lee `gpu.rs` de `sysfs` (y de `amd-smi` si está), que da además
//! uso, temperatura, potencia y el reloj de memoria. Eso NO existe en los otros
//! dos sistemas, así que allí se pregunta por las herramientas del propio sistema:
//!
//! * **macOS**: `system_profiler SPDisplaysDataType -json`, que es la que usa el
//!   propio «Informe del sistema» de Apple.
//! * **Windows**: `Get-CimInstance Win32_VideoController` (WMI), convertido a JSON.
//!
//! Lo que sale de ahí es el NOMBRE, la VRAM y el driver. Uso, temperatura y
//! potencia **no se pueden leer sin privilegios** en esos sistemas, así que la
//! ficha se marca como PARCIAL y la interfaz enseña «—» donde no hay dato, en vez
//! de un 0 % que parecería «la tarjeta está parada». Los parsers son puros y se
//! prueban en cualquier sistema; lo que no se puede probar aquí es que esas
//! herramientas existan en la máquina, y eso se dice con "sin datos" si fallan.
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GpuBasica {
    pub nombre: String,
    /// VRAM en MB. `None` si el sistema no la publica.
    pub vram_mb: Option<f64>,
    pub driver: Option<String>,
    /// De dónde sale el dato, para poder comprobarlo.
    pub fuente: String,
}

/// Las GPU del equipo, según su sistema. En Linux devuelve vacío: allí manda
/// `gpu.rs` con `sysfs`, que sabe mucho más.
pub fn gpus() -> Vec<GpuBasica> {
    #[cfg(target_os = "macos")]
    {
        let out = crate::proceso::ejecutar(
            "system_profiler",
            &["SPDisplaysDataType".into(), "-json".into()],
            &[],
            std::time::Duration::from_secs(20),
        );
        match out {
            Ok(o) if o.status.success() => {
                parsear_macos(&String::from_utf8_lossy(&o.stdout))
            }
            _ => Vec::new(),
        }
    }
    #[cfg(target_os = "windows")]
    {
        let out = crate::proceso::ejecutar(
            "powershell",
            &[
                "-NoProfile".into(),
                "-Command".into(),
                "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion | ConvertTo-Json -Compress".into(),
            ],
            &[],
            std::time::Duration::from_secs(20),
        );
        match out {
            Ok(o) if o.status.success() => parsear_windows(&String::from_utf8_lossy(&o.stdout)),
            _ => Vec::new(),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Parser del JSON de `system_profiler` (puro: se prueba aquí con una salida real).
// Se usa solo en macOS (aquí se compila y se prueba): en Linux y Windows no hay
// quien lo llame, así que el aviso de código muerto se silencia A CONCIENCIA.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parsear_macos(json: &str) -> Vec<GpuBasica> {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(lista) = v.get("SPDisplaysDataType").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    lista
        .iter()
        .filter_map(|g| {
            let nombre = g.get("_name").and_then(|x| x.as_str())?.to_string();
            let vram = g
                .get("spdisplays_vram")
                .and_then(|x| x.as_str())
                .and_then(tamano_a_mb);
            let driver = g
                .get("spdisplays_metal")
                .and_then(|x| x.as_str())
                .map(|m| format!("Metal {m}"));
            Some(GpuBasica {
                nombre,
                vram_mb: vram,
                driver,
                fuente: "system_profiler".to_string(),
            })
        })
        .collect()
}

/// «16 GB», «1536 MB», «8 GB» → MB. Es el formato que usa Apple en su informe.
// Se usa solo en macOS (aquí se compila y se prueba): lo llama el parser de
// `system_profiler`; en Linux y Windows no hay quien lo llame.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn tamano_a_mb(texto: &str) -> Option<f64> {
    let t = texto.trim();
    let (numero, unidad) = t.split_once(' ')?;
    let n: f64 = numero.trim().parse().ok()?;
    match unidad.trim().to_uppercase().as_str() {
        "GB" => Some(n * 1024.0),
        "MB" => Some(n),
        "TB" => Some(n * 1024.0 * 1024.0),
        _ => None,
    }
}

/// Parser del JSON de `Get-CimInstance Win32_VideoController` (puro).
///
/// OJO con `AdapterRAM`: es un entero de 32 bits, así que **no puede pasar de
/// 4 GB** en tarjetas mayores. Se devuelve lo que dice Windows y se avisa en la
/// fuente: inventarse los 8 GB «que debería tener» sería peor que decir 4.
// Se usa solo en Windows (aquí se compila y se prueba): en Linux y macOS no hay
// quien lo llame, así que el aviso de código muerto se silencia A CONCIENCIA.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn parsear_windows(json: &str) -> Vec<GpuBasica> {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let lista: Vec<&serde_json::Value> = match &v {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Object(_) => vec![&v],
        _ => return Vec::new(),
    };
    lista
        .iter()
        .filter_map(|g| {
            let nombre = g.get("Name").and_then(|x| x.as_str())?.to_string();
            // Una tarjeta virtual o el adaptador básico de Microsoft no son una GPU
            // del equipo: se descartan para no enseñar ruido.
            if nombre.to_lowercase().contains("basic display") {
                return None;
            }
            let bytes = g.get("AdapterRAM").and_then(|x| x.as_u64()).unwrap_or(0);
            let vram = (bytes > 0).then(|| bytes as f64 / 1024.0 / 1024.0);
            let driver = g.get("DriverVersion").and_then(|x| x.as_str()).map(|s| s.to_string());
            Some(GpuBasica {
                nombre,
                vram_mb: vram,
                driver,
                fuente: if vram.is_some() {
                    "WMI (AdapterRAM: puede estar topado a 4 GB)".to_string()
                } else {
                    "WMI".to_string()
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lee_el_informe_de_macos() {
        // Salida REAL de `system_profiler SPDisplaysDataType -json` (documentada por
        // Apple y reproducida tal cual en equipos M1/M2).
        let json = r#"{
          "SPDisplaysDataType" : [
            {
              "_name" : "Apple M1 Pro",
              "spdisplays_metal" : "3",
              "spdisplays_vendor" : "sppci_vendor_Apple",
              "spdisplays_vram" : "16 GB",
              "sppci_bus" : "spdisplays_builtin"
            }
          ]
        }"#;
        let g = parsear_macos(json);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].nombre, "Apple M1 Pro");
        assert_eq!(g[0].vram_mb, Some(16.0 * 1024.0));
        assert_eq!(g[0].driver.as_deref(), Some("Metal 3"));
        // Un JSON que no es eso no revienta: devuelve una lista vacía.
        assert!(parsear_macos("{}").is_empty());
        assert!(parsear_macos("no soy json").is_empty());
    }

    #[test]
    fn lee_una_y_varias_gpu_de_windows() {
        // Una sola: `ConvertTo-Json` devuelve un OBJETO, no un array.
        let uno = r#"{"Name":"NVIDIA GeForce RTX 3080","AdapterRAM":4294967295,"DriverVersion":"31.0.15.3699"}"#;
        let g = parsear_windows(uno);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].nombre, "NVIDIA GeForce RTX 3080");
        // 4294967295 bytes es el tope del tipo (4 GB): se enseña tal cual, sin
        // redondear hacia arriba «lo que debería tener» la tarjeta.
        let vram = g[0].vram_mb.unwrap();
        assert!((vram - 4096.0).abs() < 1.0, "VRAM leída: {vram}");
        assert!(g[0].fuente.contains("4 GB"), "{}", g[0].fuente);

        // Varias: array.
        let varias = r#"[{"Name":"Intel UHD Graphics 630","AdapterRAM":1073741824},{"Name":"NVIDIA GeForce RTX 4070","AdapterRAM":0}]"#;
        let g2 = parsear_windows(varias);
        assert_eq!(g2.len(), 2);
        assert_eq!(g2[0].vram_mb, Some(1024.0));
        // Sin dato de VRAM: `None` (la interfaz enseña "—"), no un 0.
        assert_eq!(g2[1].vram_mb, None);
    }

    #[test]
    fn no_cuenta_el_adaptador_basico_de_windows() {
        let json = r#"[{"Name":"Microsoft Basic Display Adapter","AdapterRAM":0}]"#;
        assert!(parsear_windows(json).is_empty());
    }

    #[test]
    fn las_unidades_de_tamano_se_leen_como_tocan() {
        assert_eq!(tamano_a_mb("1536 MB"), Some(1536.0));
        assert_eq!(tamano_a_mb("8 GB"), Some(8192.0));
        assert_eq!(tamano_a_mb("1 TB"), Some(1024.0 * 1024.0));
        // Un formato que no se entiende no se inventa.
        assert_eq!(tamano_a_mb("muchísima"), None);
        assert_eq!(tamano_a_mb("16"), None);
    }

    #[test]
    fn en_linux_no_devuelve_nada_por_aqui() {
        // En Linux las GPU las da `gpu.rs` (sysfs/amd-smi), que sabe más.
        #[cfg(target_os = "linux")]
        assert!(gpus().is_empty());
    }
}
