//! Qué ocupa la memoria de la GPU: qué modelos están servidos y CÓMO.
//!
//! POR QUÉ NO HAY UN DESGLOSE "PESOS / CACHÉ KV / SOBRECARGA", que es lo que
//! enseñan otras herramientas: porque **ningún motor de este equipo publica ese
//! reparto**. Se comprobó uno a uno antes de escribir esto:
//!
//!   * `GET /props` de llama-server da la ruta del modelo, su cuantización
//!     (`model_ftype`), los slots y el build — pero **no** el tamaño de la caché
//!     KV ni las dimensiones de la arquitectura.
//!   * `llama-swap` (`/running`) da el modelo, su estado y su línea de comandos,
//!     pero no cuánta VRAM ocupa cada uno.
//!   * `amd-smi process` en este equipo responde «No running processes detected»
//!     aunque haya un llama-server usando la GPU: no hay VRAM por proceso.
//!   * El tamaño de la caché KV **solo** aparece en el log de arranque de
//!     llama-server, y llama-swap no reenvía la salida de sus hijos (`/logs` trae
//!     únicamente sus propias líneas de petición, comprobado).
//!
//! Inventarse el reparto sería exactamente lo que este programa no hace. Lo que sí
//! hay, y es lo que enseña la sección, es lo MEDIDO:
//!
//!   * los **pesos**: el tamaño del fichero, leído del disco;
//!   * la **configuración con la que se sirve**, que sale de la línea de comandos
//!     con la que llama-swap lo arrancó: el contexto (`--ctx`/`-c`), si la caché KV
//!     va cuantizada (`--cache-type-k q4_0`), cuántas capas van a la GPU (`-ngl`);
//!   * la **VRAM total en uso** de la tarjeta, de sysfs;
//!   * la **RAM** que ocupa cada proceso, de `/proc`.
//!
//! Y el resto de la VRAM va en un solo bloque que dice lo que es: «caché KV,
//! sobrecarga del motor y lo que ocupen los demás programas». Es una resta de dos
//! números medidos, no una estimación.

use serde::Serialize;

/// Los parámetros de arranque de un modelo servido, leídos de su línea de comandos.
///
/// Se leen de la ORDEN con la que llama-swap arrancó el servidor, que es el dato
/// real: no se deduce del modelo ni se copia de la configuración del proxy.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Banderas {
    /// La ruta del `.gguf` (`--model` o `-m`).
    pub ruta: Option<String>,
    /// El contexto con el que se sirve (`--ctx` o `-c`), en tokens.
    pub contexto: Option<i64>,
    /// Cuántas capas están en la GPU (`-ngl`). `-1` o `99` significan todas.
    pub ngl: Option<i64>,
    /// La cuantización de la caché KV (`--cache-type-k`), si se le pasó.
    pub kv_k: Option<String>,
    pub kv_v: Option<String>,
    /// Si se activó flash attention (`-fa`): sin ella no se puede cuantizar la KV.
    pub flash_attention: bool,
    /// Todas las banderas que se pudieron reconocer, para poder enseñarlas.
    pub relevantes: Vec<String>,
}

/// Saca los parámetros que importan para la memoria de una línea de comandos.
///
/// Es una función PURA y por eso tiene pruebas: la línea de comandos es texto
/// libre escrito por el usuario en su `llama-swap.yaml`, así que aquí no se puede
/// suponer un orden ni una forma concreta.
pub fn parsear_banderas(cmd: &str) -> Banderas {
    let partes: Vec<&str> = cmd.split_whitespace().collect();
    let mut b = Banderas::default();
    let mut i = 0;
    while i < partes.len() {
        let p = partes[i];
        let siguiente = || partes.get(i + 1).and_then(|v| v.parse::<i64>().ok());
        let siguiente_texto = || partes.get(i + 1).map(|v| v.to_string());
        match p {
            "--model" | "-m" => {
                b.ruta = siguiente_texto();
                i += 2;
                continue;
            }
            "--ctx" | "-c" => {
                b.contexto = siguiente();
                if let Some(v) = siguiente() {
                    b.relevantes.push(format!("-c {v}"));
                }
                i += 2;
                continue;
            }
            "-ngl" | "--n-gpu-layers" => {
                b.ngl = siguiente();
                if let Some(v) = siguiente() {
                    b.relevantes.push(format!("-ngl {v}"));
                }
                i += 2;
                continue;
            }
            "--cache-type-k" | "-ctk" => {
                b.kv_k = siguiente_texto();
                if let Some(v) = siguiente_texto() {
                    b.relevantes.push(format!("-ctk {v}"));
                }
                i += 2;
                continue;
            }
            "--cache-type-v" | "-ctv" => {
                b.kv_v = siguiente_texto();
                if let Some(v) = siguiente_texto() {
                    b.relevantes.push(format!("-ctv {v}"));
                }
                i += 2;
                continue;
            }
            "-fa" | "--flash-attn" => {
                // Viene con valor (`-fa on`) o como bandera suelta.
                let valor = partes.get(i + 1).copied().unwrap_or("");
                b.flash_attention = valor == "on" || valor == "1" || valor == "true" || valor.is_empty();
                if b.flash_attention {
                    b.relevantes.push("-fa on".into());
                }
                i += if valor == "on" || valor == "1" || valor == "true" { 2 } else { 1 };
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    b
}

/// Un modelo servido ahora mismo, con lo que se sabe de su memoria.
#[derive(Debug, Clone, Serialize)]
pub struct ModeloCargado {
    /// El identificador con el que se le piden las cosas (el de llama-swap).
    pub id: String,
    /// El nombre con el que el proxy lo llama ("Modelo 8B (Q2_0 g64, rapido)").
    pub nombre: String,
    pub ruta: String,
    /// Los pesos: el tamaño del fichero en disco. `None` si no se pudo leer.
    pub pesos_gb: Option<f64>,
    pub contexto: Option<i64>,
    pub ngl: Option<i64>,
    /// La cuantización de la caché KV, si va cuantizada: es lo que permite servir
    /// contextos enormes sin comerse la VRAM.
    pub kv_quant: Option<String>,
    pub flash_attention: bool,
    /// Segundos que le quedan antes de que el proxy lo descargue solo.
    pub ttl_s: Option<i64>,
    /// Las banderas reconocidas, para poder enseñarlas tal cual.
    pub banderas: Vec<String>,
    /// La línea de comandos completa: es la prueba de todo lo de arriba.
    pub cmd: String,
}

/// La línea de comandos del proceso `llama-server` que sirve ese fichero.
///
/// Devuelve `None` si no se encuentra: puede estar arrancando, o el motor no ser
/// llama.cpp. No es un error, es que no se sabe.
fn cmd_de_proceso(ruta: &str) -> Option<String> {
    if ruta.is_empty() {
        return None;
    }
    crate::scan::ai_procs()
        .into_iter()
        .find(|p| p.cmd.contains(ruta) && p.cmd.contains("llama-server"))
        .map(|p| p.cmd)
}

/// El resumen de memoria de la GPU, tal como se enseña.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Memoria {
    pub modelos: Vec<ModeloCargado>,
    /// Suma de los pesos de los modelos cargados, en GB.
    pub pesos_gb: f64,
    /// VRAM en uso de la tarjeta, de sysfs. `None` si no hay GPU amdgpu.
    pub vram_usada_gb: Option<f64>,
    pub vram_total_gb: Option<f64>,
    /// Lo que NO son pesos: caché KV, sobrecarga del motor y el resto de programas.
    /// Es una RESTA de dos medidas, y se dice así.
    pub resto_gb: Option<f64>,
}

/// Lee los modelos servidos por un llama-swap y completa lo que se sabe.
///
/// Es `async` porque habla por HTTP con el proxy, pero todo lo demás (leer el
/// tamaño del fichero, resolver el proceso) es local.
pub async fn cargados(host: &str, puerto: u16, vram: Option<(f64, f64)>) -> Memoria {
    let mut out = Memoria::default();
    let Some(lista) = crate::servers::llama_swap_cargados(host, puerto).await else {
        // Sin llama-swap en marcha no hay modelos servidos que contar; la VRAM sí
        // se puede decir, porque sale de sysfs.
        if let Some((usada, total)) = vram {
            out.vram_usada_gb = Some(usada);
            out.vram_total_gb = Some(total);
            out.resto_gb = Some(usada);
        }
        return out;
    };

    for item in lista {
        let cmd = item.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = item.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let nombre = item
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or(&id)
            .to_string();
        let ttl_s = item.get("ttl").and_then(|v| v.as_i64());
        let b = parsear_banderas(&cmd);
        let ruta = b.ruta.clone().unwrap_or_default();
        // ── La línea de comandos EFECTIVA ──────────────────────────────────
        // La que da llama-swap es la que ÉL lanzó, y en este equipo eso es un
        // script envoltorio (`modelo-local-server.sh --port … --model … --ctx …`): el
        // contexto se ve, pero `-ngl`, `-fa` y `--cache-type-k` los pone el
        // script por dentro, así que ahí no están.
        //
        // El proceso `llama-server` que está sirviendo SÍ los lleva, y se puede
        // encontrar: es el que tiene la misma ruta de modelo en su línea de
        // comandos. Se usa la suya cuando aparece, y si no aparece se queda la de
        // llama-swap (con las banderas que sí traiga) — que es lo honesto: no se
        // supone lo que no se ve.
        let efectivo = cmd_de_proceso(&ruta).unwrap_or_else(|| cmd.clone());
        let b = if efectivo != cmd { parsear_banderas(&efectivo) } else { b };
        let pesos_gb = std::fs::metadata(&ruta).ok().map(|m| m.len() as f64 / 1e9);
        out.modelos.push(ModeloCargado {
            id,
            nombre,
            ruta,
            pesos_gb,
            contexto: b.contexto,
            ngl: b.ngl,
            kv_quant: b.kv_k.clone().or(b.kv_v.clone()),
            flash_attention: b.flash_attention,
            ttl_s,
            banderas: b.relevantes.clone(),
            cmd: efectivo,
        });
    }

    out.pesos_gb = out.modelos.iter().filter_map(|m| m.pesos_gb).sum();
    if let Some((usada, total)) = vram {
        out.vram_usada_gb = Some(usada);
        out.vram_total_gb = Some(total);
        out.resto_gb = Some((usada - out.pesos_gb).max(0.0));
    }
    out
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// La línea real con la que este equipo sirve el Modelo 8B (copiada de
    /// `llama-swap /running`): el contexto, la caché cuantizada y las capas.
    #[test]
    fn se_leen_las_banderas_de_una_linea_real() {
        let cmd = "/opt/llama.cpp/bin/llama-server -m /home/usuario/models/modelos/modelo-8b-Q2_0.gguf --host 127.0.0.1 --port 5803 -ngl 99 -fa on -c 65536 --temp 1.0 --jinja --parallel 1 --reasoning on --cache-type-k q4_0 --cache-type-v q4_0";
        let b = parsear_banderas(cmd);
        assert_eq!(b.ruta.as_deref(), Some("/home/usuario/models/modelos/modelo-8b-Q2_0.gguf"));
        assert_eq!(b.contexto, Some(65536));
        assert_eq!(b.ngl, Some(99));
        assert_eq!(b.kv_k.as_deref(), Some("q4_0"));
        assert_eq!(b.kv_v.as_deref(), Some("q4_0"));
        assert!(b.flash_attention);
        // Y las banderas se pueden enseñar tal cual.
        for esperado in ["-c 65536", "-ngl 99", "-ctk q4_0", "-ctv q4_0", "-fa on"] {
            assert!(b.relevantes.iter().any(|x| x == esperado), "falta {esperado}: {:?}", b.relevantes);
        }
    }

    /// La otra forma de escribirlas (la larga) tiene que dar lo mismo: en un
    /// `llama-swap.yaml` cada uno escribe lo que quiere.
    #[test]
    fn las_banderas_largas_dan_lo_mismo_que_las_cortas() {
        let corto = parsear_banderas("-m /x.gguf -c 32768 -ngl 20 --cache-type-k f16");
        let largo = parsear_banderas("--model /x.gguf --ctx 32768 --n-gpu-layers 20 --cache-type-k f16");
        assert_eq!(corto.ruta, largo.ruta);
        assert_eq!(corto.contexto, largo.contexto);
        assert_eq!(corto.ngl, largo.ngl);
        assert_eq!(corto.kv_k, largo.kv_k);
    }

    /// Una línea sin nada de esto no puede inventarse valores: todo queda vacío y
    /// la interfaz enseñará «—».
    #[test]
    fn sin_banderas_no_hay_datos_que_ensenar() {
        let b = parsear_banderas("/usr/bin/llama-server");
        assert_eq!(b, Banderas::default());
        assert!(b.relevantes.is_empty());
    }

    /// `-fa` puede venir como bandera suelta o con valor: las dos cuentan como
    /// activada, porque sin flash attention no se puede cuantizar la caché KV (y
    /// eso cambia cuánta VRAM se come el contexto).
    #[test]
    fn flash_attention_se_reconoce_con_y_sin_valor() {
        assert!(parsear_banderas("llama-server -fa on").flash_attention);
        assert!(parsear_banderas("llama-server -fa").flash_attention);
        assert!(!parsear_banderas("llama-server -fa off").flash_attention);
    }

    /// Un modelo sin cuantizar la caché no lleva `--cache-type-k`, y eso NO es un
    /// error: es que la caché va en su precisión normal, que ocupa mucho más.
    #[test]
    fn sin_cuantizar_la_cache_no_hay_bandera_pero_tampoco_invencion() {
        let b = parsear_banderas("llama-server -m /x.gguf -c 8192 -ngl 99");
        assert_eq!(b.kv_k, None);
        assert_eq!(b.kv_v, None);
        assert_eq!(b.contexto, Some(8192));
    }
}
