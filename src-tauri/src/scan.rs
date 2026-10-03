//! Procesos del equipo y nombres de los modelos.
//!
//! POR QUÉ YA NO LEE `/proc`: esto abría `/proc/<pid>/comm`, `/cmdline`, `/stat` y
//! `/status` a mano, contaba el RSS en PÁGINAS y dividía entre 1024 (salía 4× más
//! pequeño de lo real) y calculaba el CPU por diferencias de ticks con `_SC_CLK_TCK`
//! fijo a 100. Nada de eso existe fuera de Linux. Ahora los procesos los da
//! `plataforma::procesos()` (que usa `sysinfo` en los tres sistemas y ya devuelve
//! bytes y segundos), y aquí solo queda lo que es NUESTRO: reconocer qué proceso es
//! de qué motor de IA y deducir la cuantización del nombre de un modelo.
use std::collections::HashMap;

use crate::plataforma;
use crate::types::AiProc;

/// Patrones por motor. Se busca en el NOMBRE del ejecutable y en la línea de
/// comandos completa (un `python -m vllm.entrypoints...` no se llama «vllm»).
const PATTERNS: &[(&str, &[&str])] = &[
    ("llama-swap", &["llama-swap", "modelo-local-local", "modelo-local_swap"]),
    ("llama-cpp", &["llama-server", "llama-emb", "llama-batch", "llama-quantize", "llama-cli", "llamafile"]),
    ("ollama", &["ollama"]),
    ("vllm", &["vllm"]),
    ("comfyui", &["comfyui", "comfy-ui"]),
    ("tgwebui", &["text-generation-webui", "sd-webui"]),
    ("exllama", &["exllama", "exllama_v2"]),
    ("lmstudio", &["lmstudio", "lm-studio", "LM Studio"]),
];

pub fn kind_patterns(kind: &str) -> Vec<&'static str> {
    PATTERNS
        .iter()
        .find(|(t, _)| *t == kind)
        .map(|(_, p)| p.to_vec())
        .unwrap_or_default()
}

fn match_proc(name: &str, cmdline: &str, patterns: &[&str]) -> bool {
    let name_l = name.to_lowercase();
    let cmd_l = cmdline.to_lowercase();
    patterns.iter().any(|p| name_l.contains(&p.to_lowercase()) || cmd_l.contains(&p.to_lowercase()))
}

/// Procesos vivos, en una sola lectura. Todo lo de este fichero sale de aquí.
fn procesos() -> Vec<plataforma::Proceso> {
    plataforma::procesos()
}

pub fn running_pids(patterns: &[&str]) -> Vec<(i32, i64)> {
    procesos()
        .into_iter()
        .filter(|p| match_proc(&p.nombre, &p.cmd, patterns))
        .map(|p| (p.pid, p.uptime_secs))
        .collect()
}

/// Procesos de CADA motor, con UNA sola lectura de la tabla de procesos.
///
/// POR QUÉ: la foto de servidores preguntaba por cada fila (`running_pids`), y
/// `running_pids` recorre TODOS los procesos. Con las 8 filas sembradas eso eran 8
/// vueltas completas para acabar mirando los mismos procesos. Aquí se lee una vez
/// y cada proceso se asigna al primer motor que lo reconozca (un proceso es de un
/// motor, no de varios).
pub fn pids_por_kind(kinds: &[&str]) -> HashMap<String, Vec<(i32, i64)>> {
    let patrones: Vec<(&str, Vec<&'static str>)> = kinds.iter().map(|k| (*k, kind_patterns(k))).collect();
    let mut out: HashMap<String, Vec<(i32, i64)>> = HashMap::new();
    for p in procesos() {
        for (kind, pats) in &patrones {
            if pats.is_empty() {
                continue;
            }
            if match_proc(&p.nombre, &p.cmd, pats) {
                out.entry((*kind).to_string()).or_default().push((p.pid, p.uptime_secs));
                break;
            }
        }
    }
    out
}

pub fn stop_by_pattern(patterns: &[&str]) -> i32 {
    procesos()
        .into_iter()
        .filter(|p| match_proc(&p.nombre, &p.cmd, patterns))
        .filter(|p| kill_process(p.pid))
        .count() as i32
}

/// Mata un proceso. Antes esto lanzaba el binario `kill` como subproceso (que en
/// Windows no existe); ahora lo hace `plataforma`, que usa la señal del sistema.
pub fn kill_process(pid: i32) -> bool {
    plataforma::matar(pid)
}

fn tag_of(name: &str, cmdline: &str) -> String {
    let name_l = name.to_lowercase();
    let cmd_l = cmdline.to_lowercase();
    for (tag, patterns) in PATTERNS {
        if patterns
            .iter()
            .any(|p| name_l.contains(&p.to_lowercase()) || cmd_l.contains(&p.to_lowercase()))
        {
            return tag.to_string();
        }
    }
    String::from("ai")
}

pub fn ai_procs() -> Vec<AiProc> {
    procesos()
        .into_iter()
        .filter_map(|p| {
            let tag = tag_of(&p.nombre, &p.cmd);
            if tag == "ai" {
                // No es un proceso de IA: ni se mide ni se lista.
                return None;
            }
            Some(AiProc {
                pid: p.pid,
                name: p.nombre,
                // La línea de comandos va ENTERA, sin recortar.
                //
                // POR QUÉ: antes se recortaba a 220 caracteres «para que la interfaz
                // no se rompiera», y eso tiraba información que SÍ se usa: las
                // banderas del final (`--cache-type-k q4_0`, `-fa on`) son justo las
                // que explican por qué un contexto de 65536 tokens no se come la
                // VRAM. La sección de Memoria las leía y no las encontraba, y en
                // pantalla salía «sin cuantizar» para un modelo que SÍ la lleva.
                //
                // Recortar es cosa de la VISTA (las listas ya llevan `truncate` y su
                // `title`): el dato se guarda completo y quien lo enseña decide.
                cmd: p.cmd,
                cpu_pct: p.cpu_pct,
                mem_mb: p.memoria_mb,
                uptime_secs: p.uptime_secs,
                tag,
            })
        })
        .collect()
}

/* ── Nombres de los modelos ───────────────────────────────────────────────── */

/// Prefijos de cuantización que se reconocen.
///
/// Lista explícita a propósito: la versión anterior aceptaba cualquier cosa que
/// empezara por "Q", "I" o "F", y eso marcaba como cuantización nombres que no
/// lo eran. Además le faltaba "PTQ", que es justo una de las que se usan aquí.
pub(crate) const PREFIJOS_CUANT: &[&str] = &["IQ", "PTQ", "PQ", "MXFP", "BF", "Q", "F"];

fn parece_cuantizacion(seg: &str) -> bool {
    seg.len() >= 2
        && PREFIJOS_CUANT.iter().any(|p| seg.starts_with(p))
        // Todas llevan dígitos (Q8_0, F16, MXFP4…): eso descarta palabras como
        // "Preview" o "Final" sin tener que enumerarlas.
        && seg.chars().any(|c| c.is_ascii_digit())
}

/// Cuantización deducida del final del nombre: `...-Q8_0.gguf` -> `Q8_0`.
///
/// OJO con esto, que estaba mal: usaba `rsplitn(2, '-').nth(1)`, que devuelve el
/// trozo de la IZQUIERDA del último guion (el nombre entero sin la coletilla) en
/// vez del último segmento. Resultado: no reconocía ninguna cuantización y la
/// tabla de modelos enseñaba un guion en todas las filas.
pub(crate) fn quant_from_name(name: &str) -> Option<String> {
    let base = name.strip_suffix(".gguf").unwrap_or(name);
    let last_seg = base.rsplitn(2, '-').next()?;
    parece_cuantizacion(last_seg).then(|| last_seg.to_string())
}

/* ── Pruebas ────────────────────────────────────────────────────────────────── */
#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn saca_la_cuantizacion_del_nombre_real() {
        // Nombres tal cual están en ~/models en esta máquina.
        assert_eq!(
            quant_from_name("MiMo-V2.6-Distill-Qwen-9B-Q8_0.gguf").as_deref(),
            Some("Q8_0")
        );
        assert_eq!(
            quant_from_name("Modelo-27B-PQ2_0.gguf").as_deref(),
            Some("PQ2_0")
        );
        assert_eq!(
            quant_from_name("Modelo-27B-PTQ1_0.gguf").as_deref(),
            Some("PTQ1_0")
        );
        assert_eq!(
            quant_from_name("modelo-8b-Q2_0_g64.gguf").as_deref(),
            Some("Q2_0_g64")
        );
    }

    #[test]
    fn reconoce_todas_las_familias_de_cuantizacion_que_se_usan() {
        for (nombre, esperado) in [
            ("modelo-IQ4_XS.gguf", "IQ4_XS"),
            ("modelo-MXFP4.gguf", "MXFP4"),
            ("modelo-BF16.gguf", "BF16"),
            ("modelo-F16.gguf", "F16"),
        ] {
            assert_eq!(quant_from_name(nombre).as_deref(), Some(esperado), "{nombre}");
        }
    }

    #[test]
    fn no_inventa_cuantizacion_si_el_nombre_no_la_lleva() {
        assert_eq!(quant_from_name("modelo-final.gguf"), None);
        // El trozo a la derecha del último guion tiene que PARECER una
        // cuantización; si no, es mejor no decir nada.
        assert_eq!(quant_from_name("MiMo-V2.6-Distill-Qwen-9B.gguf"), None);
        assert_eq!(quant_from_name("sin-guion.gguf"), None);
        // Palabras que empiezan como una cuantización pero no lo son.
        assert_eq!(quant_from_name("modelo-Preview.gguf"), None);
        assert_eq!(quant_from_name("modelo-Final.gguf"), None);
    }

    #[test]
    fn reconoce_los_motores_por_su_linea_de_comandos() {
        // Un `python -m vllm...` no se llama «vllm»: por eso se mira la línea entera.
        assert!(match_proc("python", "python -m vllm.entrypoints.openai.api_server", &["vllm"]));
        assert!(match_proc("llama-server", "", &["llama-server"]));
        assert!(!match_proc("firefox", "firefox", &["llama-server"]));
    }

    #[test]
    fn los_procesos_de_ia_salen_con_su_datos() {
        let ps = ai_procs();
        for p in &ps {
            assert_ne!(p.tag, "ai", "se coló un proceso que no es de IA: {p:?}");
            assert!(p.pid > 0);
            assert!(p.mem_mb >= 0.0);
            assert!(p.uptime_secs >= 0);
        }
    }

    #[test]
    fn buscar_un_motor_que_no_esta_no_devuelve_nada() {
        assert!(running_pids(&["motor-que-no-existe-en-esta-maquina"]).is_empty());
        assert!(pids_por_kind(&["motor-que-no-existe-en-esta-maquina"]).is_empty());
    }

    #[test]
    fn matar_un_proceso_inexistente_devuelve_false() {
        assert!(!kill_process(2_147_483_000));
    }

    #[test]
    fn los_patrones_de_cada_motor_estan_definidos() {
        for (kind, _) in PATTERNS {
            assert!(!kind_patterns(kind).is_empty(), "{kind} sin patrones");
        }
        assert!(kind_patterns("no-existe").is_empty());
    }
}
