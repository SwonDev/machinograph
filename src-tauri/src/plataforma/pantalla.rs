//! Las pantallas en macOS y Windows.
//!
//! En Linux las lee y las CAMBIA `display.rs` (kscreen-doctor en KDE Wayland,
//! xrandr en X11). Aquí se lee lo que publican los otros dos sistemas, y se dice
//! claro lo que no se puede hacer: **cambiar el modo**. Ninguno de los dos expone
//! una forma soportada de hacerlo desde fuera (macOS no tiene CLI pública para
//! eso; en Windows habría que llamar a `ChangeDisplaySettingsEx` con la API de
//! Win32), así que en vez de fingir un botón que no hace nada, la sección enseña
//! las pantallas y explica que el cambio se hace desde los ajustes del sistema.
//!
//! De dónde sale la lectura:
//!
//! * **macOS**: `system_profiler SPDisplaysDataType -json` (el «Informe del
//!   sistema» de Apple), del que se saca nombre, resolución y refresco.
//! * **Windows**: `Win32_VideoController` por WMI, que publica la resolución y el
//!   refresco actuales de cada adaptador con pantalla.
//!
//! Los parsers son puros y se prueban aquí con salidas con el formato documentado.
//
// En Linux este módulo no se usa (allí manda `display.rs` con kscreen-doctor o
// xrandr), pero SÍ se compila y se prueba: es la única forma de que el código de
// macOS y Windows no se pudra sin que nadie lo mire. El aviso de código muerto se
// silencia a conciencia: el `allow` del módulo cubre Linux, donde no se usa ningún
// parser, y cada parser lleva el suyo para el sistema que no lo usa.
#![cfg_attr(target_os = "linux", allow(dead_code))]

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SalidaBasica {
    pub nombre: String,
    pub ancho: i32,
    pub alto: i32,
    pub hz: f64,
    pub principal: bool,
    pub fuente: String,
}

/// Por qué en este sistema se pueden VER las pantallas pero no CAMBIAR su modo
/// desde Machinograph. Se enseña como motivo del error, no como un fallo del programa.
pub fn motivo_sin_cambio() -> String {
    format!(
        "En {} se ven las pantallas y su modo actual, pero cambiar el modo no se puede hacer desde \
         Machinograph: este sistema no expone una forma soportada de hacerlo fuera de sus ajustes. \
         Cámbialo desde los ajustes de pantalla del sistema.",
        super::nombre_so()
    )
}

pub fn salidas() -> Vec<SalidaBasica> {
    #[cfg(target_os = "macos")]
    {
        let out = crate::proceso::ejecutar(
            "system_profiler",
            &["SPDisplaysDataType".into(), "-json".into()],
            &[],
            std::time::Duration::from_secs(20),
        );
        match out {
            Ok(o) if o.status.success() => parsear_macos(&String::from_utf8_lossy(&o.stdout)),
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
                "Get-CimInstance Win32_VideoController | Select-Object Name,CurrentHorizontalResolution,CurrentVerticalResolution,CurrentRefreshRate | ConvertTo-Json -Compress".into(),
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

/// `"3024 x 1964 @ 120.00Hz"` → `(3024, 1964, 120.0)`. También acepta el formato
/// sin refresco (`"2560 x 1440"`), que aparece cuando el monitor no lo informa.
// Solo lo usa el parser de macOS (aquí se compila y se prueba): en Linux y
// Windows la resolución llega ya separada por sus parsers.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn partir_resolucion(texto: &str) -> Option<(i32, i32, f64)> {
    let t = texto.trim();
    let (res, resto) = match t.split_once('@') {
        Some((r, resto)) => (r.trim(), Some(resto.trim())),
        None => (t, None),
    };
    let (w, h) = res.split_once('x').or_else(|| res.split_once('X'))?;
    let ancho: i32 = w.trim().parse().ok()?;
    let alto: i32 = h.trim().parse().ok()?;
    let hz = resto
        .map(|r| {
            r.trim_end_matches(|c: char| c.is_ascii_alphabetic())
                .trim()
                .parse::<f64>()
                .unwrap_or(0.0)
        })
        .unwrap_or(0.0);
    Some((ancho, alto, hz))
}

/// Parser del JSON de `system_profiler` (puro).
// Solo lo llama `salidas()` en macOS (aquí se compila y se prueba): en Linux y
// Windows la lectura va por otros parsers.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parsear_macos(json: &str) -> Vec<SalidaBasica> {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(gpus) = v.get("SPDisplaysDataType").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in gpus {
        let Some(pantallas) = g.get("spdisplays_ndrvs").and_then(|x| x.as_array()) else {
            continue;
        };
        for p in pantallas {
            let Some(nombre) = p.get("_name").and_then(|x| x.as_str()) else {
                continue;
            };
            let (ancho, alto, hz) = p
                .get("spdisplays_resolution")
                .and_then(|x| x.as_str())
                .and_then(partir_resolucion)
                .unwrap_or((0, 0, 0.0));
            out.push(SalidaBasica {
                nombre: nombre.to_string(),
                ancho,
                alto,
                hz,
                principal: p.get("spdisplays_main").and_then(|x| x.as_str()) == Some("spdisplays_yes"),
                fuente: "system_profiler".to_string(),
            });
        }
    }
    out
}

/// Parser del JSON de `Win32_VideoController` (puro).
///
/// Un adaptador sin pantalla conectada publica `null` en resolución y refresco: en
/// ese caso se enseña (0, 0, 0), que la interfaz pinta como «—» porque `0` no es un
/// modo de pantalla posible.
// Solo lo llama `salidas()` en Windows (aquí se compila y se prueba): en Linux y
// macOS la lectura va por otros parsers.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn parsear_windows(json: &str) -> Vec<SalidaBasica> {
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
        .filter_map(|s| {
            let nombre = s.get("Name").and_then(|x| x.as_str())?.to_string();
            if nombre.to_lowercase().contains("basic display") {
                return None;
            }
            let n = |clave: &str| s.get(clave).and_then(|x| x.as_u64()).unwrap_or(0);
            Some(SalidaBasica {
                nombre,
                ancho: n("CurrentHorizontalResolution") as i32,
                alto: n("CurrentVerticalResolution") as i32,
                hz: n("CurrentRefreshRate") as f64,
                // Windows no dice cuál es "la principal": la primera con pantalla
                // se marca como tal, y se dice de dónde sale.
                principal: n("CurrentHorizontalResolution") > 0,
                fuente: "WMI".to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn parte_resoluciones_de_macos() {
        assert_eq!(partir_resolucion("3024 x 1964 @ 120.00Hz"), Some((3024, 1964, 120.0)));
        assert_eq!(partir_resolucion("2560 x 1440"), Some((2560, 1440, 0.0)));
        assert_eq!(partir_resolucion("basura"), None);
    }

    #[test]
    fn lee_las_pantallas_del_informe_de_macos() {
        let json = r#"{
          "SPDisplaysDataType" : [
            {
              "_name" : "Apple M1 Pro",
              "spdisplays_ndrvs" : [
                {
                  "_name" : "Color LCD",
                  "spdisplays_main" : "spdisplays_yes",
                  "spdisplays_resolution" : "3024 x 1964 @ 120.00Hz"
                },
                {
                  "_name" : "DELL U2720Q",
                  "spdisplays_resolution" : "3840 x 2160 @ 60.00Hz"
                }
              ]
            }
          ]
        }"#;
        let s = parsear_macos(json);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].nombre, "Color LCD");
        assert_eq!((s[0].ancho, s[0].alto), (3024, 1964));
        assert_eq!(s[0].hz, 120.0);
        assert!(s[0].principal);
        // La segunda no está marcada como principal y trae su modo.
        assert!(!s[1].principal);
        assert_eq!((s[1].ancho, s[1].alto, s[1].hz), (3840, 2160, 60.0));
    }

    #[test]
    fn lee_las_pantallas_de_windows() {
        // Con dos adaptadores y uno de ellos sin pantalla (resolución null).
        let json = r#"[{"Name":"Intel UHD Graphics 630","CurrentHorizontalResolution":3840,"CurrentVerticalResolution":2160,"CurrentRefreshRate":60},{"Name":"NVIDIA GeForce RTX 3070","CurrentHorizontalResolution":null,"CurrentVerticalResolution":null,"CurrentRefreshRate":null}]"#;
        let s = parsear_windows(json);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].ancho, s[0].alto, s[0].hz), (3840, 2160, 60.0));
        // El adaptador sin pantalla sale con ceros, que la interfaz enseña como "—".
        assert_eq!((s[1].ancho, s[1].alto, s[1].hz), (0, 0, 0.0));
        assert!(!s[1].principal);
    }

    #[test]
    fn un_json_que_no_es_el_esperado_no_revienta() {
        assert!(parsear_macos("{}").is_empty());
        assert!(parsear_windows("no json").is_empty());
        assert!(parsear_macos(r#"{"SPDisplaysDataType":[{"_name":"x"}]}"#).is_empty());
    }
}
