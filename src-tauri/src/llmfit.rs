//! Integración con **llmfit** (`AlexsJones/llmfit`, MIT, ★37k), la herramienta
//! que perfila el hardware y dice qué modelos te caben.
//!
//! Por qué se integra en vez de reimplementarse: llmfit ya trae el perfil de
//! hardware, un catálogo de cientos de modelos, el encaje por cuantización, una
//! nota por componentes y una proyección de tokens/s. Volver a escribir eso (y
//! mantenerlo) sería duplicar algo que está mejor hecho y que se actualiza solo.
//! Además trae `--json` pensado para que lo consuman otras herramientas.
//!
//! Y con Machinograph se complementan en las dos direcciones:
//!
//! * llmfit **estima** la velocidad a partir del ancho de banda teórico de la GPU
//!   (en esta máquina: 512 GB/s × 0,55 de eficiencia ≈ 281 GB/s). Machinograph lo
//!   **mide** de verdad con `llama-bench`: aquí salieron **330 GB/s efectivos**
//!   (143,13 tok/s con un modelo de 2,30 GB). Por eso la interfaz dice siempre si
//!   el número es *estimado* (llmfit) o *medido* (Machinograph).
//! * Para los modelos que ya están en disco, Machinograph no necesita estimar nada: el
//!   planificador nativo de llama.cpp dice el contexto exacto que cabe.
//!
//! Nada de esto se inventa: si llmfit no está instalado, se dice; si un modelo no
//! trae un dato, se deja vacío.
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Un dato que viene de fuera puede cambiar de forma entre versiones, así que se
/// declara solo lo que Machinograph enseña y todo lo demás es opcional: lo que no esté
/// no rompe la lectura.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Gpu {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub vram_gb: f64,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub unified_memory: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sistema {
    #[serde(default)]
    pub cpu_name: String,
    #[serde(default)]
    pub cpu_cores: i64,
    #[serde(default)]
    pub available_ram_gb: f64,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub gpu_name: String,
    #[serde(default)]
    pub gpu_vram_gb: f64,
    #[serde(default)]
    pub gpu_count: i64,
    #[serde(default)]
    pub gpus: Vec<Gpu>,
}

/// Las cuatro notas que llmfit da a cada modelo.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Componentes {
    #[serde(default)]
    pub quality: f64,
    #[serde(default)]
    pub speed: f64,
    #[serde(default)]
    pub fit: f64,
    #[serde(default)]
    pub context: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recomendacion {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub params_b: f64,
    #[serde(default)]
    pub parameter_count: String,
    #[serde(default)]
    pub use_case: String,
    #[serde(default)]
    pub category: String,
    /// `Perfect`, `Good`, `Marginal`… tal cual lo dice llmfit, sin traducir.
    #[serde(default)]
    pub fit_level: String,
    #[serde(default)]
    pub run_mode: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub best_quant: Option<String>,
    #[serde(default)]
    pub estimated_tps: Option<f64>,
    #[serde(default)]
    pub measured_tps: Option<f64>,
    #[serde(default)]
    pub disk_size_gb: Option<f64>,
    #[serde(default)]
    pub memory_required_gb: Option<f64>,
    #[serde(default)]
    pub utilization_pct: Option<f64>,
    #[serde(default)]
    pub context_length: Option<i64>,
    #[serde(default)]
    pub effective_context_length: Option<i64>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub score_components: Option<Componentes>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub is_moe: bool,
    #[serde(default)]
    pub installed: bool,
    /// `estimated` o `measured`: de dónde sale la velocidad.
    #[serde(default)]
    pub estimate_confidence: Option<String>,
    /// El comando de `llama-bench` que propone llmfit para verificarlo. Es el
    /// mismo tipo de medición que ya hace Machinograph en `perf.rs`.
    #[serde(default)]
    pub verify_command: Option<String>,
    #[serde(default)]
    pub llamacpp_command: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Lo que hace falta para mover un modelo: memoria y núcleos.
///
/// Los números son OPCIONALES a propósito, y no por comodidad: llmfit manda
/// `vram_gb: null` en la vía "solo CPU", y eso significa "no necesita VRAM
/// ninguna", que no es lo mismo que 0 (que sería "necesita cero"). Un
/// `#[serde(default)]` no basta: cubre el campo ausente, no el `null`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recursos {
    #[serde(default)]
    pub vram_gb: Option<f64>,
    #[serde(default)]
    pub ram_gb: Option<f64>,
    #[serde(default)]
    pub cpu_cores: Option<i64>,
}

/// Una forma de ejecutar el modelo: en la GPU, con capas en CPU o solo en CPU.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Via {
    /// `gpu`, `cpu_offload` o `cpu_only`.
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub feasible: bool,
    #[serde(default)]
    pub fit_level: Option<String>,
    /// Tokens/s estimados por llmfit en esta vía.
    #[serde(default)]
    pub estimated_tps: Option<f64>,
    #[serde(default)]
    pub minimum: Option<Recursos>,
    #[serde(default)]
    pub recommended: Option<Recursos>,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Plan de hardware para un modelo a un contexto concreto: cuánta memoria pide y
/// qué se puede esperar. Llmfit avisa en `estimate_notice` de que son
/// estimaciones suyas, no medidas, y eso se enseña tal cual.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plan {
    #[serde(default)]
    pub estimate_notice: Option<String>,
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub context: i64,
    #[serde(default)]
    pub quantization: Option<String>,
    #[serde(default)]
    pub kv_quant: Option<String>,
    #[serde(default)]
    pub disk_size_gb: Option<f64>,
    #[serde(default)]
    pub minimum: Option<Recursos>,
    #[serde(default)]
    pub recommended: Option<Recursos>,
    #[serde(default)]
    pub run_paths: Vec<Via>,
}

/// Un escalón de la escalera de concurrencia: cuántas sesiones aguanta el
/// equipo a ese contexto.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Escalon {
    #[serde(default)]
    pub requested_context: i64,
    #[serde(default)]
    pub effective_context: i64,
    /// Cuánto ocupa la caché KV de UNA sesión a ese contexto.
    #[serde(default)]
    pub per_session_kv_gb: f64,
    /// Cuántas sesiones caben a la vez (0 = no cabe ni una).
    #[serde(default)]
    pub max_sessions: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EstimacionConcurrencia {
    #[serde(default)]
    pub kv_budget_gb: f64,
    #[serde(default)]
    pub kv_quant: Option<String>,
    #[serde(default)]
    pub pool_gb: Option<f64>,
    #[serde(default)]
    pub weights_resident_gb: Option<f64>,
    /// Contexto máximo NATIVO del modelo según llmfit: es el techo real de la
    /// escalera. Se enseña porque explica por qué no se puede subir más.
    #[serde(default)]
    pub native_context: Option<i64>,
    /// Cuantización de los PESOS (los que están residentes). No se mezcla con
    /// `kv_quant`, que es la de la caché: son dos cosas distintas y confundirlas
    /// da cuentas que no cuadran.
    #[serde(default)]
    pub quant: Option<String>,
    /// Memoria por sesión de las capas recurrentes (modelos híbridos). En los
    /// densos viene vacío, y entonces no se enseña nada.
    #[serde(default)]
    pub per_session_recurrent_gb: Option<f64>,
    #[serde(default)]
    pub ladder: Vec<Escalon>,
}

/// Cuántas sesiones simultáneas aguanta el equipo con un modelo, a cada contexto.
///
/// Sale de un cálculo de capacidad de memoria (los pesos se cargan una vez y cada
/// sesión añade su propia caché KV), no de una medida de rendimiento bajo carga:
/// llmfit lo dice así y aquí se repite, para no prometer más de lo que es.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Concurrencia {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub run_mode: Option<String>,
    #[serde(default)]
    pub fit_level: Option<String>,
    #[serde(default)]
    pub max_context_for_target: Option<i64>,
    #[serde(default)]
    pub estimate: Option<EstimacionConcurrencia>,
}

/// Una pasada de la medición.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PasadaBench {
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub prompt_tokens: i64,
    #[serde(default)]
    pub total_ms: f64,
    /// Tokens por segundo de esa pasada.
    #[serde(default)]
    pub tps: f64,
    #[serde(default)]
    pub ttft_ms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResumenBench {
    #[serde(default)]
    pub avg_tps: f64,
    #[serde(default)]
    pub max_tps: f64,
    #[serde(default)]
    pub min_tps: f64,
    #[serde(default)]
    pub num_runs: i64,
    #[serde(default)]
    pub avg_output_tokens: f64,
    #[serde(default)]
    pub avg_total_ms: f64,
    #[serde(default)]
    pub avg_ttft_ms: Option<f64>,
}

/// Medición de llmfit CONTRA UN SERVIDOR EN MARCHA (a diferencia de
/// `llama-bench`, que mide en aislado con sus propios flags). Mide lo que
/// realmente sirve el motor, con su configuración y su proxy por medio.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResultadoBench {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub runs: Vec<PasadaBench>,
    pub summary: ResumenBench,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Bench {
    pub result: ResultadoBench,
}

#[derive(Debug, Clone, Serialize)]
pub struct Estado {
    pub instalado: bool,
    pub binario: Option<String>,
    pub version: Option<String>,
    pub sistema: Option<Sistema>,
}

#[derive(Debug, Deserialize)]
struct EnvSistema {
    system: Sistema,
}

#[derive(Debug, Deserialize)]
struct EnvRecomendaciones {
    system: Sistema,
    models: Vec<Recomendacion>,
}

/* ── Parseo (puro, para poder probarlo con salidas reales) ────────────────── */

pub fn parsear_sistema(json: &str) -> Result<Sistema, String> {
    serde_json::from_str::<EnvSistema>(json)
        .map(|e| e.system)
        .map_err(|e| format!("no se pudo leer la salida de llmfit system: {e}"))
}

pub fn parsear_recomendaciones(json: &str) -> Result<(Sistema, Vec<Recomendacion>), String> {
    let env: EnvRecomendaciones = serde_json::from_str(json)
        .map_err(|e| format!("no se pudo leer la salida de llmfit recommend: {e}"))?;
    Ok((env.system, env.models))
}

/* ── Ejecución ────────────────────────────────────────────────────────────── */

/// Ruta de `llmfit`, buscando también en `~/.local/bin` (que no siempre está en
/// el PATH de una app de escritorio lanzada desde el menú).
pub fn binario() -> Option<String> {
    // Con límite: si `llmfit` se quedara colgado, esto tiene que contestar "no
    // está" en vez de retener a quien pregunta.
    if let Ok(salida) = crate::proceso::ejecutar(
        "llmfit",
        &["--version".to_string()],
        &[],
        Duration::from_secs(5),
    ) {
        if salida.status.success() {
            return Some("llmfit".to_string());
        }
    }
    // Lo que la app se ha instalado ella misma vive en su carpeta de datos
    // (`<datos>/machinograph/bin`), donde NO llega el PATH de una app de escritorio. Se
    // mira antes que el home porque es justo donde la app lo deja, y se comprueba
    // que arranca: un binario a medias no vale como "está instalado".
    if let Some(p) = crate::provision::binario_gestionado("llmfit") {
        if let Ok(s) = crate::proceso::ejecutar(
            &p,
            &["--version".to_string()],
            &[],
            Duration::from_secs(5),
        ) {
            if s.status.success() {
                return Some(p);
            }
        }
    }
    let home = dirs::home_dir()?;
    let p = home.join(".local").join("bin").join("llmfit");
    p.is_file().then(|| p.to_string_lossy().to_string())
}

pub fn version() -> Option<String> {
    let bin = binario()?;
    let salida = crate::proceso::ejecutar(
        &bin,
        &["--version".to_string()],
        &[],
        Duration::from_secs(5),
    )
    .ok()?;
    salida
        .status
        .success()
        .then(|| String::from_utf8_lossy(&salida.stdout).trim().to_string())
}

/// Límite para las preguntas a llmfit.
///
/// `system`, `plan`, `concurrency` y `recommend` tardan menos de un segundo en
/// caliente, pero el arranque en frío puede tardar más: el valor es holgado a
/// propósito, y sobre todo EXISTE. Sin él, un llmfit colgado dejaba el comando del
/// usuario esperando para siempre.
const LIMITE: Duration = Duration::from_secs(30);

async fn ejecutar(args: &[&str]) -> Result<String, String> {
    let Some(bin) = binario() else {
        return Err(
            "llmfit no está instalado. Es una herramienta aparte (MIT, de AlexsJones): https://github.com/AlexsJones/llmfit"
                .into(),
        );
    };
    let futuro = tokio::process::Command::new(bin)
        .args(args)
        // Si se agota el límite se suelta el futuro y `kill_on_drop` termina el
        // proceso: no queda nada corriendo por detrás.
        .kill_on_drop(true)
        .output();
    let salida = match tokio::time::timeout(LIMITE, futuro).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("no se pudo ejecutar llmfit: {e}")),
        Err(_) => {
            return Err(format!(
                "llmfit no respondió en {} s a `{}`; se ha terminado",
                LIMITE.as_secs(),
                args.join(" ")
            ))
        }
    };
    if !salida.status.success() {
        return Err(format!(
            "llmfit falló: {}",
            String::from_utf8_lossy(&salida.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&salida.stdout).to_string())
}

pub async fn estado() -> Estado {
    let bin = binario();
    // El perfil de hardware SÍ se rellena: antes iba siempre a `None`, un campo que
    // la interfaz leía y no tenía nunca nada. Cuesta una llamada a llmfit (ya
    // acotada) y, si falla, se queda en `None` en vez de inventarse el perfil.
    let sistema = if bin.is_some() { sistema().await.ok() } else { None };
    Estado {
        instalado: bin.is_some(),
        version: version(),
        sistema,
        binario: bin,
    }
}

pub fn parsear_plan(json: &str) -> Result<Plan, String> {
    serde_json::from_str(json).map_err(|e| format!("no se pudo leer la salida de llmfit plan: {e}"))
}

/// Plan de hardware para servir un modelo a un contexto dado.
///
/// Ojo con el selector: llmfit exige un nombre que no sea ambiguo (el catálogo
/// tiene decenas de `Qwen3.5-4B`). Si lo es, devuelve un error que dice con
/// cuáles coincide, y eso se le enseña al usuario tal cual (recortado, porque la
/// lista puede ser larguísima).
pub async fn plan(modelo: &str, contexto: i64, quant: Option<&str>) -> Result<Plan, String> {
    let mut args: Vec<String> = vec![
        "plan".into(),
        modelo.to_string(),
        "--context".into(),
        contexto.clamp(512, 1_048_576).to_string(),
        "--json".into(),
    ];
    if let Some(q) = quant.filter(|q| !q.is_empty()) {
        args.push("--quant".into());
        args.push(q.to_string());
    }
    let prestados: Vec<&str> = args.iter().map(String::as_str).collect();
    let salida = ejecutar(&prestados).await.map_err(recortar)?;
    parsear_plan(&salida)
}

/// Los errores de llmfit pueden traer listas enormes (todos los modelos que
/// coinciden con un nombre ambiguo). Se recortan para que se puedan leer.
fn recortar(e: String) -> String {
    if e.chars().count() <= 400 {
        return e;
    }
    let corte: String = e.chars().take(400).collect();
    format!("{corte}…")
}

pub fn parsear_concurrencia(json: &str) -> Result<Concurrencia, String> {
    serde_json::from_str(json)
        .map_err(|e| format!("no se pudo leer la salida de llmfit concurrency: {e}"))
}

pub async fn concurrencia(modelo: &str) -> Result<Concurrencia, String> {
    let salida = ejecutar(&["concurrency", modelo, "--json"])
        .await
        .map_err(recortar)?;
    parsear_concurrencia(&salida)
}

/// Parsea la salida de `llmfit bench --json`.
///
/// Llmfit imprime el JSON y DESPUÉS un par de líneas de aviso ("Results saved
/// locally…"), así que no se puede pasar la salida entera a serde: hay que
/// quedarse con el objeto, desde la primera llave hasta la última.
pub fn parsear_bench(json: &str) -> Result<ResultadoBench, String> {
    let (i, f) = match (json.find('{'), json.rfind('}')) {
        (Some(i), Some(f)) if f > i => (i, f),
        _ => return Err("llmfit bench no devolvió un JSON con resultado".into()),
    };
    let env: Bench = serde_json::from_str(&json[i..=f])
        .map_err(|e| format!("no se pudo leer la salida de llmfit bench: {e}"))?;
    Ok(env.result)
}

pub async fn sistema() -> Result<Sistema, String> {
    parsear_sistema(&ejecutar(&["system", "--json"]).await?)
}

/// Recomendaciones para este hardware.
///
/// Los filtros son los que acepta llmfit: caso de uso, nivel mínimo de encaje y
/// capacidad (`vision`, `tools`…). Se pasan tal cual y, si no vienen, no se añade
/// ninguna bandera.
pub async fn recomendar(
    limite: i64,
    caso_de_uso: Option<&str>,
    encaje_minimo: Option<&str>,
    capacidad: Option<&str>,
    con_comando: bool,
) -> Result<(Sistema, Vec<Recomendacion>), String> {
    let mut args: Vec<String> = vec![
        "recommend".into(),
        "--json".into(),
        "--limit".into(),
        limite.clamp(1, 500).to_string(),
    ];
    if let Some(c) = caso_de_uso.filter(|c| !c.is_empty()) {
        args.push("--use-case".into());
        args.push(c.to_string());
    }
    if let Some(e) = encaje_minimo.filter(|e| !e.is_empty()) {
        args.push("--min-fit".into());
        args.push(e.to_string());
    }
    if let Some(c) = capacidad.filter(|c| !c.is_empty()) {
        args.push("--capability".into());
        args.push(c.to_string());
    }
    if con_comando {
        args.push("--output-llamacpp".into());
    }

    let prestados: Vec<&str> = args.iter().map(String::as_str).collect();
    parsear_recomendaciones(&ejecutar(&prestados).await?)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Salidas REALES de llmfit 1.1.16 en esta máquina, guardadas en `fixtures/`
    /// para que las pruebas no dependan de tener llmfit instalado ni de la red.
    const SISTEMA: &str = include_str!("../fixtures/llmfit-system.json");
    const RECOMENDACIONES: &str = include_str!("../fixtures/llmfit-recommend.json");

    #[test]
    fn lee_el_perfil_de_hardware() {
        let s = parsear_sistema(SISTEMA).unwrap();
        assert!(s.cpu_name.contains("5800X"), "cpu = {}", s.cpu_name);
        assert_eq!(s.cpu_cores, 16);
        assert!(s.available_ram_gb > 1.0);
        assert_eq!(s.backend, "Vulkan");
        assert!((s.gpu_vram_gb - 15.98).abs() < 0.01, "vram = {}", s.gpu_vram_gb);
        assert_eq!(s.gpu_count, 1);
        assert!(!s.gpus.is_empty());
    }

    #[test]
    fn lee_las_recomendaciones_con_sus_notas() {
        let (s, ms) = parsear_recomendaciones(RECOMENDACIONES).unwrap();
        assert!(!ms.is_empty());
        assert!(s.gpu_vram_gb > 15.0);
        let m = &ms[0];
        assert!(!m.name.is_empty());
        assert!(m.params_b > 0.0);
        assert!(!m.use_case.is_empty(), "cada modelo trae su caso de uso");
        assert!(["Perfect", "Good", "Marginal", "Poor"].contains(&m.fit_level.as_str()));
        assert!(m.estimated_tps.unwrap_or(0.0) > 0.0);
        assert!(m.memory_required_gb.unwrap_or(0.0) > 0.0);
        let c = m.score_components.as_ref().expect("trae notas por componente");
        assert!(c.quality > 0.0 && c.speed > 0.0 && c.fit > 0.0 && c.context > 0.0);
        assert!(m.verify_command.is_some(), "propone con qué verificarlo");
    }

    #[test]
    fn distingue_estimado_de_medido() {
        // En esta máquina, todo lo que devuelve llmfit es ESTIMADO (usa el ancho
        // de banda teórico). El dato medido lo pone Machinograph con llama-bench, así
        // que la interfaz tiene que poder distinguirlos.
        let (_, ms) = parsear_recomendaciones(RECOMENDACIONES).unwrap();
        assert_eq!(ms[0].estimate_confidence.as_deref(), Some("estimated"));
        assert!(ms[0].measured_tps.is_none());
    }

    #[test]
    fn aguanta_que_llmfit_cambie_y_quite_campos() {
        // Solo con lo imprescindible: el resto son opcionales a propósito, porque
        // es una herramienta de fuera y su formato puede cambiar.
        let minimo = r#"{"system":{},"models":[{"name":"algo/modelo"}]}"#;
        let (_, ms) = parsear_recomendaciones(minimo).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].name, "algo/modelo");
        assert_eq!(ms[0].params_b, 0.0);
        assert!(ms[0].estimated_tps.is_none());
        assert!(ms[0].score_components.is_none());
    }

    #[test]
    fn un_json_que_no_es_de_llmfit_da_error_claro() {
        let e = parsear_recomendaciones("no soy json").unwrap_err();
        assert!(e.contains("llmfit recommend"), "mensaje: {e}");
        assert!(parsear_sistema("{}").is_err(), "sin 'system' no hay perfil");
    }

    const PLAN: &str = include_str!("../fixtures/llmfit-plan.json");

    #[test]
    fn lee_un_plan_de_hardware() {
        let p = parsear_plan(PLAN).unwrap();
        assert!(!p.model_name.is_empty());
        assert!(p.context > 0);
        assert!(p.disk_size_gb.unwrap_or(0.0) > 0.0);
        // Avisa de que es una estimación suya, no una medida: hay que enseñarlo.
        assert!(p.estimate_notice.is_some());
        // Y trae las tres formas de ejecutarlo con su viabilidad y su velocidad.
        assert_eq!(p.run_paths.len(), 3);
        let vias: Vec<&str> = p.run_paths.iter().map(|v| v.path.as_str()).collect();
        assert!(vias.contains(&"gpu") && vias.contains(&"cpu_only"));
        for v in &p.run_paths {
            assert!(
                v.minimum.as_ref().and_then(|m| m.cpu_cores).is_some(),
                "cada vía dice al menos cuántos núcleos necesita"
            );
        }
        // El caso que obligó a que los números sean opcionales: en "solo CPU" la
        // VRAM viene como `null` (no necesita ninguna), y eso no es un cero.
        let solo_cpu = p.run_paths.iter().find(|v| v.path == "cpu_only").unwrap();
        assert_eq!(solo_cpu.minimum.as_ref().unwrap().vram_gb, None);
        assert!(solo_cpu.minimum.as_ref().unwrap().ram_gb.unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn un_plan_que_no_es_json_da_error_claro() {
        assert!(parsear_plan("vaya").unwrap_err().contains("llmfit plan"));
    }

    #[test]
    fn los_errores_larguisimos_se_recortan() {
        let largo = "Error: ambigua. Coincide con: ".to_string() + &"x/".repeat(500);
        let r = recortar(largo);
        assert!(r.chars().count() <= 401, "no se recortó: {}", r.chars().count());
        assert!(r.ends_with('…'));
        // Y uno corto se deja tal cual.
        assert_eq!(recortar("corto".into()), "corto");
    }

    const CONCURRENCIA: &str = include_str!("../fixtures/llmfit-concurrencia.json");

    #[test]
    fn lee_la_escalera_de_concurrencia() {
        let c = parsear_concurrencia(CONCURRENCIA).unwrap();
        let e = c.estimate.expect("trae la estimación");
        assert!(!e.ladder.is_empty());
        assert!(e.kv_budget_gb > 0.0);
        // La escalera es coherente: a más contexto, menos sesiones.
        let sesiones: Vec<i64> = e.ladder.iter().map(|x| x.max_sessions).collect();
        assert!(
            sesiones.windows(2).all(|w| w[0] >= w[1]),
            "a más contexto no pueden caber más sesiones: {sesiones:?}"
        );
        // Y cuanto más contexto por sesión, más memoria por sesión.
        let kv: Vec<f64> = e.ladder.iter().map(|x| x.per_session_kv_gb).collect();
        assert!(kv.windows(2).all(|w| w[0] <= w[1]), "KV por sesión: {kv:?}");
        // El primer escalón es el que más sesiones aguanta.
        assert!(e.ladder[0].max_sessions >= 1);
        // Los tres datos que la interfaz enseña desde ahora: si alguno dejara de
        // llegar, el panel mostraría "—" sin que nadie se entere.
        assert!(e.native_context.unwrap_or(0) > 0, "llmfit da el contexto nativo");
        assert!(e.quant.is_some(), "llmfit da la cuantización del modelo");
    }

    #[test]
    fn una_concurrencia_ilegible_da_error_claro() {
        assert!(parsear_concurrencia("{}").is_ok(), "sin datos no falla, queda vacío");
        assert!(parsear_concurrencia("vaya").unwrap_err().contains("llmfit concurrency"));
    }

    const BENCH: &str = include_str!("../fixtures/llmfit-bench.json");

    #[test]
    fn lee_una_medicion_de_llmfit() {
        let r = parsear_bench(BENCH).unwrap();
        assert_eq!(r.model, "modelo-8b");
        assert_eq!(r.provider, "llamacpp");
        assert_eq!(r.runs.len(), 2);
        assert!(r.summary.avg_tps > 0.0);
        assert!(r.summary.max_tps >= r.summary.avg_tps);
        assert!(r.summary.min_tps <= r.summary.avg_tps);
        assert_eq!(r.summary.num_runs, 2);
        // El aviso va DETRÁS del JSON, y aun así tiene que parsearse.
        let con_aviso = format!("{BENCH}\n\n  Results saved locally (1 submission(s) pending). Contribuye con `llmfit bench --share`.\n");
        assert!(parsear_bench(&con_aviso).is_ok(), "el aviso posterior rompe el parseo");
        // Y sin JSON, error claro.
        assert!(parsear_bench("no hay json").unwrap_err().contains("llmfit bench"));
    }

    /// Prueba de verdad contra llmfit instalado: ejecuta el binario, lee su JSON
    /// real y comprueba que el resultado tiene sentido. Se salta sola si llmfit no
    /// está (no es un fallo del código, es una herramienta de fuera).
    #[tokio::test]
    async fn habla_con_llmfit_de_verdad() {
        if binario().is_none() {
            return;
        }
        let s = sistema().await.expect("llmfit system tiene que responder");
        assert!(s.gpu_vram_gb > 0.0, "tiene que detectar VRAM");
        assert!(!s.cpu_name.is_empty());

        let (_, ms) = recomendar(3, None, None, None, false)
            .await
            .expect("llmfit recommend tiene que responder");
        assert!(!ms.is_empty(), "con esta GPU tiene que recomendar algo");
        assert!(
            ms.iter().all(|m| !m.name.is_empty()),
            "todos los modelos traen nombre"
        );
        // El filtro por caso de uso tiene que llegar hasta el binario.
        let (_, solo_codigo) = recomendar(3, Some("coding"), None, None, false)
            .await
            .expect("el filtro por caso de uso tiene que funcionar");
        assert!(
            solo_codigo.iter().all(|m| m.use_case.to_lowercase().contains("cod")),
            "el filtro no se aplicó: {:?}",
            solo_codigo.iter().map(|m| &m.use_case).collect::<Vec<_>>()
        );
    }
}
