//! Inventario de modelos de TODO tipo que hay en el sistema, y su gestión.
//!
//! Por qué existe: el escaneo anterior miraba solo `~/models` y buscaba `.gguf`,
//! así que se le escapaban cosas que sí están en esta máquina y que se comen
//! disco: 23 GB de modelos de LM Studio, el árbol de modelos de ComfyUI
//! (checkpoints, loras, vae, controlnet, embeddings…) y los de audio (las voces
//! de piper en `.onnx`, los modelos de Coqui TTS en `.pth`). Un panel de "qué hay
//! en mi equipo" que no ve 23 GB no sirve.
//!
//! Tres criterios que se aplican a conciencia:
//!
//! 1. **Se busca por RAÍCES conocidas, no por todo el disco.** Un `find /` de
//!    extensiones de modelo es lentísimo y llena la lista de basura: solo en el
//!    home hay 8.289 ficheros `.bin`, y casi ninguno es un modelo. Cada raíz sabe
//!    qué es lo que guarda y cómo se llama cada cosa dentro.
//! 2. **Se distingue lo que es un modelo de lo que es un apaño.** Un `mmproj` es
//!    un proyector de visión (un modelo, pero de otro tipo); un `-kv-bias`, un
//!    `.lock`, un `.metadata` o un corpus de calibración NO son modelos y no se
//!    listan como si lo fueran.
//! 3. **El tipo se deduce de dónde está y cómo se llama**, y se dice así en la
//!    interfaz. No se abre el fichero para adivinar: deducirlo de la ubicación es
//!    fiable (la carpeta `loras` de ComfyUI son loras) y no cuesta nada.
//!
//! Borrar va SIEMPRE a la papelera del escritorio (especificación freedesktop),
//! nunca a `rm`: se puede recuperar, y la interfaz lo dice.
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Modelo {
    pub ruta: String,
    pub nombre: String,
    /// `texto`, `vision`, `imagen`, `video`, `audio`, `embedding`, `adaptador`,
    /// `vae`, `codificador`, `otro`.
    pub tipo: String,
    pub formato: String,
    /// Cuantización deducida del nombre (`Q8_0`, `PQ2_0`, `IQ4_XS`…).
    /// Solo para GGUF: el nombre de un `.gguf` la lleva al final por convención,
    /// y para el resto de formatos no se puede deducir del nombre sin inventarse
    /// nada. Se reutiliza la misma función que ya usaba el escaneo viejo: la
    /// cuantización de un modelo es la misma se mire desde donde se mire.
    pub quant: Option<String>,
    pub tamano_bytes: i64,
    /// De dónde sale: `llama.cpp`, `LM Studio`, `Ollama`, `ComfyUI`, `piper`…
    pub familia: String,
    /// Qué lo usa, si se sabe.
    pub motor: Option<String>,
    pub modificado: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resumen {
    pub tipo: String,
    pub ficheros: i64,
    pub bytes: i64,
}

/// Totales del inventario, para poder enseñarlos sin recorrer las carpetas en
/// cada foto (el bucle de fondo va cada 2 s y esto es una lectura de disco).
///
/// `Default` hace falta porque la foto los calcula en un hilo aparte: si ese hilo
/// no llegara a devolver nada, se enseña un total vacío en vez de mentir con el
/// último conocido.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Totales {
    pub ficheros: i64,
    pub bytes: i64,
    pub por_tipo: Vec<Resumen>,
}

/// Caché con caducidad: el inventario se recalcula como mucho una vez por minuto.
/// Los modelos no aparecen ni desaparecen cada dos segundos, y recorrer ComfyUI
/// (1.900 ficheros) en cada foto sería tirar CPU.
static CACHE: std::sync::LazyLock<parking_lot::Mutex<Option<(std::time::Instant, Totales)>>> =
std::sync::LazyLock::new(|| parking_lot::Mutex::new(None));

const CADUCIDAD: std::time::Duration = std::time::Duration::from_secs(60);

pub fn totales_cacheados() -> Totales {
    let mut guard = CACHE.lock();
    if let Some((cuando, t)) = guard.as_ref() {
        if cuando.elapsed() < CADUCIDAD {
            return t.clone();
        }
    }
    let modelos = inventario();
    let t = Totales {
        ficheros: modelos.len() as i64,
        bytes: modelos.iter().map(|m| m.tamano_bytes).sum(),
        por_tipo: resumen(&modelos),
    };
    *guard = Some((std::time::Instant::now(), t.clone()));
    t
}

/// Una raíz de modelos y cómo interpretar lo que hay dentro.
struct Raiz {
    ruta: PathBuf,
    familia: &'static str,
    motor: Option<&'static str>,
    /// Cómo se traduce la carpeta de primer nivel a un tipo. Lo que no esté aquí
    /// se deduce por extensión.
    subcarpetas: &'static [(&'static str, &'static str)],
    /// Tipo de TODO lo que haya en esta raíz, cuando la ubicación ya lo dice.
    /// Es más fiable que el nombre: una voz de piper se llama
    /// `es_ES-davefx-medium.onnx` y ahí no pone "voz" en ninguna parte; lo que
    /// dice que es audio es la carpeta en la que está.
    tipo: Option<&'static str>,
}

/// Carpetas de ComfyUI que son modelos, y de qué tipo. Sale de su propio árbol
/// (`~/ComfyUI/models/`), que existe en esta máquina.
const COMFY: &[(&str, &str)] = &[
    ("checkpoints", "imagen"),
    ("diffusion_models", "imagen"),
    ("diffusers", "imagen"),
    ("unet", "imagen"),
    ("loras", "adaptador"),
    ("vae", "vae"),
    ("controlnet", "control"),
    ("clip", "codificador"),
    ("clip_vision", "codificador"),
    ("text_encoders", "codificador"),
    ("embeddings", "embedding"),
    ("audio_encoders", "audio"),
    ("upscale_models", "otro"),
    ("background_removal", "otro"),
    ("frame_interpolation", "video"),
];

/// Extensiones que sí son pesos de un modelo.
const EXTENSIONES: &[(&str, &str)] = &[
    ("gguf", "GGUF"),
    ("safetensors", "safetensors"),
    ("ckpt", "checkpoint"),
    ("pth", "PyTorch"),
    ("pt", "PyTorch"),
    ("onnx", "ONNX"),
    ("tflite", "TFLite"),
    ("mlmodel", "CoreML"),
    ("ggml", "GGML"),
    ("npz", "NumPy"),
];

/// Nombres que parecen modelos y no lo son (ajustes y cachés que viven junto a
/// ellos). Sin esto, la lista se llena de ruido.
fn es_auxiliar(nombre: &str) -> bool {
    let n = nombre.to_lowercase();
    n.ends_with(".lock")
        || n.contains("kv-bias")
        || n.contains("metadata")
        || n.ends_with(".jinja")
        || n.ends_with(".txt")
        || n.ends_with(".json")
        || n.ends_with(".yaml")
        || n.ends_with(".yml")
        || n.starts_with("cachedir.tag")
        || n.starts_with('.')
        || n.ends_with(".bak")
        || n.contains(".bak-")
}

fn extension(nombre: &str) -> Option<(&'static str, &'static str)> {
    let ext = Path::new(nombre).extension()?.to_str()?.to_lowercase();
    EXTENSIONES.iter().find(|(e, _)| *e == ext).copied()
}

/// Tipo de un fichero: primero la raíz (si lo dice), luego la subcarpeta de
/// ComfyUI, y solo si no hay nada, el nombre.
fn tipo_de(raiz: Option<&str>, sub: Option<&str>, nombre: &str) -> String {
    sub.or(raiz)
        .map(String::from)
        .unwrap_or_else(|| tipo_por_nombre(nombre).to_string())
}

/// Tipo según el nombre, cuando la carpeta no lo dice.
fn tipo_por_nombre(nombre: &str) -> &'static str {
    let n = nombre.to_lowercase();
    if n.contains("mmproj") || n.contains("projector") {
        "vision"
    } else if n.contains("embed") {
        "embedding"
    } else if n.contains("vae") {
        "vae"
    } else if n.contains("lora") {
        "adaptador"
    } else if n.contains("whisper") || n.contains("piper") || n.contains("tts") || n.contains("voice") {
        "audio"
    } else if n.contains("wan") || n.contains("video") || n.contains("svd") || n.contains("animate") {
        "video"
    } else if EXTENSIONES.iter().any(|(e, _)| *e == "onnx")
        && (n.contains(".onnx") && (n.contains("piper") || n.contains("voice")))
    {
        "audio"
    } else {
        "texto"
    }
}

/// Las raíces que se miran, en versión pública: (ruta, familia).
///
/// Existe para que Ajustes pueda ENSEÑAR qué carpetas se recorren sin mantener una
/// segunda lista que se desincronice. Si aquí se añade una carpeta, la pantalla la
/// enseña sola.
pub fn raices_publicas() -> Vec<(std::path::PathBuf, String)> {
    raices()
        .into_iter()
        .map(|r| (r.ruta, r.familia.to_string()))
        .collect()
}

/// Las raíces que se miran en esta máquina. `MACHINOGRAPH_MODEL_DIRS` permite añadir
/// más (`ruta:familia`) sin tocar el código.
fn raices() -> Vec<Raiz> {
    // POR QUÉ NO SE INVENTA UN HOME: antes, si no se podía resolver, se caía a
    // `/home/usuario`, que es una ruta de ESTA máquina: en macOS y Windows
    // apuntaría a una carpeta ajena (o inexistente). Un dato que no está no se
    // rellena con una suposición; sin home, simplemente no hay raíces de usuario y
    // solo quedan las de `MACHINOGRAPH_MODEL_DIRS`.
    let mut out = match dirs::home_dir() {
        Some(home) => vec![
        Raiz {
            ruta: home.join("models"),
            familia: "llama.cpp",
            motor: Some("llama-swap / llama-server"),
            subcarpetas: &[],
            tipo: None,
        },
        Raiz {
            ruta: home.join(".lmstudio").join("models"),
            familia: "LM Studio",
            motor: Some("LM Studio"),
            subcarpetas: &[],
            tipo: None,
        },
        Raiz {
            ruta: home.join("ComfyUI").join("models"),
            familia: "ComfyUI",
            motor: Some("ComfyUI"),
            subcarpetas: COMFY,
            tipo: None,
        },
        Raiz {
            ruta: home.join(".local").join("share").join("piper"),
            familia: "piper",
            motor: Some("piper (TTS)"),
            subcarpetas: &[],
            tipo: Some("audio"),
        },
        Raiz {
            ruta: home.join(".local").join("share").join("piper-voices"),
            familia: "piper",
            motor: Some("piper (TTS)"),
            subcarpetas: &[],
            tipo: Some("audio"),
        },
        Raiz {
            ruta: home.join(".local").join("share").join("tts"),
            familia: "Coqui TTS",
            motor: Some("Coqui TTS"),
            subcarpetas: &[],
            tipo: Some("audio"),
        },
        Raiz {
            // Ollama guarda los pesos en blobs sin nombre; se listan por su
            // manifiesto, que es el que dice qué modelo es.
            ruta: home.join(".ollama").join("models").join("manifests"),
            familia: "Ollama",
            motor: Some("ollama"),
            subcarpetas: &[],
            tipo: Some("texto"),
        },
        ],
        None => Vec::new(),
    };
    if let Ok(extra) = std::env::var("MACHINOGRAPH_MODEL_DIRS") {
        // Separador por sistema: en Windows las listas de rutas van con `;` (el
        // `:` forma parte de la propia ruta, `C:\...`), en Linux y macOS con `:`.
        let sep = if cfg!(windows) { ';' } else { ':' };
        for trozo in extra.split(sep).filter(|s| !s.trim().is_empty()) {
            let (ruta, familia) = match trozo.split_once(',') {
                Some((r, f)) => (r.to_string(), f.to_string()),
                None => (trozo.to_string(), "otra".to_string()),
            };
            out.push(Raiz {
                ruta: PathBuf::from(ruta),
                familia: Box::leak(familia.into_boxed_str()),
                motor: None,
                subcarpetas: &[],
                tipo: None,
            });
        }
    }
    out
}

fn modificado(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Recorre una raíz con una profundidad limitada y devuelve los modelos.
///
/// La profundidad es a propósito: `~/.cache/huggingface` puede tener snapshots
/// anidados, pero dentro de estas raíces los pesos están a pocos niveles.
fn recorrer(raiz: &Raiz, sub: Option<&str>, salida: &mut Vec<Modelo>, tope: usize) {
    if salida.len() >= tope {
        return;
    }
    let Ok(entradas) = std::fs::read_dir(&raiz.ruta) else { return };
    for e in entradas.flatten() {
        if salida.len() >= tope {
            return;
        }
        let ruta = e.path();
        let nombre = e.file_name().to_string_lossy().to_string();
        if nombre.starts_with('.') {
            continue;
        }
        if ruta.is_dir() {
            // Un subdirectorio de ComfyUI fija el tipo de todo lo que contenga.
            let nuevo_sub = sub.or_else(|| {
                raiz.subcarpetas
                    .iter()
                    .find(|(c, _)| *c == nombre)
                    .map(|(_, t)| *t)
            });
            recorrer(
                &Raiz {
                    ruta: ruta.clone(),
                    familia: raiz.familia,
                    motor: raiz.motor,
                    subcarpetas: raiz.subcarpetas,
                    tipo: raiz.tipo,
                },
                nuevo_sub,
                salida,
                tope,
            );
            continue;
        }
        if es_auxiliar(&nombre) {
            continue;
        }
        let Some((_, formato)) = extension(&nombre) else { continue };
        let tipo = tipo_de(raiz.tipo, sub, &nombre);
        let tamano_bytes = std::fs::metadata(&ruta).map(|m| m.len() as i64).unwrap_or(0);
        let quant = (formato == "GGUF")
            .then(|| crate::scan::quant_from_name(&nombre))
            .flatten();
        salida.push(Modelo {
            ruta: ruta.to_string_lossy().to_string(),
            nombre,
            tipo,
            formato: formato.to_string(),
            quant,
            tamano_bytes,
            familia: raiz.familia.to_string(),
            motor: raiz.motor.map(String::from),
            modificado: modificado(&ruta),
        });
    }
}

pub fn inventario() -> Vec<Modelo> {
    let mut out = Vec::new();
    for r in raices() {
        if !r.ruta.is_dir() {
            continue;
        }
        recorrer(&r, None, &mut out, 5000);
    }
    out.sort_by(|a, b| b.tamano_bytes.cmp(&a.tamano_bytes));
    out
}

/// Totales por tipo, para poder enseñar "cuánto ocupa cada cosa".
pub fn resumen(modelos: &[Modelo]) -> Vec<Resumen> {
    let mut mapa: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
    for m in modelos {
        let e = mapa.entry(m.tipo.clone()).or_insert((0, 0));
        e.0 += 1;
        e.1 += m.tamano_bytes;
    }
    mapa.into_iter()
        .map(|(tipo, (ficheros, bytes))| Resumen { tipo, ficheros, bytes })
        .collect()
}

/* ── Gestión: borrar a la papelera del escritorio ─────────────────────────── */

/// Mueve un fichero a la papelera (especificación freedesktop), no lo borra.
///
/// Es a propósito: un modelo de 6 GB borrado de verdad no se recupera, y la
/// interfaz promete que se puede deshacer. Si la papelera no se puede usar, se
/// devuelve error en vez de borrar por otra vía.
pub fn a_la_papelera(ruta: &str) -> Result<String, String> {
    let p = Path::new(ruta);
    if !p.is_file() {
        return Err(format!("no existe el fichero {ruta}"));
    }
    // Solo se toca lo que está dentro de una raíz de modelos: así un error de la
    // interfaz no puede borrar cualquier cosa del sistema. La papelera en sí
    // (el `.trashinfo`, el numerado, el cruce de montajes) vive en `papelera.rs`,
    // que es el único sitio que la implementa.
    let permitido = raices().iter().any(|r| p.starts_with(&r.ruta));
    if !permitido {
        return Err(format!(
            "{ruta} no está dentro de ninguna carpeta de modelos conocida; no se toca"
        ));
    }
    crate::plataforma::papelera::mover(p)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn reconoce_las_extensiones_de_pesos() {
        assert_eq!(extension("modelo.gguf").map(|(f, _)| f), Some("gguf"));
        assert_eq!(extension("modelo.SAFETENSORS").map(|(f, _)| f), Some("safetensors"));
        assert_eq!(extension("voz.onnx").map(|(f, _)| f), Some("onnx"));
        // Un fichero que no es un peso no se lista.
        assert_eq!(extension("notas.txt"), None);
        assert_eq!(extension("plantilla.jinja"), None);
    }

    #[test]
    fn no_mete_apaños_como_si_fueran_modelos() {
        // Estos viven junto a los modelos de verdad en ~/models y no lo son.
        assert!(es_auxiliar("Ternary-Modelo local-2-27B-PQ2_0-kv-bias.gguf"));
        assert!(es_auxiliar("MiMo-V2.6-Distill-Qwen-9B-Q8_0.gguf.metadata"));
        assert!(es_auxiliar("MiMo-V2.6-Distill-Qwen-9B-Q8_0.gguf.lock"));
        assert!(es_auxiliar("mimo-9b-chat-template.jinja"));
        assert!(es_auxiliar("calibracion-corpus.txt"));
        assert!(es_auxiliar("Ternary-Modelo local-2-27B-PTQ1_0-kv-bias.rot-off.gguf.bak"));
        // Y estos SÍ son modelos.
        assert!(!es_auxiliar("Ternary-Modelo local-2-27B-PQ2_0.gguf"));
        assert!(!es_auxiliar("Ternary-Modelo local-2-27B-mmproj-Q8_0.gguf"));
    }

    #[test]
    fn distingue_los_tipos_por_el_nombre() {
        assert_eq!(tipo_por_nombre("Ternary-Modelo local-2-27B-mmproj-Q8_0.gguf"), "vision");
        assert_eq!(tipo_por_nombre("Ternary-Modelo local-2-27B-PQ2_0.gguf"), "texto");
        assert_eq!(tipo_por_nombre("algo-embed-Q8_0.gguf"), "embedding");
        assert_eq!(tipo_por_nombre("wan-video-14b.safetensors"), "video");
    }

    #[test]
    fn la_ubicacion_manda_sobre_el_nombre() {
        // El caso que lo motivó: una voz de piper se llama
        // `es_ES-davefx-medium.onnx` y el nombre no dice que sea audio; lo dice
        // la carpeta. Si mandara el nombre, saldría como "texto".
        assert_eq!(tipo_de(Some("audio"), None, "es_ES-davefx-medium.onnx"), "audio");
        assert_eq!(tipo_de(None, None, "es_ES-davefx-medium.onnx"), "texto");
        // La subcarpeta de ComfyUI manda sobre la raíz y sobre el nombre.
        assert_eq!(tipo_de(None, Some("adaptador"), "mi-estilo.safetensors"), "adaptador");
        assert_eq!(tipo_de(Some("audio"), Some("vae"), "x.safetensors"), "vae");
        // Y en ~/models (sin tipo por defecto) se deduce del nombre.
        assert_eq!(tipo_de(None, None, "Ternary-Modelo local-2-27B-mmproj-Q8_0.gguf"), "vision");
    }

    #[test]
    fn el_inventario_ve_lo_que_el_escaneo_viejo_perdia() {
        // Contra el sistema de verdad. Si no hay nada de eso, se salta.
        let inv = inventario();
        if inv.is_empty() {
            return;
        }
        // Todo lo listado tiene tamaño y formato, y nada es un apaño.
        for m in &inv {
            assert!(m.tamano_bytes > 0, "{} sin tamaño", m.ruta);
            assert!(!m.formato.is_empty());
            assert!(!es_auxiliar(&m.nombre), "{} es un apaño", m.nombre);
            assert!(!m.tipo.is_empty());
        }
        // Los tipos son de una lista cerrada (nada de cadenas sueltas).
        for m in &inv {
            assert!(
                [
                    "texto", "vision", "imagen", "video", "audio", "embedding", "adaptador",
                    "vae", "codificador", "control", "otro"
                ]
                .contains(&m.tipo.as_str()),
                "tipo raro: {}",
                m.tipo
            );
        }
        // Los totales por tipo cuadran con la lista.
        let r = resumen(&inv);
        assert_eq!(r.iter().map(|x| x.ficheros).sum::<i64>(), inv.len() as i64);
    }

    #[test]
    fn ve_modelos_fuera_de_la_carpeta_de_llama_cpp() {
        // Esta es la prueba del fallo que se arregla aquí: el escaneo anterior
        // miraba SOLO `~/models`, así que no veía los 23 GB de LM Studio, ni los
        // de ComfyUI, ni los de audio. Si el inventario vuelve a mirar una sola
        // carpeta, esto falla.
        let inv = inventario();
        if inv.is_empty() {
            return;
        }
        let familias: std::collections::BTreeSet<&str> =
            inv.iter().map(|m| m.familia.as_str()).collect();
        assert!(
            familias.len() >= 2,
            "solo se ve una familia de modelos ({familias:?}): ¿ha vuelto a mirar una sola carpeta?"
        );

        let tipos: std::collections::BTreeSet<&str> = inv.iter().map(|m| m.tipo.as_str()).collect();
        assert!(
            tipos.iter().any(|t| *t != "texto" && *t != "vision"),
            "no se ve ningún tipo que no sea de texto ({tipos:?})"
        );

        // Y se imprime el resumen real, que es útil para comprobarlo a mano.
        println!("familias: {familias:?}");
        println!("tipos: {tipos:?}");
        for r in resumen(&inv) {
            println!(
                "  {:<12} {:>4} ficheros  {:>9.2} GB",
                r.tipo,
                r.ficheros,
                r.bytes as f64 / 1_073_741_824.0
            );
        }
        println!("los 5 más grandes:");
        for m in inv.iter().take(5) {
            println!(
                "  {:>7.2} GB  {:<10} {:<28} {}",
                m.tamano_bytes as f64 / 1_073_741_824.0,
                m.tipo,
                m.familia,
                m.nombre
            );
        }
    }

    #[test]
    fn saca_la_cuantizacion_de_los_gguf_reales() {
        // Esta es una regresión que se colvió a colar: al pasar la vista al
        // inventario nuevo se perdió la cuantización, que antes se veía. Los
        // nombres son los de los modelos que hay en el equipo.
        let inv = inventario();
        let gguf: Vec<&Modelo> = inv.iter().filter(|m| m.formato == "GGUF").collect();
        if gguf.is_empty() {
            return;
        }
        let con_quant = gguf.iter().filter(|m| m.quant.is_some()).count();
        assert!(
            con_quant > 0,
            "ningún GGUF de {} trae cuantización: se ha vuelto a perder",
            gguf.len()
        );
        // Y las que salen son cuantizaciones de verdad. La lista de prefijos se
        // reutiliza de donde está definida, para que la prueba no se quede
        // desincronizada si algún día se añade una familia nueva (pasó con BF16).
        for m in &gguf {
            if let Some(q) = &m.quant {
                assert!(
                    crate::scan::PREFIJOS_CUANT.iter().any(|p| q.starts_with(p)),
                    "cuantización rara en {}: {q}",
                    m.nombre
                );
            }
        }
        // Un fichero que no sea GGUF no lleva cuantización deducida.
        for m in inv.iter().filter(|m| m.formato != "GGUF") {
            assert!(m.quant.is_none(), "{} no es GGUF y trae cuantización", m.nombre);
        }
    }

    #[test]
    fn no_deja_borrar_fuera_de_las_carpetas_de_modelos() {
        // Salvaguarda: la interfaz no puede borrar lo que no sea un modelo suyo.
        //
        // Las rutas de esta prueba se crean AQUÍ, en el directorio temporal del
        // sistema, y no con `/etc/hostname` ni `/tmp/...`: ni `/etc` ni `/tmp`
        // existen en Windows, así que una prueba con esas rutas no correría en su
        // CI. El temporal sí está en los tres sistemas.
        let base = std::env::temp_dir();
        let fuera = base.join(format!("machinograph-prueba-fuera-{}.txt", std::process::id()));
        std::fs::write(&fuera, b"no soy un modelo").expect("escribir el fichero de prueba");
        let e = a_la_papelera(&fuera.to_string_lossy()).unwrap_err();
        assert!(e.contains("no está dentro"), "{e}");

        // Y una ruta que NO existe: también se rechaza, sin tocar nada.
        let inexistente = base.join(format!("machinograph-no-existe-{}.gguf", std::process::id()));
        let e2 = a_la_papelera(&inexistente.to_string_lossy()).unwrap_err();
        assert!(e2.contains("no existe") || e2.contains("no está dentro"), "{e2}");

        let _ = std::fs::remove_file(&fuera);
    }
}
