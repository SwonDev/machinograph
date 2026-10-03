//! Modo línea de comandos: `machinograph --cli …` sin abrir ninguna ventana.
//!
//! POR QUÉ EXISTE: la misma información que enseña el panel hace falta en un
//! script (una tarea programada, un cron, un `node_exporter`), y para eso abrir
//! una ventana no sirve. Kudu tiene su `--cli` y Magnitude su `magnitude serve`;
//! esto es el equivalente aquí, con dos reglas que se copian de Kudu porque están
//! bien pensadas:
//!
//! 1. **Con `--json`, stdout lleva SOLO el documento JSON.** Nada de líneas de
//!    progreso mezcladas: si hay avisos, van a stderr. Así se puede hacer
//!    `machinograph --cli estado --json | jq .` y funciona.
//! 2. **Sin `--cli` no cambia nada**: la app arranca con su ventana como siempre.
//!
//! Y una de esta casa: **nada de red**. Ningún comando habla con servicios
//! externos. La ÚNICA excepción es `provision --instalar`, que baja los binarios
//! que la app usa (llmfit, llama.cpp) de las releases oficiales del proyecto
//! cuando el usuario lo pide expresamente; `provision` a secas solo MIRA lo que ya
//! hay, sin tocar la red. Se dice aquí porque es una excepción de verdad.
use crate::almacen;
use crate::bases;
use crate::db;
use crate::inventario;
use crate::limpieza;
use crate::plataforma;
use crate::provision;
use crate::salud;
use serde::Serialize;

/* ── Los comandos ─────────────────────────────────────────────────────────── */

#[derive(Debug, PartialEq)]
pub enum Comando {
    Ayuda,
    Version,
    Estado { json: bool },
    Servidores { json: bool },
    Modelos { json: bool },
    Uso { json: bool, periodo: String },
    Analizar { json: bool, ruta: Option<String>, hijos: usize },
    Historial { json: bool, ruta: Option<String> },
    Grandes { json: bool, ruta: Option<String>, limite: usize },
    Duplicados { json: bool, ruta: Option<String>, min_bytes: u64 },
    Vacias { json: bool, ruta: Option<String> },
    Enlaces { json: bool, ruta: Option<String> },
    Buscar { json: bool, ruta: Option<String>, texto: String },
    Limpiar { json: bool, aplicar: bool, categorias: Vec<String> },
    Bases { json: bool, aplicar: bool },
    Borrar { json: bool, definitivo: bool, rutas: Vec<String> },
    Copias { json: bool },
    Papelera { json: bool, vaciar: bool },
    Actualizar { json: bool },
    Programar { json: bool },
    Seguridad { json: bool },
    Huellas { json: bool },
    Exclusiones {
        json: bool,
        anadir: Option<String>,
        quitar: Option<String>,
    },
    Metrica { json: bool },
    Provision { json: bool, instalar: bool },
    /// Autorreparación: `reparar` a `false` solo informa (y dice lo que se haría).
    Salud { json: bool, reparar: bool },
}

/// ¿El usuario ha pedido el modo CLI?
pub fn invocado() -> bool {
    std::env::args().skip(1).any(|a| a == "--cli" || a == "-c")
}

const AYUDA: &str = "\
Machinograph en la línea de comandos (no abre ventana)

USO
  machinograph --cli [comando] [opciones]

COMANDOS
  estado                     Qué está pasando en la máquina: CPU, memoria, discos, GPU y servidores
  servidores                 Los servidores de IA dados de alta y su estado
  modelos                    Los modelos que hay en disco, con lo que ocupa cada uno
  uso [--periodo hoy|todo]   Lo que se ha servido (tokens y velocidad)
  analizar [ruta]            Qué ocupa cada cosa dentro de una carpeta (como du --max-depth=1)
  historial [ruta]           Las medidas de disco guardadas de esa carpeta y, si hay dos, el
                             crecimiento (qué ha subido y qué ha bajado desde la anterior)
  grandes [ruta]             Los ficheros más grandes del árbol
  duplicados [ruta]          Ficheros repetidos, por contenido
  vacias [ruta]              Carpetas que no tienen ningún fichero
  enlaces [ruta]             Enlaces simbólicos rotos
  buscar <texto> [ruta]      Buscar por nombre dentro de una carpeta
  limpiar                    Qué basura se puede tirar (solo mira); con --aplicar, la tira
                             (las huellas de privacidad solo si usas --categoria privacidad)
  bases                      Las bases SQLite de tus aplicaciones: cuánto ocupan y cuánto
                             devolvería un VACUUM (mide; con --aplicar, compacta las libres)
  borrar <ruta>…             Manda a la papelera; con --definitivo, borra de verdad
  copias                     Las copias de seguridad de los ficheros que ha tocado Machinograph
  papelera                   Cuánto hay en la papelera; con --vaciar, la vacía
  actualizar                 Qué está desactualizado, según la herramienta de cada sistema
  programar                  La limpieza programada y cómo ponerla en el planificador del sistema
  seguridad                  Indicadores de compromiso (qué se ejecuta solo), con su prueba
  huellas                    Las huellas de tu actividad (historial, recientes, portapapeles)
  exclusiones                Qué está excluido (no se mide ni se borra); con --anadir/--quitar, lo cambia
  metricas                   Métricas en formato Prometheus (para node_exporter)
  provision                  Qué herramientas usa Machinograph y cuáles faltan; con --instalar, las baja
                             (llmfit y llama.cpp, desde sus releases oficiales; es lo único que usa red)
  salud                      Lo que la app puede arreglarse sola (puerto de la puerta, base del histórico,
                             arranque automático, configuración que escribió); con --reparar, lo arregla

OPCIONES
  --json                     Salida en JSON, y SOLO JSON, por stdout
  --hijos N                  Cuántos hijos enseñar en «analizar» (por defecto 20)
  --limite N                 Cuántos resultados enseñar (por defecto 50)
  --min-bytes N              Tamaño mínimo para «duplicados» (por defecto 1 MB)
  --categoria X              Repetible: limita «limpiar» a esa categoría
  --anadir PATRÓN            En «exclusiones»: añade una carpeta, un comodín o un nombre
  --quitar PATRÓN             En «exclusiones»: quita una exclusión
  --definitivo               En «borrar»: no pasa por la papelera
  --vaciar                   En «papelera»: la vacía (es lo que libera el espacio)
  --instalar                 En «provision»: baja lo que falte (es el único comando con red)
  --reparar                  En «salud»: arregla de verdad lo que esté roto (sin él, solo informa)
  --ayuda, -h                Esto
  --version, -v              La versión

Sin ruta, los comandos que la piden usan tu carpeta personal.";

/// Parser PURO (se prueba sin tocar el disco ni la máquina).
pub fn parsear(args: &[String]) -> Result<Comando, String> {
    let mut json = false;
    let mut aplicar = false;
    let mut instalar = false;
    let mut reparar = false;
    let mut definitivo = false;
    let mut hijos = 20usize;
    let mut limite = 50usize;
    let mut min_bytes = 1024 * 1024u64;
    let mut periodo = "hoy".to_string();
    let mut categorias: Vec<String> = Vec::new();
    let mut anadir: Option<String> = None;
    let mut quitar: Option<String> = None;
    let mut sueltos: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let valor = |i: usize| args.get(i + 1).cloned();
        match a.as_str() {
            "--cli" | "-c" => {}
            "--json" => json = true,
            // `--vaciar` es lo mismo que `--aplicar` para la papelera, pero se lee
            // mejor: la opción dice qué va a pasar de verdad.
            "--aplicar" | "--vaciar" => aplicar = true,
            "--instalar" => instalar = true,
            // Solo afecta a «salud»: es el permiso para escribir (apartar una base
            // rota, restaurar un fichero, reescribir el arranque). Sin él, solo mira.
            "--reparar" => reparar = true,
            "--definitivo" => definitivo = true,
            "--ayuda" | "-h" | "ayuda" => return Ok(Comando::Ayuda),
            "--version" | "-v" | "version" => return Ok(Comando::Version),
            "--hijos" => {
                hijos = valor(i).and_then(|v| v.parse().ok()).ok_or("--hijos necesita un número")?;
                i += 1;
            }
            "--limite" => {
                limite = valor(i).and_then(|v| v.parse().ok()).ok_or("--limite necesita un número")?;
                i += 1;
            }
            "--min-bytes" => {
                min_bytes = valor(i).and_then(|v| v.parse().ok()).ok_or("--min-bytes necesita un número")?;
                i += 1;
            }
            "--periodo" => {
                periodo = valor(i).ok_or("--periodo necesita hoy o todo")?;
                i += 1;
            }
            "--categoria" => {
                let c = valor(i).ok_or("--categoria necesita un nombre")?;
                categorias.push(c);
                i += 1;
            }
            "--anadir" | "--añadir" => {
                anadir = Some(valor(i).ok_or("--anadir necesita una ruta o un patrón")?);
                i += 1;
            }
            "--quitar" => {
                quitar = Some(valor(i).ok_or("--quitar necesita una ruta o un patrón")?);
                i += 1;
            }
            otro if otro.starts_with('-') => return Err(format!("opción desconocida: {otro}")),
            otro => sueltos.push(otro.to_string()),
        }
        i += 1;
    }

    let comando = sueltos.first().map(|s| s.as_str()).unwrap_or("estado");
    let resto: Vec<String> = sueltos.iter().skip(1).cloned().collect();
    let ruta = resto
        .iter()
        .find(|r| r.starts_with('/') || r.starts_with('~') || r.contains(std::path::MAIN_SEPARATOR))
        .cloned();

    Ok(match comando {
        "estado" => Comando::Estado { json },
        "servidores" => Comando::Servidores { json },
        "modelos" => Comando::Modelos { json },
        "uso" => Comando::Uso { json, periodo },
        "analizar" => Comando::Analizar { json, ruta, hijos },
        "historial" | "historico" | "histórico" | "crecimiento" => Comando::Historial { json, ruta },
        "grandes" => Comando::Grandes { json, ruta, limite },
        "duplicados" => Comando::Duplicados { json, ruta, min_bytes },
        "vacias" | "vacías" => Comando::Vacias { json, ruta },
        "enlaces" => Comando::Enlaces { json, ruta },
        "buscar" => {
            // El texto que se busca es el primer suelto que no sea la ruta.
            let texto = resto
                .iter()
                .find(|r| Some(*r) != ruta.as_ref())
                .cloned()
                .ok_or("«buscar» necesita el texto que buscar")?;
            Comando::Buscar { json, ruta, texto }
        }
        "limpiar" => Comando::Limpiar { json, aplicar, categorias },
        "bases" | "bases-de-datos" | "databases" => Comando::Bases { json, aplicar },
        "borrar" => {
            let rutas: Vec<String> = resto.iter().filter(|r| !r.starts_with('-')).cloned().collect();
            if rutas.is_empty() {
                return Err("«borrar» necesita al menos una ruta".into());
            }
            Comando::Borrar { json, definitivo, rutas }
        }
        "copias" => Comando::Copias { json },
        "papelera" => Comando::Papelera { json, vaciar: aplicar },
        "limpiar-papelera" => Comando::Papelera { json, vaciar: true },
        "actualizar" | "actualizaciones" => Comando::Actualizar { json },
        "programar" | "programacion" => Comando::Programar { json },
        "seguridad" | "escanear" => Comando::Seguridad { json },
        "huellas" | "privacidad" => Comando::Huellas { json },
        "exclusiones" | "excluir" => Comando::Exclusiones { json, anadir, quitar },
        "metricas" | "métricas" => Comando::Metrica { json },
        "provision" | "provisionar" => Comando::Provision { json, instalar },
        "salud" | "autorreparacion" | "autorreparación" => Comando::Salud { json, reparar },
        otro => return Err(format!("comando desconocido: {otro}. Prueba con --ayuda")),
    })
}

/* ── Salida ───────────────────────────────────────────────────────────────── */

/// Imprime en JSON (solo el documento) o en texto, y devuelve el código de salida.
fn salir<T: Serialize>(json: bool, valor: &T, texto: String) -> i32 {
    if json {
        match serde_json::to_string_pretty(valor) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("no se pudo serializar la salida: {e}");
                return 1;
            }
        }
    } else {
        println!("{texto}");
    }
    0
}

fn error(msg: &str) -> i32 {
    eprintln!("{msg}");
    2
}

/* ── Ejecución ────────────────────────────────────────────────────────────── */

pub fn ejecutar(args: &[String]) -> i32 {
    let comando = match parsear(args) {
        Ok(c) => c,
        Err(e) => return error(&e),
    };
    match comando {
        Comando::Ayuda => {
            println!("{AYUDA}");
            0
        }
        Comando::Version => {
            println!("machinograph {} ({})", env!("CARGO_PKG_VERSION"), plataforma::nombre_so());
            0
        }
        otro => match ejecutar_comando(otro) {
            Ok(codigo) => codigo,
            Err(e) => error(&e),
        },
    }
}

fn ejecutar_comando(c: Comando) -> Result<i32, String> {
    match c {
        Comando::Estado { json } | Comando::Servidores { json } | Comando::Metrica { json } => {
            // Estos tres necesitan la foto completa (sondea servidores por HTTP),
            // así que se monta un runtime de Tokio solo para eso: fuera del host de
            // Tauri no hay runtime propio.
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("no se pudo preparar el sondeo: {e}"))?;
            let s = rt.block_on(crate::types::Snapshot::build());
            Ok(match c {
                Comando::Estado { json } => estado(json, &s),
                Comando::Servidores { json } => servidores(json, &s),
                _ => metrica(json, &s),
            })
        }
        Comando::Modelos { json } => {
            let modelos = inventario::inventario();
            let total: u64 = modelos.iter().map(|m| m.tamano_bytes.max(0) as u64).sum();
            let mut texto = format!(
                "{} modelos, {} en total\n",
                modelos.len(),
                almacen::legible(total)
            );
            let mut ordenados = modelos.clone();
            ordenados.sort_by(|a, b| b.tamano_bytes.cmp(&a.tamano_bytes));
            for m in ordenados.iter().take(50) {
                texto.push_str(&format!(
                    "  {:>10}  {:<12} {}\n",
                    almacen::legible(m.tamano_bytes.max(0) as u64),
                    m.tipo,
                    m.nombre
                ));
            }
            Ok(salir(json, &modelos, texto))
        }
        Comando::Uso { json, periodo } => {
            let desde = if periodo == "todo" {
                0
            } else {
                use chrono::{Local, TimeZone};
                let hoy = Local::now().date_naive();
                Local
                    .from_local_datetime(&hoy.and_hms_opt(0, 0, 0).unwrap())
                    .single()
                    .map(|d| d.timestamp())
                    .unwrap_or(0)
            };
            let resumen = db::uso_resumen(desde, None).map_err(|e| e.to_string())?;
            let por_modelo = db::uso_por_modelo(desde).map_err(|e| e.to_string())?;
            let texto = format!(
                "{} peticiones · {} tokens de entrada · {} de salida · {} de caché",
                resumen.peticiones, resumen.prompt_tokens, resumen.completion_tokens, resumen.cached_tokens
            );
            Ok(salir(
                json,
                &serde_json::json!({ "periodo": periodo, "resumen": resumen, "por_modelo": por_modelo }),
                texto,
            ))
        }
        Comando::Analizar { json, ruta, hijos } => {
            let raiz = raiz_o_home(ruta);
            // Las exclusiones se cargan UNA vez al empezar el recorrido.
            let filtro = almacen::Filtro::reales();
            let a = almacen::arbol_con(&raiz, hijos, &filtro)?;
            let mut texto = format!(
                "{} → {} ({} ficheros, {} carpetas, {} ms)\n",
                a.ruta,
                almacen::legible(a.bytes),
                a.ficheros,
                a.dirs,
                a.ms
            );
            for h in &a.hijos {
                texto.push_str(&format!(
                    "  {:>10}  {:<4} {}\n",
                    almacen::legible(h.bytes),
                    if h.es_dir { "dir" } else { "file" },
                    h.nombre
                ));
            }
            if a.resto_n > 0 {
                texto.push_str(&format!(
                    "  … y {} más que suman {}\n",
                    a.resto_n,
                    almacen::legible(a.resto_bytes)
                ));
            }
            if a.truncado {
                texto.push_str("  (el recorrido se cortó por presupuesto: el total puede quedarse corto)\n");
            }
            if !a.excluidos.is_empty() {
                texto.push_str(&format!(
                    "  ({} exclusión(es) han dejado fuera parte del análisis: {})\n",
                    a.excluidos.len(),
                    a.excluidos.join(" · ")
                ));
            }
            Ok(salir(json, &a, texto))
        }
        Comando::Historial { json, ruta } => {
            let raiz = raiz_o_home(ruta);
            let activo = db::setting_int("historial_activo", 1) != 0;
            let instantaneas = db::instantaneas(&raiz, 30).map_err(|e| e.to_string())?;
            let crecimiento = db::crecimiento(&raiz).map_err(|e| e.to_string())?;

            let momento = |ts: i64| {
                use chrono::{Local, TimeZone};
                Local
                    .timestamp_opt(ts, 0)
                    .single()
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| ts.to_string())
            };

            let mut texto = format!(
                "Histórico de {raiz} · un punto por día, retención {} días · medida diaria automática {}\n",
                db::HISTORIAL_DIAS,
                if activo { "activada" } else { "apagada" }
            );
            if instantaneas.is_empty() {
                texto.push_str("  (sin medidas guardadas todavía)\n");
            }
            for i in &instantaneas {
                texto.push_str(&format!(
                    "  {}  {:>10}  {} ficheros{}",
                    momento(i.ts),
                    almacen::legible(i.bytes),
                    i.ficheros,
                    if i.es_parcial() { "  (medida parcial)" } else { "" }
                ));
                if i.resto_n > 0 {
                    texto.push_str(&format!("  (+{} hijos sin listar)", i.resto_n));
                }
                texto.push('\n');
            }
            if let Some(c) = &crecimiento {
                let signo = if c.delta_bytes >= 0 { "+" } else { "-" };
                texto.push_str(&format!(
                    "Crecimiento desde {} hasta {} ({} h): {}{}",
                    momento(c.antes_ts),
                    momento(c.ahora_ts),
                    (c.segundos as f64 / 3600.0).round() as i64,
                    signo,
                    almacen::legible(c.delta_bytes.unsigned_abs())
                ));
                if c.antes_bytes > 0 {
                    let pct = c.delta_bytes as f64 / c.antes_bytes as f64 * 100.0;
                    texto.push_str(&format!(" ({pct:+.1} %)"));
                }
                texto.push('\n');
                if let Some(m) = &c.motivo {
                    texto.push_str(&format!("  (comparación parcial: {m})\n"));
                }
                if c.hijos_parcial {
                    texto.push_str(
                        "  (alguna medida no guardó todos los hijos: puede haber altas o bajas que solo sean cambios de puesto)\n",
                    );
                }
                for h in c.hijos.iter().take(10) {
                    let hs = if h.delta >= 0 { "+" } else { "-" };
                    texto.push_str(&format!(
                        "  {}{}  {}{}\n",
                        hs,
                        almacen::legible(h.delta.unsigned_abs()),
                        h.nombre,
                        if h.nuevo {
                            "  (nuevo)"
                        } else if h.desaparecido {
                            "  (desaparecido)"
                        } else {
                            ""
                        }
                    ));
                }
            }
            Ok(salir(
                json,
                &serde_json::json!({
                    "ruta": raiz,
                    "retencion_dias": db::HISTORIAL_DIAS,
                    "activo": activo,
                    "instantaneas": instantaneas,
                    "crecimiento": crecimiento,
                }),
                texto,
            ))
        }
        Comando::Grandes { json, ruta, limite } => {
            let raiz = raiz_o_home(ruta);
            let filtro = almacen::Filtro::reales();
            let v = almacen::grandes_con(&raiz, limite, &filtro)?;
            let texto = v
                .iter()
                .map(|f| format!("  {:>10}  {}", almacen::legible(f.bytes), f.ruta))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(salir(json, &v, texto))
        }
        Comando::Duplicados { json, ruta, min_bytes } => {
            let raiz = raiz_o_home(ruta);
            let filtro = almacen::Filtro::reales();
            let v = almacen::duplicados_con(&raiz, min_bytes, 500, &filtro)?;
            let total: u64 = v.iter().map(|d| d.desperdicio).sum();
            let mut texto = format!(
                "{} grupos de ficheros repetidos · se pueden liberar {}\n",
                v.len(),
                almacen::legible(total)
            );
            for d in v.iter().take(50) {
                texto.push_str(&format!(
                    "  {:>10} ×{}  {}\n",
                    almacen::legible(d.bytes),
                    d.rutas.len(),
                    d.rutas.first().cloned().unwrap_or_default()
                ));
            }
            Ok(salir(json, &v, texto))
        }
        Comando::Vacias { json, ruta } => {
            let raiz = raiz_o_home(ruta);
            let filtro = almacen::Filtro::reales();
            let v = almacen::vacias_con(&raiz, 1000, &filtro)?;
            let texto = format!("{} carpetas sin ningún fichero\n{}", v.len(), v.join("\n"));
            Ok(salir(json, &v, texto))
        }
        Comando::Enlaces { json, ruta } => {
            let raiz = raiz_o_home(ruta);
            let filtro = almacen::Filtro::reales();
            let v = almacen::enlaces_rotos_con(&raiz, 1000, &filtro)?;
            let mut texto = format!("{} enlaces rotos\n", v.len());
            for e in &v {
                texto.push_str(&format!("  {} → {}\n", e.ruta, e.destino));
            }
            if cfg!(target_os = "windows") {
                texto.push_str(
                    "  (en Windows los accesos directos son ficheros .lnk y comprobarlos necesita la API del shell: aquí no se revisan)\n",
                );
            }
            Ok(salir(json, &v, texto))
        }
        Comando::Buscar { json, ruta, texto } => {
            let raiz = raiz_o_home(ruta);
            let filtro = almacen::Filtro::reales();
            let v = almacen::buscar_con(&raiz, &texto, 200, &filtro)?;
            let salida = v
                .iter()
                .map(|c| {
                    format!(
                        "  {:>10}  {}",
                        c.bytes.map(|b| almacen::legible(b)).unwrap_or_else(|| "—".into()),
                        c.ruta
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(salir(json, &v, salida))
        }
        Comando::Limpiar { json, aplicar, categorias } => {
            let cats = if categorias.is_empty() { None } else { Some(categorias) };
            if aplicar {
                let e = limpieza::escanear(cats.clone())?;
                // Solo se limpia lo que se puede limpiar desde aquí (lo que necesita
                // root o tiene comando propio se salta, y `limpiar` lo dice), y las
                // HUELLAS solo si se han pedido por su nombre. Ver
                // `limpieza::ids_a_limpiar`, que es donde está la regla y su prueba.
                let pide_privacidad = cats
                    .as_ref()
                    .map(|c| c.iter().any(|x| x == "privacidad"))
                    .unwrap_or(false);
                let ids = limpieza::ids_a_limpiar(&e.objetivos, pide_privacidad);
                if ids.is_empty() {
                    return Ok(salir(json, &e, "No hay nada que limpiar.".into()));
                }
                let inf = limpieza::limpiar(&ids)?;
                return Ok(salir(json, &inf, inf.mensaje.clone()));
            }
            let e = limpieza::escanear(cats)?;
            let mut texto = format!(
                "{} objetivos · se pueden liberar {} ({} elementos)\n",
                e.objetivos.len(),
                almacen::legible(e.bytes),
                e.elementos
            );
            for o in &e.objetivos {
                let nota = if o.root {
                    format!(" (necesita root: {})", o.comando.clone().unwrap_or_default())
                } else if let Some(c) = &o.comando {
                    format!(" (con su comando: {c})")
                } else {
                    String::new()
                };
                texto.push_str(&format!(
                    "  {:>10} {:>7} elem  {}{}\n",
                    almacen::legible(o.bytes),
                    o.elementos,
                    o.subcategoria,
                    nota
                ));
            }
            Ok(salir(json, &e, texto))
        }
        Comando::Bases { json, aplicar } => {
            if aplicar {
                // Compacta las que tengan algo que recuperar; de las bloqueadas se
                // informa con el proceso que las tiene abiertas, sin tocarlas.
                let inf = bases::compactar_lote(&[]);
                let mut texto = format!("{}\n", inf.mensaje);
                for r in &inf.resultados {
                    let cifra = r
                        .liberado
                        .map(almacen::legible)
                        .unwrap_or_else(|| "—".to_string());
                    texto.push_str(&format!(
                        "  {:<12} {:<20} {:>10}  {}\n",
                        if r.ok { "compactada" } else { "sin tocar" },
                        r.app,
                        cifra,
                        r.ruta
                    ));
                    if !r.bloqueantes.is_empty() {
                        texto.push_str(&format!(
                            "               la tienen abierta: {}\n",
                            r.bloqueantes.join(", ")
                        ));
                    }
                }
                return Ok(salir(json, &inf, texto));
            }
            let l = bases::listar();
            let mut texto = format!(
                "{} bases · se recuperarían {} con VACUUM ({} medidas, {} bloqueadas, {} sin permiso)\n",
                l.total,
                almacen::legible(l.bytes_recuperables),
                l.medidas,
                l.bloqueadas,
                l.sin_permiso
            );
            for b in &l.bases {
                // Una base que no se pudo medir NO lleva un 0: lleva «—».
                let cifra = b.recuperable.map(almacen::legible).unwrap_or_else(|| "—".to_string());
                let estado = match b.estado {
                    bases::Estado::Ok => String::new(),
                    bases::Estado::Bloqueada => "bloqueada ".to_string(),
                    bases::Estado::SinPermiso => "sin permiso ".to_string(),
                    bases::Estado::Error => "no legible ".to_string(),
                };
                texto.push_str(&format!(
                    "  {:>10}  {:<20} {:<16} {}  {}{}\n",
                    cifra,
                    b.app,
                    b.perfil.clone().unwrap_or_else(|| "-".to_string()),
                    b.ruta,
                    estado,
                    b.nota.clone().unwrap_or_default()
                ));
            }
            if let Some(n) = &l.nota {
                texto.push_str(&format!("  {n}\n"));
            }
            if !l.sin_traducir.is_empty() {
                texto.push_str(&format!(
                    "  {} objetivos de Kudu no se pueden resolver en este sistema: {}\n",
                    l.sin_traducir.len(),
                    l.sin_traducir.join(", ")
                ));
            }
            Ok(salir(json, &l, texto))
        }
        Comando::Borrar { json, definitivo, rutas } => {
            let m = almacen::borrar(&rutas, definitivo)?;
            Ok(salir(json, &serde_json::json!({ "mensaje": m }), m))
        }
        Comando::Copias { json } => {
            let v = crate::copias::listar(200)?;
            let texto = v
                .iter()
                .map(|c| {
                    format!(
                        "  {}  {}  {}",
                        if c.existe { "ok " } else { "no " },
                        c.ruta_original,
                        c.motivo
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(salir(json, &v, texto))
        }
        Comando::Papelera { json, vaciar } => {
            let r = plataforma::rutas();
            let resumen = plataforma::papelera::resumen();
            if vaciar {
                let m = plataforma::papelera::vaciar()?;
                let salida = serde_json::json!({ "mensaje": m, "ruta": r.papelera().to_string_lossy() });
                return Ok(salir(json, &salida, m));
            }
            match resumen {
                Some((elementos, bytes)) => {
                    let texto = format!(
                        "Papelera ({}): {} elementos, {}\n\
                         (ese espacio NO se libera hasta vaciarla: `machinograph --cli papelera --vaciar`)",
                        r.papelera().display(),
                        elementos,
                        almacen::legible(bytes)
                    );
                    Ok(salir(
                        json,
                        &serde_json::json!({
                            "ruta": r.papelera().to_string_lossy(),
                            "elementos": elementos,
                            "bytes": bytes
                        }),
                        texto,
                    ))
                }
                None => Ok(salir(
                    json,
                    &serde_json::Value::Null,
                    "Este sistema no deja contar la papelera desde aquí.".into(),
                )),
            }
        }
        Comando::Actualizar { json } => {
            let fuentes = crate::actualizar::comprobar();
            let pendientes: usize = fuentes.iter().map(|f| f.actualizaciones.len()).sum();
            let mut texto = format!("{pendientes} actualizaciones pendientes\n");
            for f in &fuentes {
                if !f.disponible {
                    continue;
                }
                if f.actualizaciones.is_empty() {
                    texto.push_str(&format!("  {:<28} al día\n", f.nombre));
                } else {
                    texto.push_str(&format!(
                        "  {:<28} {} pendientes — se aplican con: {}\n",
                        f.nombre,
                        f.actualizaciones.len(),
                        f.comando_aplicar
                    ));
                    for a in f.actualizaciones.iter().take(10) {
                        texto.push_str(&format!("      {a}\n"));
                    }
                }
                // El aviso de la herramienta se enseña SIEMPRE (también cuando dice
                // que está al día): es su letra pequeña, no nuestra opinión.
                if let Some(n) = &f.nota {
                    texto.push_str(&format!("      (aviso de la herramienta: {n})\n"));
                }
                if let Some(e) = &f.error {
                    texto.push_str(&format!("      no se pudo comprobar: {e}\n"));
                }
            }
            Ok(salir(json, &fuentes, texto))
        }
        Comando::Programar { json } => {
            let p = db::programacion().map_err(|e| e.to_string())?;
            let recetas = crate::programar::recetas(&p);
            let mut texto = if p.activa {
                format!(
                    "Limpieza programada a las {:02}:{:02} ({} categorías). Mide y avisa; NO borra.\n",
                    p.hora,
                    p.minuto,
                    if p.categorias.is_empty() { "todas las".to_string() } else { p.categorias.len().to_string() }
                )
            } else {
                "Limpieza programada: desactivada\n".to_string()
            };
            if let Some(u) = &p.ultima {
                texto.push_str(&format!("Última ejecución: {u}\n"));
            }
            for r in &recetas {
                texto.push_str(&format!("\n[{}] {}\n{}\n{}\n", r.titulo, r.destino, r.contenido, r.instrucciones));
            }
            Ok(salir(
                json,
                &serde_json::json!({ "programacion": p, "recetas": recetas }),
                texto,
            ))
        }
        Comando::Seguridad { json } => {
            let hallazgos = crate::seguridad::revisar();
            let mut texto = String::new();
            for h in &hallazgos {
                let marca = match h.veredicto {
                    crate::seguridad::Veredicto::Ok => "ok      ",
                    crate::seguridad::Veredicto::Aviso => "aviso   ",
                    crate::seguridad::Veredicto::Problema => "PROBLEMA",
                    crate::seguridad::Veredicto::Desconocido => "?       ",
                };
                texto.push_str(&format!("  {marca}  {}\n      {}\n", h.titulo, h.detalle));
                if let Some(r) = &h.remedio {
                    texto.push_str(&format!("      → {r}\n"));
                }
            }
            texto.push_str(
                "\n(esto NO es un antivirus: mira los sitios donde se esconde la persistencia, no dentro de los binarios)\n",
            );
            Ok(salir(json, &hallazgos, texto))
        }
        Comando::Huellas { json } => {
            let e = limpieza::escanear(Some(vec!["privacidad".to_string()]))?;
            let mut texto = format!("{} huellas de tu actividad\n", e.objetivos.len());
            for o in &e.objetivos {
                texto.push_str(&format!(
                    "  {:>10}  {:<32} {}\n",
                    almacen::legible(o.bytes),
                    o.subcategoria,
                    o.rutas.first().cloned().unwrap_or_default()
                ));
            }
            texto.push_str(
                "\nPara borrarlas: `machinograph --cli limpiar --aplicar --categoria privacidad`\n\
                 (no se borran con `--aplicar` a secas, a propósito: no se recuperan)\n",
            );
            Ok(salir(json, &e, texto))
        }
        Comando::Exclusiones { json, anadir, quitar } => {
            if let Some(p) = anadir {
                let m = crate::exclusiones::anadir(&p)?;
                return Ok(salir(json, &serde_json::json!({ "mensaje": m }), m));
            }
            if let Some(p) = quitar {
                let m = crate::exclusiones::quitar(&p)?;
                return Ok(salir(json, &serde_json::json!({ "mensaje": m }), m));
            }
            let vigentes = crate::exclusiones::vigentes();
            let mut texto = if vigentes.is_empty() {
                "No hay ninguna exclusión. Se mide (y se puede borrar) todo lo que permite la lista blanca.\n".to_string()
            } else {
                format!("{} exclusión(es)\n", vigentes.len())
            };
            for v in &vigentes {
                texto.push_str(&format!("  {}\n", v.descripcion()));
            }
            texto.push_str(
                "\nSe aplican al analizador de disco, a la limpieza y al borrado.\n\
                 Un patrón puede ser una carpeta (${HOME}/VMs), un comodín (*.iso) o un\n\
                 nombre que valga para cualquier carpeta (node_modules).\n",
            );
            Ok(salir(
                json,
                &vigentes
                    .iter()
                    .map(|v| {
                        serde_json::json!({
                            "patron": v.patron,
                            "descripcion": v.descripcion(),
                            "resuelta": v.base.as_ref().map(|b| b.to_string_lossy().to_string()),
                        })
                    })
                    .collect::<Vec<_>>(),
                texto,
            ))
        }
        Comando::Provision { json, instalar } => {
            if instalar {
                // Único comando con red, y solo cuando se pide con --instalar. El
                // progreso va a stderr para no mezclarlo con el JSON de stdout.
                let resultados = provision::instalar_bloqueante(Vec::new(), false, |l| {
                    eprintln!("{l}");
                });
                if json {
                    let filas: Vec<serde_json::Value> = resultados
                        .iter()
                        .map(|(n, r)| {
                            serde_json::json!({
                                "herramienta": n,
                                "ok": r.is_ok(),
                                "detalle": match r {
                                    Ok(m) => m.clone(),
                                    Err(e) => e.clone(),
                                },
                            })
                        })
                        .collect();
                    return Ok(salir(true, &filas, String::new()));
                }
                let mut texto = String::new();
                for (n, r) in &resultados {
                    match r {
                        Ok(m) => texto.push_str(&format!("  {n}: {m}\n")),
                        Err(e) => texto.push_str(&format!("  {n}: FALLÓ: {e}\n")),
                    }
                }
                if texto.is_empty() {
                    texto.push_str("No faltaba nada por instalar.\n");
                }
                print!("{texto}");
                return Ok(0);
            }

            // Sin --instalar solo se MIRA: ni red ni escritura.
            let estados = provision::estados();
            let mut texto = String::new();
            for e in &estados {
                let marca = match e.estado {
                    provision::Estado::Listo => "listo",
                    provision::Estado::Falta => "falta",
                    provision::Estado::Descargando => "descargando",
                    provision::Estado::Roto => "roto",
                    provision::Estado::NoInstalable => "no instalable",
                };
                texto.push_str(&format!(
                    "  {:<28} {:<14} {} {}\n",
                    e.nombre,
                    marca,
                    e.version.clone().unwrap_or_else(|| "—".into()),
                    e.ruta.clone().unwrap_or_else(|| e.origen.clone())
                ));
                if let Some(c) = &e.comando_manual {
                    // Solo cuando FALTA: si ya está, el comando de instalación es
                    // ruido (no hay nada que instalar).
                    if e.estado == provision::Estado::NoInstalable {
                        texto.push_str(&format!("      se instala a mano: {c}\n"));
                    }
                }
            }
            let auto = provision::auto_activo();
            texto.push_str(&format!(
                "\nInstalación automática: {}.\n\
                 Con --instalar, Machinograph baja lo que falte (llmfit y llama.cpp) desde sus\n\
                 releases oficiales, sin permisos de administrador.\n",
                if auto { "activada" } else { "apagada" }
            ));
            let salida = serde_json::json!({
                "auto_provision": auto,
                "en_curso": provision::en_curso(),
                "herramientas": estados,
            });
            Ok(salir(json, &salida, texto))
        }
        Comando::Salud { json, reparar } => {
            // `revisar` es asíncrona (le da un momento a la puerta para atar el
            // puerto antes de decir que no escucha) y el CLI no tiene runtime
            // todavía: se crea uno propio, que es el único de este proceso.
            let revision = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("no se pudo crear el runtime: {e}"))?
                .block_on(salud::revisar(reparar));
            let mut texto = String::new();
            for c in &revision.comprobaciones {
                let marca = match c.estado {
                    salud::EstadoSalud::Correcto => "ok       ",
                    salud::EstadoSalud::Reparado => "REPARADO ",
                    salud::EstadoSalud::NoSePudo => "NO SE PUDO",
                };
                texto.push_str(&format!("{marca}  {}: {}\n", c.titulo, c.detalle));
                if let Some(a) = &c.como_arreglarlo {
                    texto.push_str(&format!("           qué hacer: {a}\n"));
                }
            }
            texto.push_str(&revision.resumen);
            Ok(salir(json, &revision, texto))
        }
        Comando::Ayuda | Comando::Version => unreachable!("se resuelven antes"),
    }
}

fn raiz_o_home(ruta: Option<String>) -> String {
    match ruta {
        Some(r) if !r.trim().is_empty() => {
            // `~` al principio es lo que escribe cualquiera en una terminal.
            if let Some(resto) = r.strip_prefix("~/") {
                plataforma::rutas().home.join(resto).to_string_lossy().to_string()
            } else {
                r
            }
        }
        _ => plataforma::rutas().home.to_string_lossy().to_string(),
    }
}

/* ── Informes ─────────────────────────────────────────────────────────────── */

fn estado(json: bool, s: &crate::types::Snapshot) -> i32 {
    let g = s.gpu.first();
    let servidores_activos = s.servers.iter().filter(|x| x.state == "active").count();
    let texto = format!(
        "{} · CPU {:.1} % de {} núcleos · memoria {:.1} % ({:.0} de {:.0} MB)\n\
         disco {} {:.1} % ({:.0} de {:.0} GB libres de {:.0})\n\
         {} servidor(es) activo(s) · {} proceso(s) de IA\n{}",
        plataforma::nombre_so(),
        s.system.cpu_pct,
        s.system.cores,
        s.system.mem.pct,
        s.system.mem.used_mb,
        s.system.mem.total_mb,
        s.disk.mount,
        s.disk.pct,
        s.disk.free_gb,
        s.disk.used_gb,
        s.disk.total_gb,
        servidores_activos,
        s.ai_procs.len(),
        g.map(|g| format!("GPU: {} · {:.0} % · {:.0}/{:.0} MB\n", g.name, g.util, g.mem_used_mb, g.mem_total_mb))
            .unwrap_or_default()
    );
    let salida = serde_json::json!({
        "so": s.so,
        "cpu_pct": s.system.cpu_pct,
        "cores": s.system.cores,
        "mem": s.system.mem,
        "swap": s.system.swap,
        "disco": s.disk,
        "gpu": s.gpu,
        "servidores_activos": servidores_activos,
        "procesos_ia": s.ai_procs,
        "uptime_secs": s.uptime_secs,
    });
    salir(json, &salida, texto)
}

fn servidores(json: bool, s: &crate::types::Snapshot) -> i32 {
    let mut texto = String::new();
    for sv in &s.servers {
        let modelos = sv
            .models
            .iter()
            .filter(|m| m.state == "loaded")
            .map(|m| m.label.clone())
            .collect::<Vec<_>>()
            .join(", ");
        texto.push_str(&format!(
            "  {:<8} {:<24} {:<10} {}{}\n",
            sv.state,
            sv.name,
            format!(":{}", sv.port),
            if modelos.is_empty() { String::new() } else { format!("cargados: {modelos}") },
            sv.error.clone().map(|e| format!(" · {e}")).unwrap_or_default()
        ));
    }
    if s.servers.is_empty() {
        texto.push_str("  No hay ningún servidor dado de alta.\n");
    }
    salir(json, &s.servers, texto)
}

/// Métricas en formato Prometheus, para el colector de texto de `node_exporter`.
///
/// Es una salida de texto (no JSON) porque su destino es un fichero `.prom`; con
/// `--json` se devuelven los mismos números como objetos, por si se quieren
/// procesar de otra forma.
fn metrica(json: bool, s: &crate::types::Snapshot) -> i32 {
    let mut lineas: Vec<String> = Vec::new();
    let mut gauge = |nombre: &str, ayuda: &str, valor: f64| {
        lineas.push(format!("# HELP {nombre} {ayuda}"));
        lineas.push(format!("# TYPE {nombre} gauge"));
        lineas.push(format!("{nombre} {valor}"));
    };
    gauge("machinograph_cpu_pct", "Uso de CPU en porcentaje", s.system.cpu_pct);
    gauge("machinograph_mem_pct", "Uso de memoria en porcentaje", s.system.mem.pct);
    gauge("machinograph_mem_used_mb", "Memoria en uso (MB)", s.system.mem.used_mb);
    gauge("machinograph_mem_total_mb", "Memoria total (MB)", s.system.mem.total_mb);
    gauge("machinograph_disk_pct", "Uso del disco del usuario en porcentaje", s.disk.pct);
    gauge("machinograph_disk_free_gb", "Espacio libre en el disco del usuario (GB)", s.disk.free_gb);
    gauge("machinograph_servidores_activos", "Servidores de IA en marcha", s.servers.iter().filter(|x| x.state == "active").count() as f64);
    gauge("machinograph_procesos_ia", "Procesos de IA detectados", s.ai_procs.len() as f64);
    for g in &s.gpu {
        let etiqueta = format!("{{gpu=\"{}\"}}", g.name.replace('"', ""));
        gauge(&format!("machinograph_gpu_util{etiqueta}"), "Uso de la GPU en porcentaje", g.util);
        gauge(&format!("machinograph_gpu_mem_used_mb{etiqueta}"), "VRAM en uso (MB)", g.mem_used_mb);
        gauge(&format!("machinograph_gpu_mem_total_mb{etiqueta}"), "VRAM total (MB)", g.mem_total_mb);
        if let Some(t) = g.temp_c {
            gauge(&format!("machinograph_gpu_temp_c{etiqueta}"), "Temperatura de la GPU (°C)", t);
        }
    }
    // Los discos, uno por punto de montaje.
    for d in plataforma::discos() {
        let etiqueta = format!("{{punto=\"{}\"}}", d.punto.replace('"', ""));
        gauge(&format!("machinograph_disco_uso_pct{etiqueta}"), "Uso de cada disco en porcentaje", d.uso_pct);
        gauge(&format!("machinograph_disco_libre_bytes{etiqueta}"), "Bytes libres de cada disco", d.libre as f64);
    }
    if json {
        let salida = serde_json::json!({
            "cpu_pct": s.system.cpu_pct,
            "mem_pct": s.system.mem.pct,
            "mem_used_mb": s.system.mem.used_mb,
            "mem_total_mb": s.system.mem.total_mb,
            "disk_pct": s.disk.pct,
            "disk_free_gb": s.disk.free_gb,
            "servidores_activos": s.servers.iter().filter(|x| x.state == "active").count(),
            "procesos_ia": s.ai_procs.len(),
            "gpu": s.gpu,
            "discos": plataforma::discos().into_iter().map(|d| serde_json::json!({
                "punto": d.punto, "total": d.total, "libre": d.libre, "uso_pct": d.uso_pct
            })).collect::<Vec<_>>(),
        });
        return salir(true, &salida, String::new());
    }
    println!("{}", lineas.join("\n"));
    0
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn sin_comando_enseña_el_estado() {
        assert_eq!(parsear(&args("--cli")).unwrap(), Comando::Estado { json: false });
        assert_eq!(parsear(&args("--cli --json")).unwrap(), Comando::Estado { json: true });
    }

    #[test]
    fn lee_las_opciones_con_su_valor() {
        assert_eq!(
            parsear(&args("--cli analizar /tmp --hijos 5 --json")).unwrap(),
            Comando::Analizar { json: true, ruta: Some("/tmp".into()), hijos: 5 }
        );
        assert_eq!(
            parsear(&args("--cli duplicados /var --min-bytes 4096")).unwrap(),
            Comando::Duplicados { json: false, ruta: Some("/var".into()), min_bytes: 4096 }
        );
        assert_eq!(
            parsear(&args("--cli limpiar --categoria apps --categoria sistema --aplicar")).unwrap(),
            Comando::Limpiar { json: false, aplicar: true, categorias: vec!["apps".into(), "sistema".into()] }
        );
        assert_eq!(
            parsear(&args("--cli uso --periodo todo")).unwrap(),
            Comando::Uso { json: false, periodo: "todo".into() }
        );
    }

    #[test]
    fn buscar_lleva_el_texto_y_la_ruta_cada_uno_a_su_sitio() {
        assert_eq!(
            parsear(&args("--cli buscar .gguf /home/alguien")).unwrap(),
            Comando::Buscar { json: false, ruta: Some("/home/alguien".into()), texto: ".gguf".into() }
        );
        // Sin ruta, el texto sigue siendo el que no es ruta.
        assert_eq!(
            parsear(&args("--cli buscar modelo.onnx")).unwrap(),
            Comando::Buscar { json: false, ruta: None, texto: "modelo.onnx".into() }
        );
    }

    #[test]
    fn bases_lista_y_compacta_con_aplicar() {
        assert_eq!(parsear(&args("--cli bases")).unwrap(), Comando::Bases { json: false, aplicar: false });
        assert_eq!(
            parsear(&args("--cli bases --json")).unwrap(),
            Comando::Bases { json: true, aplicar: false }
        );
        assert_eq!(
            parsear(&args("--cli bases --aplicar")).unwrap(),
            Comando::Bases { json: false, aplicar: true }
        );
    }

    #[test]
    fn borrar_necesita_rutas() {
        assert!(parsear(&args("--cli borrar")).is_err());
        assert_eq!(
            parsear(&args("--cli borrar /tmp/a /tmp/b --definitivo")).unwrap(),
            Comando::Borrar { json: false, definitivo: true, rutas: vec!["/tmp/a".into(), "/tmp/b".into()] }
        );
    }

    #[test]
    fn una_opcion_desconocida_o_un_comando_que_no_existe_se_dicen() {
        let e = parsear(&args("--cli estado --lo-que-sea")).unwrap_err();
        assert!(e.contains("desconocida"), "{e}");
        let e2 = parsear(&args("--cli inventar")).unwrap_err();
        assert!(e2.contains("desconocido"), "{e2}");
        // Y `--ayuda` y `--version` no necesitan nada más.
        assert_eq!(parsear(&args("--cli --ayuda")).unwrap(), Comando::Ayuda);
        assert_eq!(parsear(&args("--cli -v")).unwrap(), Comando::Version);
    }

    #[test]
    fn una_opcion_que_necesita_valor_lo_exige() {
        assert!(parsear(&args("--cli --hijos")).is_err());
        assert!(parsear(&args("--cli --categoria")).is_err());
        assert!(parsear(&args("--cli --periodo")).is_err());
    }

    #[test]
    fn la_papelera_se_puede_mirar_o_vaciar() {
        assert_eq!(parsear(&args("--cli papelera")).unwrap(), Comando::Papelera { json: false, vaciar: false });
        assert_eq!(
            parsear(&args("--cli papelera --vaciar --json")).unwrap(),
            Comando::Papelera { json: true, vaciar: true }
        );
        // Y el alias deja claro qué hace sin mirar la ayuda.
        assert_eq!(
            parsear(&args("--cli limpiar-papelera")).unwrap(),
            Comando::Papelera { json: false, vaciar: true }
        );
    }

    #[test]
    fn las_actualizaciones_y_la_programacion_tambien_se_piden() {
        assert_eq!(parsear(&args("--cli actualizar --json")).unwrap(), Comando::Actualizar { json: true });
        assert_eq!(parsear(&args("--cli programar")).unwrap(), Comando::Programar { json: false });
    }

    #[test]
    fn la_tilde_del_usuario_no_rompe_el_nombre_del_comando() {
        assert!(matches!(parsear(&args("--cli métricas")).unwrap(), Comando::Metrica { .. }));
    }

    #[test]
    fn historial_acepta_ruta_y_json() {
        assert_eq!(
            parsear(&args("--cli historial")).unwrap(),
            Comando::Historial { json: false, ruta: None }
        );
        assert_eq!(
            parsear(&args("--cli historial /home/alguien --json")).unwrap(),
            Comando::Historial { json: true, ruta: Some("/home/alguien".into()) }
        );
        // El alias dice lo mismo sin la tilde.
        assert!(matches!(parsear(&args("--cli historico")).unwrap(), Comando::Historial { .. }));
    }

    #[test]
    fn provision_se_mira_o_se_instala_con_su_bandera() {
        assert_eq!(
            parsear(&args("--cli provision")).unwrap(),
            Comando::Provision { json: false, instalar: false }
        );
        assert_eq!(
            parsear(&args("--cli provision --json")).unwrap(),
            Comando::Provision { json: true, instalar: false }
        );
        // `--instalar` es lo que le da permiso para tocar la red.
        assert_eq!(
            parsear(&args("--cli provision --instalar")).unwrap(),
            Comando::Provision { json: false, instalar: true }
        );
    }

    /// `salud` por defecto solo informa; `--reparar` es lo que le da permiso para
    /// escribir (apartar una base rota, restaurar un fichero, reescribir el
    /// arranque). Que sean cosas distintas se comprueba aquí.
    #[test]
    fn salud_informa_por_defecto_y_solo_repara_con_su_bandera() {
        assert_eq!(
            parsear(&args("--cli salud")).unwrap(),
            Comando::Salud { json: false, reparar: false }
        );
        assert_eq!(
            parsear(&args("--cli salud --json")).unwrap(),
            Comando::Salud { json: true, reparar: false }
        );
        assert_eq!(
            parsear(&args("--cli salud --reparar --json")).unwrap(),
            Comando::Salud { json: true, reparar: true }
        );
    }
}
