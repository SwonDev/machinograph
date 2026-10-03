//! Los clientes de IA que hay en este equipo y cómo se conectan a un endpoint
//! local.
//!
//! Por qué existe: cuando algo (Magnitude, el propio `llmfit`, un servidor
//! propio) ofrece "conectar tu agente", escribe en SU ruta esperada. En este
//! equipo eso falla en silencio con gentle-shell, que usa un home **aislado**
//! (`~/.gentle-shell/agent/`): la herramienta escribe en `~/.pi/agent/models.json`
//! y el usuario se queda sin la conexión sin saber por qué.
//!
//! Qué se hace aquí, en dos escalones:
//!
//! 1. **Detectar** qué clientes hay, dónde está su configuración, si ya apuntan
//!    a algo local, y qué líneas lo demuestran.
//! 2. **Escribir**, pero solo donde el formato está COMPROBADO leyendo el
//!    fichero real (`ESCRIBIBLES`) y con red: copia de seguridad con fecha,
//!    escritura atómica, verificación releyendo y **restauración automática** si
//!    la verificación no cuadra.
//!
//! Lo que NO se hace: inventar el formato de un cliente que no se ha comprobado.
//! Para esos se enseña su configuración real y se genera texto para pegar, en vez
//! de escribir a ciegas en un fichero ajeno que puede tener comentarios, claves y
//! una estructura que no conocemos.
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Rutas que se miran para saber si algo apunta a la propia máquina.
const LOCALES: &[&str] = &["127.0.0.1", "localhost", "::1"];

/// Los clientes en los que SÍ se escribe, además de detectarlos.
///
/// Son dos y comparten formato: `gentle-shell` y `pi`. Los dos usan JSON con
/// `providers.<id>` y `models` como lista de OBJETOS (`id`, `name`, `api`,
/// `contextWindow`, `maxTokens`, `reasoning`, `compat`…) y no como lista de
/// nombres. Eso está comprobado leyendo los dos ficheros de este equipo: Pi
/// "a secas" vive en `~/.pi`, y gentle-shell es un Pi con el home aislado, así que
/// el formato es el MISMO por construcción (y se verificó abriendo los dos).
///
/// Un cliente cuya forma no se haya leído NO entra aquí: prefiero decir «no se
/// escribe» antes que reescribir un fichero ajeno con claves y comentarios y
/// romperlo sin que nadie se entere.
const ESCRIBIBLES: &[&str] = &["gentle-shell", "pi"];

/// La ruta del fichero de modelos de un cliente escribible.
///
/// Devuelve `None` para los que no lo son: así el que llama no puede escribir en
/// un cliente por descuido, y el error lo dice con palabras.
fn ruta_de(cliente: &str) -> Option<PathBuf> {
    let h = home();
    match cliente {
        "gentle-shell" => Some(ruta_gentle_shell()),
        "pi" => Some(h.join(".pi").join("agent").join("models.json")),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Cliente {
    pub id: String,
    pub nombre: String,
    /// Ruta de su configuración (exista o no).
    pub config: String,
    pub existe: bool,
    /// Parece que ya apunta a un endpoint de esta máquina.
    pub apunta_local: bool,
    /// Líneas del fichero que mencionan un endpoint local, tal cual están.
    /// Es una referencia para copiar, no una interpretación.
    pub como_lo_tiene: Vec<String>,
    /// Se puede generar una propuesta para este cliente (formato comprobado)
    /// y, por tanto, aplicarla: es la misma condición, a propósito. Un cliente
    /// del que no se sabe el formato no puede ni proponerse ni escribirse.
    pub admite_escritura: bool,
    /// Los modelos que SU configuración ya declara PARA UN MOTOR LOCAL, leídos de
    /// su fichero.
    ///
    /// Existe por un fallo real, medido en la app: el formulario proponía por
    /// defecto solo los modelos que publica el servidor, y al aplicar eso se
    /// PERDÍA uno que ya estaba declarado y el servidor no anuncia. Con esta
    /// lista, lo que se propone por defecto es «lo que ya tienes + lo que publica
    /// el servidor», así que aplicar sin tocar nada no puede quitarte nada.
    pub modelos_declarados: Vec<String>,
    pub nota: String,
}

/// La propuesta: el texto que se escribiría, para poder revisarlo ANTES.
#[derive(Debug, Clone, Serialize)]
pub struct Propuesta {
    pub cliente: String,
    pub destino: String,
    pub formato: String,
    /// El fichero **completo** como quedaría, no un trozo suelto: así se revisa
    /// exactamente lo que se va a escribir y no hay que decidir dónde encajarlo.
    pub contenido: String,
    pub resumen: String,
    /// Cómo se llamará la copia de seguridad. Es un PATRÓN, no una promesa: lleva
    /// la fecha y la hora, así que el nombre exacto todavía no existe.
    pub copia_patron: Option<String>,
}

/// El resultado de aplicar de verdad.
#[derive(Debug, Clone, Serialize)]
pub struct Aplicado {
    pub cliente: String,
    pub destino: String,
    /// Copia del fichero tal y como estaba antes de tocarlo.
    pub copia: String,
    /// Comprobado DESPUÉS de escribir, releyendo el fichero del cliente.
    pub apunta_local: bool,
    pub resumen: String,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/usuario"))
}

fn ruta_gentle_shell() -> PathBuf {
    home().join(".gentle-shell").join("agent").join("models.json")
}

/// Lee las líneas del fichero que mencionan un endpoint local.
fn lineas_locales(ruta: &Path) -> Vec<String> {
    let Ok(texto) = std::fs::read_to_string(ruta) else {
        return Vec::new();
    };
    texto
        .lines()
        .filter(|l| LOCALES.iter().any(|loc| l.contains(loc)))
        .map(|l| l.trim().to_string())
        .take(20)
        .collect()
}

/// Los `id` de modelo que un fichero YA declara **para un motor local**, en orden
/// y sin repetir. Se leen de `providers.*.models[].id`, el formato comprobado.
///
/// Solo los de proveedores cuyo `baseUrl` apunta a esta máquina: en el mismo
/// fichero conviven proveedores de la nube (`deepseek`), y sus modelos no tienen
/// nada que hacer en la lista de un motor local (el primer intento los metía, y
/// eso habría añadido un modelo de la nube al proveedor local).
fn ids_declarados(doc: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(proveedores) = doc.get("providers").and_then(Value::as_object) else {
        return out;
    };
    for p in proveedores.values() {
        let local = p
            .get("baseUrl")
            .and_then(Value::as_str)
            .is_some_and(|u| LOCALES.iter().any(|l| u.contains(l)));
        if !local {
            continue;
        }
        let Some(modelos) = p.get("models").and_then(Value::as_array) else {
            continue;
        };
        for m in modelos {
            if let Some(id) = m.get("id").and_then(Value::as_str) {
                if !id.is_empty() && !out.iter().any(|x| x == id) {
                    out.push(id.to_string());
                }
            }
        }
    }
    out
}

/// Los modelos declarados en un fichero, si es JSON legible; si no, vacío: no se
/// inventa nada.
fn declarados_de(ruta: &Path) -> Vec<String> {
    std::fs::read_to_string(ruta)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| ids_declarados(&v))
        .unwrap_or_default()
}

pub fn clientes() -> Vec<Cliente> {
    let h = home();

    // gentle-shell (Pi) con su home AISLADO: es el caso que motivó todo esto.
    let gentle = ruta_gentle_shell();
    let gentle_nota = if gentle.is_file() {
        "Home aislado: las herramientas que 'conectan tu agente' suelen escribir en ~/.pi/agent/models.json, que NO es este fichero.".to_string()
    } else {
        "No hay configuración de modelos todavía.".to_string()
    };

    // mcode: su proveedor local se llama `custom_provider:...`.
    let mcode = h.join(".minimax").join("config.yaml");
    // Codex: los proveedores van en `model_providers`.
    let codex = h.join(".codex").join("config.toml");
    // Pi "a secas": su home es `~/.pi`, y su fichero de modelos tiene LA MISMA
    // forma que el de gentle-shell (`providers.<id>.models` como objetos), porque
    // gentle-shell es un Pi con el home aislado. Es justo el fichero al que
    // apuntan las herramientas que "conectan tu agente" cuando no conocen el home
    // aislado: tenerlo localizado y legible es la mitad del problema que motiva
    // esta pantalla.
    let pi = h.join(".pi").join("agent").join("models.json");
    // Claude Code: no tiene fichero de proveedores, pero sí variables de entorno
    // en `settings.json`, y `ANTHROPIC_BASE_URL` es la documentada para apuntarlo
    // a otro endpoint. Se detecta; no se escribe (su fichero lleva más ajustes).
    let claude = h.join(".claude").join("settings.json");

    let lista = vec![
        Cliente {
            id: "gentle-shell".into(),
            nombre: "gentle-shell (Pi)".into(),
            existe: gentle.is_file(),
            apunta_local: !lineas_locales(&gentle).is_empty(),
            como_lo_tiene: lineas_locales(&gentle),
            modelos_declarados: declarados_de(&gentle),
            config: gentle.to_string_lossy().to_string(),
            admite_escritura: true,
            nota: gentle_nota,
        },
        Cliente {
            id: "pi".into(),
            nombre: "Pi (~/.pi)".into(),
            existe: pi.is_file(),
            apunta_local: !lineas_locales(&pi).is_empty(),
            como_lo_tiene: lineas_locales(&pi),
            modelos_declarados: declarados_de(&pi),
            config: pi.to_string_lossy().to_string(),
            // MISMO formato que gentle-shell (`providers.<id>.models` como
            // objetos), así que se escribe con el MISMO camino ya probado: copia
            // con fecha, escritura atómica, permisos del original, relectura y
            // restauración automática si no cuadra.
            admite_escritura: true,
            nota: "Es el fichero al que escriben las herramientas que «conectan tu agente» cuando no conocen el home aislado de gentle-shell. Su formato está comprobado: es el mismo que el de gentle-shell.".into(),
        },
        Cliente {
            id: "claude".into(),
            nombre: "Claude Code".into(),
            existe: claude.is_file(),
            apunta_local: !lineas_locales(&claude).is_empty(),
            como_lo_tiene: lineas_locales(&claude),
            modelos_declarados: Vec::new(),
            config: claude.to_string_lossy().to_string(),
            admite_escritura: false,
            nota: "Se apunta con la variable `ANTHROPIC_BASE_URL` de su `settings.json`, que lleva más ajustes (claves, modelos, permisos): se detecta y se enseña, pero no se reescribe.".into(),
        },
        Cliente {
            id: "mcode".into(),
            nombre: "MiniMax Code (mcode)".into(),
            existe: mcode.is_file(),
            apunta_local: !lineas_locales(&mcode).is_empty(),
            como_lo_tiene: lineas_locales(&mcode),
            modelos_declarados: Vec::new(),
            config: mcode.to_string_lossy().to_string(),
            // No se escribe: su formato (YAML con `custom_provider`) lo escribe
            // su propio CLI y no está comprobado aquí. Se enseña cómo lo tiene,
            // que sí es real.
            admite_escritura: false,
            nota: "Su formato de proveedores no está comprobado aquí, así que no se toca: se enseña lo que ya tiene configurado.".into(),
        },
        Cliente {
            id: "codex".into(),
            nombre: "Codex".into(),
            existe: codex.is_file(),
            apunta_local: !lineas_locales(&codex).is_empty(),
            como_lo_tiene: lineas_locales(&codex),
            modelos_declarados: Vec::new(),
            config: codex.to_string_lossy().to_string(),
            admite_escritura: false,
            nota: "Igual que mcode: se enseña su configuración real (TOML con `model_providers`) y no se reescribe un fichero con claves y comentarios.".into(),
        },
    ];

    lista
}

/* ── Preparar: qué se escribiría ──────────────────────────────────────────── */

/// Comprueba lo que llega antes de construir nada.
///
/// Más estricto de lo que parece necesario a propósito: un identificador que
/// empiece por `-` es un argumento para un CLI, y las comillas o los espacios
/// dentro de un nombre son la forma clásica de colar un campo de más en un
/// fichero de configuración. Aquí no se llega ni a construirlo.
fn validar(id: &str, endpoint: &str, api: &str, modelos: &[String]) -> Result<(), String> {
    if id.trim().is_empty()
        || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(
            "el identificador del proveedor solo puede llevar letras, números, guiones y guion bajo"
                .into(),
        );
    }
    if endpoint.trim().is_empty() {
        return Err("falta la dirección del endpoint".into());
    }
    if api.trim().is_empty() {
        return Err("falta el tipo de api".into());
    }
    if modelos.is_empty() {
        return Err("hace falta al menos un modelo para el proveedor".into());
    }
    for m in modelos {
        if m.trim().is_empty()
            || m.starts_with('-')
            || m.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\')
        {
            return Err(format!("'{m}' no vale como identificador de modelo"));
        }
    }
    Ok(())
}

/// El objeto de un modelo que YA está declarado en el fichero del usuario.
///
/// Se busca por `id` en cualquier proveedor. Reutilizarlo no es inventar nada: es
/// no perder lo que él ya tenía escrito (`contextWindow` medido, `reasoning`,
/// `compat`, `thinkingLevelMap`…). Sin esto, cambiar el endpoint de un proveedor
/// que ya existe borraría toda esa información.
fn modelo_ya_declarado(doc: &Value, id: &str) -> Option<Value> {
    let proveedores = doc.get("providers")?.as_object()?;
    for p in proveedores.values() {
        let Some(modelos) = p.get("models").and_then(Value::as_array) else {
            continue;
        };
        for m in modelos {
            if m.get("id").and_then(Value::as_str) == Some(id) {
                return Some(m.clone());
            }
        }
    }
    None
}

/// El proveedor que se va a escribir, con cada modelo en su sitio.
///
/// Devuelve también qué modelos han salido "mínimos" (los que no estaban ya en el
/// fichero), porque eso hay que decirlo: de un modelo nuevo solo se sabe su
/// identificador, así que no se le inventa ni contexto ni compatibilidad.
fn construir_proveedor(
    doc: Option<&Value>,
    nombre: &str,
    endpoint: &str,
    api: &str,
    modelos: &[String],
) -> (Value, Vec<String>) {
    let mut lista = Vec::new();
    let mut minimos = Vec::new();
    for m in modelos {
        match doc.and_then(|d| modelo_ya_declarado(d, m)) {
            Some(ya) => lista.push(ya),
            None => {
                minimos.push(m.clone());
                // Lo mínimo que describe un modelo de verdad: con qué API se
                // habla, cómo se llama y qué identificador tiene. El contexto y
                // la compatibilidad NO se inventan.
                lista.push(json!({ "api": api, "id": m, "name": m }));
            }
        }
    }
    (
        json!({
            "name": nombre,
            "baseUrl": endpoint,
            "api": api,
            "apiKey": "local",
            "models": lista,
        }),
        minimos,
    )
}

#[derive(Debug)]
struct Preparado {
    destino: PathBuf,
    contenido: String,
    resumen: String,
}

/// Qué se escribiría, sin escribir nada. Es el paso que comparten la propuesta y
/// la aplicación, para que NO puedan discrepar: lo que se revisa es exactamente
/// lo que se escribe.
fn preparar_en(
    destino: &Path,
    id: &str,
    nombre: &str,
    endpoint: &str,
    api: &str,
    modelos: &[String],
) -> Result<Preparado, String> {
    validar(id, endpoint, api, modelos)?;

    // Si el fichero existe y no es JSON, se para AQUÍ: reescribirlo sería
    // cargarse lo que haya dentro.
    let base: Option<Value> = if destino.is_file() {
        let texto = std::fs::read_to_string(destino)
            .map_err(|e| format!("no se pudo leer {}: {e}", destino.display()))?;
        Some(serde_json::from_str(&texto).map_err(|e| {
            format!(
                "{} no es JSON válido ({e}), así que no se toca",
                destino.display()
            )
        })?)
    } else {
        None
    };

    let (proveedor, minimos) =
        construir_proveedor(base.as_ref(), nombre, endpoint, api, modelos);

    let mut doc = match base {
        Some(v) => v,
        None => json!({ "providers": {} }),
    };
    // `providers` tiene que ser un objeto: si el fichero trae otra cosa, tampoco
    // se toca (no se sabe qué quería decir).
    let raiz = doc
        .as_object_mut()
        .ok_or("la raíz del fichero no es un objeto JSON")?;
    if !raiz.contains_key("providers") {
        raiz.insert("providers".into(), Value::Object(Map::new()));
    }
    let proveedores = raiz
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or("la clave `providers` del fichero no es un objeto")?;
    let ya_estaba = proveedores.contains_key(id);
    proveedores.insert(id.to_string(), proveedor);

    let contenido = format!(
        "{}\n",
        serde_json::to_string_pretty(&doc).map_err(|e| format!("no se pudo componer el JSON: {e}"))?
    );

    let mut resumen = format!(
        "El fichero COMPLETO como quedaría: el proveedor '{id}' se {} apuntando a {endpoint}, con {} modelo(s). Todo lo demás se queda igual, incluidos los otros proveedores.",
        if ya_estaba { "actualiza" } else { "añade" },
        modelos.len()
    );
    if !minimos.is_empty() {
        resumen.push_str(&format!(
            " De ellos, {} no estaban ya en el fichero ({}): van con lo mínimo —api, id y nombre— porque su contexto y su compatibilidad no se pueden inventar; el contexto real de tus modelos está medido en Rendimiento.",
            minimos.len(),
            minimos.join(", ")
        ));
    }

    Ok(Preparado {
        destino: destino.to_path_buf(),
        contenido,
        resumen,
    })
}

fn preparar(
    cliente: &str,
    id: &str,
    nombre: &str,
    endpoint: &str,
    api: &str,
    modelos: &[String],
) -> Result<Preparado, String> {
    if !ESCRIBIBLES.contains(&cliente) {
        return Err(format!(
            "no se genera una propuesta para '{cliente}': su formato no está comprobado aquí, y prefiero no inventármelo"
        ));
    }
    let ruta = ruta_de(cliente).ok_or_else(|| {
        format!("no hay ruta conocida para '{cliente}': su formato no está comprobado aquí")
    })?;
    preparar_en(&ruta, id, nombre, endpoint, api, modelos)
}

/// La propuesta: el texto final, para leerlo antes de aplicarlo.
pub fn propuesta(
    cliente: &str,
    id: &str,
    nombre: &str,
    endpoint: &str,
    api: &str,
    modelos: &[String],
) -> Result<Propuesta, String> {
    let p = preparar(cliente, id, nombre, endpoint, api, modelos)?;
    Ok(Propuesta {
        cliente: cliente.to_string(),
        destino: p.destino.to_string_lossy().to_string(),
        formato: "json".into(),
        contenido: p.contenido,
        resumen: p.resumen,
        copia_patron: Some(format!(
            "{}.bak-AAAAAMMDD-HHMMSS",
            p.destino.to_string_lossy()
        )),
    })
}

/* ── Aplicar: escribir de verdad, con red ─────────────────────────────────── */

/// Una copia con fecha, al lado del fichero y sin pisar ninguna anterior.
///
/// Se sigue la convención que el usuario ya usa en este directorio
/// (`models.json.bak-antes-deepseek`): se ve de un vistazo cuándo se hizo.
fn ruta_de_copia(destino: &Path, sello: &str) -> PathBuf {
    let base = format!("{}.bak-{sello}", destino.to_string_lossy());
    let mut candidato = PathBuf::from(&base);
    let mut n = 1;
    while candidato.exists() {
        candidato = PathBuf::from(format!("{base}-{n}"));
        n += 1;
    }
    candidato
}

/// Escribe el contenido nuevo dejando el original a salvo.
///
/// El verificador se recibe como parámetro a propósito: es la única forma de
/// probar de verdad la RESTAURACIÓN (una prueba le pasa un verificador que falla
/// siempre y comprueba que el fichero acaba como estaba). Un camino de rescate
/// que no se prueba es un camino que no existe.
fn escribir_con_copia(
    destino: &Path,
    contenido: &str,
    copia: &Path,
    verificar: impl Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    std::fs::copy(destino, copia).map_err(|e| {
        format!(
            "no se pudo hacer la copia de seguridad {}: {e}",
            copia.display()
        )
    })?;

    // Los permisos se copian del original porque este fichero lleva una clave de
    // API y está en 600: escribir con `fs::write` lo dejaría en 644 y lo haría
    // legible para cualquiera. No es cosmética.
    let permisos = std::fs::metadata(destino).ok().map(|m| m.permissions());

    // Escritura atómica: primero un temporal al lado y luego un `rename`, para
    // que un corte a mitad no deje una configuración a medias.
    let temporal = destino.with_file_name(format!(
        ".{}.tmp-{}",
        destino.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let escrito = std::fs::write(&temporal, contenido)
        .map_err(|e| format!("no se pudo escribir {}: {e}", temporal.display()))
        .and_then(|_| {
            if let Some(p) = permisos {
                let _ = std::fs::set_permissions(&temporal, p);
            }
            std::fs::rename(&temporal, destino)
                .map_err(|e| format!("no se pudo poner el fichero nuevo en su sitio: {e}"))
        });
    if let Err(e) = escrito {
        let _ = std::fs::remove_file(&temporal);
        return Err(e);
    }

    if let Err(e) = verificar(destino) {
        // El rescate: se devuelve el original y se dice qué ha pasado y dónde
        // quedó la copia. Nunca se deja el fichero a medias sin avisar.
        let restaurado = std::fs::copy(copia, destino).is_ok();
        return Err(if restaurado {
            format!(
                "la comprobación posterior falló ({e}); se ha dejado el fichero como estaba, desde {}",
                copia.display()
            )
        } else {
            format!(
                "la comprobación posterior falló ({e}) y NO se pudo restaurar solo: tu configuración original está intacta en {}",
                copia.display()
            )
        });
    }
    Ok(())
}

/// Relee el fichero escrito y comprueba que dice lo que tenía que decir.
fn verificar_escrito(destino: &Path, id: &str, endpoint: &str) -> Result<(), String> {
    let texto = std::fs::read_to_string(destino)
        .map_err(|e| format!("no se pudo releer {}: {e}", destino.display()))?;
    let doc: Value = serde_json::from_str(&texto)
        .map_err(|e| format!("lo escrito no es JSON válido: {e}"))?;
    let base = doc
        .get("providers")
        .and_then(|p| p.get(id))
        .and_then(|p| p.get("baseUrl"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("el proveedor '{id}' no aparece en el fichero escrito"))?;
    if base != endpoint {
        return Err(format!(
            "el proveedor '{id}' quedó apuntando a '{base}' en vez de a '{endpoint}'"
        ));
    }
    Ok(())
}

/// Escribe el proveedor en la configuración del cliente. Devuelve la ruta de la
/// copia de seguridad, que es parte del resultado: el usuario tiene que poder
/// volver atrás sin buscarla.
pub fn aplicar_en(
    destino: &Path,
    id: &str,
    nombre: &str,
    endpoint: &str,
    api: &str,
    modelos: &[String],
    sello: &str,
) -> Result<Aplicado, String> {
    let p = preparar_en(destino, id, nombre, endpoint, api, modelos)?;

    if !p.destino.is_file() {
        return Err(format!(
            "no hay configuración previa en {}: no la creo de cero, porque no sé qué más necesita ese cliente. Revisa la propuesta y pégala tú.",
            p.destino.display()
        ));
    }

    let copia = ruta_de_copia(&p.destino, sello);
    escribir_con_copia(&p.destino, &p.contenido, &copia, |d| {
        verificar_escrito(d, id, endpoint)
    })?;

    // Lo que dice el fichero DESPUÉS de escribirlo, releído con la misma
    // detección que usa la interfaz: no se promete "conectado" por haber
    // escrito, se comprueba.
    let apunta_local = clientes()
        .iter()
        .find(|c| c.config == p.destino.to_string_lossy())
        .map(|c| c.apunta_local)
        .unwrap_or(false);

    Ok(Aplicado {
        cliente: "gentle-shell".into(),
        destino: p.destino.to_string_lossy().to_string(),
        copia: copia.to_string_lossy().to_string(),
        resumen: format!(
            "Proveedor '{id}' escrito en {} y comprobado releyendo el fichero. El original está en {}.",
            p.destino.display(),
            copia.display()
        ),
        apunta_local,
    })
}

pub fn aplicar(
    cliente: &str,
    id: &str,
    nombre: &str,
    endpoint: &str,
    api: &str,
    modelos: &[String],
) -> Result<Aplicado, String> {
    if !ESCRIBIBLES.contains(&cliente) {
        return Err(format!(
            "no se escribe en la configuración de '{cliente}': su formato no está comprobado aquí"
        ));
    }
    let ruta = ruta_de(cliente).ok_or_else(|| {
        format!("no hay ruta conocida para '{cliente}': su formato no está comprobado aquí")
    })?;
    let sello = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    aplicar_en(&ruta, id, nombre, endpoint, api, modelos, &sello)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Fichero real de gentle-shell, recortado: es el formato que hay que
    /// respetar (modelos como objetos, con su contexto y su compatibilidad).
    fn base_real() -> &'static str {
        r#"{
          "providers": {
            "ejemplo-local": {
              "name": "Modelo local (llama-swap)",
              "baseUrl": "http://127.0.0.1:8080/v1",
              "api": "openai-completions",
              "apiKey": "local",
              "models": [
                {
                  "api": "openai-completions",
                  "reasoning": true,
                  "maxTokens": 32768,
                  "id": "modelo-local-27b",
                  "name": "Modelo local 27B · Q4_K_M · Vulkan",
                  "contextWindow": 262144,
                  "thinkingLevelMap": { "medium": "medium" }
                }
              ]
            }
          }
        }"#
    }

    fn temporal(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("machinograph-conexiones-{nombre}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join("models.json")
    }

    #[test]
    fn detecta_los_clientes_de_esta_maquina() {
        let c = clientes();
        // Los cinco que existen en este equipo: gentle-shell, Pi, Claude Code,
        // mcode y Codex.
        assert_eq!(c.len(), 5);
        // gentle-shell es el que motivó esto: tiene que salir con su home aislado.
        let g = c.iter().find(|x| x.id == "gentle-shell").unwrap();
        assert!(g.config.contains(".gentle-shell/agent/models.json"));
        assert!(g.nota.contains("aislado"));
        assert!(g.admite_escritura);

        // Pi "a secas" escribe en OTRO fichero, el que buscan las herramientas que
        // no conocen el home aislado, y su formato es el mismo.
        let pi = c.iter().find(|x| x.id == "pi").unwrap();
        assert!(pi.config.contains(".pi/agent/models.json"));
        assert!(pi.admite_escritura);
        assert_ne!(pi.config, g.config, "no pueden ser el mismo fichero");

        // Y de los otros tres NO se inventa el formato: se detectan y se enseña lo
        // que tienen, sin escribir nada.
        for otro in c.iter().filter(|x| !ESCRIBIBLES.contains(&x.id.as_str())) {
            assert!(!otro.admite_escritura, "{} no debería admitir escritura", otro.id);
        }
    }

    #[test]
    fn dice_si_ya_apunta_a_algo_local() {
        for c in clientes().iter().filter(|c| c.existe) {
            if c.como_lo_tiene.is_empty() {
                assert!(!c.apunta_local, "{} dice apuntar local sin líneas", c.id);
            } else {
                assert!(c.apunta_local, "{} tiene líneas locales y dice que no", c.id);
            }
        }
    }

    #[test]
    fn saca_los_modelos_declarados_para_un_motor_local() {
        let doc: Value = serde_json::from_str(base_real()).unwrap();
        assert_eq!(ids_declarados(&doc), vec!["modelo-local-27b".to_string()]);
        // Un proveedor de la NUBE no aporta modelos a la lista de un motor local.
        let con_nube: Value = serde_json::from_str(
            r#"{"providers":{
                 "local":{"baseUrl":"http://127.0.0.1:8080/v1","models":[{"id":"a"},{"id":"a"}]},
                 "nube":{"baseUrl":"https://api.deepseek.com/v1","models":[{"id":"deepseek-flash"}]}}}"#,
        )
        .unwrap();
        assert_eq!(ids_declarados(&con_nube), vec!["a".to_string()], "sin nube y sin repetir");
        // Sin la clave, o con basura, no se inventa nada.
        assert!(ids_declarados(&json!({})).is_empty());
        assert!(ids_declarados(&json!({ "providers": { "x": { "models": "no" } } })).is_empty());
    }

    #[test]
    fn la_propuesta_es_el_fichero_entero_con_el_formato_real() {
        // Se trabaja sobre una COPIA temporal del fichero real: una prueba no puede
        // leer ni escribir la configuración de nadie. Antes leía la del equipo, así
        // que dependía de lo que hubiera en él (y en otro equipo fallaba).
        let destino = temporal("propuesta");
        std::fs::write(&destino, base_real()).unwrap();
        let p = preparar_en(
            &destino,
            "magnitude",
            "Magnitude (local)",
            "http://127.0.0.1:10100/inference/v1",
            "openai-completions",
            &["modelo-local-27b".into(), "modelo-nuevo".into()],
        )
        .unwrap();
        let v: Value = serde_json::from_str(&p.contenido).expect("tiene que ser JSON válido");

        // El modelo que YA estaba declarado conserva sus metadatos enteros: es lo
        // que impide que cambiar el endpoint borre el contexto de 262144.
        let reusado = &v["providers"]["magnitude"]["models"][0];
        assert_eq!(reusado["contextWindow"], 262144);
        assert_eq!(reusado["thinkingLevelMap"]["medium"], "medium");
        assert_eq!(reusado["name"], "Modelo local 27B · Q4_K_M · Vulkan");

        // El que no estaba sale con lo mínimo, y SIN inventarse contexto.
        let nuevo = &v["providers"]["magnitude"]["models"][1];
        assert_eq!(nuevo["id"], "modelo-nuevo");
        assert_eq!(nuevo["name"], "modelo-nuevo");
        assert!(nuevo.get("contextWindow").is_none(), "no se inventa el contexto");
        assert!(nuevo.get("compat").is_none());

        // Y se dice cuál se queda sin metadatos, en vez de callarlo.
        assert!(p.resumen.contains("modelo-nuevo"), "el resumen tiene que avisar: {}", p.resumen);
        assert_eq!(v["providers"]["magnitude"]["baseUrl"], "http://127.0.0.1:10100/inference/v1");
        // El formato es el real: `models` son OBJETOS, no cadenas.
        assert!(v["providers"]["magnitude"]["models"][0].is_object());
        // Y el destino es el fichero de esa copia temporal, no el de nadie.
        assert_eq!(p.destino, destino);
        let _ = std::fs::remove_file(&destino);
    }

    #[test]
    fn no_pierde_lo_que_ya_habia_en_el_fichero() {
        let destino = temporal("fusion");
        std::fs::write(&destino, base_real()).unwrap();
        let p = preparar_en(
            &destino,
            "ejemplo-local",
            "Modelo local (llama-swap)",
            "http://127.0.0.1:8080/v1",
            "openai-completions",
            &["modelo-local-27b".into()],
        )
        .unwrap();
        let v: Value = serde_json::from_str(&p.contenido).unwrap();
        // El proveedor que ya estaba se actualiza, y su modelo sigue completo.
        assert_eq!(v["providers"]["ejemplo-local"]["models"][0]["contextWindow"], 262144);
        assert_eq!(v["providers"]["ejemplo-local"]["apiKey"], "local");
        assert_eq!(v["providers"].as_object().unwrap().len(), 1);
        let _ = std::fs::remove_file(&destino);
    }

    #[test]
    fn no_toca_un_fichero_que_no_es_json() {
        let destino = temporal("nojson");
        std::fs::write(&destino, "esto no es JSON\n").unwrap();
        let r = preparar_en(&destino, "local", "Local", "http://127.0.0.1:8080/v1", "openai-completions", &["m".into()]);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("no es JSON válido"));
        // Y sigue intacto.
        assert_eq!(std::fs::read_to_string(&destino).unwrap(), "esto no es JSON\n");
        let _ = std::fs::remove_file(&destino);
    }

    /// La ruta de un cliente que no está en la lista NO existe: así no se puede
    /// escribir en un cliente cuyo formato no se ha leído, ni por descuido.
    #[test]
    fn solo_hay_ruta_para_los_clientes_escribibles() {
        for c in ESCRIBIBLES {
            assert!(ruta_de(c).is_some(), "{c} debería tener ruta");
        }
        for c in ["codex", "mcode", "claude", "inventado"] {
            assert!(ruta_de(c).is_none(), "{c} no debería tener ruta de escritura");
        }
        // Y las dos rutas escribibles son DISTINTAS: apuntar las dos al mismo
        // fichero sería escribir en un cliente creyendo que es el otro.
        assert_ne!(ruta_de("gentle-shell"), ruta_de("pi"));
    }

    #[test]
    fn valida_lo_que_se_le_pasa() {
        let base = ("gentle-shell", "id", "Nombre", "http://127.0.0.1:1/v1", "openai-completions");
        assert!(propuesta(base.0, "", base.2, base.3, base.4, &["m".into()]).is_err());
        assert!(propuesta(base.0, "con espacio", base.2, base.3, base.4, &["m".into()]).is_err());
        assert!(propuesta(base.0, base.1, base.2, "", base.4, &["m".into()]).is_err());
        assert!(propuesta(base.0, base.1, base.2, base.3, base.4, &[]).is_err());
        // Un modelo que empieza por guion es un argumento para un CLI, no un nombre.
        assert!(propuesta(base.0, base.1, base.2, base.3, base.4, &["--help".into()]).is_err());
        // Comillas o espacios dentro del nombre: fuera.
        assert!(propuesta(base.0, base.1, base.2, base.3, base.4, &["a\"b".into()]).is_err());
    }

    #[test]
    fn no_inventa_propuestas_para_lo_que_no_conoce() {
        assert!(propuesta("mcode", "x", "X", "http://127.0.0.1:8080/v1", "openai", &["m".into()]).is_err());
        assert!(propuesta("codex", "x", "X", "http://127.0.0.1:8080/v1", "openai", &["m".into()]).is_err());
        assert!(aplicar("codex", "x", "X", "http://127.0.0.1:8080/v1", "openai", &["m".into()]).is_err());
        // Claude Code: su fichero lleva claves, modelos y permisos, así que no se
        // reescribe. Se detecta y se enseña, que es lo que sí es real.
        assert!(propuesta("claude", "x", "X", "http://127.0.0.1:8080/v1", "openai", &["m".into()]).is_err());
    }

    #[test]
    fn siembra_una_copia_con_fecha_y_no_pisa_la_anterior() {
        let destino = temporal("copias");
        std::fs::write(&destino, base_real()).unwrap();
        let a = ruta_de_copia(&destino, "20260927-094500");
        assert!(a.to_string_lossy().ends_with("models.json.bak-20260927-094500"));
        std::fs::write(&a, "ocupado").unwrap();
        let b = ruta_de_copia(&destino, "20260927-094500");
        assert_ne!(a, b, "dos copias en el mismo segundo no pueden pisarse");
        assert!(b.to_string_lossy().ends_with("-1"));
        let _ = std::fs::remove_file(&destino);
        let _ = std::fs::remove_file(&a);
    }

    #[test]
    fn aplica_escribe_con_copia_y_comprueba() {
        let destino = temporal("aplica");
        let original = base_real();
        std::fs::write(&destino, original).unwrap();

        let r = aplicar_en(
            &destino,
            "magnitude",
            "Magnitude (local)",
            "http://127.0.0.1:10100/inference/v1",
            "openai-completions",
            &["modelo-local-27b".into()],
            "20260927-094500",
        )
        .unwrap();

        // 1. Se escribió lo que se dijo.
        let escrito: Value = serde_json::from_str(&std::fs::read_to_string(&destino).unwrap()).unwrap();
        assert_eq!(escrito["providers"]["magnitude"]["baseUrl"], "http://127.0.0.1:10100/inference/v1");
        // 2. Lo de antes sigue ahí.
        assert_eq!(escrito["providers"]["ejemplo-local"]["models"][0]["contextWindow"], 262144);
        // 3. La copia es el original, byte a byte.
        let copia = PathBuf::from(&r.copia);
        assert_eq!(std::fs::read_to_string(&copia).unwrap(), original);
        // 4. Y se dice dónde quedó.
        assert!(r.resumen.contains("Proveedor 'magnitude'"));

        let _ = std::fs::remove_file(&destino);
        let _ = std::fs::remove_file(&copia);
    }

    #[test]
    fn si_la_comprobacion_falla_deja_el_fichero_como_estaba() {
        let destino = temporal("rescate");
        let original = base_real();
        std::fs::write(&destino, original).unwrap();
        let copia = destino.with_file_name("copia-de-prueba.json");

        // Un verificador que falla siempre: es la única forma de probar el
        // rescate sin depender de que algo se rompa de verdad.
        let r = escribir_con_copia(&destino, "{\"providers\":{}}", &copia, |_| {
            Err("fallo a propósito".to_string())
        });

        assert!(r.is_err());
        // El fichero tiene que haber vuelto a como estaba.
        assert_eq!(std::fs::read_to_string(&destino).unwrap(), original);
        assert!(r.unwrap_err().contains("se ha dejado el fichero como estaba"));

        let _ = std::fs::remove_file(&destino);
        let _ = std::fs::remove_file(&copia);
    }

    #[test]
    fn no_crea_una_configuracion_donde_no_habia() {
        let destino = temporal("nuevo").with_file_name("no-existe.json");
        let r = aplicar_en(&destino, "local", "Local", "http://127.0.0.1:8080/v1", "openai-completions", &["m".into()], "sello");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("no la creo de cero"));
        assert!(!destino.exists(), "no puede dejar un fichero a medias");
    }

    #[test]
    fn el_fichero_escrito_es_json_valido_y_conserva_los_permisos() {
        let destino = temporal("permisos");
        std::fs::write(&destino, base_real()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&destino, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let r = aplicar_en(&destino, "otro", "Otro", "http://127.0.0.1:8080/v1", "openai-completions", &["modelo-local-27b".into()], "sello").unwrap();
        serde_json::from_str::<Value>(&std::fs::read_to_string(&destino).unwrap()).expect("JSON válido");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let modo = std::fs::metadata(&destino).unwrap().permissions().mode() & 0o777;
            assert_eq!(modo, 0o600, "el fichero lleva una clave de API: no puede quedar legible");
        }
        let _ = std::fs::remove_file(&destino);
        let _ = std::fs::remove_file(&r.copia);
    }

    /// La prueba que de verdad importa: aplicar sobre el fichero REAL y
    /// comprobar que queda equivalente.
    ///
    /// Va marcada `#[ignore]` a propósito: `cargo test` NUNCA debe tocar la
    /// configuración de verdad de nadie. Se lanza a mano cuando se quiere
    /// comprobar de punta a punta:
    ///
    /// ```text
    /// cargo test --offline -- --ignored aplica_sobre_el_fichero_real
    /// ```
    ///
    /// Qué demuestra: que reescribir el proveedor que YA existe, con los mismos
    /// valores, deja el fichero **equivalente** (mismo JSON, mismas claves,
    /// mismos modelos con sus metadatos), que la copia es el original byte a
    /// byte, y —al terminar— que el fichero vuelve a estar EXACTAMENTE como
    /// estaba: la prueba restaura los bytes originales y borra su propia copia,
    /// así que no deja rastro.
    #[test]
    #[ignore = "toca el fichero real de ~/.gentle-shell: se lanza a mano"]
    fn aplica_sobre_el_fichero_real_y_lo_deja_equivalente() {
        let destino = ruta_gentle_shell();
        if !destino.is_file() {
            println!("no hay {} en este equipo: nada que comprobar", destino.display());
            return;
        }
        let original = std::fs::read(&destino).unwrap();
        let antes: Value = serde_json::from_slice(&original).unwrap();

        // Los mismos valores que ya tiene: si el merge pierde algo, se ve aquí.
        let proveedores = antes
            .get("providers")
            .and_then(Value::as_object)
            .expect("el fichero real tiene `providers`");
        let (id, prov) = proveedores
            .iter()
            .find(|(_, v)| {
                v.get("baseUrl")
                    .and_then(Value::as_str)
                    .is_some_and(|u| u.contains("127.0.0.1"))
            })
            .expect("tiene que haber un proveedor local");
        let nombre = prov.get("name").and_then(Value::as_str).unwrap();
        let endpoint = prov.get("baseUrl").and_then(Value::as_str).unwrap();
        let api = prov.get("api").and_then(Value::as_str).unwrap();
        let modelos: Vec<String> = prov
            .get("models")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        println!("proveedor real: {id} · {endpoint} · {} modelos", modelos.len());

        let r = aplicar_en(&destino, id, nombre, endpoint, api, &modelos, "verificacion-real").unwrap();

        // 1. El JSON tiene que seguir siendo EQUIVALENTE al de antes: mismas
        //    claves y mismos valores (el orden de las claves no importa).
        let despues: Value = serde_json::from_slice(&std::fs::read(&destino).unwrap()).unwrap();
        assert_eq!(despues, antes, "el fichero real cambió de contenido al reescribirse");

        // 2. La copia es el original, byte a byte.
        let copia = PathBuf::from(&r.copia);
        assert_eq!(std::fs::read(&copia).unwrap(), original, "la copia no es el original");

        // 3. Se restaura el original tal cual y se limpia la copia de la prueba.
        std::fs::write(&destino, &original).unwrap();
        assert_eq!(std::fs::read(&destino).unwrap(), original, "no se pudo dejar igual");
        let _ = std::fs::remove_file(&copia);
    }
}
