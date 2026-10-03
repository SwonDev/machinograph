//! Encaje y rendimiento de los modelos, con las herramientas NATIVAS de llama.cpp.
//!
//! De dónde sale esto: Magnitude (`magnitudedev/magnitude`, Apache-2.0) hace su
//! evaluación de modelos con el planificador de encaje de llama.cpp
//! (`FitReport`, `FitCalibration`, `bytes_per_second`). Ese mismo planificador ya
//! viene en los llama.cpp que hay instalados en esta máquina, como dos
//! herramientas sueltas, así que no hay que reimplementar nada ni depender de
//! Magnitude:
//!
//!   * `llama-fit-params --fit on` imprime los argumentos ya ajustados a la
//!     memoria libre: `-c 262144 -ngl -1` significa "con este modelo y tu VRAM,
//!     caben todas las capas y un contexto de 262144". Es la respuesta a "¿me
//!     cabe?" ANTES de intentar cargarlo.
//!   * `llama-bench -o json` mide tokens/s REALES de prefill y de generación.
//!
//! Dos cosas que se aprendieron midiendo y que están codificadas aquí:
//!
//! 1. **El resultado del encaje depende de la caché KV.** El mismo 27B da
//!    `-c 137472` con KV en f16 y `-c 262144` con `-ctk q4_0 -ctv q4_0`, que es
//!    como se sirve de verdad. Medir con otros flags da un número que no
//!    corresponde a nada.
//! 2. **No todos los runtimes leen todos los modelos.** Los Modelo local ternarios
//!    (PQ2_0/PTQ1_0) fallan en el llama.cpp oficial con `invalid ggml type 142`;
//!    solo los lee el fork. Por eso se prueban los runtimes y se informa de cuál
//!    sirve para cada modelo, en vez de dar por hecho que hay uno válido.
use std::sync::LazyLock;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::time::Duration;

/// Argumentos con los que se sirve de verdad en esta máquina. Si se midiera con
/// los valores por defecto de llama.cpp, los números no valdrían para comparar.
pub const KV_K: &str = "q4_0";
pub const KV_V: &str = "q4_0";
pub const FLASH_ATTN: &str = "on";

/// Rango de contexto que se le puede pedir al planificador.
///
/// POR QUÉ HAY TOPE: `perf:fit` le pasaba el contexto TAL CUAL a
/// `llama-fit-params`. Con uno absurdo, el planificador intenta dimensionar la
/// caché KV para ese contexto y el núcleo lo mata por falta de memoria. Medido en
/// esta máquina con el mismo comando que lanza el backend:
///
///     -c 999999999  ->  SIGKILL, 26,8 GB de RSS, "Out of memory: Killed process"
///
/// El tope es el MISMO que ya usa `llmfit::plan` para su `--context`
/// (`clamp(512, 1_048_576)`), así que las dos herramientas hablan del mismo rango
/// y ninguna acepta un contexto que no existe. Un valor absurdo pero terminable
/// (2000000) tampoco colará como plan legítimo.
pub const CTX_MIN: i64 = 512;
pub const CTX_MAX: i64 = 1_048_576;

/// Acota un contexto pedido al rango utilizable.
pub fn ctx_acotado(ctx: i64) -> i64 {
    ctx.clamp(CTX_MIN, CTX_MAX)
}

/// Zona segura para el planificador. Tarda ~0,4 s con un modelo pequeño, así que
/// 20 s es holgado incluso con un modelo grande en disco lento, y evita que un
/// cuelgue retenga el hilo del bucle de encajes (que entonces deja de recalcular
/// sin decirlo).
const LIMITE_FIT: Duration = Duration::from_secs(20);

/// Runtimes que ya se ha COMPROBADO que no calculan el contexto (devuelven
/// `-c 0`).
///
/// Es una propiedad del binario, no del modelo: la build vieja de `~/.local/bin`
/// responde así. Se recuerda para no gastar en ella el primer intento de cada
/// modelo, que es lo que pasaba al ordenar los runtimes solo por "el fichero
/// existe".
static SIN_CONTEXTO: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn runtimes_sin_contexto() -> HashSet<String> {
    // `parking_lot` devuelve el guardia directamente (no hay envenenamiento que
    // recuperar), así que el valor se clona tal cual.
    SIN_CONTEXTO.lock().clone()
}

#[derive(Debug, Clone, Serialize)]
pub struct Runtime {
    /// Nombre corto para la interfaz (el del directorio).
    pub nombre: String,
    pub dir: String,
    pub fit: Option<String>,
    pub bench: Option<String>,
}

/// Busca los runtimes de llama.cpp: directorios que tengan `llama-fit-params` o
/// `llama-bench`. No se busca en todo el disco (sería lento): se miran los sitios
/// donde están los de esta máquina y lo que diga `MACHINOGRAPH_LLAMA_DIRS` (separados
/// por `:` en Linux y macOS y por `;` en Windows), para poder añadir otros sin
/// tocar el código.
pub fn runtimes() -> Vec<Runtime> {
    // POR QUÉ NO SE INVENTA UN HOME: antes se caía a `/home/usuario` si no se podía
    // resolver el home, y esa es una ruta de ESTA máquina: en macOS y Windows
    // señalaría a una carpeta ajena (o inexistente). Sin home y sin
    // `MACHINOGRAPH_LLAMA_DIRS` no hay candidatos, y eso es lo honesto.
    let mut candidatos: Vec<PathBuf> = Vec::new();
    // PRIMERO lo que la app se ha instalado ella misma (`<datos>/machinograph/llama/…`):
    // si la app acaba de bajar un llama.cpp, es el que debe usarse, sin depender de
    // que el usuario lo tenga además en otro sitio. Ver `provision.rs`.
    candidatos.extend(crate::provision::dirs_llama());
    if let Some(home) = dirs::home_dir() {
        candidatos.push(home.join(".local/bin"));

        // Cada build del proyecto de Modelo local vive en su propio directorio.
        let builds = home.join("Proyectos").join("modelo-local-local").join("bin");
        if let Ok(entradas) = std::fs::read_dir(&builds) {
            for e in entradas.flatten() {
                if e.path().is_dir() {
                    candidatos.push(e.path());
                }
            }
        }
    }
    if let Ok(extra) = std::env::var("MACHINOGRAPH_LLAMA_DIRS") {
        // Separador por sistema: en Windows las listas de rutas van con `;` (el
        // `:` forma parte de `C:\...`), en Linux y macOS con `:`.
        let sep = if cfg!(windows) { ';' } else { ':' };
        candidatos.extend(
            extra
                .split(sep)
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from),
        );
    }

    let mut out: Vec<Runtime> = Vec::new();
    for dir in candidatos {
        // En Windows los binarios llevan sufijo `.exe` (`llama-bench.exe`); en
        // Linux y macOS no. Sin el sufijo, en Windows no se encontraría ningún
        // runtime y la sección saldría vacía sin decir por qué.
        let sufijo = if cfg!(windows) { ".exe" } else { "" };
        let fit = dir.join(format!("llama-fit-params{sufijo}"));
        let bench = dir.join(format!("llama-bench{sufijo}"));
        let tiene_fit = fit.is_file();
        let tiene_bench = bench.is_file();
        if !tiene_fit && !tiene_bench {
            continue;
        }
        let nombre = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string());
        if out.iter().any(|r| r.dir == dir.to_string_lossy()) {
            continue;
        }
        out.push(Runtime {
            nombre,
            dir: dir.to_string_lossy().to_string(),
            fit: tiene_fit.then(|| fit.to_string_lossy().to_string()),
            bench: tiene_bench.then(|| bench.to_string_lossy().to_string()),
        });
    }
    // Los que se ha comprobado que NO calculan el contexto van al final, y dentro
    // de cada grupo los que tienen `llama-fit-params` primero. Ojo: `fit.is_some()`
    // solo significa "el fichero existe", y el de `~/.local/bin` existe pero
    // devuelve `-c 0`: por eso no basta con ordenar por eso y hay que recordar
    // cuáles sirven de verdad.
    let malos = runtimes_sin_contexto();
    out.sort_by_key(|r| {
        (
            u8::from(malos.contains(&r.nombre)),
            u8::from(r.fit.is_none()),
        )
    });
    out
}

/// ¿Este texto de error significa que el runtime no entiende el modelo?
///
/// Es el caso de los Modelo local ternarios en el llama.cpp oficial: el tipo 142 de
/// ggml solo existe en el fork. Distinguirlo importa, porque no es lo mismo "no
/// cabe" que "este binario no sabe leerlo".
///
/// Las señales son las ESPECÍFICAS y no cualquier subcadena de "no se pudo
/// cargar": antes bastaba con que la salida contuviera `failed to load model`, así
/// que un fichero truncado, un `failed to read tensor info` o quedarse sin RAM se
/// tomaban por "este binario no sabe leer el modelo" y se pasaba al siguiente
/// runtime con un motivo falso. Lo que no esté aquí es un FALLO de verdad, con su
/// motivo.
pub fn error_de_formato(texto: &str) -> bool {
    texto.contains("invalid ggml type") || texto.contains("unknown model architecture")
}

/// Saca el `-c N` y el `-ngl M` de la salida de `llama-fit-params`.
///
/// La salida útil es una sola línea: `-c 262144 -ngl -1`. `-ngl -1` = todas las
/// capas en la GPU.
///
/// El `-ngl` se devuelve como `Option` porque puede NO venir, y ahí está la
/// trampa: `ngl.unwrap_or(0)` convertía "no lo dice" en "0 capas en la GPU", que
/// significa justo lo contrario (todo en CPU). El parser es fiel a lo que lee; la
/// política la decide quien lo usa.
pub fn analizar_fit(salida: &str) -> Option<(i64, Option<i64>)> {
    let mut ctx = None;
    let mut ngl = None;
    let campos: Vec<&str> = salida.split_whitespace().collect();
    let mut i = 0;
    while i < campos.len() {
        match campos[i] {
            "-c" | "--ctx-size" => {
                if let Some(v) = campos.get(i + 1) {
                    ctx = v.parse::<i64>().ok();
                }
            }
            "-ngl" | "--n-gpu-layers" => {
                if let Some(v) = campos.get(i + 1) {
                    ngl = v.parse::<i64>().ok();
                }
            }
            _ => {}
        }
        i += 1;
    }
    Some((ctx?, ngl))
}

/// Una medida de `llama-bench`.
#[derive(Debug, Clone, Serialize)]
pub struct Medida {
    /// "prefill" (procesar el prompt) o "decode" (generar).
    pub tipo: String,
    pub n_prompt: i64,
    pub n_gen: i64,
    pub tok_s: f64,
    pub desviacion: f64,
}

/// Datos de contexto que `llama-bench` informa junto a las medidas.
#[derive(Debug, Clone, Serialize)]
pub struct InfoBench {
    pub modelo: String,
    pub model_type: String,
    pub tamano_bytes: i64,
    pub n_params: i64,
    pub build: String,
    pub gpu: String,
    pub ngl: i64,
}

/// Parsea la salida JSON de `llama-bench`.
///
/// Devuelve una medida por cada configuración probada: con `-p` sale la de
/// prefill (`n_gen == 0`) y con `-n` la de generación (`n_prompt == 0`).
pub fn analizar_bench(json: &str) -> Result<(Vec<Medida>, InfoBench), String> {
    let raiz: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("JSON de llama-bench ilegible: {e}"))?;
    let lista = raiz.as_array().ok_or("se esperaba una lista")?;
    let primero = lista.first().ok_or("llama-bench no devolvió medidas")?;

    let num = |v: &serde_json::Value, k: &str| -> f64 { v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) };
    let entero = |v: &serde_json::Value, k: &str| -> i64 { v.get(k).and_then(|x| x.as_i64()).unwrap_or(0) };
    let texto = |v: &serde_json::Value, k: &str| -> String {
        v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    };

    let info = InfoBench {
        modelo: texto(primero, "model_filename"),
        model_type: texto(primero, "model_type"),
        tamano_bytes: entero(primero, "model_size"),
        n_params: entero(primero, "model_n_params"),
        build: format!(
            "b{} ({})",
            entero(primero, "build_number"),
            texto(primero, "build_commit")
        ),
        gpu: texto(primero, "gpu_info"),
        ngl: entero(primero, "n_gpu_layers"),
    };

    let mut medidas = Vec::new();
    for e in lista {
        let n_prompt = entero(e, "n_prompt");
        let n_gen = entero(e, "n_gen");
        if n_prompt == 0 && n_gen == 0 {
            continue;
        }
        medidas.push(Medida {
            tipo: if n_gen > 0 { "decode" } else { "prefill" }.to_string(),
            n_prompt,
            n_gen,
            tok_s: num(e, "avg_ts"),
            desviacion: num(e, "stddev_ts"),
        });
    }
    if medidas.is_empty() {
        return Err("llama-bench no informó de ninguna medida utilizable".into());
    }
    Ok((medidas, info))
}

/// ¿La ruta del modelo es legible por un llama.cpp (existe y es un .gguf)?
pub fn modelo_valido(ruta: &str) -> Result<(), String> {
    let p = Path::new(ruta);
    if !p.is_file() {
        return Err(format!("no existe el fichero {ruta}"));
    }
    if p.extension().map(|e| e != "gguf").unwrap_or(true) {
        return Err(format!("{ruta} no es un .gguf"));
    }
    Ok(())
}

/// Cómo de bien encaja, según lo que devolvió el planificador.
///
/// Los tres estados no son una suposición: salen de medir. Con el 27B ternario en
/// esta GPU, el planificador devuelve:
///
///   * `-c 262144 -ngl -1` cuando el contexto pedido entra en la VRAM (todas las
///     capas en la GPU);
///   * `-c 524288 -ngl 57` cuando NO entra: **no baja el contexto**, deja el que
///     le pediste y manda capas a la CPU. Por eso no basta con comparar el
///     contexto: hay que mirar también `-ngl`, o se diría "cabe" de algo que va a
///     ir a paso de tortuga;
///   * un contexto MENOR que el pedido cuando no cabe ni repartiendo capas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Encaje {
    /// Todo en la GPU (`-ngl -1`).
    Gpu,
    /// Cabe, pero con parte de las capas en CPU: funcionará, mucho más lento.
    Mixto,
    /// No cabe: el contexto que entra es menor que el pedido.
    NoCabe,
}

pub fn clasificar(ctx_pedido: Option<i64>, ctx_plan: i64, ngl: i64) -> Encaje {
    match ctx_pedido {
        Some(p) if ctx_plan < p => Encaje::NoCabe,
        _ if ngl < 0 => Encaje::Gpu,
        _ => Encaje::Mixto,
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lee_el_encaje_de_la_salida_real() {
        // Salida literal del fork con el 27B ternario y KV q4_0:
        //   I llama_fit_params: printing fitted CLI arguments to stdout...
        //   -c 262144 -ngl -1
        let salida = "I llama_fit_params: printing fitted CLI arguments to stdout...\n-c 262144 -ngl -1\n";
        assert_eq!(analizar_fit(salida), Some((262144, Some(-1))));

        // Y con la caché en f16 el mismo modelo da bastante menos:
        assert_eq!(analizar_fit("-c 137472 -ngl -1"), Some((137472, Some(-1))));

        // Sin línea de argumentos no hay encaje que leer.
        assert_eq!(analizar_fit("E failed to load model"), None);
    }

    #[test]
    fn sin_el_reparto_de_capas_no_se_inventa_uno() {
        // El dato puede faltar. Antes `ngl.unwrap_or(0)` lo convertía en "0 capas
        // en la GPU", que significa todo en CPU: el veredicto salía "Mixto (solo 0
        // capas en la GPU)", un aviso alarmante y falso. Ahora se distingue.
        assert_eq!(analizar_fit("-c 65536"), Some((65536, None)));
        assert_eq!(analizar_fit("-c 65536 -ngl -1"), Some((65536, Some(-1))));
        // Y `-ngl 0` de verdad SÍ es todo en CPU: eso se sigue leyendo tal cual.
        assert_eq!(analizar_fit("-c 4096 -ngl 0"), Some((4096, Some(0))));
    }

    #[test]
    fn el_contexto_que_se_le_pasa_al_planificador_va_acotado() {
        // Este es el valor que provocó el OOM medido (SIGKILL a los 26,8 GB).
        assert_eq!(ctx_acotado(999_999_999), CTX_MAX);
        assert_eq!(ctx_acotado(2_000_000), CTX_MAX);
        assert_eq!(ctx_acotado(CTX_MAX), CTX_MAX);
        // Por abajo, un contexto que no existe tampoco se pasa tal cual.
        assert_eq!(ctx_acotado(0), CTX_MIN);
        assert_eq!(ctx_acotado(-5), CTX_MIN);
        // Y un valor razonable se respeta entero.
        assert_eq!(ctx_acotado(4096), 4096);
        assert_eq!(ctx_acotado(CTX_MAX - 1), CTX_MAX - 1);
        // El tope es el mismo que el de `llmfit::plan`, no uno inventado aquí.
        assert_eq!(ctx_acotado(CTX_MAX + 1), CTX_MAX);
    }

    #[test]
    fn clasifica_los_tres_encajes_reales() {
        // Medido en esta máquina con el 27B ternario (16 GB de VRAM, KV q4_0):
        // entra a 262144 con todo en GPU...
        assert_eq!(clasificar(Some(262144), 262144, -1), Encaje::Gpu);
        // ...y a 524288 NO entra, pero mantiene el contexto y manda capas a CPU:
        // eso es "mixto", no "cabe" a secas.
        assert_eq!(clasificar(Some(524288), 524288, 57), Encaje::Mixto);
        // Y si el planificador devuelve menos contexto del pedido, no cabe.
        assert_eq!(clasificar(Some(262144), 137472, -1), Encaje::NoCabe);
        // Sin contexto pedido no hay nada que comparar: manda el -ngl.
        assert_eq!(clasificar(None, 47616, -1), Encaje::Gpu);
        assert_eq!(clasificar(None, 47616, 20), Encaje::Mixto);
    }

    #[test]
    fn distingue_no_cabe_de_no_se_puede_leer() {
        // Este es el error del llama.cpp OFICIAL con un Modelo local ternario. No es
        // "no cabe": es que ese binario no sabe leer la cuantización.
        let oficial = "E gguf_init_from_reader: tensor 'output.weight' has invalid ggml type 142. should be in [0, 43)";
        assert!(error_de_formato(oficial));
        assert!(error_de_formato("unknown model architecture: 'ternary-modelo-local'"));
        assert!(!error_de_formato("failed to fit params to free device memory"));

        // Y las señales GENÉRICAS ya no valen: un fichero truncado, un problema de
        // lectura o quedarse sin memoria NO son "este binario no sabe leerlo", y
        // tomarlos por eso hacía pasar al siguiente runtime con un motivo falso.
        assert!(!error_de_formato("failed to load model"));
        assert!(!error_de_formato("failed to read tensor info"));
        assert!(!error_de_formato("unsupported"));
        assert!(!error_de_formato("ggml_vulkan: out of memory"));
    }

    #[test]
    fn lee_las_medidas_de_llama_bench() {
        // Recortado de una salida real de llama-bench -o json con `-p 64 -n 16`.
        let json = r#"[
          {"build_number":11146,"build_commit":"7fe450e19","gpu_info":"AMD Radeon RX 6800 XT (RADV NAVI21)",
           "model_filename":"/home/usuario/models/modelos/modelo-8b-Q2_0.gguf",
           "model_type":"qwen3 8B Q2_0","model_size":2304175360,"model_n_params":8188548096,
           "n_gpu_layers":-1,"n_prompt":64,"n_gen":0,"avg_ts":1092.364058,"stddev_ts":0.0},
          {"build_number":11146,"build_commit":"7fe450e19","gpu_info":"AMD Radeon RX 6800 XT (RADV NAVI21)",
           "model_filename":"/home/usuario/models/modelos/modelo-8b-Q2_0.gguf",
           "model_type":"qwen3 8B Q2_0","model_size":2304175360,"model_n_params":8188548096,
           "n_gpu_layers":-1,"n_prompt":0,"n_gen":16,"avg_ts":52.5,"stddev_ts":0.3}
        ]"#;
        let (medidas, info) = analizar_bench(json).unwrap();
        assert_eq!(medidas.len(), 2);
        assert_eq!(medidas[0].tipo, "prefill");
        assert!((medidas[0].tok_s - 1092.364).abs() < 0.01);
        assert_eq!(medidas[1].tipo, "decode");
        assert!((medidas[1].tok_s - 52.5).abs() < 0.01);
        assert_eq!(info.build, "b11146 (7fe450e19)");
        assert_eq!(info.n_params, 8188548096);
        assert!(info.gpu.contains("RX 6800 XT"));
    }

    #[test]
    fn rechaza_json_que_no_sirve() {
        assert!(analizar_bench("no soy json").is_err());
        assert!(analizar_bench("[]").is_err());
    }

    #[test]
    fn no_acepta_modelos_que_no_son_gguf() {
        // Las dos rutas se crean en el directorio temporal del sistema: `/tmp` no
        // existe en Windows, así que una prueba con `/tmp/...` o `/etc/hostname`
        // no correría en su CI. Se comprueban los DOS motivos de rechazo, los
        // mismos que antes: que el fichero no exista y que exista sin ser .gguf.
        let base = std::env::temp_dir();

        let inexistente = base.join(format!("machinograph-no-existe-{}.gguf", std::process::id()));
        assert!(modelo_valido(&inexistente.to_string_lossy()).is_err());

        let otro = base.join(format!("machinograph-no-gguf-{}.txt", std::process::id()));
        std::fs::write(&otro, b"no soy un modelo").expect("escribir el fichero de prueba");
        assert!(modelo_valido(&otro.to_string_lossy()).is_err());
        let _ = std::fs::remove_file(&otro);
    }
}

/* ── Encaje automático (en segundo plano) ─────────────────────────────────── */

/// Resultado de un cálculo de encaje, guardable y enseñable.
#[derive(Debug, Clone, Serialize)]
pub struct Fit {
    pub modelo: String,
    pub runtime: String,
    /// Contexto que entra con este modelo y esta configuración.
    pub ctx_max: i64,
    /// `-ngl`: -1 = todas las capas en la GPU.
    pub ngl: i64,
    /// `Gpu`, `Mixto` o `NoCabe`, según lo pedido (si se pidió algo).
    pub encaje: Encaje,
    pub pedido: Option<i64>,
    pub detalle: String,
    /// Cuándo se calculó: en la interfaz se enseña con su edad, porque la
    /// memoria libre cambia y un dato viejo no es lo mismo que uno de ahora.
    pub ts: i64,
}

enum Intento {
    /// El planificador dio un resultado utilizable.
    Ok(i64, i64),
    /// Este binario no sabe leer el modelo (p. ej. los ternarios en el oficial).
    NoSabeLeer,
    /// Sabe leerlo, pero no calcula el contexto: devuelve `-c 0`.
    ///
    /// Pasa de verdad: en esta máquina hay varias instalaciones de llama.cpp y la
    /// de `~/.local/bin` (una build de mayo) responde `-c 0 -ngl -1`. Un encaje sin
    /// contexto no es un encaje, así que no se acepta como resultado: se prueba el
    /// siguiente runtime en vez de enseñar "hasta 0 de contexto".
    SinContexto,
    /// Da contexto, pero no dice el reparto de capas (`-ngl`).
    ///
    /// Antes esto se convertía en `-ngl 0`, que significa "todo en CPU", así que el
    /// veredicto salía "Mixto (solo 0 capas en la GPU)": un aviso alarmante Y falso.
    /// `-ngl 0` de verdad sí es todo en CPU, así que el mapeo solo vale cuando el
    /// dato existe. Si falta, no se puede determinar y se dice eso.
    SinReparto,
    Fallo(String),
}

fn intentar_fit(bin: &str, modelo: &str, ctx: Option<i64>, kv: bool) -> Intento {
    let mut args: Vec<String> = vec![
        "-m".to_string(),
        modelo.to_string(),
        "--fit".to_string(),
        "on".to_string(),
    ];
    if let Some(c) = ctx {
        // ACOTADO aquí, en el único sitio donde se construye el comando, para que
        // valga también para el encaje automático y para el que pide el usuario.
        args.push("-c".to_string());
        args.push(ctx_acotado(c).to_string());
    }
    if kv {
        args.extend([
            "-ctk".to_string(),
            KV_K.to_string(),
            "-ctv".to_string(),
            KV_V.to_string(),
            "-fa".to_string(),
            FLASH_ATTN.to_string(),
        ]);
    }
    let salida = match crate::proceso::ejecutar(bin, &args, &[], LIMITE_FIT) {
        Ok(s) => s,
        // El mensaje distingue "no se pudo lanzar" de "no responde", y los dos son
        // motivo para pasar al siguiente runtime en vez de morir en silencio.
        Err(e) => return Intento::Fallo(e),
    };
    let out = String::from_utf8_lossy(&salida.stdout);
    let err = String::from_utf8_lossy(&salida.stderr);
    if let Some((c, ngl)) = analizar_fit(&out) {
        if c <= 0 {
            return Intento::SinContexto;
        }
        let Some(n) = ngl else {
            return Intento::SinReparto;
        };
        return Intento::Ok(c, n);
    }
    if error_de_formato(&err) || error_de_formato(&out) {
        return Intento::NoSabeLeer;
    }
    Intento::Fallo(
        err.lines()
            .last()
            .unwrap_or("sin detalle")
            .to_string(),
    )
}

/// Calcula el encaje con el planificador nativo, probando los runtimes hasta dar
/// con uno que SEPA leer el modelo.
///
/// Es bloqueante a propósito (tarda ~0,4 s y no usa la GPU): quien lo llame desde
/// código asíncrono debe hacerlo en `spawn_blocking`.
pub fn fit_de_modelo(
    modelo: &str,
    ctx: Option<i64>,
    kv: bool,
    solo_runtime: Option<&str>,
    runtime_preferido: Option<&str>,
) -> Result<Fit, String> {
    modelo_valido(modelo)?;
    // El contexto se acota AQUÍ, en la entrada, para que el mismo número se use en
    // todo: en el comando que se lanza, en el veredicto y en el aviso ("pides N").
    // Si se acotara solo dentro de `intentar_fit`, el aviso diría "pides
    // 999999999" de algo que se le pidió al planificador como 1048576.
    let ctx = ctx.map(ctx_acotado);
    let todos = runtimes();
    let mut candidatos: Vec<Runtime> = match solo_runtime {
        Some(n) => todos.into_iter().filter(|r| r.nombre == n).collect(),
        None => todos,
    };
    if candidatos.is_empty() {
        return Err("no hay ningún runtime de llama.cpp instalado (mira `perf:tools`)".into());
    }
    // El preferido puede venir de quien llama (Ajustes) o de `MACHINOGRAPH_LLAMA_FIT`
    // (el nombre del directorio del runtime, tal cual lo enseña `perf:tools`): así
    // se puede fijar cuál se usa sin tocar el código.
    let pref = runtime_preferido
        .map(str::to_string)
        .or_else(|| std::env::var("MACHINOGRAPH_LLAMA_FIT").ok())
        .filter(|p| !p.trim().is_empty());

    // El preferido primero y, en general, los que ya se ha visto que NO calculan el
    // contexto al final: si no, cada modelo gasta su primer intento en el runtime
    // inservible (medido: `~/.local/bin` devuelve `-c 0` con los ternarios, mientras
    // el de `vulkan/` devuelve `-c 65536`).
    let malos = runtimes_sin_contexto();
    candidatos.sort_by_key(|r| {
        let es_pref = pref.as_deref().is_some_and(|p| p == r.nombre);
        (u8::from(!es_pref), u8::from(malos.contains(&r.nombre)))
    });

    let mut intentos: Vec<String> = Vec::new();
    for r in candidatos {
        let Some(bin) = r.fit.clone() else { continue };
        match intentar_fit(&bin, modelo, ctx, kv) {
            Intento::Ok(ctx_max, ngl) => {
                let encaje = clasificar(ctx, ctx_max, ngl);
                let detalle = match (encaje, ctx) {
                    (Encaje::NoCabe, Some(p)) => format!(
                        "No cabe: pides {p} y con este modelo entran {ctx_max} (ngl {ngl})."
                    ),
                    (Encaje::Gpu, Some(p)) => format!(
                        "Cabe entero en la GPU: {p} de contexto, todas las capas (tope real {ctx_max})."
                    ),
                    (Encaje::Mixto, _) => format!(
                        "Cabe, pero con solo {ngl} capas en la GPU a {ctx_max} de contexto: el resto irá en CPU y bajará mucho la velocidad."
                    ),
                    (Encaje::Gpu, None) => {
                        format!("Cabe entero en la GPU: hasta {ctx_max} de contexto.")
                    }
                    // No puede darse (sin contexto pedido no hay nada que no
                    // quepa), pero el compilador no lo sabe y hay que cubrirlo.
                    (Encaje::NoCabe, None) => {
                        format!("Con este modelo entran {ctx_max} de contexto (ngl {ngl}).")
                    }
                };
                return Ok(Fit {
                    modelo: modelo.to_string(),
                    runtime: r.nombre,
                    ctx_max,
                    ngl,
                    encaje,
                    pedido: ctx,
                    detalle,
                    ts: ahora(),
                });
            }
            Intento::NoSabeLeer => {
                intentos.push(format!("{}: no sabe leer este modelo", r.nombre));
            }
            Intento::SinContexto => {
                // Se recuerda para no volver a gastar el primer intento en él.
                { let mut g = SIN_CONTEXTO.lock();
                    g.insert(r.nombre.clone());
                }
                intentos.push(format!("{}: no calcula el contexto (devuelve 0)", r.nombre));
            }
            Intento::SinReparto => {
                intentos.push(format!("{}: no dice el reparto de capas (-ngl)", r.nombre));
            }
            Intento::Fallo(e) => return Err(format!("{} no pudo calcular el encaje: {e}", r.nombre)),
        }
    }
    Err(format!(
        "ninguna instalación de llama.cpp pudo calcular el encaje de este modelo ({})",
        intentos.join("; ")
    ))
}

fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Modelos que merece la pena encajar en segundo plano: los `.gguf` que son el
/// modelo principal (los `mmproj` son proyectores de visión y no se sirven solos).
pub fn modelos_encajables() -> Vec<String> {
    crate::inventario::inventario()
        .into_iter()
        .filter(|m| m.tipo == "texto" && m.formato == "GGUF")
        .map(|m| m.ruta)
        .collect()
}

#[cfg(test)]
mod pruebas_fit {
    use super::*;

    #[test]
    fn encuentra_modelos_que_encajar_en_esta_maquina() {
        // Contra el inventario real: si hay .gguf de texto, tienen que salir.
        let m = modelos_encajables();
        if m.is_empty() {
            return;
        }
        assert!(
            m.iter().all(|r| r.ends_with(".gguf")),
            "solo se encajan .gguf: {m:?}"
        );
        // Los proyectores de visión NO se encajan solos.
        assert!(
            !m.iter().any(|r| r.to_lowercase().contains("mmproj")),
            "un mmproj no es un modelo servible"
        );
    }

    #[test]
    fn un_contexto_cero_no_es_un_encaje() {
        // El parser es fiel (lee lo que le dan), pero la POLÍTICA no lo acepta:
        // `-c 0 -ngl -1` es lo que devuelve la instalación vieja de ~/.local/bin.
        assert_eq!(analizar_fit("-c 0 -ngl -1"), Some((0, Some(-1))));
        assert_eq!(analizar_fit("-c 262144 -ngl -1"), Some((262144, Some(-1))));
        // Y en la práctica: el mismo modelo tiene que acabar resolviéndose con un
        // runtime que SÍ calcule el contexto, no con 0.
        let Some(modelo) = modelos_encajables().into_iter().next() else {
            return;
        };
        let f = fit_de_modelo(&modelo, None, true, None, None).unwrap();
        assert!(
            f.ctx_max > 0,
            "se aceptó un encaje sin contexto ({}), runtime {}",
            f.ctx_max,
            f.runtime
        );
    }

    /// Prueba de INTEGRACIÓN: ejecuta el planificador de verdad sobre un modelo
    /// real de este equipo, así que no puede correr en cualquier máquina (ni en el
    /// CI, que no tiene modelos). Se lanza a mano:
    ///
    /// ```bash
    /// cd src-tauri && cargo test -- --ignored encaja_un_modelo_de_verdad
    /// ```
    ///
    /// Lo que comprueba es la cadena entera: elegir el modelo más pequeño, pedirle
    /// el plan a `llama-fit-params` y clasificar el resultado. Por eso NO se fija un
    /// veredicto concreto: depende del modelo que haya.
    #[test]
    #[ignore = "necesita un modelo .gguf real y un llama.cpp que sepa leerlo"]
    fn encaja_un_modelo_de_verdad_y_lo_clasifica() {
        // Prueba de integración real: ejecuta el planificador sobre el modelo más
        // pequeño que haya. Se salta si no hay ninguno.
        let Some(modelo) = modelos_encajables()
            .into_iter()
            .min_by_key(|r| std::fs::metadata(r).map(|m| m.len()).unwrap_or(u64::MAX))
        else {
            return;
        };
        let f = fit_de_modelo(&modelo, Some(4096), true, None, None)
            .expect("el planificador tiene que dar un resultado");
        assert!(f.ctx_max > 0, "contexto: {}", f.ctx_max);
        assert!(!f.runtime.is_empty());
        assert!(!f.detalle.is_empty());
        assert!(f.ts > 0);

        // El veredicto NO se fija a un valor, y esto se aprendió a golpes: el
        // encaje depende de la VRAM LIBRE en ese momento. Con la GPU despejada el
        // planificador devuelve `-ngl -1` y sale `Gpu`; con algo ocupándola
        // (un modelo cargado, un juego…) reparte capas a la CPU y sale `Mixto`.
        // Exigir `Gpu` hacía que esta prueba se pusiera en rojo sola sin que el
        // código tuviera nada mal (pasó de verdad: 14 GB de VRAM ocupados por
        // otra cosa, y la batería entera en rojo por esta línea).
        //
        // Lo que sí es invariante —y es lo que se comprueba— es la COHERENCIA
        // entre el plan que dio el binario y el veredicto que se enseña.
        match f.encaje {
            Encaje::Gpu => assert!(
                f.ngl < 0,
                "dice «cabe entero en la GPU» y repartió capas (ngl {}): {}",
                f.ngl,
                f.detalle
            ),
            Encaje::Mixto => assert!(
                f.ngl >= 0,
                "dice «con capas en la CPU» y no repartió nada (ngl {}): {}",
                f.ngl,
                f.detalle
            ),
            Encaje::NoCabe => assert!(
                f.pedido.is_some_and(|p| f.ctx_max < p),
                "dice que no cabe y dio contexto de sobra: {}",
                f.detalle
            ),
        }
    }
}
