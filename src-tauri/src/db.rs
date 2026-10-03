use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use parking_lot::{Mutex, MutexGuard};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;

use crate::historial::{comparar, comparar_ultimas, Crecimiento, HijoInstantanea, Instantanea};

#[derive(Debug, Clone, Serialize)]
pub struct ServerRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub port: u16,
    pub cmd: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionRow {
    pub ts: i64,
    pub kind: String,
    pub detail: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateRow {
    pub ts: i64,
    pub component: String,
    pub cmd: String,
    pub code: Option<i64>,
    pub ok: bool,
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricRow {
    pub ts: i64,
    pub cpu: f64,
    pub mem: f64,
    pub disk: f64,
    pub gpu_mem_used: Option<f64>,
    pub gpu_mem_total: Option<f64>,
    /// `NULL` cuando el driver no publica el dato. Antes se guardaba un `0` que se
    /// pintaba como si fuera una lectura (0 °C / 0 W).
    pub gpu_temp: Option<f64>,
    pub gpu_power: Option<f64>,
}

/// Una medición guardada de `llama-bench`.
#[derive(Debug, Clone, Serialize)]
pub struct BenchRow {
    pub ts: i64,
    pub modelo: String,
    pub runtime: String,
    /// "prefill" o "decode".
    pub tipo: String,
    pub n_prompt: i64,
    pub n_gen: i64,
    pub tok_s: f64,
    pub desviacion: f64,
    pub build: String,
    pub gpu: String,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS servers(
    id TEXT PRIMARY KEY,
    name TEXT,
    kind TEXT,
    port INTEGER,
    cmd TEXT,
    enabled INTEGER DEFAULT 1
);
CREATE TABLE IF NOT EXISTS actions(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER,
    kind TEXT,
    detail TEXT,
    ok INTEGER,
    message TEXT
);
CREATE TABLE IF NOT EXISTS updates(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER,
    component TEXT,
    cmd TEXT,
    output TEXT,
    code INTEGER,
    ok INTEGER
);
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT);
-- Exclusiones del usuario: lo que NO se mide ni se borra (ver `exclusiones.rs`).
-- Se guarda el patrón tal cual lo escribió, no su expansión: si mañana cambia
-- `${HOME}`, la exclusión sigue significando lo mismo.
CREATE TABLE IF NOT EXISTS exclusiones(
    patron TEXT PRIMARY KEY,
    ts INTEGER
);
-- Metadatos del propio fichero (no ajustes del usuario). Hoy guarda si los
-- servidores conocidos ya se sembraron una vez: ver `init`.
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS metrics(
    ts INTEGER,
    cpu REAL,
    mem REAL,
    disk REAL,
    gpu_mem_used REAL,
    gpu_mem_total REAL,
    gpu_temp REAL,
    gpu_power REAL
);
-- Mediciones reales de tokens/s (llama-bench). Se guardan con su runtime y su
-- build porque el mismo modelo da números distintos con cada binario y cada
-- configuración de caché: sin eso, el histórico no sería comparable.
CREATE TABLE IF NOT EXISTS benchmarks(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER,
    modelo TEXT,
    runtime TEXT,
    tipo TEXT,
    n_prompt INTEGER,
    n_gen INTEGER,
    tok_s REAL,
    desviacion REAL,
    build TEXT,
    gpu TEXT
);
CREATE INDEX IF NOT EXISTS idx_actions_ts ON actions(ts);
CREATE INDEX IF NOT EXISTS idx_updates_ts ON updates(ts);
CREATE INDEX IF NOT EXISTS idx_metrics_ts ON metrics(ts);
-- Encaje de cada modelo, calculado en segundo plano (no a petición). Es un
-- upsert por modelo: interesa el último, con su fecha para poder decir su edad.
CREATE TABLE IF NOT EXISTS fits(
    modelo TEXT PRIMARY KEY,
    runtime TEXT,
    ctx_max INTEGER,
    ngl INTEGER,
    encaje TEXT,
    pedido INTEGER,
    detalle TEXT,
    ts INTEGER
);
CREATE INDEX IF NOT EXISTS idx_benchmarks_ts ON benchmarks(ts);
-- Uso: una fila por petición que ha pasado por la puerta de enlace
-- (`gateway.rs`). LOS TOKENS SON NULL CUANDO EL MOTOR NO LOS PUBLICA: un 0 ahí
-- afirmaría "no se generó nada", que es distinto de "no lo sabemos". Lo mismo
-- con `ttft_ms`: no todos los motores lo dicen.
--
-- `origen` distingue quien llama (local o red) y `cliente` es el User-Agent
-- tal cual, porque sirve para saber QUÉ herramienta fue sin inventarse un
-- catálogo de clientes.
CREATE TABLE IF NOT EXISTS uso(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER,
    modelo TEXT,
    ruta TEXT,
    metodo TEXT,
    estado INTEGER,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    cached_tokens INTEGER,
    ttft_ms INTEGER,
    generacion_ms INTEGER,
    duracion_ms INTEGER,
    bytes_entrada INTEGER,
    bytes_salida INTEGER,
    origen TEXT,
    cliente TEXT
);
CREATE INDEX IF NOT EXISTS idx_uso_ts ON uso(ts);
CREATE INDEX IF NOT EXISTS idx_uso_modelo ON uso(modelo);
-- Copias de seguridad de los ficheros que la app MODIFICA (configuraciones de
-- clientes de IA, entradas de arranque…). Anotar la ruta aquí es lo que permite
-- enseñarlas y restaurarlas desde el Centro de recuperación sin recorrer el disco
-- buscando `.bak-`: la copia vive al lado del original (que es donde uno la
-- busca), y esta tabla es el índice.
CREATE TABLE IF NOT EXISTS copias(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER,
    ruta_original TEXT,
    ruta_copia TEXT,
    bytes INTEGER,
    motivo TEXT
);
CREATE INDEX IF NOT EXISTS idx_copias_ts ON copias(ts);
-- La limpieza programada. Una sola fila (id=1): es una decisión del usuario, no
-- una lista. `ultima` guarda el día en que se ejecutó por última vez, para no
-- repetirla dos veces el mismo día si la app se reinicia.
CREATE TABLE IF NOT EXISTS programacion(
    id INTEGER PRIMARY KEY CHECK (id = 1),
    activa INTEGER DEFAULT 0,
    hora INTEGER DEFAULT 3,
    minuto INTEGER DEFAULT 30,
    categorias TEXT DEFAULT '',
    ultima TEXT
);
INSERT OR IGNORE INTO programacion(id) VALUES (1);
-- Histórico del uso de disco (lo que Kudu llama «storage history and growth
-- comparisons»). Un PASE = una medida del analizador en un momento (`ts`): la
-- fila de la raíz (`es_raiz = 1`) y una fila por hijo de primer nivel
-- (`es_raiz = 0`, con el mismo `raiz` y `ts`). Así comparar dos pases es leer
-- dos conjuntos de filas y la retención puede borrar un pase ENTERO.
--
-- No se guarda un `parcial` calculado: se guardan sus CAUSAS (`truncado`,
-- `excluidos`, `resto_n`) para poder decir exactamente por qué una medida no es
-- completa, en vez de un «parcial» sin explicación.
CREATE TABLE IF NOT EXISTS instantaneas(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    raiz TEXT NOT NULL,
    ruta TEXT NOT NULL,
    es_raiz INTEGER NOT NULL DEFAULT 1,
    nombre TEXT NOT NULL DEFAULT '',
    bytes INTEGER NOT NULL,
    ficheros INTEGER NOT NULL,
    dirs INTEGER NOT NULL,
    truncado INTEGER NOT NULL DEFAULT 0,
    excluidos TEXT NOT NULL DEFAULT '[]',
    resto_n INTEGER NOT NULL DEFAULT 0,
    resto_bytes INTEGER NOT NULL DEFAULT 0
);
-- «Dame las dos últimas de esta ruta»: por `ruta` (la fila de la raíz).
CREATE INDEX IF NOT EXISTS idx_inst_ruta_ts ON instantaneas(ruta, ts);
-- Los hijos del mismo pase: por `(raiz, ts)`. Y la retención, que agrupa por raíz.
CREATE INDEX IF NOT EXISTS idx_inst_raiz_ts ON instantaneas(raiz, ts);
"#;

const KNOWN_SERVERS: &[(&str, &str, &str, u16)] = &[
    ("llama-swap:8080", "Llama-Swap (local)", "llama-swap", 8080),
    ("llama-cpp:5801", "llama.cpp", "llama-cpp", 5801),
    ("ollama:9000", "Ollama", "ollama", 9000),
    ("lmstudio:1234", "LM Studio", "lmstudio", 1234),
    ("exllama:8123", "ExLlama v2", "exllama", 8123),
    ("vllm:8000", "vLLM", "vllm", 8000),
    ("tgwebui:5005", "Text-gen WebUI", "tgwebui", 5005),
    ("comfyui:8188", "ComfyUI", "comfyui", 8188),
];

/// Un ajuste que el backend LEE de verdad, con su rango.
///
/// La lista es corta a propósito. La pantalla de Ajustes solo puede ofrecer lo
/// que está aquí: un ajuste que no se consulta en ninguna parte sería una
/// mentira en la interfaz (el usuario lo cambiaría y no pasaría nada).
pub struct Ajuste {
    pub clave: &'static str,
    pub descripcion: &'static str,
    pub por_defecto: i64,
    pub min: i64,
    pub max: i64,
    /// Un ajuste de sí/no (0 o 1). La interfaz lo pinta como casilla, no como
    /// número: pedir «0 o 1» a mano es una forma rara de ofrecer un interruptor.
    pub booleano: bool,
}

pub const AJUSTES: &[Ajuste] = &[
    Ajuste {
        clave: "snapshot_interval_ms",
        descripcion: "Cada cuánto se toma la foto del sistema",
        por_defecto: 2000,
        // El mínimo era 250 ms, y el diseño no lo aguanta: una foto sondea 8
        // servidores, lee /proc y lanza binarios externos. A 250 ms se solapaban
        // las vueltas y, con los procesos en serie, eso dejaba un worker de tokio
        // ocupado para siempre. Con el trabajo pesado en hilos aparte y las
        // lecturas lentas cacheadas, una vuelta baja de ~0,5 s, así que 1 s es el
        // ritmo más rápido que se puede sostener sin pisarse.
        min: 1_000,
        max: 60_000,
        booleano: false,
    },
    Ajuste {
        clave: "metric_retention_hours",
        descripcion: "Cuántas horas de métricas se guardan",
        por_defecto: 2,
        min: 1,
        max: 168,
        booleano: false,
    },
    Ajuste {
        clave: "historial_activo",
        descripcion: "Medir tu carpeta personal y tu carpeta de modelos una vez al día para poder comparar el crecimiento del disco (es una lectura de disco: en un hogar grande puede tardar hasta los 45 s del presupuesto del analizador, en segundo plano)",
        por_defecto: 1,
        min: 0,
        max: 1,
        booleano: true,
    },
    Ajuste {
        clave: "historial_umbral_gb",
        descripcion: "Crecimiento de tu carpeta personal en una semana a partir del cual Inicio avisa",
        // 5 GB a la semana: por debajo de eso el aviso saltaría con cualquier
        // caché puntual y dejaría de mirarse, que es lo que hace inútil un aviso.
        por_defecto: 5,
        min: 1,
        max: 10_000,
        booleano: false,
    },
];

pub fn ajuste(clave: &str) -> Option<&'static Ajuste> {
    AJUSTES.iter().find(|a| a.clave == clave)
}

/// Valor efectivo de un ajuste: lo guardado si es válido, si no el de por
/// defecto. Nunca devuelve basura, así que quien lo use puede confiar en él.
pub fn setting_int(clave: &str, por_defecto: i64) -> i64 {
    let Some(a) = ajuste(clave) else {
        return por_defecto;
    };
    get_setting(clave)
        .ok()
        .flatten()
        .and_then(|v| valor_a_entero(&v))
        .filter(|v| *v >= a.min && *v <= a.max)
        .unwrap_or(a.por_defecto)
}

/// El valor llega de la interfaz como texto, y puede venir con comillas si en
/// SQLite se guardó como JSON (`"2000"`).
fn valor_a_entero(v: &str) -> Option<i64> {
    v.trim().trim_matches('"').trim().parse::<i64>().ok()
}

/// Marca de tiempo UNIX en segundos. Se publica porque el histórico de disco
/// necesita fechar sus medidas con el mismo reloj que el resto del histórico.
pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Ruta del fichero de la base de datos (la de siempre: no cambia).
fn ruta_bd() -> PathBuf {
    // `MACHINOGRAPH_DB_PATH` permite llevar la base de datos a otro sitio. Existe por
    // dos motivos: las pruebas de la puerta de enlace, que necesitan una BD propia
    // (escribir en la del usuario desde un test sería inaceptable), y poder mover
    // el histórico a otro disco sin tocar el código.
    if let Ok(p) = std::env::var("MACHINOGRAPH_DB_PATH") {
        let p = p.trim();
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".local").join("share").join("machinograph").join("data.db")
}

/// Abre (creando lo que haga falta) la base de datos en `path`.
///
/// Es fallible A PROPÓSITO, y es la pieza que se prueba: antes era
/// `Connection::open(&path).expect("could not open machinograph db")`, así que un disco
/// lleno, un permiso cambiado o un `data.db` corrupto mataban el proceso (en
/// release el perfil es `panic = "abort"`): la ventana desaparecía sin un mensaje
/// y sin volver a arrancar. Ahora el motivo se devuelve para que LLEGUE a la
/// interfaz.
///
/// Ojo: hay que crear el DIRECTORIO (…/share/machinograph), no su padre; creando el
/// padre solo se creaba ~/.local/share y `open` fallaba al faltar la carpeta.
pub fn connect_to(path: &Path) -> anyhow::Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| anyhow::anyhow!("no se pudo crear {}: {e}", dir.display()))?;
    }
    let c = Connection::open(path)
        .map_err(|e| anyhow::anyhow!("no se pudo abrir {}: {e}", path.display()))?;
    // WAL y espera: sin esto, dos escrituras a la vez (la foto y un comando) podían
    // dar `database is locked`, y ese error se descartaba en silencio — o sea,
    // métricas perdidas sin decir nada.
    c.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
        .map_err(|e| anyhow::anyhow!("no se pudo activar WAL: {e}"))?;
    c.busy_timeout(Duration::from_secs(5))
        .map_err(|e| anyhow::anyhow!("no se pudo fijar la espera de bloqueo: {e}"))?;
    Ok(c)
}

/// La ruta del fichero de la base de datos en uso. Se publica para poder decir
/// DÓNDE está en la interfaz (una ruta que no se enseña no se puede comprobar).
pub fn ruta_actual() -> PathBuf {
    ruta_bd()
}

/// ¿La base recién abierta está sana? Se usa `quick_check` (la versión barata de
/// `integrity_check`): detecta páginas ilegibles y estructuras rotas sin recorrer
/// el fichero entero.
fn esta_sana(c: &Connection) -> Result<(), String> {
    let mut stmt = c.prepare("PRAGMA quick_check").map_err(|e| e.to_string())?;
    let filas: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if filas.len() == 1 && filas[0].eq_ignore_ascii_case("ok") {
        return Ok(());
    }
    if filas.is_empty() {
        return Err("`PRAGMA quick_check` no devolvió nada".into());
    }
    Err(filas.join("; "))
}

/// Aparta una base de datos dañada renombrándola con la fecha:
/// `data.db.corrupta-20261003-130501`. NUNCA la borra: el histórico del usuario
/// puede ser recuperable con las herramientas de SQLite, y borrarlo sería
/// exactamente lo que esta app no hace.
///
/// El WAL y el SHM se mueven CON ella (mismo sufijo): si se quedaran al lado, la
/// base nueva los tomaría por suyos y podría arrancar con las páginas de la rota.
pub fn apartar(path: &Path) -> anyhow::Result<PathBuf> {
    let marca = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    // La marca tiene resolución de segundo: dos reparaciones en el mismo segundo
    // chocarían y la segunda destruiría a la primera. Por eso se numera.
    let mut destino = PathBuf::from(format!("{}.corrupta-{marca}", path.to_string_lossy()));
    let mut n = 1;
    while destino.exists() {
        destino = PathBuf::from(format!("{}.corrupta-{marca}-{n}", path.to_string_lossy()));
        n += 1;
    }
    std::fs::rename(path, &destino)
        .map_err(|e| anyhow::anyhow!("no se pudo apartar {}: {e}", path.display()))?;
    for sufijo in ["-wal", "-shm"] {
        let origen = PathBuf::from(format!("{}{sufijo}", path.to_string_lossy()));
        if origen.is_file() {
            let destino_aux = PathBuf::from(format!("{}{sufijo}", destino.to_string_lossy()));
            std::fs::rename(&origen, &destino_aux).map_err(|e| {
                anyhow::anyhow!(
                    "la base quedó apartada en {} pero no se pudo mover {}: {e}",
                    destino.display(),
                    origen.display()
                )
            })?;
        }
    }
    Ok(destino)
}

/// Abre la base de datos REPARANDO lo que se pueda reparar.
///
/// POR QUÉ: una base que no se puede abrir dejaba la app sin histórico y con un
/// aviso, y arreglarlo exigía borrar el fichero a mano (perdiendo lo que hubiera
/// dentro). Ahora, si no se puede abrir o no pasa `PRAGMA quick_check`, se APARTA
/// con la fecha en el nombre y se crea una nueva con su esquema: la app sigue
/// funcionando desde cero y el fichero viejo queda donde se pueda recuperar.
///
/// Devuelve la conexión y, si hubo que reparar, qué se hizo y dónde quedó lo
/// apartado (para poder CONTARLO en vez de hacerlo en silencio).
pub fn connect_reparando(path: &Path) -> anyhow::Result<(Connection, Option<String>)> {
    let intento = connect_to(path).and_then(|c| {
        esta_sana(&c)
            .map_err(|e| anyhow::anyhow!("la base de datos no pasa la comprobación de integridad: {e}"))?;
        Ok(c)
    });
    let motivo = match intento {
        Ok(c) => return Ok((c, None)),
        Err(e) => e.to_string(),
    };
    let apartada = match apartar(path) {
        Ok(p) => p,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "{motivo}; y tampoco se pudo apartar el fichero para empezar de cero: {e}"
            ))
        }
    };
    let nueva = connect_to(path)?;
    Ok((
        nueva,
        Some(format!(
            "La base de datos no se pudo usar ({motivo}). Se ha apartado en {} SIN BORRARLA y se ha creado una nueva en {}: la app sigue funcionando con el histórico desde cero. Lo que hubiera en la vieja se puede intentar recuperar con `sqlite3 {} .dump`.",
            apartada.display(),
            path.display(),
            apartada.display()
        )),
    ))
}

/// Conexión única del proceso.
///
/// POR QUÉ UNA SOLA: antes CADA función abría la BD y, además, `open()` volvía a
/// ejecutar el esquema y la siembra: una sola vuelta de la foto abría el fichero 4
/// veces y lanzaba 11 `CREATE` + 8 `INSERT OR IGNORE` para escribir lo mismo.
/// Ahora se abre una vez y `init` solo se ejecuta al abrir. Como
/// `rusqlite::Connection` no es `Sync`, se comparte detrás de un `Mutex`: SQLite
/// serializa las escrituras de todas formas, así que no se pierde nada.
static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

/// Último error de la base de datos, para poder ENSEÑARLO.
///
/// Se anota al fallar y se limpia en cuanto una operación vuelve a funcionar. Lo
/// publica la foto en `Snapshot.db_error`: si no, un fallo de disco dejaría la app
/// viva pero sin métricas y sin decir por qué.
static ULTIMO_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Motivo por el que la base de datos no está disponible, si lo está fallando.
/// `None` cuando todo va bien.
pub fn ultimo_error() -> Option<String> {
    ULTIMO_ERROR.lock().clone()
}

/// Qué se hizo cuando hubo que apartar la base dañada, si pasó en esta ejecución.
/// Se queda ahí toda la sesión: la comprobación de salud lo cuenta aunque se lance
/// a mano mucho después del arranque (una reparación que nadie ve es
/// indistinguible de una pérdida de datos).
static REPARACION_BD: Mutex<Option<String>> = Mutex::new(None);

fn anota_reparacion(aviso: &str) {
    let mut g = REPARACION_BD.lock();
    *g = Some(aviso.to_string());
}

/// Lo que se hizo la última vez que hubo que reparar la base, o `None` si no ha
/// hecho falta en esta ejecución.
pub fn reparacion_bd() -> Option<String> {
    REPARACION_BD.lock().clone()
}

/// Fuerza la apertura de la base (con su reparación, si hace falta). Devuelve el
/// motivo si NO quedó utilizable, para poder decirlo en vez de fallar en silencio.
pub fn asegurar() -> Result<(), String> {
    conn().map(|_| ()).map_err(|e| e.to_string())
}

fn anota_error(e: &anyhow::Error) {
    { let mut g = ULTIMO_ERROR.lock();
        *g = Some(e.to_string());
    }
}

fn anota_bien() {
    { let mut g = ULTIMO_ERROR.lock();
        *g = None;
    }
}

fn conn() -> anyhow::Result<&'static Mutex<Connection>> {
    if let Some(m) = DB.get() {
        anota_bien();
        return Ok(m);
    }
    let (c, reparacion) = match connect_reparando(&ruta_bd()) {
        Ok(v) => v,
        Err(e) => {
            anota_error(&e);
            return Err(e);
        }
    };
    if let Err(e) = init(&c) {
        anota_error(&e);
        return Err(e);
    }
    if let Some(aviso) = reparacion {
        // Se guarda para que la comprobación de salud lo cuente: una reparación
        // que nadie ve es indistinguible de una pérdida de datos.
        anota_reparacion(&aviso);
    }
    let m = DB.get_or_init(|| Mutex::new(c));
    anota_bien();
    Ok(m)
}

/// Conexión lista para usar, bloqueada.
fn conn_guard() -> anyhow::Result<MutexGuard<'static, Connection>> {
    let m = conn()?;
    // Un `Mutex` envenenado solo pasa si un hilo entró en pánico con el guard
    // cogido; se recupera el contenido en vez de arrastrar el fallo a todo el mundo.
    Ok(m.lock())
}

/// Crea el esquema y siembra los servidores conocidos, UNA SOLA VEZ.
///
/// Antes se llamaba en cada apertura y los 8 `KNOWN_SERVERS` se reinsertaban con
/// `INSERT OR IGNORE`: quitar un servidor en Ajustes no servía de nada, porque
/// volvía en la siguiente apertura (≤2 s después). La siembra deja ahora una
/// bandera en `meta`, así que se hace una vez y borrar un servidor se respeta.
fn init(c: &Connection) -> anyhow::Result<()> {
    c.execute_batch(SCHEMA)?;
    let ya_sembrado: Option<String> = c
        .query_row(
            "SELECT value FROM meta WHERE key = 'servers_seeded'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if ya_sembrado.is_none() {
        for (id, name, kind, port) in KNOWN_SERVERS {
            c.execute(
                "INSERT OR IGNORE INTO servers(id, name, kind, port, enabled) VALUES (?1, ?2, ?3, ?4, 1)",
                params![id, name, kind, port],
            )?;
        }
        c.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('servers_seeded', '1')",
            [],
        )?;
    }
    Ok(())
}

pub fn servers() -> anyhow::Result<Vec<ServerRow>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT id, name, kind, port, cmd, enabled FROM servers ORDER BY id",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    loop {
        match rows.next()? {
            Some(r) => {
                out.push(ServerRow {
                    id: r.get("id")?,
                    name: r.get("name")?,
                    kind: r.get("kind")?,
                    port: {
                        let p: i32 = r.get("port")?;
                        u16::try_from(p).unwrap_or_default()
                    },
                    cmd: r.get::<_, Option<String>>("cmd")?,
                    enabled: r.get("enabled")?,
                });
            }
            None => break,
        }
    }
    Ok(out)
}

pub fn add_server(id: &str, name: &str, kind: &str, port: u16, enabled: bool) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT OR REPLACE INTO servers(id, name, kind, port, cmd, enabled) VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
        params![id, name, kind, port, enabled],
    )?;
    Ok(())
}

pub fn update_server(id: &str, cmd: &str, enabled: bool) -> anyhow::Result<()> {
    let c = conn_guard()?;
    let sql_cmd: Option<String> = if cmd.is_empty() { None } else { Some(cmd.to_string()) };
    c.execute(
        "UPDATE servers SET cmd = ?, enabled = ? WHERE id = ?",
        params![sql_cmd, enabled, id],
    )?;
    Ok(())
}

pub fn remove_server(id: &str) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute("DELETE FROM servers WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn get_setting(key: &str) -> anyhow::Result<Option<String>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    loop {
        match rows.next()? {
            Some(r) => {
                let v: Option<String> = r.get("value")?;
                return Ok(v);
            }
            None => break,
        }
    }
    Ok(None)
}

/// Escribe un ajuste, validando clave y rango.
///
/// Antes aceptaba cualquier clave y cualquier valor: se guardaban sin más y
/// nadie los leía, así que la interfaz podía ofrecer ajustes que no hacían nada.
pub fn set_setting(key: &str, value: &str) -> anyhow::Result<()> {
    let a = ajuste(key).ok_or_else(|| anyhow::anyhow!("ajuste desconocido: {key}"))?;
    let n = valor_a_entero(value).ok_or_else(|| anyhow::anyhow!("{value} no es un número"))?;
    if n < a.min || n > a.max {
        return Err(anyhow::anyhow!(
            "{key} debe estar entre {} y {}",
            a.min,
            a.max
        ));
    }
    let c = conn_guard()?;
    c.execute(
        "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, ?2)",
        params![key, n.to_string()],
    )?;
    Ok(())
}

/// Todos los ajustes conocidos con su valor EFECTIVO (lo guardado o, si no hay
/// nada válido, el de por defecto). Se devuelven los dos, porque la interfaz
/// necesita poder distinguir "está así" de "está así porque nadie lo ha tocado".
pub fn settings_all() -> anyhow::Result<Value> {
    let c = conn_guard()?;
    let mut stmt = c.prepare("SELECT key, value FROM settings")?;
    let mut rows = stmt.query([])?;
    let mut guardados = serde_json::Map::new();
    loop {
        match rows.next()? {
            Some(r) => {
                let k: String = r.get("key")?;
                let v: String = r.get("value")?;
                guardados.insert(k, Value::String(v));
            }
            None => break,
        }
    }

    let mut out = serde_json::Map::new();
    for a in AJUSTES {
        // El valor efectivo se calcula sobre lo que ya se ha leído, sin volver a
        // entrar en la BD: `setting_int` abriría otra vez la conexión y con el
        // guard cogido eso sería pedir el mismo candado dos veces.
        let guardado_ok = guardados
            .get(a.clave)
            .and_then(|v| v.as_str())
            .and_then(valor_a_entero)
            .filter(|v| *v >= a.min && *v <= a.max);
        let efectivo = guardado_ok.unwrap_or(a.por_defecto);
        // `guardado` distingue "el usuario lo puso así" de "es el valor por
        // defecto porque nadie lo ha tocado". Si lo guardado no valía (fuera de
        // rango, texto), cuenta como no guardado: es el defecto el que manda.
        let guardado = guardado_ok.is_some();
        out.insert(
            a.clave.to_string(),
            serde_json::json!({
                "valor": efectivo,
                "por_defecto": a.por_defecto,
                "min": a.min,
                "max": a.max,
                "descripcion": a.descripcion,
                "guardado": guardado,
                "booleano": a.booleano,
            }),
        );
    }
    Ok(Value::Object(out))
}

/* ── Exclusiones del usuario ──────────────────────────────────────────────── */

/// Las exclusiones guardadas (patrón y fecha), más recientes primero.
///
/// El orden es el que se enseña: lo último que has excluido es lo que estás
/// mirando. La expansión de las variables NO se guarda: se hace al usarlas, para
/// que la lista siga valiendo si cambia tu carpeta personal.
pub fn exclusiones_listar() -> anyhow::Result<Vec<(String, i64)>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare("SELECT patron, COALESCE(ts, 0) FROM exclusiones ORDER BY ts DESC, patron ASC")?;
    let filas = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(filas.filter_map(|f| f.ok()).collect())
}

pub fn exclusiones_anadir(patron: &str) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT OR REPLACE INTO exclusiones(patron, ts) VALUES (?1, ?2)",
        params![patron, now_ts()],
    )?;
    Ok(())
}

/// Quita una exclusión y dice cuántas filas se han ido (0 = no estaba).
pub fn exclusiones_quitar(patron: &str) -> anyhow::Result<usize> {
    let c = conn_guard()?;
    Ok(c.execute("DELETE FROM exclusiones WHERE patron = ?1", params![patron])?)
}

pub fn insert_action(kind: &str, detail: &str, ok: bool, message: &str) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT INTO actions(ts, kind, detail, ok, message) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now_ts(), kind, detail, ok, message],
    )?;
    Ok(())
}

pub fn actions(limit: i64) -> anyhow::Result<Vec<ActionRow>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT ts, kind, detail, ok, message FROM actions ORDER BY ts DESC, id DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    loop {
        match rows.next()? {
            Some(r) => {
                out.push(ActionRow {
                    ts: r.get("ts")?,
                    kind: r.get("kind")?,
                    detail: r.get("detail")?,
                    ok: r.get("ok")?,
                    message: r.get("message")?,
                });
                if out.len() as i64 >= limit {
                    break;
                }
            }
            None => break,
        }
    }
    Ok(out)
}

pub fn insert_update(
    component: &str,
    cmd: &str,
    output: &str,
    code: Option<i64>,
    ok: bool,
) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT INTO updates(ts, component, cmd, output, code, ok) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![now_ts(), component, cmd, output, code, ok],
    )?;
    Ok(())
}

/* ── Copias de seguridad (Centro de recuperación) ─────────────────────────── */

/// Una copia anotada. `existe` se comprueba al leer: una copia que alguien borró
/// por fuera no se puede restaurar, y decir que sí sería la clase de mentira que
/// esta app evita.
#[derive(Debug, Clone, Serialize)]
pub struct CopiaRow {
    pub id: i64,
    pub ts: i64,
    pub ruta_original: String,
    pub ruta_copia: String,
    pub bytes: i64,
    pub motivo: String,
    pub existe: bool,
}

pub fn insert_copia(
    ruta_original: &str,
    ruta_copia: &str,
    bytes: i64,
    motivo: &str,
) -> anyhow::Result<i64> {
    let c = conn_guard()?;
    c.execute(
        "INSERT INTO copias(ts, ruta_original, ruta_copia, bytes, motivo) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now_ts(), ruta_original, ruta_copia, bytes, motivo],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn copias(limit: i64) -> anyhow::Result<Vec<CopiaRow>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT id, ts, ruta_original, ruta_copia, bytes, motivo FROM copias ORDER BY ts DESC, id DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    loop {
        match rows.next()? {
            Some(r) => {
                let ruta_copia: String = r.get("ruta_copia")?;
                out.push(CopiaRow {
                    id: r.get("id")?,
                    ts: r.get("ts")?,
                    ruta_original: r.get("ruta_original")?,
                    existe: std::path::Path::new(&ruta_copia).is_file(),
                    ruta_copia,
                    bytes: r.get("bytes")?,
                    motivo: r.get("motivo")?,
                });
                if out.len() as i64 >= limit {
                    break;
                }
            }
            None => break,
        }
    }
    Ok(out)
}

pub fn copia(id: i64) -> anyhow::Result<Option<CopiaRow>> {
    Ok(copias(1000)?.into_iter().find(|c| c.id == id))
}

pub fn borrar_copia(id: i64) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute("DELETE FROM copias WHERE id = ?1", params![id])?;
    Ok(())
}

/// Las rutas ORIGINALES que la app ha modificado (las que tienen copia anotada).
///
/// Es la lista de ficheros «suyos» que se pueden revisar y reparar al arrancar: la
/// copia es la prueba de que la app lo escribió, así que solo se restaura lo que
/// ella misma tocó (nunca un fichero ajeno).
pub fn rutas_con_copia() -> anyhow::Result<Vec<String>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare("SELECT DISTINCT ruta_original FROM copias ORDER BY ruta_original")?;
    let filas = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(filas.collect::<Result<Vec<_>, _>>()?)
}

/// La copia MÁS RECIENTE de un original, si la hay. Es la que se usa para
/// restaurar: la última que hizo la app antes de su último cambio.
pub fn copia_mas_reciente(ruta_original: &str) -> anyhow::Result<Option<CopiaRow>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT id, ts, ruta_original, ruta_copia, bytes, motivo FROM copias WHERE ruta_original = ?1 ORDER BY ts DESC, id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query(params![ruta_original])?;
    match rows.next()? {
        Some(r) => {
            let ruta_copia: String = r.get("ruta_copia")?;
            Ok(Some(CopiaRow {
                id: r.get("id")?,
                ts: r.get("ts")?,
                ruta_original: r.get("ruta_original")?,
                existe: std::path::Path::new(&ruta_copia).is_file(),
                ruta_copia,
                bytes: r.get("bytes")?,
                motivo: r.get("motivo")?,
            }))
        }
        None => Ok(None),
    }
}

/* ── La limpieza programada ───────────────────────────────────────────────── */

/// Lo que el usuario ha configurado para la limpieza automática.
///
/// `Default` = desactivada a las 03:30 con todas las categorías: un valor sensato
/// para una fila que existe siempre (la tabla siembra la suya).
#[derive(Debug, Clone, Serialize)]
pub struct Programacion {
    pub activa: bool,
    pub hora: u32,
    pub minuto: u32,
    /// Categorías del catálogo. Vacío = todas.
    pub categorias: Vec<String>,
    /// El día (local, `AAAA-MM-DD`) de la última ejecución, si hubo.
    pub ultima: Option<String>,
}

impl Default for Programacion {
    fn default() -> Self {
        Self { activa: false, hora: 3, minuto: 30, categorias: Vec::new(), ultima: None }
    }
}

pub fn programacion() -> anyhow::Result<Programacion> {
    let c = conn_guard()?;
    let mut stmt = c.prepare("SELECT activa, hora, minuto, categorias, ultima FROM programacion WHERE id = 1")?;
    let mut filas = stmt.query([])?;
    match filas.next()? {
        Some(r) => {
            let categorias: String = r.get("categorias")?;
            Ok(Programacion {
                activa: r.get::<_, i64>("activa")? != 0,
                hora: r.get::<_, i64>("hora")?.clamp(0, 23) as u32,
                minuto: r.get::<_, i64>("minuto")?.clamp(0, 59) as u32,
                categorias: categorias
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
                ultima: r.get("ultima")?,
            })
        }
        None => Ok(Programacion::default()),
    }
}

pub fn guardar_programacion(p: &Programacion) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "UPDATE programacion SET activa = ?1, hora = ?2, minuto = ?3, categorias = ?4 WHERE id = 1",
        params![
            if p.activa { 1 } else { 0 },
            p.hora.clamp(0, 23) as i64,
            p.minuto.clamp(0, 59) as i64,
            p.categorias.join(",")
        ],
    )?;
    Ok(())
}

/// Anota que la limpieza programada se ha ejecutado hoy.
pub fn marcar_programacion_hecha(dia: &str) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute("UPDATE programacion SET ultima = ?1 WHERE id = 1", params![dia])?;
    Ok(())
}

/* ── Actualizaciones (los comandos lanzados desde Mantenimiento) ──────────── */

pub fn updates(limit: i64) -> anyhow::Result<Vec<UpdateRow>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT ts, component, cmd, output, code, ok FROM updates ORDER BY ts DESC, id DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    loop {
        match rows.next()? {
            Some(r) => {
                out.push(UpdateRow {
                    ts: r.get("ts")?,
                    component: r.get("component")?,
                    cmd: r.get("cmd")?,
                    code: r.get("code")?,
                    ok: r.get("ok")?,
                    output: r.get("output")?,
                });
                if out.len() as i64 >= limit {
                    break;
                }
            }
            None => break,
        }
    }
    Ok(out)
}

pub fn insert_metric(
    cpu: f64,
    mem: f64,
    disk: f64,
    gpu_mem_used: Option<f64>,
    gpu_mem_total: Option<f64>,
    gpu_temp: Option<f64>,
    gpu_power: Option<f64>,
) -> anyhow::Result<()> {
    // La retención sale de los ajustes (por defecto 2 h), no de una constante
    // escondida aquí: es lo que la pantalla de Ajustes promete. Se lee ANTES de
    // coger la conexión porque `setting_int` vuelve a entrar en la BD: hacerlo con
    // el guard cogido sería pedir el mismo candado dos veces.
    let horas = setting_int("metric_retention_hours", 2);
    let c = conn_guard()?;
    let ts = now_ts();
    c.execute(
        "INSERT INTO metrics(ts, cpu, mem, disk, gpu_mem_used, gpu_mem_total, gpu_temp, gpu_power)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ts, cpu, mem, disk, gpu_mem_used, gpu_mem_total, gpu_temp, gpu_power
        ],
    )?;
    c.execute_batch(&format!(
        "DELETE FROM metrics WHERE ts < {}",
        ts - horas * 3600
    ))?;
    Ok(())
}

/// Una fila de encaje, tal cual se guarda.
#[derive(Debug, Clone, Serialize)]
pub struct FitRow {
    pub modelo: String,
    pub runtime: String,
    pub ctx_max: i64,
    pub ngl: i64,
    /// `Gpu`, `Mixto`, `NoCabe` o `Error` (con el motivo en `detalle`).
    pub encaje: String,
    pub pedido: Option<i64>,
    pub detalle: String,
    pub ts: i64,
}

/// Guarda (o reemplaza) el encaje de un modelo.
pub fn insert_fit(f: &crate::perf::Fit) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT OR REPLACE INTO fits(modelo, runtime, ctx_max, ngl, encaje, pedido, detalle, ts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            f.modelo,
            f.runtime,
            f.ctx_max,
            f.ngl,
            format!("{:?}", f.encaje),
            f.pedido,
            f.detalle,
            f.ts
        ],
    )?;
    Ok(())
}

/// Guarda un encaje que NO se pudo calcular, con el motivo. Se guarda a
/// propósito: si no, la interfaz no podría distinguir "todavía no se ha
/// calculado" de "este binario no sabe leerlo".
pub fn insert_fit_error(modelo: &str, motivo: &str) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT OR REPLACE INTO fits(modelo, runtime, ctx_max, ngl, encaje, pedido, detalle, ts)
         VALUES (?1, '', 0, 0, 'Error', NULL, ?2, ?3)",
        params![modelo, motivo, now_ts()],
    )?;
    Ok(())
}

pub fn fits() -> anyhow::Result<Vec<FitRow>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT modelo, runtime, ctx_max, ngl, encaje, pedido, detalle, ts FROM fits ORDER BY ts DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    loop {
        match rows.next()? {
            Some(r) => out.push(FitRow {
                modelo: r.get("modelo")?,
                runtime: r.get("runtime")?,
                ctx_max: r.get("ctx_max")?,
                ngl: r.get("ngl")?,
                encaje: r.get("encaje")?,
                pedido: r.get("pedido")?,
                detalle: r.get("detalle")?,
                ts: r.get("ts")?,
            }),
            None => break,
        }
    }
    Ok(out)
}

pub fn metrics(since: i64) -> anyhow::Result<Vec<MetricRow>> {    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT ts, cpu, mem, disk, gpu_mem_used, gpu_mem_total, gpu_temp, gpu_power
         FROM metrics WHERE ts >= ?1 ORDER BY ts ASC",
    )?;
    let mut rows = stmt.query(params![since])?;
    let mut out = Vec::new();
    loop {
        match rows.next()? {
            Some(r) => {
                out.push(MetricRow {
                    ts: r.get("ts")?,
                    cpu: r.get("cpu")?,
                    mem: r.get("mem")?,
                    disk: r.get("disk")?,
                    gpu_mem_used: r.get("gpu_mem_used")?,
                    gpu_mem_total: r.get("gpu_mem_total")?,
                    gpu_temp: r.get("gpu_temp")?,
                    gpu_power: r.get("gpu_power")?,
                });
            }
            None => break,
        }
    }
    out.reverse();
    Ok(out)
}

/* ── Mediciones de rendimiento ────────────────────────────────────────────── */

#[allow(clippy::too_many_arguments)]
pub fn insert_benchmark(
    modelo: &str,
    runtime: &str,
    tipo: &str,
    n_prompt: i64,
    n_gen: i64,
    tok_s: f64,
    desviacion: f64,
    build: &str,
    gpu: &str,
) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT INTO benchmarks(ts, modelo, runtime, tipo, n_prompt, n_gen, tok_s, desviacion, build, gpu)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![now_ts(), modelo, runtime, tipo, n_prompt, n_gen, tok_s, desviacion, build, gpu],
    )?;
    Ok(())
}

/// Últimas mediciones. Se devuelven con el modelo y el runtime para poder
/// comparar peras con peras (el mismo modelo con otro binario no es comparable).
pub fn benchmarks(limit: i64) -> anyhow::Result<Vec<BenchRow>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT ts, modelo, runtime, tipo, n_prompt, n_gen, tok_s, desviacion, build, gpu
         FROM benchmarks ORDER BY ts DESC, id DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    loop {
        match rows.next()? {
            Some(r) => {
                out.push(BenchRow {
                    ts: r.get("ts")?,
                    modelo: r.get("modelo")?,
                    runtime: r.get("runtime")?,
                    tipo: r.get("tipo")?,
                    n_prompt: r.get("n_prompt")?,
                    n_gen: r.get("n_gen")?,
                    tok_s: r.get("tok_s")?,
                    desviacion: r.get("desviacion")?,
                    build: r.get("build")?,
                    gpu: r.get("gpu")?,
                });
                if out.len() as i64 >= limit {
                    break;
                }
            }
            None => break,
        }
    }
    Ok(out)
}

/* ── Uso (lo que se ha servido por la puerta de enlace) ───────────────────── */

/// Una petición registrada por la puerta de enlace.
///
/// Campos `Option` a propósito: el motor no siempre publica los tokens ni el
/// tiempo hasta el primer token, y un `0` ahí afirmaría un dato que no existe.
#[derive(Debug, Clone, Serialize, Default)]
pub struct UsoFila {
    pub ts: i64,
    pub modelo: String,
    pub ruta: String,
    pub metodo: String,
    pub estado: i64,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub generacion_ms: Option<i64>,
    pub duracion_ms: i64,
    pub bytes_entrada: i64,
    pub bytes_salida: i64,
    pub origen: String,
    pub cliente: String,
}

/// Lo que la vista de Uso agrega de un periodo. Cada cifra lleva también si hay
/// DATO: `peticiones` cuenta todas, pero los tokens solo suman las que los
/// publicaron, así que se devuelve cuántas los publicaron para poder decirlo.
#[derive(Debug, Clone, Serialize, Default)]
pub struct UsoResumen {
    pub peticiones: i64,
    pub con_tokens: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub con_ttft: i64,
    pub ttft_medio_ms: Option<f64>,
    /// Tokens de salida entre el tiempo de generación sumado: una media
    /// ponderada por trabajo, no la media de las medias (que daría más peso a
    /// una petición corta que a una larga).
    pub tok_s: Option<f64>,
}

/// Un punto de la actividad diaria (para la gráfica).
#[derive(Debug, Clone, Serialize)]
pub struct UsoDia {
    /// `YYYY-MM-DD` en hora LOCAL del equipo.
    pub dia: String,
    pub peticiones: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

/// Un modelo con uso en el periodo, para el selector y el desglose.
#[derive(Debug, Clone, Serialize)]
pub struct UsoModelo {
    pub modelo: String,
    pub peticiones: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

pub fn insert_uso(u: &UsoFila) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT INTO uso(ts, modelo, ruta, metodo, estado, prompt_tokens, completion_tokens,
                         cached_tokens, ttft_ms, generacion_ms, duracion_ms, bytes_entrada,
                         bytes_salida, origen, cliente)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            u.ts,
            u.modelo,
            u.ruta,
            u.metodo,
            u.estado,
            u.prompt_tokens,
            u.completion_tokens,
            u.cached_tokens,
            u.ttft_ms,
            u.generacion_ms,
            u.duracion_ms,
            u.bytes_entrada,
            u.bytes_salida,
            u.origen,
            u.cliente,
        ],
    )?;
    Ok(())
}

/// Últimas peticiones, con un filtro opcional por modelo (`None` = todos).
///
/// Se excluyen las que no son de inferencia (un `GET /health` o el listado de
/// modelos): sumarían "peticiones" sin decir nada de lo que se ha servido. El
/// filtro va por `completion_tokens IS NOT NULL OR prompt_tokens IS NOT NULL`, que
/// es exactamente "el motor publicó uso de esta petición".
pub fn uso_reciente(modelo: Option<&str>, limit: i64) -> anyhow::Result<Vec<UsoFila>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare(
        "SELECT ts, modelo, ruta, metodo, estado, prompt_tokens, completion_tokens,
                cached_tokens, ttft_ms, generacion_ms, duracion_ms, bytes_entrada,
                bytes_salida, origen, cliente
         FROM uso
         WHERE (?1 IS NULL OR modelo = ?1)
         ORDER BY ts DESC, id DESC
         LIMIT ?2",
    )?;
    let mut rows = stmt.query(params![modelo, limit])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(UsoFila {
            ts: r.get("ts")?,
            modelo: r.get("modelo")?,
            ruta: r.get("ruta")?,
            metodo: r.get("metodo")?,
            estado: r.get("estado")?,
            prompt_tokens: r.get("prompt_tokens")?,
            completion_tokens: r.get("completion_tokens")?,
            cached_tokens: r.get("cached_tokens")?,
            ttft_ms: r.get("ttft_ms")?,
            generacion_ms: r.get("generacion_ms")?,
            duracion_ms: r.get("duracion_ms")?,
            bytes_entrada: r.get("bytes_entrada")?,
            bytes_salida: r.get("bytes_salida")?,
            origen: r.get("origen")?,
            cliente: r.get("cliente")?,
        });
    }
    Ok(out)
}

/// Las peticiones que el motor reconoció como INFERENCIA.
///
/// El criterio es "publicó uso": una petición que no lo trae (un `GET /v1/models`,
/// un `HEAD`, o un motor que no informa) no es un turno de generación y contarla
/// inflaría el número de peticiones. Es el mismo criterio para el resumen, el
/// desglose por modelo y la actividad diaria, así que no pueden discrepar.
const SOLO_INFERENCIA: &str =
    "(prompt_tokens IS NOT NULL OR completion_tokens IS NOT NULL)";

/// Resumen de un periodo. `desde` es la marca de tiempo mínima (0 = todo).
pub fn uso_resumen(desde: i64, modelo: Option<&str>) -> anyhow::Result<UsoResumen> {
    let c = conn_guard()?;
    let sql = format!(
        "SELECT
            COUNT(*) AS peticiones,
            SUM(CASE WHEN completion_tokens IS NOT NULL OR prompt_tokens IS NOT NULL THEN 1 ELSE 0 END) AS con_tokens,
            COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
            COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
            COALESCE(SUM(cached_tokens), 0) AS cached_tokens,
            SUM(CASE WHEN ttft_ms IS NOT NULL THEN 1 ELSE 0 END) AS con_ttft,
            AVG(ttft_ms) AS ttft_medio,
            COALESCE(SUM(completion_tokens), 0) AS gen_tokens,
            SUM(generacion_ms) AS gen_ms
         FROM uso
         WHERE ts >= ?1 AND (?2 IS NULL OR modelo = ?2)"
    );
    let mut stmt = c.prepare(&sql)?;
    let r = stmt.query_row(params![desde, modelo], |r| {
        let gen_tokens: f64 = r.get("gen_tokens")?;
        let gen_ms: Option<f64> = r.get("gen_ms")?;
        let ttft_medio: Option<f64> = r.get("ttft_medio")?;
        let tok_s = match gen_ms {
            Some(ms) if ms > 0.0 => Some(gen_tokens / (ms / 1000.0)),
            _ => None,
        };
        Ok(UsoResumen {
            peticiones: r.get("peticiones")?,
            con_tokens: r.get("con_tokens")?,
            prompt_tokens: r.get("prompt_tokens")?,
            completion_tokens: r.get("completion_tokens")?,
            cached_tokens: r.get("cached_tokens")?,
            con_ttft: r.get("con_ttft")?,
            ttft_medio_ms: ttft_medio,
            tok_s,
        })
    })?;
    Ok(r)
}

/// Actividad por día LOCAL, para la gráfica. Se devuelve del más viejo al más
/// nuevo (como se pinta).
///
/// El día se calcula con `date(ts, 'unixepoch', 'localtime')`: si se calculara
/// en UTC, un turno de las 23:30 saldría contado en el día siguiente y la gráfica
/// mentiría sobre cuándo se trabajó.
pub fn uso_diario(desde: i64, modelo: Option<&str>) -> anyhow::Result<Vec<UsoDia>> {
    let c = conn_guard()?;
    let sql = format!(
        "SELECT date(ts, 'unixepoch', 'localtime') AS dia,
                COUNT(*) AS peticiones,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens
         FROM uso
         WHERE ts >= ?1 AND (?2 IS NULL OR modelo = ?2) AND {SOLO_INFERENCIA}
         GROUP BY dia ORDER BY dia ASC"
    );
    let mut stmt = c.prepare(&sql)?;
    let mut rows = stmt.query(params![desde, modelo])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(UsoDia {
            dia: r.get("dia")?,
            peticiones: r.get("peticiones")?,
            prompt_tokens: r.get("prompt_tokens")?,
            completion_tokens: r.get("completion_tokens")?,
        });
    }
    Ok(out)
}

/// Uso por modelo en el periodo, de más a menos peticiones.
pub fn uso_por_modelo(desde: i64) -> anyhow::Result<Vec<UsoModelo>> {
    let c = conn_guard()?;
    let sql = format!(
        "SELECT modelo, COUNT(*) AS peticiones,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens
         FROM uso
         WHERE ts >= ?1 AND {SOLO_INFERENCIA}
         GROUP BY modelo ORDER BY COUNT(*) DESC, modelo ASC"
    );
    let mut stmt = c.prepare(&sql)?;
    let mut rows = stmt.query(params![desde])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(UsoModelo {
            modelo: r.get("modelo")?,
            peticiones: r.get("peticiones")?,
            prompt_tokens: r.get("prompt_tokens")?,
            completion_tokens: r.get("completion_tokens")?,
        });
    }
    Ok(out)
}

/// Borra el uso anterior a `antes`. Se llama desde el mismo bucle que ya limpia
/// métricas, para que esto no crezca sin fin.
pub fn purgar_uso(antes: i64) -> anyhow::Result<usize> {
    let c = conn_guard()?;
    let n = c.execute("DELETE FROM uso WHERE ts < ?1", params![antes])?;
    Ok(n)
}

/* ── Histórico del uso de disco (instantáneas) ────────────────────────────── */

/// Días de histórico de disco que se conservan: un punto por día y raíz. Sin este
/// tope la tabla crecería sin fin, porque un pase del hogar son cientos de filas
/// (la raíz más sus hijos) y el analizador se puede lanzar muchas veces.
pub const HISTORIAL_DIAS: i64 = 90;

/// Tope de filas de `instantaneas`, como red de seguridad GLOBAL. La retención por
/// raíz ya limita cada carpeta que se repite, pero las carpetas NUEVAS que se
/// analizan cada día no las limita nadie: al pasarse del tope, se borran los pases
/// más viejos, sean de la raíz que sean.
const MAX_FILAS_HISTORIAL: i64 = 400_000;

/// Guarda una medida (la raíz y sus hijos) y aplica la retención.
pub fn guardar_instantanea(i: &Instantanea) -> anyhow::Result<()> {
    let mut c = conn_guard()?;
    guardar_instantanea_en(&mut c, i)
}

/// La misma operación sobre una conexión cualquiera: es lo que permite probar la
/// ida y vuelta con una base temporal sin tocar la del usuario.
fn guardar_instantanea_en(c: &mut Connection, i: &Instantanea) -> anyhow::Result<()> {
    // Todo el pase va en UNA transacción: si se fuera a medias, quedaría una raíz
    // sin hijos (o al revés) y una comparación mentiría sin que se note.
    let tx = c.transaction()?;
    let excluidos = serde_json::to_string(&i.excluidos)?;
    tx.execute(
        "INSERT INTO instantaneas(ts, raiz, ruta, es_raiz, nombre, bytes, ficheros, dirs, truncado, excluidos, resto_n, resto_bytes)
         VALUES (?1, ?2, ?3, 1, '', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            i.ts,
            i.ruta,
            i.ruta,
            i.bytes as i64,
            i.ficheros as i64,
            i.dirs as i64,
            i.truncado as i64,
            excluidos,
            i.resto_n as i64,
            i.resto_bytes as i64
        ],
    )?;
    for h in &i.hijos {
        tx.execute(
            "INSERT INTO instantaneas(ts, raiz, ruta, es_raiz, nombre, bytes, ficheros, dirs, truncado, excluidos, resto_n, resto_bytes)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, 0, '[]', 0, 0)",
            params![
                i.ts,
                i.ruta,
                h.ruta,
                h.nombre,
                h.bytes as i64,
                h.ficheros as i64,
                h.dirs as i64
            ],
        )?;
    }
    retener_raiz(&tx, &i.ruta, HISTORIAL_DIAS)?;
    podar_global(&tx, MAX_FILAS_HISTORIAL)?;
    tx.commit()?;
    Ok(())
}

/// Las últimas `limite` medidas de `ruta` (la más reciente primero), cada una con
/// los hijos que se guardaron en su mismo pase.
pub fn instantaneas(ruta: &str, limite: usize) -> anyhow::Result<Vec<Instantanea>> {
    let c = conn_guard()?;
    instantaneas_en(&c, ruta, limite)
}

fn instantaneas_en(c: &Connection, ruta: &str, limite: usize) -> anyhow::Result<Vec<Instantanea>> {
    // Los hijos se leen DESPUÉS y con la sentencia de la raíz ya cerrada: si no,
    // el préstamo de la conexión se solaparía con el de cada hijo.
    let mut out: Vec<Instantanea> = {
        let mut stmt = c.prepare(
            "SELECT ts, ruta, bytes, ficheros, dirs, truncado, excluidos, resto_n, resto_bytes
             FROM instantaneas WHERE ruta = ?1 AND es_raiz = 1
             ORDER BY ts DESC LIMIT ?2",
        )?;
        let filas = stmt.query_map(params![ruta, limite as i64], |r| {
            let excluidos: String = r.get(6)?;
            Ok(Instantanea {
                ts: r.get(0)?,
                ruta: r.get(1)?,
                bytes: r.get::<_, i64>(2)? as u64,
                ficheros: r.get::<_, i64>(3)? as u64,
                dirs: r.get::<_, i64>(4)? as u64,
                truncado: r.get::<_, i64>(5)? != 0,
                excluidos: serde_json::from_str(&excluidos).unwrap_or_default(),
                resto_n: r.get::<_, i64>(7)? as u64,
                resto_bytes: r.get::<_, i64>(8)? as u64,
                hijos: Vec::new(),
            })
        })?;
        filas.collect::<Result<Vec<_>, _>>()?
    };
    for i in &mut out {
        i.hijos = hijos_en(c, ruta, i.ts)?;
    }
    Ok(out)
}

/// Una medida concreta (por su `ts`), con sus hijos. Se usa para la base de la
/// comparación «hacia atrás», que no es la segunda más reciente.
fn instantanea_en(c: &Connection, raiz: &str, ts: i64) -> anyhow::Result<Option<Instantanea>> {
    let mut i = {
        let mut stmt = c.prepare(
            "SELECT ts, ruta, bytes, ficheros, dirs, truncado, excluidos, resto_n, resto_bytes
             FROM instantaneas WHERE ruta = ?1 AND es_raiz = 1 AND ts = ?2 LIMIT 1",
        )?;
        stmt.query_row(params![raiz, ts], |r| {
            let excluidos: String = r.get(6)?;
            Ok(Instantanea {
                ts: r.get(0)?,
                ruta: r.get(1)?,
                bytes: r.get::<_, i64>(2)? as u64,
                ficheros: r.get::<_, i64>(3)? as u64,
                dirs: r.get::<_, i64>(4)? as u64,
                truncado: r.get::<_, i64>(5)? != 0,
                excluidos: serde_json::from_str(&excluidos).unwrap_or_default(),
                resto_n: r.get::<_, i64>(7)? as u64,
                resto_bytes: r.get::<_, i64>(8)? as u64,
                hijos: Vec::new(),
            })
        })
        .optional()?
    };
    if let Some(i) = &mut i {
        i.hijos = hijos_en(c, raiz, i.ts)?;
    }
    Ok(i)
}

fn hijos_en(c: &Connection, raiz: &str, ts: i64) -> anyhow::Result<Vec<HijoInstantanea>> {
    let mut stmt = c.prepare(
        "SELECT ruta, nombre, bytes, ficheros, dirs FROM instantaneas
         WHERE raiz = ?1 AND ts = ?2 AND es_raiz = 0 ORDER BY bytes DESC, nombre ASC",
    )?;
    let filas = stmt.query_map(params![raiz, ts], |r| {
        Ok(HijoInstantanea {
            ruta: r.get(0)?,
            nombre: r.get(1)?,
            bytes: r.get::<_, i64>(2)? as u64,
            ficheros: r.get::<_, i64>(3)? as u64,
            dirs: r.get::<_, i64>(4)? as u64,
        })
    })?;
    Ok(filas.collect::<Result<Vec<_>, _>>()?)
}

/// La comparación entre las DOS últimas medidas de `ruta`. `None` si hay menos de
/// dos: sin con qué comparar no se inventa un crecimiento.
pub fn crecimiento(ruta: &str) -> anyhow::Result<Option<Crecimiento>> {
    let c = conn_guard()?;
    ultimo_crecimiento(&c, ruta)
}

fn ultimo_crecimiento(c: &Connection, ruta: &str) -> anyhow::Result<Option<Crecimiento>> {
    Ok(comparar_ultimas(&instantaneas_en(c, ruta, 2)?))
}

/// La comparación entre la última medida de `ruta` y la más reciente que sea de
/// hace AL MENOS `dias` (es la base que usa Inicio para «la última semana»).
///
/// `None` si el histórico no llega tan atrás: sin una medida de entonces no hay
/// semana que comparar, y devolver la última comparación disponible como si fuera
/// de una semana sería mentir con la fecha del aviso.
pub fn crecimiento_en(ruta: &str, dias: i64) -> anyhow::Result<Option<Crecimiento>> {
    let c = conn_guard()?;
    crecimiento_hacia_atras(&c, ruta, dias)
}

fn crecimiento_hacia_atras(
    c: &Connection,
    ruta: &str,
    dias: i64,
) -> anyhow::Result<Option<Crecimiento>> {
    let Some(ahora) = instantaneas_en(c, ruta, 1)?.into_iter().next() else {
        return Ok(None);
    };
    let corte = ahora.ts - dias.max(1) * 86_400;
    let base_ts: Option<i64> = {
        let mut stmt = c.prepare(
            "SELECT ts FROM instantaneas WHERE ruta = ?1 AND es_raiz = 1 AND ts <= ?2
             ORDER BY ts DESC LIMIT 1",
        )?;
        stmt.query_row(params![ruta, corte], |r| r.get(0)).optional()?
    };
    let Some(base_ts) = base_ts else {
        return Ok(None);
    };
    let Some(base) = instantanea_en(c, ruta, base_ts)? else {
        return Ok(None);
    };
    Ok(Some(comparar(&base, &ahora)))
}

/// Deja, como mucho, un pase por día local y no más de `dias` días para esa raíz.
///
/// El pase más reciente se conserva SIEMPRE (es el que se compara). Devuelve
/// cuántas filas se han ido. Se agrupa por día LOCAL y no UTC porque el usuario
/// analiza en su hora: dos medidas a las 23:30 y a las 00:30 son de dos días
/// distintos para él.
fn retener_raiz(c: &Connection, raiz: &str, dias: i64) -> anyhow::Result<usize> {
    let pases: Vec<i64> = {
        let mut stmt =
            c.prepare("SELECT DISTINCT ts FROM instantaneas WHERE raiz = ?1 ORDER BY ts DESC")?;
        let filas = stmt.query_map(params![raiz], |r| r.get(0))?;
        filas.collect::<Result<Vec<_>, _>>()?
    };
    let mut dias_vistos: BTreeSet<String> = BTreeSet::new();
    let mut conservar: Vec<i64> = Vec::new();
    for (i, ts) in pases.iter().enumerate() {
        let dia = dia_local(*ts);
        if i == 0 {
            dias_vistos.insert(dia);
            conservar.push(*ts);
            continue;
        }
        // Ya hay un punto (más nuevo) de ese día: este es un análisis repetido.
        if dias_vistos.contains(&dia) {
            continue;
        }
        // Ya hay un punto por cada uno de los `dias` días que se conservan.
        if dias_vistos.len() as i64 >= dias {
            continue;
        }
        dias_vistos.insert(dia);
        conservar.push(*ts);
    }
    let mut borrados = 0;
    for ts in &pases {
        if !conservar.contains(ts) {
            borrados += c.execute(
                "DELETE FROM instantaneas WHERE raiz = ?1 AND ts = ?2",
                params![raiz, ts],
            )?;
        }
    }
    Ok(borrados)
}

/// El día LOCAL de una marca UNIX, como `YYYY-MM-DD`, para agrupar un punto por
/// día. Si el `ts` no es representable, se usa el número: es preferible a tratarlo
/// como si fuera de hoy.
fn dia_local(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// Red de seguridad GLOBAL: si la tabla pasa del tope, borra los pases más viejos
/// (de cualquier raíz) hasta bajar. Devuelve cuántas filas se han ido.
fn podar_global(c: &Connection, max_filas: i64) -> anyhow::Result<usize> {
    let mut total: i64 = c.query_row("SELECT count(*) FROM instantaneas", [], |r| r.get(0))?;
    if total <= max_filas {
        return Ok(0);
    }
    let pases: Vec<(String, i64)> = {
        let mut stmt =
            c.prepare("SELECT raiz, ts FROM instantaneas GROUP BY raiz, ts ORDER BY ts ASC")?;
        let filas = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        filas.collect::<Result<Vec<_>, _>>()?
    };
    let mut borrados = 0;
    for (raiz, ts) in &pases {
        if total <= max_filas {
            break;
        }
        let n = c.execute(
            "DELETE FROM instantaneas WHERE raiz = ?1 AND ts = ?2",
            params![raiz, ts],
        )?;
        total -= n as i64;
        borrados += n;
    }
    Ok(borrados)
}

/// Día (local, `YYYY-MM-DD`) de la última medida diaria de disco. Vive en `meta`
/// porque no es un ajuste del usuario: es estado de la app, como la bandera de
/// siembra de los servidores.
pub fn historial_ultimo_dia() -> anyhow::Result<Option<String>> {
    let c = conn_guard()?;
    let mut stmt = c.prepare("SELECT value FROM meta WHERE key = 'historial_dia'")?;
    let mut rows = stmt.query([])?;
    match rows.next()? {
        Some(r) => Ok(Some(r.get(0)?)),
        None => Ok(None),
    }
}

/// Anota que la medida diaria de disco ya se hizo hoy (para no repetirla en cada
/// vuelta del bucle).
pub fn marcar_historial_dia(dia: &str) -> anyhow::Result<()> {
    let c = conn_guard()?;
    c.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES ('historial_dia', ?1)",
        params![dia],
    )?;
    Ok(())
}

/* ── Pruebas ────────────────────────────────────────────────────────────────── */

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Carpeta de prueba propia, en el temporal del sistema.
    fn carpeta(nombre: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("machinograph-prueba-{}-{nombre}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    /// El fallo que MATABA la aplicación: una ruta imposible tiene que dar `Err`.
    ///
    /// `Connection::open(...).expect(...)` con el perfil de release (`panic =
    /// "abort"`) cerraba el proceso sin mensaje cuando la carpeta no se podía
    /// crear (disco lleno, permisos, un fichero donde debería haber un
    /// directorio). Se prueba con el caso fácil de reproducir: el "directorio" de
    /// la BD es un fichero normal.
    #[test]
    fn una_bd_imposible_da_error_en_vez_de_panico() {
        let base = carpeta("imposible");
        std::fs::create_dir_all(&base).unwrap();
        let fichero = base.join("esto-es-un-fichero");
        std::fs::write(&fichero, b"x").unwrap();

        let r = connect_to(&fichero.join("data.db"));

        let e = r.err().expect("tiene que devolver Err, no entrar en pánico");
        assert!(
            e.to_string().contains("no se pudo"),
            "el motivo tiene que ser legible para poder enseñarlo: {e}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Una ruta normal sí abre, deja la BD en WAL y crea las carpetas que falten.
    #[test]
    fn una_bd_normal_abre_en_wal() {
        let base = carpeta("wal");
        let ruta = base.join("sub").join("data.db");

        let c = connect_to(&ruta).expect("tiene que abrir sin problema");
        let modo: String = c
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();

        assert_eq!(
            modo.to_lowercase(),
            "wal",
            "sin WAL vuelve el `database is locked`"
        );
        assert!(ruta.is_file(), "el fichero tiene que quedar creado");
        drop(c);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Una base que no se puede abrir (basura en el fichero) se APARTA con fecha y
    /// se crea una nueva. Y, sobre todo: la basura SIGUE ahí. Se comprueba con
    /// basura de verdad, no con una simulación.
    #[test]
    fn una_bd_corrupta_se_aparta_se_crea_otra_y_la_basura_sigue_existiendo() {
        let base = carpeta("corrupta");
        std::fs::create_dir_all(&base).unwrap();
        let ruta = base.join("data.db");
        let basura: &[u8] = b"esto no es una base de datos: es basura";
        std::fs::write(&ruta, basura).unwrap();

        let (c, reparacion) = connect_reparando(&ruta).expect("tiene que reparar y abrir");

        // Se dice QUÉ se hizo, con la ruta de lo apartado.
        let aviso = reparacion.expect("tiene que contarlo, no reparar en silencio");
        assert!(aviso.contains(".corrupta-"), "el aviso dice dónde quedó: {aviso}");
        assert!(aviso.contains("SIN BORRARLA"), "y que no se borró nada: {aviso}");

        // (a) hay base nueva y USABLE: acepta el esquema y escribe.
        init(&c).expect("la base nueva tiene que aceptar el esquema");
        c.execute("INSERT INTO meta(key, value) VALUES ('prueba', '1')", []).unwrap();
        let n: i64 = c
            .query_row("SELECT count(*) FROM meta WHERE key='prueba'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "la base nueva tiene que poder escribir");
        drop(c);

        // (b) la basura sigue existiendo, en el fichero apartado.
        let apartada = std::fs::read_dir(&base)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                let s = p.to_string_lossy();
                s.contains(".corrupta-") && !s.ends_with("-wal") && !s.ends_with("-shm")
            })
            .expect("tiene que quedar el fichero apartado");
        assert_eq!(
            std::fs::read(&apartada).unwrap(),
            basura,
            "lo apartado tiene que ser el fichero de antes, byte a byte"
        );
        assert!(ruta.is_file(), "y la base nueva tiene que estar en su sitio");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Una base que se abre pero está rota por dentro la caza `PRAGMA
    /// quick_check`: se le destrozan las páginas conservando la cabecera, así que
    /// SQLite la abre y el que tiene que darse cuenta es la comprobación.
    #[test]
    fn una_bd_ilegible_por_dentro_tambien_se_aparta() {
        let base = carpeta("quick");
        std::fs::create_dir_all(&base).unwrap();
        let ruta = base.join("data.db");
        {
            let c = connect_to(&ruta).unwrap();
            init(&c).unwrap();
            c.execute("INSERT INTO meta(key, value) VALUES ('a', 'b')", []).unwrap();
            // Todo al fichero principal: si quedara en el WAL, dañar el fichero no
            // se notaría.
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        }
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&ruta).unwrap();
            f.seek(SeekFrom::Start(100)).unwrap();
            f.write_all(&[0xFFu8; 900]).unwrap();
        }

        let (c, reparacion) = connect_reparando(&ruta).expect("tiene que apartar y abrir otra");

        assert!(reparacion.is_some(), "una base rota por dentro hay que apartarla");
        init(&c).expect("la nueva tiene que ser usable");
        drop(c);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Una base SANA no se toca ni se aparta: reparar lo que está bien sería
    /// destruir el histórico del usuario.
    #[test]
    fn una_bd_sana_no_se_toca() {
        let base = carpeta("sana");
        std::fs::create_dir_all(&base).unwrap();
        let ruta = base.join("data.db");
        {
            let c = connect_to(&ruta).unwrap();
            init(&c).unwrap();
            c.execute("INSERT INTO meta(key, value) VALUES ('sana', '1')", []).unwrap();
        }

        let (c, reparacion) = connect_reparando(&ruta).expect("tiene que abrir");

        assert!(reparacion.is_none(), "no había nada que reparar");
        let n: i64 = c
            .query_row("SELECT count(*) FROM meta WHERE key='sana'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "los datos tienen que seguir ahí");
        drop(c);
        let apartados = std::fs::read_dir(&base)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupta-"))
            .count();
        assert_eq!(apartados, 0, "no puede haber apartado nada");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Una medida de prueba sobre `/tmp/x`, con los hijos que se le pasen.
    fn media_de_prueba(ts: i64, bytes: u64, hijos: &[(&str, u64)]) -> Instantanea {
        Instantanea {
            ts,
            ruta: "/tmp/x".into(),
            bytes,
            ficheros: 10,
            dirs: 2,
            truncado: false,
            excluidos: vec![],
            resto_n: 0,
            resto_bytes: 0,
            hijos: hijos
                .iter()
                .map(|(n, b)| HijoInstantanea {
                    ruta: format!("/tmp/x/{n}"),
                    nombre: (*n).into(),
                    bytes: *b,
                    ficheros: 1,
                    dirs: 0,
                })
                .collect(),
        }
    }

    /// Ida y vuelta del histórico con una base TEMPORAL: se guardan dos medidas y
    /// se lee el crecimiento. Comprueba que la tabla, el índice y la conversión de
    /// fila a `Instantanea` cuadran, y que con UNA sola medida no hay comparación.
    #[test]
    fn ida_y_vuelta_del_historial_de_disco() {
        let base = carpeta("historial-disco");
        std::fs::create_dir_all(&base).unwrap();
        let ruta = base.join("data.db");
        let mut c = connect_to(&ruta).unwrap();
        init(&c).unwrap();

        let antes = media_de_prueba(1_700_000_000, 1_000, &[("grande", 600), ("pequena", 400)]);
        let ahora = media_de_prueba(1_700_086_400, 1_300, &[("grande", 900), ("pequena", 300)]);

        guardar_instantanea_en(&mut c, &antes).unwrap();
        // Con UNA sola medida no hay comparación: `None`, nunca un cero falso.
        assert!(ultimo_crecimiento(&c, "/tmp/x").unwrap().is_none());

        guardar_instantanea_en(&mut c, &ahora).unwrap();

        let v = instantaneas_en(&c, "/tmp/x", 10).unwrap();
        assert_eq!(v.len(), 2, "las dos medidas tienen que estar");
        assert_eq!(v[0].ts, ahora.ts, "la más reciente va primero");
        assert_eq!(v[0].hijos.len(), 2, "los hijos se guardan con su pase");
        assert_eq!(v[0].hijos[0].nombre, "grande", "los hijos van de mayor a menor");

        let cr = ultimo_crecimiento(&c, "/tmp/x").unwrap().expect("hay dos medidas");
        assert_eq!(cr.delta_bytes, 300);
        assert_eq!(cr.delta_ficheros, 0);
        assert!(!cr.parcial);
        assert_eq!(cr.hijos[0].nombre, "grande");
        assert_eq!(cr.hijos[0].delta, 300);

        // La comparación «hacia atrás» que usa Inicio: exige una base de hace AL
        // MENOS `dias`. Con medidas separadas 1 día, hay base para 1 día y no la
        // hay para 2, así que no se inventa una semana que no existe.
        assert!(crecimiento_hacia_atras(&c, "/tmp/x", 1).unwrap().is_some());
        assert!(crecimiento_hacia_atras(&c, "/tmp/x", 2).unwrap().is_none());

        drop(c);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// La retención deja UN punto por día: analizar tres veces el mismo día no
    /// guarda tres puntos, y el más reciente no se toca.
    #[test]
    fn la_retencion_deja_un_punto_por_dia() {
        use chrono::{Local, TimeZone};
        let base = carpeta("historial-retencion");
        std::fs::create_dir_all(&base).unwrap();
        let ruta = base.join("data.db");
        let mut c = connect_to(&ruta).unwrap();
        init(&c).unwrap();

        // Momentos a la misma hora de días distintos: así el día LOCAL de cada uno
        // no depende de en qué momento se ejecute la prueba.
        let ts_de = |dias_atras: i64, hora: u32| {
            let dia = Local::now().date_naive() - chrono::Duration::days(dias_atras);
            Local
                .from_local_datetime(&dia.and_hms_opt(hora, 0, 0).unwrap())
                .single()
                .unwrap()
                .timestamp()
        };
        let t_ayer = ts_de(1, 12);
        let t_hoy_10 = ts_de(0, 10);
        let t_hoy_11 = ts_de(0, 11);
        let t_hoy_12 = ts_de(0, 12);

        guardar_instantanea_en(&mut c, &media_de_prueba(t_ayer, 100, &[])).unwrap();
        guardar_instantanea_en(&mut c, &media_de_prueba(t_hoy_10, 200, &[])).unwrap();
        guardar_instantanea_en(&mut c, &media_de_prueba(t_hoy_11, 300, &[])).unwrap();
        guardar_instantanea_en(&mut c, &media_de_prueba(t_hoy_12, 400, &[])).unwrap();

        let v = instantaneas_en(&c, "/tmp/x", 10).unwrap();
        assert_eq!(v.len(), 2, "un punto por día: ayer y hoy");
        assert_eq!(v[0].ts, t_hoy_12, "el más reciente de hoy es el que queda");
        assert_eq!(v[1].ts, t_ayer, "y el de ayer se conserva para comparar");

        drop(c);
        let _ = std::fs::remove_dir_all(&base);
    }
}
