use std::collections::HashMap;
use parking_lot::Mutex;
use std::time::{Duration, Instant};

use std::sync::LazyLock;

use crate::db::ServerRow;
use crate::types::{Server, ServerModel};

/// Un solo cliente HTTP para todo el módulo.
///
/// Antes se creaba un `reqwest::Client` nuevo por petición: cada uno monta su
/// grupo de conexiones y su resolución, y con las 8 filas sembradas eso era trabajo
/// tirado en cada vuelta. Uno solo reutiliza conexiones.
///
/// El tiempo de espera es CORTO a propósito: aquí solo se pregunta "¿estás y qué
/// sirves?". Con 3 s por ruta y por servidor, un puerto que acepta pero tarda
/// (llama-swap cargando un modelo) dejaba la vuelta en 9-12 s y congelaba el panel
/// y el comando `snapshot:now`.
const TIMEOUT: Duration = Duration::from_millis(400);
static CLIENTE: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_millis(300))
        .build()
        // Si no se pudiera construir (no debería), se usa el de por defecto en vez
        // de dejar el módulo sin cliente.
        .unwrap_or_else(|_| reqwest::Client::new())
});

async fn probe_url(host: &str, port: u16, path: &str) -> Option<String> {
    let url = format!("http://{host}:{port}{path}");
    let resp = CLIENTE.get(&url).send().await.ok()?;
    resp.text().await.ok()
}

fn parse_models(v: &serde_json::Value) -> Option<Vec<ServerModel>> {
    let arr: Vec<serde_json::Value> = if let Some(m) = v.get("models").and_then(|x| x.as_array()) {
        m.to_vec()
    } else if let Some(d) = v.get("data").and_then(|x| x.as_array()) {
        d.to_vec()
    } else if let Some(a) = v.as_array() {
        a.to_vec()
    } else {
        return None;
    };
    Some(
        arr.into_iter()
            .map(|m| {
                let id = m
                    .as_str()
                    .map(String::from)
                    .or_else(|| m.get("id").and_then(|x| x.as_str()).map(String::from))
                    .unwrap_or_default();
                let label = if id.is_empty() {
                    m.get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string()
                } else {
                    id.clone()
                };
                // El estado sale de lo que diga el servidor. llama-swap lo
                // publica como `{"status": {"value": "unloaded"}}` en
                // `/v1/models`, y otros motores lo ponen como texto plano
                // (`{"status": "loaded"}`). Antes se fijaba "loaded" para todo,
                // así que la interfaz distinguía cargado/descargado con un valor
                // que era siempre el mismo.
                let state = m
                    .get("status")
                    .and_then(|s| s.get("value").and_then(|v| v.as_str()).or_else(|| s.as_str()))
                    .unwrap_or("")
                    .to_string();
                ServerModel {
                    id: if id.is_empty() { label.clone() } else { id },
                    label,
                    state,
                    quant: m.get("quant").and_then(|x| x.as_str()).map(String::from),
                    size_mb: m
                        .get("size")
                        .and_then(|x| x.as_i64())
                        .map(|s| (s as f64) / 1_048_576.0),
                }
            })
            .filter(|m| !m.id.is_empty())
            .collect(),
    )
}

/// ¿Responde algo en el puerto del servidor?
///
/// Se prueban rutas genéricas porque cada motor tiene la suya (`/health` en
/// Ollama, `/` en casi todos, `/v1/models` en los compatibles con OpenAI) y aquí
/// solo interesa saber si hay alguien escuchando, no qué contesta.
///
/// Las tres se piden EN PARALELO: en serie, contra un puerto que acepta y no
/// contesta, eran 3 × 400 ms por servidor.
async fn answers(host: &str, port: u16) -> bool {
    let (a, b, c) = tokio::join!(
        probe_url(host, port, "/health"),
        probe_url(host, port, "/"),
        probe_url(host, port, "/v1/models"),
    );
    a.is_some() || b.is_some() || c.is_some()
}

/// Modelos que llama-swap dice que tiene CARGADOS ahora mismo (`GET /running`).
///
/// Esto es más fiable que deducirlo del `status` de `/v1/models`: `/running` es
/// la lista de lo que de verdad está ocupando VRAM, con su estado
/// (`ready` = listo). Además trae el comando REAL con el que se sirve el modelo
/// (con su `--ctx`, sus flags…), que es justo lo que hace falta para comparar
/// "así se está sirviendo" con "así podría servirse según el planificador".
pub async fn llama_swap_cargados(host: &str, port: u16) -> Option<Vec<serde_json::Value>> {
    let texto = probe_url(host, port, "/running").await?;
    let v: serde_json::Value = serde_json::from_str(&texto).ok()?;
    Some(v.get("running")?.as_array()?.clone())
}

async fn detect_models(kind: &str, host: &str, port: u16) -> Vec<ServerModel> {
    let paths: Vec<String> = match kind {
        // `/api/tags` es el listado real de Ollama (`/api/models` no existe).
        "ollama" => vec!["/api/tags".to_string(), "/api/models".to_string()],
        "vllm" | "llama-cpp" => vec!["/v1/models".to_string()],
        "tgwebui" => vec!["/api/v1/models".to_string()],
        _ => vec![
            "/models".to_string(),
            "/api/models".to_string(),
            "/v1/models".to_string(),
        ],
    };
    for p in paths {
        if let Some(v) = try_json(host, port, &p).await {
            if let Some(m) = parse_models(&v) {
                if !m.is_empty() {
                    return m;
                }
            }
        }
    }
    Vec::new()
}

async fn try_json(host: &str, port: u16, path: &str) -> Option<serde_json::Value> {
    let text = probe_url(host, port, path).await?;
    serde_json::from_str(&text).ok()
}

/// Versión del motor, si su API la publica en claro.
///
/// Solo se rellena donde el endpoint se ha COMPROBADO en esta máquina: llama-swap
/// publica `GET /api/version` y responde `{"build_date":…,"commit":…,"version":"v256"}`
/// (comprobado contra el llama-swap que corre en 127.0.0.1:8080). Para el resto de
/// motores no se inventa una ruta: se deja `None`, que es lo que la interfaz ya
/// sabe tratar (no pinta la versión).
async fn version_de(kind: &str, host: &str, port: u16) -> Option<String> {
    match kind {
        "llama-swap" => {
            let texto = probe_url(host, port, "/api/version").await?;
            let v: serde_json::Value = serde_json::from_str(&texto).ok()?;
            v.get("version").and_then(|x| x.as_str()).map(String::from)
        }
        _ => None,
    }
}

/* ── Caché del estado por servidor ────────────────────────────────────────── */

/// Lo que cuesta obtener de un servidor: sus modelos y su versión.
type Estado = (Vec<ServerModel>, Option<String>);

/// Estado cacheado de un servidor parado.
///
/// POR QUÉ: un servidor que no está levantado no cambia solo, y aun así se le
/// preguntaba cada 2 s. Con las 8 filas sembradas (7 de los 8 motores sin instalar)
/// eso era medio centenar de peticiones por vuelta para recibir siempre lo mismo.
/// Con el proceso EN MARCHA no se cachea nada: ahí el estado cambia de verdad.
static CACHE: LazyLock<Mutex<HashMap<String, (Instant, Estado)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const TTL_CACHE: Duration = Duration::from_secs(30);

fn clave(id: &str, kind: &str, port: u16) -> String {
    format!("{id}|{kind}|{port}")
}

fn leer_cache(id: &str, kind: &str, port: u16) -> Option<Estado> {
    let g = CACHE.lock();
    let (cuando, e) = g.get(&clave(id, kind, port))?;
    (cuando.elapsed() < TTL_CACHE).then(|| e.clone())
}

fn guardar_cache(id: &str, kind: &str, port: u16, e: Estado) {
    { let mut g = CACHE.lock();
        g.insert(clave(id, kind, port), (Instant::now(), e));
    }
}

/// Tira la caché entera. Se llama al arrancar o parar un servidor: si no, el
/// estado de antes de la acción seguiría enseñándose hasta 30 s después.
pub fn invalidar_cache() {
    { let mut g = CACHE.lock();
        g.clear();
    }
}

/* ── Foto de un servidor ──────────────────────────────────────────────────── */

async fn construir(row: ServerRow, proceso: Option<Vec<(i32, i64)>>) -> Server {
    let host = "127.0.0.1";
    let pids = proceso.unwrap_or_default();
    let process_active = !pids.is_empty();
    let pid = pids.first().map(|(pid, _)| *pid);
    let proc_uptime_secs = pids.first().map(|(_, secs)| *secs);

    let cacheado = if process_active {
        None
    } else {
        leer_cache(&row.id, &row.kind, row.port)
    };
    let (mut models, version) = match cacheado {
        Some(e) => e,
        None => {
            let models = detect_models(&row.kind, host, row.port).await;
            let version = version_de(&row.kind, host, row.port).await;
            guardar_cache(&row.id, &row.kind, row.port, (models.clone(), version.clone()));
            (models, version)
        }
    };

    // Para llama-swap, el estado de cada modelo se toma de `/running`, que es
    // la lista real de lo cargado en VRAM. Así "cargado" / "descargado" no es
    // una suposición sobre el catálogo, sino el estado de verdad.
    if row.kind == "llama-swap" && process_active {
        if let Some(cargados) = llama_swap_cargados(host, row.port).await {
            let ids_cargados: Vec<String> = cargados
                .iter()
                .filter_map(|m| m.get("model").and_then(|v| v.as_str()).map(String::from))
                .collect();
            for m in models.iter_mut() {
                m.state = if ids_cargados.iter().any(|id| id == &m.id) {
                    "loaded".to_string()
                } else {
                    "unloaded".to_string()
                };
            }
        }
    }

    // Un servidor PARADO no es un error: es su estado normal, y ya se
    // publica en `state`. Antes se marcaba `error = "Process not running"`
    // para todo lo que no estuviera levantado, así que el panel avisaba
    // "7 servidores no responden" de servicios que simplemente no están
    // instalados. Error de verdad es una sola cosa: el proceso está en
    // marcha y no contesta en su puerto.
    let reachable = !models.is_empty() || (process_active && answers(host, row.port).await);
    let error = if process_active && !reachable {
        Some("En marcha, pero no contesta en su puerto".to_string())
    } else {
        None
    };

    Server {
        id: row.id,
        name: row.name,
        kind: row.kind,
        port: row.port,
        state: if process_active { "active".to_string() } else { "stopped".to_string() },
        process_active,
        version,
        pid,
        proc_uptime_secs,
        models,
        error,
    }
}

pub async fn build() -> Vec<Server> {
    let rows = match crate::db::servers() {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("db error: {e}");
            return Vec::new();
        }
    };
    // `enabled` se RESPETA. Antes no se miraba en ninguna parte: un servidor
    // "deshabilitado" seguía generando sus peticiones cada 2 s y seguía
    // apareciendo. El autor se lo prohíbe a sí mismo en `db.rs`: un ajuste que no
    // se consulta sería una mentira en la interfaz.
    let activos: Vec<ServerRow> = rows.into_iter().filter(|r| r.enabled).collect();

    // El barrido de /proc, UNA vez para todos: antes `running_pids` recorría /proc
    // entero por cada fila.
    let kinds: Vec<String> = activos.iter().map(|r| r.kind.clone()).collect();
    let mut pids = match tokio::task::spawn_blocking(move || {
        let refs: Vec<&str> = kinds.iter().map(String::as_str).collect();
        crate::scan::pids_por_kind(&refs)
    })
    .await
    {
        Ok(m) => m,
        Err(_) => HashMap::new(),
    };

    // Los servidores se sondean EN PARALELO: en serie, cada uno sumaba su cuenta
    // (hasta 9 GET con tiempo de espera) y la vuelta entera se iba a decenas de
    // segundos en cuanto uno tardaba.
    let mut tareas = tokio::task::JoinSet::new();
    for row in activos {
        let proceso = pids.remove(&row.kind);
        tareas.spawn(construir(row, proceso));
    }
    let mut out: Vec<Server> = Vec::new();
    while let Some(r) = tareas.join_next().await {
        if let Ok(s) = r {
            out.push(s);
        }
    }
    // El orden no puede depender de quién contesta antes: es el de siempre (por id).
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/* ── Logs del motor ───────────────────────────────────────────────────────── */

/// Últimas líneas del log de llama-swap (`GET /logs`, en texto plano).
///
/// No se usa el endpoint de *stream*: para mirar "¿qué ha pasado?" basta con el
/// histórico, refrescado a mano o cada pocos segundos. El stream mantiene la
/// conexión abierta y complica el ciclo de vida sin aportar nada aquí.
///
/// `?no-history` existe para pedir solo lo nuevo, pero para un visor bajo demanda
/// interesa justo lo contrario: el histórico.
pub async fn llama_swap_logs(port: u16, lineas: usize) -> Result<Vec<String>, String> {
    let url = format!("http://127.0.0.1:{port}/logs");
    let respuesta = CLIENTE
        .get(&url)
        // Sin `Accept: text/html`: con esa cabecera llama-swap redirige a su UI.
        .header("Accept", "text/plain")
        // Este sí es bajo demanda y puede traer mucho texto: se le da más margen
        // que al sondeo de la foto.
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("no se pudieron leer los logs de llama-swap: {e}"))?;
    if !respuesta.status().is_success() {
        return Err(format!("llama-swap devolvió {} al pedir los logs", respuesta.status()));
    }
    let texto = respuesta.text().await.map_err(|e| e.to_string())?;
    let todas: Vec<&str> = texto.lines().collect();
    let desde = todas.len().saturating_sub(lineas.clamp(10, 5000));
    Ok(todas[desde..].iter().map(|l| l.to_string()).collect())
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn saca_los_modelos_del_catalogo_de_llama_swap() {
        // Forma real de `/v1/models` de llama-swap (recortada): el estado va en
        // `status.value`, y antes se fijaba "loaded" para todo.
        let v: serde_json::Value = serde_json::json!({
            "data": [
                {"id": "modelo-27b", "status": {"value": "unloaded"}},
                {"id": "modelo-8b", "status": {"value": "ready"}},
            ]
        });
        let m = parse_models(&v).expect("tiene que parsear la lista");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "modelo-27b");
        assert_eq!(m[0].state, "unloaded");
        assert_eq!(m[1].state, "ready");
    }

    #[test]
    fn reconoce_los_ids_de_la_lista_cruda() {
        // Forma de `/api/tags` de Ollama: una lista de objetos con `name`.
        let v: serde_json::Value = serde_json::json!({
            "models": [{"name": "qwen3:8b", "size": 5000000000_i64}]
        });
        let m = parse_models(&v).unwrap();
        assert_eq!(m[0].label, "qwen3:8b");
        // 5 GB en MB, con el mismo factor que usa el resto del backend.
        assert!(m[0].size_mb.unwrap() > 4700.0);
    }

    #[test]
    fn no_inventa_modelos_si_el_json_no_tiene_lista() {
        assert!(parse_models(&serde_json::json!({"ok": true})).is_none());
    }
}
