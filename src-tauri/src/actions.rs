use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TCommand;
use tauri::{AppHandle, Emitter};
use serde_json::Value;

use crate::db;
use crate::display;
use crate::perf;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ActionJson {
    pub kind: String,
    pub args: Value,
}

/// Lista de cadenas de un argumento (`rutas`, `ids`). Lo que no sea una cadena
/// se descarta en vez de inventarse un valor.
fn lista_de(args: &Value, clave: &str) -> Vec<String> {
    args.get(clave)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn is_running(kind: &str) -> bool {
    let patterns = crate::scan::kind_patterns(kind);
    !patterns.is_empty() && !crate::scan::running_pids(&patterns).is_empty()
}

/* ── Lanzar un proceso con la salida en vivo ──────────────────────────────── */

/// Ejecuta un comando emitiendo cada línea EN CUANTO llega por `ai:update-line`.
///
/// Devuelve las líneas de salida estándar y las de error POR SEPARADO, y el
/// código de salida. Van separadas a propósito: `llama-bench -o json` escribe el
/// JSON en la salida estándar y sus mensajes de arranque en la de error, así que
/// juntarlas rompería el parseo.
///
/// Las dos salidas se leen en paralelo, cada una en su tarea: si se leyera una
/// entera antes de empezar la otra, el proceso se quedaría bloqueado escribiendo
/// en la que nadie lee en cuanto su buffer de 64 KB se llenara.
async fn ejecutar_streaming(app: &AppHandle, partes: &[String]) -> Result<(Vec<String>, Vec<String>, i32), String> {
    if partes.is_empty() || partes[0].is_empty() {
        return Err("Comando vacío".into());
    }
    let mut child = TCommand::new(&partes[0])
        .args(&partes[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("No se pudo lanzar {}: {e}", partes[0]))?;

    let stdout = child.stdout.take().ok_or("stdout sin tubería")?;
    let stderr = child.stderr.take().ok_or("stderr sin tubería")?;

    // Cada mensaje lleva delante si venía de la salida de error, para poder
    // separarlos después sin perder el orden en que se pintan.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(bool, String)>(4096);
    let tx_out = tx.clone();
    let lector_out = tokio::spawn(async move {
        let mut lineas = BufReader::new(stdout).lines();
        while let Ok(Some(l)) = lineas.next_line().await {
            if tx_out.send((false, l)).await.is_err() {
                break;
            }
        }
    });
    let tx_err = tx.clone();
    let lector_err = tokio::spawn(async move {
        let mut lineas = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lineas.next_line().await {
            if tx_err.send((true, l)).await.is_err() {
                break;
            }
        }
    });
    drop(tx);

    let mut salida: Vec<String> = Vec::new();
    let mut errores: Vec<String> = Vec::new();
    let mut cola: VecDeque<String> = VecDeque::new();
    while let Some((es_error, linea)) = rx.recv().await {
        let visible = if es_error { format!("[err] {linea}") } else { linea.clone() };
        let _ = app.emit("ai:update-line", visible.as_str());
        cola.push_back(visible);
        if cola.len() > 600 {
            cola.pop_front();
        }
        if es_error {
            errores.push(linea);
        } else {
            salida.push(linea);
        }
    }

    let _ = lector_out.await;
    let _ = lector_err.await;
    let estado = child.wait().await.map_err(|e| format!("falló la espera del proceso: {e}"))?;
    Ok((salida, errores, estado.code().unwrap_or(-1)))
}

/// Cola de la salida (lo último que dijo el proceso), para guardarlo en SQLite.
fn cola_de(salida: &[String], errores: &[String]) -> String {
    let mut todas: Vec<String> = Vec::new();
    todas.extend(salida.iter().cloned());
    todas.extend(errores.iter().map(|l| format!("[err] {l}")));
    let desde = todas.len().saturating_sub(300);
    todas[desde..].join("\n")
}

/* ── Servidores ───────────────────────────────────────────────────────────── */

async fn server_start(args: Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let kind = id.split(':').next().unwrap_or("").to_string();
    if is_running(&kind) {
        return Ok(format!("El servidor '{kind}' ya está en marcha"));
    }
    let rows = match db::servers() {
        Ok(rows) => rows,
        Err(_) => Vec::new(),
    };
    let mut cmd: Option<String> = rows.iter().find(|r| r.id == id).map(|r| r.cmd.clone()).unwrap_or_default();
    let default = if kind == "ollama" {
        Some("ollama serve".to_string())
    } else {
        None
    };
    if cmd.is_none() {
        cmd = default;
    }
    if cmd.is_none() {
        let home = dirs::home_dir()
            .unwrap_or_else(|| Path::new("/home/usuario").to_path_buf())
            .to_string_lossy()
            .to_string();
        cmd = Some(match kind.as_str() {
            "llama-swap" => {
                // Se mira en los sitios habituales de un binario propio y, si no
                // está, se deja el nombre pelado para que lo resuelva el PATH.
                // (Antes apuntaba a una carpeta concreta de un proyecto personal,
                // que en otro equipo no existe.)
                ["local/bin", "bin"]
                    .iter()
                    .map(|d| format!("{home}/{d}/llama-swap"))
                    .find(|p| Path::new(p).is_file())
                    .unwrap_or_else(|| "llama-swap".to_string())
            }
            "llama-cpp" => {
                if Path::new(&format!("{home}/.local/bin/llama-server")).is_file() {
                    format!("{home}/.local/bin/llama-server")
                } else {
                    "llama-server".to_string()
                }
            }
            "comfyui" => {
                if Path::new(&format!("{home}/.local/bin/comfyui")).is_file() {
                    format!("{home}/.local/bin/comfyui")
                } else {
                    "comfyui".to_string()
                }
            }
            _ => {
                return Err(format!(
                    "No hay comando configurado para '{id}'. Añádelo en Ajustes."
                ))
            }
        });
    }
    let Some(cmd) = cmd else {
        return Err(format!("No hay comando configurado para '{id}'. Añádelo en Ajustes."));
    };
    let parts = shlex::split(&cmd).ok_or_else(|| format!("Comando inválido: {cmd}"))?;
    if parts.is_empty() {
        return Err(format!("Comando vacío para '{id}'. Corrígelo en Ajustes."));
    }
    let _ = TCommand::new(&parts[0])
        .args(&parts[1..])
        .kill_on_drop(false)
        .spawn()
        .map_err(|e| format!("No se pudo lanzar el proceso: {e}"))?;
    // El estado cacheado de los servidores ya no vale.
    crate::servers::invalidar_cache();
    Ok(format!("En marcha: {cmd}"))
}

async fn server_stop(args: Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let kind = id.split(':').next().unwrap_or("");
    let patterns = crate::scan::kind_patterns(&kind);
    let n = crate::scan::stop_by_pattern(&patterns);
    // El estado cacheado de los servidores ya no vale.
    crate::servers::invalidar_cache();
    if n == 0 {
        Ok(format!("No hay ningún proceso de '{kind}' en marcha"))
    } else {
        Ok(format!("Detenidos {n} procesos de {kind}"))
    }
}

fn process_kill(args: Value) -> Result<String, String> {
    let pid = args.get("pid").and_then(|v| v.as_i64()).unwrap_or_default() as i32;
    if crate::scan::kill_process(pid) {
        Ok(format!("Proceso {pid} terminado"))
    } else {
        Err(format!("No se pudo terminar el proceso {pid}"))
    }
}

/* ── Actualizaciones ──────────────────────────────────────────────────────── */

async fn update_run(app: &AppHandle, args: Value) -> Result<String, String> {
    let cmd = args.get("cmd").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    if cmd.is_empty() {
        return Err("No se ha indicado ningún comando".into());
    }
    let parts = shlex::split(&cmd).ok_or_else(|| format!("Comando inválido: {cmd}"))?;
    if parts.is_empty() {
        return Err("Comando vacío".into());
    }

    let (salida, errores, code) = ejecutar_streaming(app, &parts).await?;
    let tail = cola_de(&salida, &errores);
    let _ = db::insert_update("update", &cmd, &tail, Some(code as i64), code == 0);

    if code == 0 {
        Ok(format!("Actualización completada: {cmd}"))
    } else {
        Err(format!("Actualización fallida (código {code})"))
    }
}

/* ── Encaje y rendimiento (herramientas nativas de llama.cpp) ─────────────── */

/// ¿Cabe este modelo, y con cuánto contexto?
///
/// Usa `llama-fit-params`, el mismo planificador nativo que emplea Magnitude para
/// su evaluación de modelos. Con `runtime = "auto"` (por defecto) se prueban los
/// runtimes instalados hasta encontrar uno que SEPA LEER el modelo: los Modelo local
/// ternarios solo los lee el fork, y el oficial falla con "invalid ggml type".
async fn perf_fit(app: &AppHandle, args: Value) -> Result<String, String> {
    let modelo = args.get("modelo").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let ctx = args.get("ctx").and_then(|v| v.as_i64());
    let kv = args.get("kv").and_then(|v| v.as_bool()).unwrap_or(true);
    let runtime = args.get("runtime").and_then(|v| v.as_str()).map(str::to_string);

    // El cálculo en sí es bloqueante (~0,4 s y sin tocar la GPU), así que va a un
    // hilo aparte en vez de bloquear el bucle asíncrono. Se reutiliza el MISMO
    // camino que el cálculo automático de fondo: una sola implementación.
    let (modelo2, runtime2) = (modelo.clone(), runtime.clone());
    let resultado = tokio::task::spawn_blocking(move || {
        perf::fit_de_modelo(
            &modelo2,
            ctx,
            kv,
            runtime2.as_deref().filter(|r| *r != "auto"),
            None,
        )
    })
    .await
    .map_err(|e| format!("el cálculo del encaje se interrumpió: {e}"))?;

    match resultado {
        Ok(f) => {
            // Se guarda para que quede como el último encaje conocido de ese
            // modelo, igual que el automático, y la interfaz lo enseñe sin más.
            let _ = db::insert_fit(&f);
            let _ = app.emit("ai:fit", &f);
            Ok(format!("{} · runtime {}", f.detalle, f.runtime))
        }
        Err(e) => Err(e),
    }
}

/// Mide tokens/s REALES con `llama-bench` y los guarda.
///
/// Medir es caro (carga el modelo y genera de verdad), así que esto solo se lanza
/// a petición, nunca en el bucle de fondo.
async fn perf_bench(app: &AppHandle, args: Value) -> Result<String, String> {
    let modelo = args.get("modelo").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let prompt = args.get("prompt").and_then(|v| v.as_i64()).unwrap_or(512);
    let gen = args.get("gen").and_then(|v| v.as_i64()).unwrap_or(128);
    let reps = args.get("reps").and_then(|v| v.as_i64()).unwrap_or(3).clamp(1, 10);
    let runtime_pedido = args.get("runtime").and_then(|v| v.as_str()).unwrap_or("auto").to_string();

    perf::modelo_valido(&modelo)?;

    let todos = perf::runtimes();
    let mut candidatos: Vec<perf::Runtime> = if runtime_pedido == "auto" {
        todos
    } else {
        todos.into_iter().filter(|r| r.nombre == runtime_pedido).collect()
    };
    if candidatos.is_empty() {
        return Err("No hay ningún runtime de llama.cpp que coincida (mira `perf:tools`).".into());
    }
    // El oficial primero: es el que sirve los modelos que no son ternarios.
    candidatos.sort_by_key(|r| if r.nombre.contains("oficial") { 0 } else { 1 });

    let mut intentos: Vec<String> = Vec::new();
    for r in candidatos {
        let Some(bench_bin) = r.bench.clone() else { continue };
        let cmd = vec![
            bench_bin,
            "-m".into(),
            modelo.clone(),
            "-p".into(),
            prompt.to_string(),
            "-n".into(),
            gen.to_string(),
            "-r".into(),
            reps.to_string(),
            "-o".into(),
            "json".into(),
            "-ctk".into(),
            perf::KV_K.into(),
            "-ctv".into(),
            perf::KV_V.into(),
            "-fa".into(),
            perf::FLASH_ATTN.into(),
        ];

        let (salida, errores, code) = ejecutar_streaming(app, &cmd).await?;
        let json: String = salida.join("\n");

        match perf::analizar_bench(&json) {
            Ok((medidas, info)) => {
                for m in &medidas {
                    let _ = db::insert_benchmark(
                        &info.modelo,
                        &r.nombre,
                        &m.tipo,
                        m.n_prompt,
                        m.n_gen,
                        m.tok_s,
                        m.desviacion,
                        &info.build,
                        &info.gpu,
                    );
                }
                let partes: Vec<String> = medidas
                    .iter()
                    .map(|m| {
                        let etiqueta = if m.tipo == "decode" {
                            format!("generacion ({} tok)", m.n_gen)
                        } else {
                            format!("prefill ({} tok)", m.n_prompt)
                        };
                        format!("{etiqueta}: {:.1} tok/s", m.tok_s)
                    })
                    .collect();
                return Ok(format!(
                    "Medido {} con {}: {}",
                    info.model_type,
                    r.nombre,
                    partes.join(" · ")
                ));
            }
            Err(e) => {
                if perf::error_de_formato(&errores.join("\n")) {
                    intentos.push(format!("{}: no puede leer este modelo", r.nombre));
                    continue;
                }
                return Err(format!(
                    "llama-bench falló con {} (código {code}): {}",
                    r.nombre,
                    errores.last().cloned().unwrap_or(e)
                ));
            }
        }
    }

    Err(format!(
        "Ningún runtime instalado sabe leer este modelo ({}).",
        intentos.join("; ")
    ))
}

/* ── Cargar y descargar modelos (llama-swap) ─────────────────────────────── */

/// Puerto por defecto de llama-swap cuando la interfaz no dice otro.
fn puerto_swap(args: &Value) -> u16 {
    args.get("port").and_then(|v| v.as_u64()).unwrap_or(8080) as u16
}

/// Trae un modelo a la VRAM.
///
/// llama-swap no tiene un endpoint de "cargar": su diseño es cargar bajo demanda,
/// al recibir la PRIMERA petición. Así que se le manda una petición mínima de
/// verdad (un token) en vez de inventarse una API que no existe. El proceso de
/// carga lo hace él, con el comando de su configuración.
async fn model_load(args: Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    if id.is_empty() {
        return Err("Falta el identificador del modelo".into());
    }
    let puerto = puerto_swap(&args);
    let url = format!("http://127.0.0.1:{puerto}/v1/chat/completions");
    let cuerpo = serde_json::json!({
        "model": id,
        "messages": [{"role": "user", "content": "."}],
        "max_tokens": 1,
        "stream": false,
    });

    let respuesta = reqwest::Client::new()
        .post(&url)
        .json(&cuerpo)
        // Holgado: si el modelo no estaba cargado, esto incluye traerlo a la VRAM.
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .map_err(|e| format!("No se pudo pedir la carga de '{id}': {e}"))?;

    if !respuesta.status().is_success() {
        let codigo = respuesta.status();
        let texto = respuesta.text().await.unwrap_or_default();
        return Err(format!(
            "llama-swap rechazó la carga de '{id}' ({codigo}): {}",
            texto.trim().chars().take(300).collect::<String>()
        ));
    }
    Ok(format!("'{id}' cargado y respondiendo (ya ocupa VRAM)."))
}

/// Saca un modelo de la VRAM sin parar el servidor.
///
/// Endpoint documentado por llama-swap: `POST /api/models/unload/:model_id`.
async fn model_unload(args: Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    if id.is_empty() {
        return Err("Falta el identificador del modelo".into());
    }
    let puerto = puerto_swap(&args);
    let url = format!("http://127.0.0.1:{puerto}/api/models/unload/{id}");

    let respuesta = reqwest::Client::new()
        .post(&url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("No se pudo pedir la descarga de '{id}': {e}"))?;

    if !respuesta.status().is_success() {
        let codigo = respuesta.status();
        let texto = respuesta.text().await.unwrap_or_default();
        return Err(format!(
            "llama-swap rechazó la descarga de '{id}' ({codigo}): {}",
            texto.trim().chars().take(300).collect::<String>()
        ));
    }
    Ok(format!("'{id}' descargado: ya no ocupa VRAM."))
}

/// Saca de la VRAM TODOS los modelos cargados de golpe.
///
/// Endpoint documentado por llama-swap: `POST /api/models/unload` (sin
/// identificador). Es la forma de dejar la VRAM libre sin parar el servidor, por
/// ejemplo antes de medir o de cargar un modelo grande.
async fn model_unload_all(args: Value) -> Result<String, String> {
    let puerto = puerto_swap(&args);
    // Primero se mira qué hay cargado, para poder decir qué se ha liberado en vez
    // de dar por hecho que había algo.
    let habia = crate::servers::llama_swap_cargados("127.0.0.1", puerto).await;
    let nombres: Vec<String> = habia
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("model").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let url = format!("http://127.0.0.1:{puerto}/api/models/unload");
    let respuesta = reqwest::Client::new()
        .post(&url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("No se pudo pedir la descarga de todos: {e}"))?;

    if !respuesta.status().is_success() {
        let codigo = respuesta.status();
        let texto = respuesta.text().await.unwrap_or_default();
        return Err(format!(
            "llama-swap rechazó la descarga de todos ({codigo}): {}",
            texto.trim().chars().take(300).collect::<String>()
        ));
    }

    if nombres.is_empty() {
        Ok("No había ningún modelo cargado; no había nada que liberar.".to_string())
    } else {
        Ok(format!(
            "VRAM liberada: descargados {} modelo(s) ({}).",
            nombres.len(),
            nombres.join(", ")
        ))
    }
}

/// Mide el rendimiento CONTRA UN SERVIDOR EN MARCHA, con llmfit.
///
/// Se diferencia de `perf:bench` (que usa `llama-bench` en aislado) en dos cosas:
/// mide lo que el motor sirve de verdad, con su configuración y su proxy por
/// medio, y **el resultado se guarda en llmfit**, cuyas estimaciones pasan a
/// apoyarse en medidas propias.
async fn llmfit_medir(app: &AppHandle, args: Value) -> Result<String, String> {
    let modelo = args.get("modelo").and_then(|v| v.as_str()).map(str::to_string);
    let proveedor = args.get("provider").and_then(|v| v.as_str()).unwrap_or("llamacpp").to_string();
    let url = args.get("url").and_then(|v| v.as_str()).map(str::to_string);
    let runs = args.get("runs").and_then(|v| v.as_i64()).unwrap_or(3).clamp(1, 10);
    let todos = args.get("todos").and_then(|v| v.as_bool()).unwrap_or(false);

    let Some(bin) = crate::llmfit::binario() else {
        return Err("llmfit no está instalado, y es quien mide aquí: https://github.com/AlexsJones/llmfit".into());
    };

    let mut cmd = vec![bin, "bench".to_string()];
    if let Some(m) = modelo.filter(|m| !m.is_empty()) {
        cmd.push(m);
    }
    cmd.push("--provider".to_string());
    cmd.push(proveedor.clone());
    if let Some(u) = url.filter(|u| !u.is_empty()) {
        cmd.push("--url".to_string());
        cmd.push(u);
    }
    cmd.push("--runs".to_string());
    cmd.push(runs.to_string());
    if todos {
        cmd.push("--all".to_string());
    }
    cmd.push("--json".to_string());

    let (salida, errores, code) = ejecutar_streaming(app, &cmd).await?;
    if code != 0 {
        return Err(format!(
            "la medición falló (código {code}): {}",
            errores.last().cloned().unwrap_or_else(|| "sin detalle".into())
        ));
    }

    let r = crate::llmfit::parsear_bench(&salida.join("\n"))?;
    // Se guarda en el histórico de Machinograph con el origen bien visible: una medida
    // por el proxy NO es comparable con una de `llama-bench` en aislado.
    let runtime = format!("llmfit ({})", r.provider);
    // Convención de la tabla: en una fila de decodificación el `n_gen` son los
    // tokens GENERADOS y el prompt va a 0 (al revés sería una fila de prefill).
    // Y la dispersión se guarda como la mitad del recorrido entre el máximo y el
    // mínimo, porque llmfit no da desviación típica: es una medida de dispersión,
    // no una típica, y así queda dicho.
    let dispersion = (r.summary.max_tps - r.summary.min_tps) / 2.0;
    let _ = db::insert_benchmark(
        &r.model,
        &runtime,
        "decode",
        0,
        r.summary.avg_output_tokens as i64,
        r.summary.avg_tps,
        dispersion,
        "",
        "",
    );
    Ok(format!(
        "Medido '{}' contra el servidor en marcha: {:.1} tok/s de media en {} pasada(s) (máx {:.1}, mín {:.1}). Guardado también en llmfit.",
        r.model, r.summary.avg_tps, r.summary.num_runs, r.summary.max_tps, r.summary.min_tps
    ))
}

/// Abre en el gestor de archivos la carpeta donde está un modelo.
///
/// Útil para lo que Machinograph no hace (mover, inspeccionar, borrar a mano): en vez
/// de reimplementar un explorador, se enseña dónde está.
async fn model_reveal(app: &AppHandle, args: Value) -> Result<String, String> {
    let ruta = args.get("ruta").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    let p = Path::new(&ruta);
    if !p.exists() {
        return Err(format!("no existe {ruta}"));
    }
    let carpeta = p.parent().unwrap_or(p).to_string_lossy().to_string();
    let cmd = vec!["xdg-open".to_string(), carpeta.clone()];
    let (_s, errores, code) = ejecutar_streaming(app, &cmd).await?;
    if code == 0 {
        Ok(format!("Abierta la carpeta {carpeta}"))
    } else {
        Err(format!(
            "no se pudo abrir {carpeta}: {}",
            errores.last().cloned().unwrap_or_default()
        ))
    }
}

/// Descarga un modelo recomendado por llmfit (un GGUF de Hugging Face).
///
/// Es una descarga de verdad (varios GB), así que va por el camino de salida en
/// vivo: se ve el progreso mientras baja. llmfit elige la cuantización que mejor
/// le encaja al equipo si no se le dice otra.
async fn llmfit_descargar(app: &AppHandle, args: Value) -> Result<String, String> {
    let modelo = args.get("modelo").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    if modelo.is_empty() {
        return Err("Falta el modelo que descargar".into());
    }
    // Una sola descarga a la vez, por el camino que sea: este botón y el panel de
    // Descubrir son dos puertas al mismo sitio, y dos procesos bajando el mismo
    // fichero escribirían en el mismo temporal.
    if crate::descarga::en_curso() {
        return Err(
            "Ya hay una descarga en curso: mírala en el panel de Descubrir o cancélala antes.".into(),
        );
    }
    let quant = args.get("quant").and_then(|v| v.as_str()).map(str::to_string);
    let Some(bin) = crate::llmfit::binario() else {
        return Err(
            "llmfit no está instalado, y es quien sabe descargar el modelo: https://github.com/AlexsJones/llmfit"
                .into(),
        );
    };
    let mut cmd = vec![bin, "download".to_string(), modelo.clone()];
    if let Some(q) = quant.filter(|q| !q.is_empty()) {
        cmd.push("--quant".to_string());
        cmd.push(q);
    }
    let (salida, errores, code) = ejecutar_streaming(app, &cmd).await?;
    if code == 0 {
        // La última línea útil suele decir dónde ha quedado.
        let ultimo = salida
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .cloned()
            .unwrap_or_default();
        Ok(format!("Descarga terminada: {modelo}. {ultimo}"))
    } else {
        Err(format!(
            "la descarga falló (código {code}): {}",
            errores.last().cloned().unwrap_or_else(|| "sin detalle".into())
        ))
    }
}

/// Borra un modelo llevándolo a la PAPELERA del escritorio, no a la basura.
///
/// El motivo es el tamaño: aquí hay modelos de 6 y 9 GB, y un borrado de verdad
/// no se deshace. Se mueve a `~/.local/share/Trash` con su `.trashinfo` (la
/// especificación freedesktop), así que se puede recuperar desde el gestor de
/// archivos, y la interfaz lo dice. Si la ruta no está dentro de una carpeta de
/// modelos conocida, no se toca nada.
///
/// ¿Es el mismo fichero? Se comparan las rutas CANÓNICAS cuando existen: el motor
/// publica la ruta tal cual se la pasaron (`--model`), y puede venir con `..`,
/// con un enlace o sin normalizar. Si alguna no se puede resolver, se compara el
/// texto tal cual (mejor una comparación floja que no comparar).
fn misma_ruta(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Mueve un modelo a la papelera, **parando antes el motor si lo está sirviendo**.
///
/// POR QUÉ: borrar el fichero que un motor tiene abierto es un error en Windows
/// («el fichero está en uso») y en Linux deja al motor sirviendo un fichero que ya
/// no existe, sin decir nada. Magnitude lo arregló avisando; aquí se va un paso
/// más: si lo sirve llama-swap, se PARA solo y luego se borra, que es lo que el
/// usuario iba a hacer de todas formas. Si pararlo falla, NO se borra: se dice el
/// motivo (borrar por debajo de un motor en marcha es peor que no borrar).
async fn modelo_borrar(args: Value) -> Result<String, String> {
    let ruta = args.get("ruta").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
    if ruta.is_empty() {
        return Err("Falta la ruta del modelo".into());
    }
    let puerto = puerto_swap(&args);
    let servido = crate::memoria::cargados("127.0.0.1", puerto, None)
        .await
        .modelos
        .into_iter()
        .find(|m| misma_ruta(&m.ruta, &ruta));

    let parado = match &servido {
        Some(m) => {
            // `model_unload` habla con la API de llama-swap; si falla, su error ya
            // explica qué pasó y NO se sigue con el borrado.
            model_unload(serde_json::json!({ "id": m.id, "puerto": puerto })).await?;
            Some(m.nombre.clone())
        }
        None => None,
    };

    let mensaje = crate::inventario::a_la_papelera(&ruta).map_err(|e| {
        format!(
            "{e}. Si lo tiene abierto un motor que no es llama-swap, páralo antes de borrarlo."
        )
    })?;
    Ok(match parado {
        Some(nombre) => format!("Se paró «{nombre}» antes de borrarlo. {mensaje}"),
        None => mensaje,
    })
}

/* ── GPU: el reloj de memoria que se clava en 96 MHz ──────────────────────── */

/// Ruta del guardia del MCLK, si existe en este equipo.
///
/// POR QUÉ SE REUTILIZA Y NO SE REIMPLEMENTA: ese programa guarda el modo de
/// pantalla, lo restaura con una trampa y comprueba el antes y el después.
/// Duplicar esa lógica aquí sería abrir dos sitios donde equivocarse.
///
/// DÓNDE SE BUSCA, en este orden:
///   1. La variable `MACHINOGRAPH_MCLK_SCRIPT` (por si vive en otro sitio).
///   2. `~/.local/bin/mclk-guard.sh` y `~/bin/mclk-guard.sh`, que son los sitios
///      habituales para un programa propio en cualquier sistema.
fn script_mclk() -> Option<String> {
    if let Ok(ruta) = std::env::var("MACHINOGRAPH_MCLK_SCRIPT") {
        let p = std::path::PathBuf::from(ruta);
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    let home = dirs::home_dir()?;
    ["local/bin", "bin"].iter().find_map(|d| {
        let p = home.join(d).join("mclk-guard.sh");
        p.is_file().then(|| p.to_string_lossy().to_string())
    })
}

/// Reparación SUAVE: cicla el modo de pantalla y vuelve al de antes.
///
/// Es la que hay que usar primero, y la que se midió que funciona: el bug está en
/// Display Core, que le fija a la SMU un reloj de memoria mínimo y no lo revisa;
/// cambiar el modo obliga a recalcularlo. De 96 a 1000 MHz en el acto, sin
/// privilegios, sin perder la VRAM y sin cortar lo que se esté generando.
async fn gpu_arreglar(app: &AppHandle) -> Result<String, String> {
    let script = script_mclk().ok_or(
        "Para ciclar la pantalla hace falta el guardia del reloj de memoria: un programa (mclk-guard.sh) que guarde el modo de pantalla, haga el ciclo y compruebe el antes y el después. Deja el tuyo en ~/.local/bin/mclk-guard.sh, o apunta a él con la variable MACHINOGRAPH_MCLK_SCRIPT.",
    )?;
    let antes = crate::gpu::estado_mclk();
    let cmd = vec![script, "--fix-display-only".to_string()];
    let (_salida, errores, code) = ejecutar_streaming(app, &cmd).await?;

    // Se vuelve a leer el reloj en vez de fiarse de que el comando saliera bien: lo
    // que importa no es el código de salida, es si el reloj ha subido.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let despues = crate::gpu::estado_mclk();
    match (antes, despues) {
        (Some(a), Some(d)) => {
            let extra = if d.activo_mhz > a.activo_mhz {
                " Ha subido."
            } else if d.degradado {
                " Sigue clavado en el mínimo: toca el reinicio de GPU."
            } else {
                " No ha cambiado (normal si no había carga en ese momento)."
            };
            Ok(format!(
                "Ciclo de pantalla hecho (código {code}). Reloj de memoria: {} MHz → {} MHz.{extra}",
                a.activo_mhz, d.activo_mhz
            ))
        }
        _ => Ok(format!(
            "Ciclo de pantalla hecho (código {code}), pero no he podido releer el reloj para confirmarlo.{}",
            if errores.is_empty() {
                String::new()
            } else {
                format!(" Último mensaje: {}", errores.last().cloned().unwrap_or_default())
            }
        )),
    }
}

/// REINICIO DE GPU: la vía brusca.
///
/// Hay que decirlo claro porque tiene consecuencias: **pierde la VRAM** (corta lo
/// que se esté generando) y reinicia el motor gráfico, así que la pantalla puede
/// parpadear o recolocarse. Necesita root. El script del guardia documenta esta
/// misma vía como plan B, solo si el ciclo de pantalla no resuelve.
async fn gpu_reiniciar(app: &AppHandle) -> Result<String, String> {
    let programa = r#"set -e
REC=""
for i in /sys/kernel/debug/dri/*/; do
  [ "$(cat "$i/name" 2>/dev/null)" = "amdgpu" ] || continue
  if [ -e "$i/amdgpu_gpu_recover" ]; then REC="$i/amdgpu_gpu_recover"; break; fi
done
if [ -z "$REC" ]; then echo "No encuentro amdgpu_gpu_recover (¿está montado debugfs?)" >&2; exit 1; fi
echo "Nodo de reinicio: $REC"
echo 1 > "$REC"
echo "Reinicio enviado al motor gráfico."
"#;

    // /sys/kernel/debug/dri es solo-root (0700), así que el descubrimiento del
    // nodo también necesita privilegios: el glob no expande sin ellos.
    let hay_root = std::process::Command::new("sudo")
        .args(["-n", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !hay_root {
        return Err(
            "Reiniciar la GPU necesita root y 'sudo' pide contraseña aquí. A mano: sudo sh -c 'echo 1 > /sys/kernel/debug/dri/1/amdgpu_gpu_recover'"
                .into(),
        );
    }

    let antes = crate::gpu::estado_mclk();
    let cmd = vec![
        "sudo".to_string(),
        "-n".to_string(),
        "sh".to_string(),
        "-c".to_string(),
        programa.to_string(),
    ];
    let (_salida, errores, code) = ejecutar_streaming(app, &cmd).await?;
    if code != 0 {
        return Err(format!(
            "El reinicio de GPU falló (código {code}): {}",
            errores.last().cloned().unwrap_or_else(|| "sin detalle".into())
        ));
    }

    // El motor gráfico tarda unos segundos en volver.
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    let despues = crate::gpu::estado_mclk();
    let detalle = match (antes, despues) {
        (Some(a), Some(d)) => format!(
            " Reloj de memoria: {} MHz → {} MHz.",
            a.activo_mhz, d.activo_mhz
        ),
        _ => String::new(),
    };
    Ok(format!(
        "GPU reiniciada (se ha perdido la VRAM; cualquier modelo cargado tendrá que volver a cargarse).{detalle}"
    ))
}

/* ── Punto de entrada de las acciones ─────────────────────────────────────── */

#[tauri::command(rename = "action:run")]
pub async fn run(app: AppHandle, aj: ActionJson) -> Result<String, String> {
    let kind = aj.kind.clone();
    let args = aj.args.clone();
    let msg: Result<String, String> = match kind.as_str() {
        "display:reapply" => display::reapply(),
        "display:apply" => {
            let output = args.get("output").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
            let w = args.get("w").and_then(|v| v.as_i64()).unwrap_or_default() as i32;
            let h = args.get("h").and_then(|v| v.as_i64()).unwrap_or_default() as i32;
            let hz = args.get("hz").and_then(|v| v.as_f64()).unwrap_or_default();
            display::apply(&output, w, h, hz)
        }
        "display:toggle" => {
            let output = args.get("output").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_default();
            let on = args.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
            display::toggle(&output, on)
        }
        "server:start" => server_start(args.clone()).await,
        "server:stop" => server_stop(args.clone()).await,
        "process:kill" => process_kill(args.clone()),
        "update:run" => update_run(&app, args.clone()).await,
        "perf:fit" => perf_fit(&app, args.clone()).await,
        "perf:bench" => perf_bench(&app, args.clone()).await,
        "modelo:borrar" => modelo_borrar(args.clone()).await,
        "modelo:abrir-carpeta" => model_reveal(&app, args.clone()).await,
        "llmfit:descargar" => llmfit_descargar(&app, args.clone()).await,
        "llmfit:medir" => llmfit_medir(&app, args.clone()).await,
        "modelo:cargar" => model_load(args.clone()).await,
        "modelo:descargar" => model_unload(args.clone()).await,
        "modelo:descargar-todos" => model_unload_all(args.clone()).await,
        "gpu:arreglar" => gpu_arreglar(&app).await,
        "gpu:reiniciar" => gpu_reiniciar(&app).await,
        // Borrado del analizador y limpieza de basura. Van por ACCIONES y no por
        // comandos porque son mutaciones: así quedan en el registro de acciones y
        // la interfaz recibe `ai:action` al terminar. Las dos recorren y borran
        // del disco, así que van a un hilo bloqueante.
        "almacen:borrar" => {
            let rutas = lista_de(&args, "rutas");
            let definitivo = args.get("definitivo").and_then(|v| v.as_bool()).unwrap_or(false);
            match tauri::async_runtime::spawn_blocking(move || crate::almacen::borrar(&rutas, definitivo)).await {
                Ok(r) => r,
                Err(e) => Err(format!("el borrado se interrumpió: {e}")),
            }
        }
        "limpieza:limpiar" => {
            let ids = lista_de(&args, "ids");
            match tauri::async_runtime::spawn_blocking(move || {
                crate::limpieza::limpiar(&ids).map(|i| i.mensaje)
            })
            .await
            {
                Ok(r) => r,
                Err(e) => Err(format!("la limpieza se interrumpió: {e}")),
            }
        }
        "arranque:activar" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let activo = args.get("activo").and_then(|v| v.as_bool()).unwrap_or(true);
            match tauri::async_runtime::spawn_blocking(move || {
                crate::plataforma::autoarranque::activar(&id, activo)
            })
            .await
            {
                Ok(r) => r,
                Err(e) => Err(format!("no se pudo cambiar el arranque: {e}")),
            }
        }
        // Centro de recuperación: devolver un fichero a como estaba y tirar una
        // copia. Restaurar hace SU propia copia antes de pisar el original, así que
        // tampoco es irreversible.
        "copias:restaurar" => {
            let id = args.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            match tauri::async_runtime::spawn_blocking(move || crate::copias::restaurar(id)).await {
                Ok(r) => r,
                Err(e) => Err(format!("la restauración se interrumpió: {e}")),
            }
        }
        "copias:borrar" => {
            let id = args.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            crate::copias::borrar(id)
        }
        // Vaciar la papelera: es la acción que MÁS espacio libera de golpe de todas
        // las de la app (lo que se mandó allí no cuenta como libre hasta vaciarla).
        "papelera:vaciar" => match tauri::async_runtime::spawn_blocking(crate::plataforma::papelera::vaciar).await {
            Ok(r) => r,
            Err(e) => Err(format!("no se pudo vaciar la papelera: {e}")),
        },
        _ => Err(format!("Acción desconocida: {kind}")),
    };

    let detail = msg.clone().unwrap_or_else(|e| e);
    match &msg {
        Ok(m) => {
            let _ = db::insert_action(&kind, &args.to_string(), true, m);
        }
        Err(e) => {
            let _ = db::insert_action(&kind, &args.to_string(), false, e);
        }
    }

    let _ = app.emit("ai:action", &Value::String(detail));

    match msg {
        Ok(m) => Ok(m),
        Err(e) => Err(e),
    }
}
