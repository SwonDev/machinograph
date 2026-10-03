//! Analizador de disco: qué ocupa el espacio, ordenado por tamaño, con búsqueda
//! y borrado controlado.
//!
//! POR QUÉ EXISTE: el inventario de modelos sabe lo que ocupan los MODELOS
//! (`.gguf`, `safetensors`, `.onnx`…), pero no el resto del disco. Un disco lleno
//! casi nunca lo llena un modelo: lo llenan `node_modules`, cachés de
//! compilación, snapshots, vídeos y descargas. Esta sección responde a "¿qué se
//! está comiendo el disco?" y deja borrar desde aquí.
//!
//! Cuatro decisiones que están tomadas a conciencia:
//!
//! 1. **Un nivel por consulta, como `du --max-depth=1`.** Se recorre el árbol
//!    ENTERO para saber cuánto ocupa cada hijo, pero solo se devuelven los hijos
//!    directos: bajar a una carpeta es volver a preguntar por ella. Así el
//!    contenido de la respuesta no depende de lo profundo que sea el árbol y la
//!    interfaz puede ir carpeta a carpeta sin traerse el disco entero.
//! 2. **Presupuesto de tiempo y de entradas.** Sin tope, recorrer un home grande
//!    bloquea el hilo minutos. Se corta a las 500 000 entradas o a los 25 s y se
//!    DICE que se ha cortado; un resultado incompleto presentado como completo
//!    sería peor que no darlo.
//! 3. **Los enlaces simbólicos no se siguen.** Ni para medir ni para borrar: un
//!    enlace a `/` dentro de una carpeta haría que "su tamaño" fuese el del disco
//!    y que borrarla se llevara medio sistema.
//! 4. **Borrar es cosa aparte y con lista blanca.** `inventario` borra dentro de
//!    las carpetas de modelos; aquí se borra dentro del home o de las rutas de
//!    caché/temporales del sistema, nunca una raíz, nunca un enlace.
use crate::exclusiones::{self, Vigente};
use serde::Serialize;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Tope de entradas vistas en una consulta. Con más, el resultado se marca
/// truncado en vez de fingir que se ha recorrido todo.
///
/// La cifra está medida en este equipo: el home tiene ~460 000 entradas y se
/// recorre en ~9 s, así que un tope de 500 000 marcaba como truncado un análisis
/// NORMAL (y el total salía corto sin necesidad). Con 2 000 000 el home completo
/// entra y el tope solo salta en árboles de verdad patológicos.
const MAX_ENTRADAS: u64 = 2_000_000;
/// Tope de tiempo por consulta. El disco no se puede quedar la ventana esperando.
const MAX_TIEMPO: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Serialize)]
pub struct Nodo {
    pub ruta: String,
    pub nombre: String,
    pub bytes: u64,
    /// Ficheros contenidos (recursivo). En un fichero, 1.
    pub ficheros: u64,
    /// Carpetas contenidas (recursivo), sin contarse a sí misma.
    pub dirs: u64,
    pub es_dir: bool,
    pub modificado: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Arbol {
    pub ruta: String,
    pub bytes: u64,
    pub ficheros: u64,
    pub dirs: u64,
    /// Hijos directos, de mayor a menor tamaño.
    pub hijos: Vec<Nodo>,
    /// Cuántos hijos NO se enseñan (se conservan los `max_hijos` más grandes) y
    /// cuánto suman, para poder decirlo sin mentir con la lista.
    pub resto_n: u64,
    pub resto_bytes: u64,
    /// Se agotó el presupuesto de tiempo o de entradas: el total puede quedarse
    /// corto.
    pub truncado: bool,
    /// Entradas que no se pudieron leer (permisos), contadas aparte porque no son
    /// lo mismo que "no hay nada".
    pub omitidos: u64,
    pub entradas: u64,
    pub ms: u64,
    /// Patrones de exclusión que han dejado algo FUERA de esta medición: solo los
    /// que han actuado de verdad, no todos los configurados. Se devuelven para
    /// poder decirlo: un total que encoge sin explicación parece un fallo.
    pub excluidos: Vec<String>,
}

/// Un disco montado. `usado`/`libre`/`total` van en BYTES (la interfaz decide
/// cómo se escriben); el backend no redondea nada.
#[derive(Debug, Clone, Serialize)]
pub struct Montaje {
    pub punto: String,
    /// El dispositivo (`/dev/nvme1n1p3`, `C:`). Puede venir vacío si el sistema no
    /// lo publica: entonces la interfaz enseña el tipo.
    pub dispositivo: String,
    /// El sistema de ficheros (`btrfs`, `ext4`, `apfs`, `NTFS`).
    pub tipo: String,
    pub total: u64,
    pub usado: u64,
    pub libre: u64,
    pub uso_pct: f64,
    /// Unidad extraíble (un USB). Se enseña: borrar en un USB que se va a
    /// desconectar es distinto de borrar en el disco del sistema.
    pub extraible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fichero {
    pub ruta: String,
    pub nombre: String,
    pub bytes: u64,
    pub modificado: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Coincidencia {
    pub ruta: String,
    pub nombre: String,
    /// `null` en una carpeta: no se mide su subárbol porque una búsqueda puede
    /// devolver cientos y medir cada una sería recorrer el disco varias veces.
    /// Un `0` ahí se leería como "ocupa cero", que es falso.
    pub bytes: Option<u64>,
    pub es_dir: bool,
    pub modificado: Option<i64>,
}

/* ── El presupuesto y la medida ───────────────────────────────────────────── */

struct Presupuesto {
    entradas: u64,
    inicio: Instant,
    agotado: bool,
    omitidos: u64,
}

impl Presupuesto {
    fn nuevo() -> Self {
        Self { entradas: 0, inicio: Instant::now(), agotado: false, omitidos: 0 }
    }

    /// Apunta `n` entradas y dice si aún se puede seguir. Cuando se agota, el
    /// recorrido no se corta de golpe (los marcos abiertos se cierran solos),
    /// pero deja de abrir carpetas nuevas.
    fn gastar(&mut self, n: u64) -> bool {
        self.entradas += n;
        if self.entradas > MAX_ENTRADAS || self.inicio.elapsed() > MAX_TIEMPO {
            self.agotado = true;
            false
        } else {
            true
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Acum {
    bytes: u64,
    ficheros: u64,
    dirs: u64,
}

/// Un marco del recorrido iterativo. `hijos` se materializa al entrar para no
/// mantener abierto el descriptor del directorio mientras se baja al fondo.
struct Marco {
    hijos: std::vec::IntoIter<PathBuf>,
    acum: Acum,
}

/// El filtro de un recorrido: las exclusiones del usuario y el registro de cuáles
/// han dejado algo fuera.
///
/// POR QUÉ ES UN PARÁMETRO Y NO SE LEE AQUÍ DENTRO: si estas funciones leyeran la
/// base de datos del usuario, las pruebas —que recorren directorios temporales—
/// dependerían de la configuración de quien ejecuta `cargo test` y el resultado de
/// medir una carpeta no sería reproducible. La base de datos se toca UNA sola vez
/// por recorrido, en `Filtro::reales()`, que es lo que usan los comandos y la CLI;
/// `arbol()`/`grandes()`/… delegan en las variantes `_con` con un filtro VACÍO, que
/// es lo que mantiene su comportamiento de siempre.
///
/// El registro de qué actuó va en `RefCell` porque el filtro se pasa por
/// referencia compartida a todo el recorrido (de un solo hilo) y todas las ramas
/// anotan en el MISMO sitio: al final se dice qué patrones dejaron algo fuera, no
/// uno por rama.
pub struct Filtro {
    vigentes: Vec<Vigente>,
    caja_sensible: bool,
    actuado: RefCell<BTreeSet<String>>,
}

impl Filtro {
    /// Sin ninguna exclusión. Es lo que usan las funciones de siempre (`arbol`,
    /// `grandes`…) para no cambiar de comportamiento ni depender de la base de
    /// datos, y también las pruebas sobre directorios temporales.
    pub fn vacio() -> Self {
        Self {
            vigentes: Vec::new(),
            caja_sensible: exclusiones::sin_distinguir_caja(),
            actuado: RefCell::new(BTreeSet::new()),
        }
    }

    /// Con las exclusiones guardadas del usuario. Este es el ÚNICO constructor que
    /// lee la base de datos, y hay que llamarlo UNA vez por recorrido (no una por
    /// carpeta). Lo llaman los comandos y la CLI.
    pub fn reales() -> Self {
        Self {
            vigentes: exclusiones::vigentes(),
            caja_sensible: exclusiones::sin_distinguir_caja(),
            actuado: RefCell::new(BTreeSet::new()),
        }
    }

    /// Con exclusiones ya preparadas, sin pasar por la base de datos. Lo usan las
    /// pruebas para excluir rutas temporales conocidas.
    ///
    /// Existe SOLO para las pruebas (de ahí el `cfg(test)`): es la puerta que
    /// permite comparar un recorrido CON exclusiones contra el mismo recorrido con
    /// la lista vacía y demostrar que, sin exclusiones, el resultado es idéntico
    /// al de antes del filtro. El código de producción entra por `reales()`.
    #[cfg(test)]
    pub fn con_vigentes(vigentes: Vec<Vigente>) -> Self {
        Self {
            vigentes,
            caja_sensible: exclusiones::sin_distinguir_caja(),
            actuado: RefCell::new(BTreeSet::new()),
        }
    }

    /// Los patrones que han dejado algo fuera durante el recorrido, para poder
    /// decirlo. Ordenados (el `BTreeSet` lo garantiza).
    pub fn actuado(&self) -> Vec<String> {
        self.actuado.borrow().iter().cloned().collect()
    }

    /// ¿Esta ruta está excluida? Si lo está, se anota QUÉ patrón la dejó fuera.
    ///
    /// Sin exclusiones devuelve `false` sin tocar el disco ni el registro: un
    /// recorrido normal no paga NADA por esta comprobación.
    fn excluye(&self, ruta: &Path) -> bool {
        if self.vigentes.is_empty() {
            return false;
        }
        match exclusiones::excluida_con(&ruta.to_string_lossy(), &self.vigentes, self.caja_sensible)
        {
            Some(v) => {
                self.actuado.borrow_mut().insert(v.patron.clone());
                true
            }
            None => false,
        }
    }
}

/// Los hijos de una carpeta, sin seguir enlaces, contando los fallos y saltando lo
/// que el usuario haya excluido.
///
/// Devuelve además si el filtro dejó algo FUERA de ESTA carpeta: `vacias` lo
/// necesita para NO decir que una carpeta está vacía cuando lo que tiene dentro es
/// algo excluido —proponerla para borrar se llevaría por delante lo excluido—.
fn hijos_de(
    p: &Path,
    pres: &mut Presupuesto,
    filtro: &Filtro,
) -> (std::vec::IntoIter<PathBuf>, bool) {
    let mut v: Vec<PathBuf> = Vec::new();
    let mut excluido_algo = false;
    match std::fs::read_dir(p) {
        Ok(it) => {
            for e in it {
                match e {
                    Ok(x) => {
                        let hijo = x.path();
                        if filtro.excluye(&hijo) {
                            excluido_algo = true;
                            continue;
                        }
                        v.push(hijo);
                    }
                    Err(_) => pres.omitidos += 1,
                }
            }
        }
        Err(_) => pres.omitidos += 1,
    }
    (v.into_iter(), excluido_algo)
}

/// Tamaño RECURSIVO de un subárbol, con pila explícita (una recursión de verdad
/// se comería la pila del hilo en un árbol de directorios muy profundo, y ahí
/// `panic = "abort"` significa ventana cerrada sin mensaje).
fn medir(p: &Path, pres: &mut Presupuesto, filtro: &Filtro) -> Acum {
    // Agotado el presupuesto no se abre ni una carpeta más: devolver 0 es rápido
    // y el `truncado` de arriba avisa de que ese 0 no significa "vacío".
    if pres.agotado {
        return Acum::default();
    }
    let mut pila = vec![Marco { hijos: hijos_de(p, pres, filtro).0, acum: Acum::default() }];
    let mut total = Acum::default();
    loop {
        let siguiente = match pila.last_mut() {
            Some(m) => m.hijos.next(),
            None => break,
        };
        match siguiente {
            Some(hijo) => {
                if !pres.gastar(1) {
                    continue;
                }
                let meta = match std::fs::symlink_metadata(&hijo) {
                    Ok(m) => m,
                    Err(_) => {
                        pres.omitidos += 1;
                        continue;
                    }
                };
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    let hijos = hijos_de(&hijo, pres, filtro).0;
                    pila.push(Marco { hijos, acum: Acum::default() });
                } else if let Some(m) = pila.last_mut() {
                    m.acum.bytes += meta.len();
                    m.acum.ficheros += 1;
                }
            }
            None => {
                let fin = match pila.pop() {
                    Some(f) => f,
                    None => break,
                };
                match pila.last_mut() {
                    Some(padre) => {
                        padre.acum.bytes += fin.acum.bytes;
                        padre.acum.ficheros += fin.acum.ficheros;
                        padre.acum.dirs += fin.acum.dirs + 1;
                    }
                    None => total = fin.acum,
                }
            }
        }
    }
    total
}

fn nombre_de(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string_lossy().to_string())
}

fn modificado_de(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Tamaño recursivo de una ruta suelta (lo usa el borrado para informar de lo
/// liberado). Devuelve 0 si no se puede leer.
pub fn tamano(p: &Path) -> u64 {
    let meta = match std::fs::symlink_metadata(p) {
        Ok(m) => m,
        Err(_) => return 0,
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_dir() {
        let mut pres = Presupuesto::nuevo();
        // `tamano` informa de lo que ocupa una ruta al borrarla: mide el contenido
        // entero sin filtro (el borrado de esta sección no es una de las
        // herramientas que respetan exclusiones; quien las respeta es `limpieza`).
        medir(p, &mut pres, &Filtro::vacio()).bytes
    } else {
        meta.len()
    }
}

/* ── El árbol de un nivel ─────────────────────────────────────────────────── */

/// El árbol de una carpeta SIN aplicar exclusiones. Se conserva con esta firma
/// porque es la que usan las pruebas: el resultado de medir un directorio temporal
/// no puede depender de la lista de exclusiones del usuario. La aplicación usa
/// `arbol_con` con `Filtro::reales()`.
///
/// Existe SOLO para las pruebas (de ahí el `cfg(test)`): es la puerta «sin
/// exclusiones» con la que se comprueba que el filtro vacío da exactamente el
/// mismo resultado que antes de existir el filtro.
#[cfg(test)]
pub fn arbol(raiz: &str, max_hijos: usize) -> Result<Arbol, String> {
    arbol_con(raiz, max_hijos, &Filtro::vacio())
}

/// El árbol de una carpeta, respetando las exclusiones del `filtro`.
///
/// Si la RAÍZ está excluida no se mide ni un byte: se devuelve vacío y `excluidos`
/// dice qué patrón lo dejó así. Recorrerla entera pese a la exclusión sería
/// contradecir su promesa.
pub fn arbol_con(raiz: &str, max_hijos: usize, filtro: &Filtro) -> Result<Arbol, String> {
    let t0 = Instant::now();
    let p = Path::new(raiz);
    let meta = std::fs::symlink_metadata(p).map_err(|e| format!("no se puede leer {raiz}: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{raiz} es un enlace simbólico; no se recorre"));
    }
    if !meta.is_dir() {
        return Err(format!("{raiz} no es una carpeta"));
    }

    if filtro.excluye(p) {
        return Ok(Arbol {
            ruta: p.to_string_lossy().to_string(),
            bytes: 0,
            ficheros: 0,
            dirs: 0,
            hijos: Vec::new(),
            resto_n: 0,
            resto_bytes: 0,
            truncado: false,
            omitidos: 0,
            entradas: 0,
            ms: t0.elapsed().as_millis() as u64,
            excluidos: filtro.actuado(),
        });
    }

    let mut pres = Presupuesto::nuevo();
    let tope = max_hijos.clamp(1, 5000);
    // Se conservan los `tope` hijos más grandes; el resto se agrega. Un `heap`
    // mínimo de tamaño fijo evita guardar un millón de hijos en memoria.
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::new();
    let mut nodos: Vec<Option<Nodo>> = Vec::new();
    let mut total = Acum::default();
    let mut resto_n = 0u64;
    let mut resto_bytes = 0u64;

    for hijo in hijos_de(p, &mut pres, filtro).0 {
        // A propósito NO se corta la lista cuando el presupuesto se agota: eso
        // hacía DESAPARECER carpetas enteras de la lista (con el tope viejo, el
        // home de este equipo enseñaba solo 5 hijos y `~/models` no salía). El
        // hijo se enseña con lo que se haya podido medir, y `truncado` avisa.
        pres.gastar(1);
        let hm = match std::fs::symlink_metadata(&hijo) {
            Ok(m) => m,
            Err(_) => {
                pres.omitidos += 1;
                continue;
            }
        };
        if hm.file_type().is_symlink() {
            continue;
        }
        let es_dir = hm.is_dir();
        let ac = if es_dir {
            let a = medir(&hijo, &mut pres, filtro);
            total.dirs += a.dirs + 1;
            a
        } else {
            Acum { bytes: hm.len(), ficheros: 1, dirs: 0 }
        };
        total.bytes += ac.bytes;
        total.ficheros += ac.ficheros;

        let idx = nodos.len();
        nodos.push(Some(Nodo {
            ruta: hijo.to_string_lossy().to_string(),
            nombre: nombre_de(&hijo),
            bytes: ac.bytes,
            ficheros: ac.ficheros,
            // Carpetas CONTENIDAS, sin contarse a sí misma (ac.dirs).
            dirs: ac.dirs,
            es_dir,
            modificado: modificado_de(&hm),
        }));
        heap.push(Reverse((ac.bytes, idx)));
        if heap.len() > tope {
            if let Some(Reverse((_, i))) = heap.pop() {
                if let Some(n) = nodos[i].take() {
                    resto_n += 1;
                    resto_bytes += n.bytes;
                }
            }
        }
    }

    let mut hijos: Vec<Nodo> = nodos.into_iter().flatten().collect();
    hijos.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.nombre.cmp(&b.nombre)));

    Ok(Arbol {
        ruta: p.to_string_lossy().to_string(),
        bytes: total.bytes,
        ficheros: total.ficheros,
        dirs: total.dirs,
        hijos,
        resto_n,
        resto_bytes,
        truncado: pres.agotado,
        omitidos: pres.omitidos,
        entradas: pres.entradas,
        ms: t0.elapsed().as_millis() as u64,
        excluidos: filtro.actuado(),
    })
}

/* ── Los ficheros más grandes ─────────────────────────────────────────────── */

/// Los ficheros más grandes SIN exclusiones (firma de las pruebas).
///
/// Existe SOLO para las pruebas (de ahí el `cfg(test)`): es la puerta «sin
/// exclusiones» que garantiza que el resultado sin filtro es el de siempre.
#[cfg(test)]
pub fn grandes(raiz: &str, limite: usize) -> Result<Vec<Fichero>, String> {
    grandes_con(raiz, limite, &Filtro::vacio())
}

/// Los ficheros más grandes, respetando las exclusiones del `filtro`.
pub fn grandes_con(raiz: &str, limite: usize, filtro: &Filtro) -> Result<Vec<Fichero>, String> {
    let p = Path::new(raiz);
    let meta = std::fs::symlink_metadata(p).map_err(|e| format!("no se puede leer {raiz}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("{raiz} no es una carpeta"));
    }
    if filtro.excluye(p) {
        return Ok(Vec::new());
    }
    let tope = limite.clamp(1, 1000);
    let mut pres = Presupuesto::nuevo();
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::new();
    let mut items: Vec<Option<Fichero>> = Vec::new();
    let mut pila = vec![p.to_path_buf()];

    while let Some(dir) = pila.pop() {
        if pres.agotado {
            break;
        }
        for hijo in hijos_de(&dir, &mut pres, filtro).0 {
            if !pres.gastar(1) {
                break;
            }
            let hm = match std::fs::symlink_metadata(&hijo) {
                Ok(m) => m,
                Err(_) => {
                    pres.omitidos += 1;
                    continue;
                }
            };
            if hm.file_type().is_symlink() {
                continue;
            }
            if hm.is_dir() {
                pila.push(hijo);
                continue;
            }
            let bytes = hm.len();
            let idx = items.len();
            items.push(Some(Fichero {
                ruta: hijo.to_string_lossy().to_string(),
                nombre: nombre_de(&hijo),
                bytes,
                modificado: modificado_de(&hm),
            }));
            heap.push(Reverse((bytes, idx)));
            if heap.len() > tope {
                if let Some(Reverse((_, i))) = heap.pop() {
                    items[i] = None;
                }
            }
        }
    }

    let mut v: Vec<Fichero> = items.into_iter().flatten().collect();
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    Ok(v)
}

/* ── Búsqueda por nombre ──────────────────────────────────────────────────── */

/// Busca por nombre SIN exclusiones (firma de las pruebas).
///
/// Existe SOLO para las pruebas (de ahí el `cfg(test)`): es la puerta «sin
/// exclusiones» que garantiza que el resultado sin filtro es el de siempre.
#[cfg(test)]
pub fn buscar(raiz: &str, consulta: &str, limite: usize) -> Result<Vec<Coincidencia>, String> {
    buscar_con(raiz, consulta, limite, &Filtro::vacio())
}

/// Busca por nombre, respetando las exclusiones del `filtro`.
pub fn buscar_con(
    raiz: &str,
    consulta: &str,
    limite: usize,
    filtro: &Filtro,
) -> Result<Vec<Coincidencia>, String> {
    let q = consulta.trim().to_lowercase();
    if q.is_empty() {
        return Err("Escribe algo que buscar".into());
    }
    let p = Path::new(raiz);
    let meta = std::fs::symlink_metadata(p).map_err(|e| format!("no se puede leer {raiz}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("{raiz} no es una carpeta"));
    }
    if filtro.excluye(p) {
        return Ok(Vec::new());
    }
    let tope = limite.clamp(1, 1000);
    let mut pres = Presupuesto::nuevo();
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::new();
    let mut items: Vec<Option<Coincidencia>> = Vec::new();
    let mut pila = vec![p.to_path_buf()];

    while let Some(dir) = pila.pop() {
        if pres.agotado {
            break;
        }
        for hijo in hijos_de(&dir, &mut pres, filtro).0 {
            if !pres.gastar(1) {
                break;
            }
            let hm = match std::fs::symlink_metadata(&hijo) {
                Ok(m) => m,
                Err(_) => {
                    pres.omitidos += 1;
                    continue;
                }
            };
            if hm.file_type().is_symlink() {
                continue;
            }
            let es_dir = hm.is_dir();
            if es_dir {
                pila.push(hijo.clone());
            }
            let nombre = nombre_de(&hijo);
            if !nombre.to_lowercase().contains(&q) {
                continue;
            }
            let bytes = if es_dir { None } else { Some(hm.len()) };
            let idx = items.len();
            items.push(Some(Coincidencia {
                ruta: hijo.to_string_lossy().to_string(),
                nombre,
                bytes,
                es_dir,
                modificado: modificado_de(&hm),
            }));
            heap.push(Reverse((bytes.unwrap_or(0), idx)));
            if heap.len() > tope {
                if let Some(Reverse((_, i))) = heap.pop() {
                    items[i] = None;
                }
            }
        }
    }

    let mut v: Vec<Coincidencia> = items.into_iter().flatten().collect();
    v.sort_by(|a, b| b.bytes.unwrap_or(0).cmp(&a.bytes.unwrap_or(0)));
    Ok(v)
}

/* ── Puntos de montaje ────────────────────────────────────────────────────── */

/// Uso de cada disco real del equipo.
///
/// Antes esto ejecutaba `df -P -B1 -T` con `LC_ALL=C` y parseaba su salida: eso es
/// de Linux (y de GNU coreutils), así que en macOS y Windows no había tabla de
/// discos. Ahora lo resuelve `plataforma::discos()`, que usa `sysinfo` y ve los
/// discos de los tres sistemas —incluidos los subvolúmenes de btrfs, que el
/// `df` de esta máquina listaba cuatro veces para el mismo dispositivo.
pub fn montajes() -> Vec<Montaje> {
    crate::plataforma::discos()
        .into_iter()
        .map(|d| Montaje {
            punto: d.punto,
            dispositivo: d.nombre,
            tipo: d.fs,
            total: d.total,
            usado: d.usado,
            libre: d.libre,
            uso_pct: d.uso_pct,
            extraible: d.extraible,
        })
        .collect()
}

/* ── Duplicados, carpetas vacías y enlaces rotos ──────────────────────────── */

/// Un grupo de ficheros con el MISMO contenido.
#[derive(Debug, Clone, Serialize)]
pub struct Duplicado {
    pub bytes: u64,
    pub rutas: Vec<String>,
    /// Lo que se liberaría dejando UNA copia de cada grupo.
    pub desperdicio: u64,
}

/// Grupo de ficheros por tamaño, en el primer paso: dos ficheros de tamaño
/// distinto no pueden ser iguales, y así no se lee el contenido de todo el disco.
fn por_tamano(
    raiz: &Path,
    min_bytes: u64,
    pres: &mut Presupuesto,
    filtro: &Filtro,
) -> HashMap<u64, Vec<PathBuf>> {
    let mut mapa: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut pila = vec![raiz.to_path_buf()];
    while let Some(dir) = pila.pop() {
        if pres.agotado {
            break;
        }
        for hijo in hijos_de(&dir, pres, filtro).0 {
            if !pres.gastar(1) {
                break;
            }
            let Ok(m) = std::fs::symlink_metadata(&hijo) else {
                pres.omitidos += 1;
                continue;
            };
            if m.file_type().is_symlink() {
                continue;
            }
            if m.is_dir() {
                pila.push(hijo);
                continue;
            }
            let bytes = m.len();
            if bytes < min_bytes {
                continue;
            }
            mapa.entry(bytes).or_default().push(hijo);
        }
    }
    mapa
}

/// Huella del contenido. `blake3` y no un hash criptográfico lento: aquí se busca
/// que dos ficheros iguales den lo mismo, no resistir un ataque.
fn huella(p: &Path) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.finalize().to_hex().to_string())
}

/// Busca ficheros repetidos por CONTENIDO (no por nombre).
///
/// En dos pasadas: primero por tamaño (barato, sin abrir nada) y solo se lee el
/// contenido de los tamaños que aparecen más de una vez. Sin ese filtro habría que
/// leer el disco entero, que es lo que hace que estas herramientas sean inusables.
///
/// `min_bytes` deja fuera los miles de ficheros pequeños que se repiten solos
/// (`.gitkeep`, miniaturas, `__init__.py`): son ruido y no liberan espacio.
#[cfg(test)]
pub fn duplicados(raiz: &str, min_bytes: u64, limite: usize) -> Result<Vec<Duplicado>, String> {
    duplicados_con(raiz, min_bytes, limite, &Filtro::vacio())
}

/// Ficheros repetidos por contenido, respetando las exclusiones del `filtro`.
pub fn duplicados_con(
    raiz: &str,
    min_bytes: u64,
    limite: usize,
    filtro: &Filtro,
) -> Result<Vec<Duplicado>, String> {
    let p = Path::new(raiz);
    let meta = std::fs::symlink_metadata(p).map_err(|e| format!("no se puede leer {raiz}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("{raiz} no es una carpeta"));
    }
    if filtro.excluye(p) {
        return Ok(Vec::new());
    }
    let mut pres = Presupuesto::nuevo();
    let candidatos = por_tamano(p, min_bytes, &mut pres, filtro);

    let mut grupos: Vec<Duplicado> = Vec::new();
    for (bytes, ficheros) in candidatos {
        if ficheros.len() < 2 {
            continue;
        }
        // Segunda pasada: se agrupa por huella del contenido.
        let mut por_huella: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for f in &ficheros {
            if pres.agotado {
                break;
            }
            if let Some(h) = huella(f) {
                por_huella.entry(h).or_default().push(f.clone());
            }
        }
        for (_, mut iguales) in por_huella {
            if iguales.len() < 2 {
                continue;
            }
            iguales.sort();
            let desperdicio = bytes * (iguales.len() as u64 - 1);
            grupos.push(Duplicado {
                bytes,
                rutas: iguales.iter().map(|r| r.to_string_lossy().to_string()).collect(),
                desperdicio,
            });
        }
    }
    // Lo que más libera, primero; y el número de rutas enseña el alcance.
    grupos.sort_by(|a, b| b.desperdicio.cmp(&a.desperdicio));
    grupos.truncate(limite.clamp(1, 5000));
    Ok(grupos)
}

/// Carpetas cuyo subárbol entero no tiene NI UN fichero.
///
/// Devuelve solo la carpeta MÁS ALTA de cada rama vacía: si `a/b/c` no tiene nada,
/// se enseña `a` y no sus hijas, porque borrar `a` se lleva las tres.
#[cfg(test)]
pub fn vacias(raiz: &str, limite: usize) -> Result<Vec<String>, String> {
    vacias_con(raiz, limite, &Filtro::vacio())
}

/// Carpetas sin ningún fichero, respetando las exclusiones del `filtro`.
///
/// Una carpeta que SOLO contiene cosas excluidas NO se propone: no está vacía (lo
/// que tiene dentro no se mira, pero está), y borrarla se llevaría por delante lo
/// excluido. Es la diferencia entre «no lo mido» y «no está».
pub fn vacias_con(raiz: &str, limite: usize, filtro: &Filtro) -> Result<Vec<String>, String> {
    let p = Path::new(raiz);
    let meta = std::fs::symlink_metadata(p).map_err(|e| format!("no se puede leer {raiz}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("{raiz} no es una carpeta"));
    }
    if filtro.excluye(p) {
        return Ok(Vec::new());
    }
    let mut pres = Presupuesto::nuevo();
    let mut out: Vec<String> = Vec::new();
    rama_vacia(p, p, &mut pres, &mut out, 0, limite.clamp(1, 5000), filtro);
    out.sort();
    Ok(out)
}

/// ¿Esta rama no tiene NI UN fichero? Devuelve `true` y deja en `out` la carpeta
/// MÁS ALTA de la rama vacía (no todas sus hijas: borrar la de arriba se lleva las
/// de dentro).
///
/// La recursión se acota en profundidad (64): un árbol más hondo que eso no es un
/// disco de usuario, y devolver `false` es lo honesto («no se puede afirmar»).
fn rama_vacia(
    dir: &Path,
    raiz: &Path,
    pres: &mut Presupuesto,
    out: &mut Vec<String>,
    prof: u32,
    limite: usize,
    filtro: &Filtro,
) -> bool {
    if pres.agotado || prof > 64 || out.len() >= limite {
        return false;
    }
    let (hijos, excluido_algo) = hijos_de(dir, pres, filtro);
    if excluido_algo {
        // Hay algo dentro que el usuario excluyó: no se puede afirmar que la
        // carpeta esté vacía, y proponerla para borrar se llevaría por delante lo
        // excluido. Se trata como contenido, igual que un enlace o un fallo de
        // permisos.
        return false;
    }
    let mut tiene_ficheros = false;
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for hijo in hijos {
        if !pres.gastar(1) {
            return false;
        }
        let Ok(m) = std::fs::symlink_metadata(&hijo) else {
            // Lo que no se puede leer no se puede afirmar que esté vacío.
            tiene_ficheros = true;
            continue;
        };
        if m.file_type().is_symlink() {
            // Un enlace cuenta como contenido: la carpeta no está vacía.
            tiene_ficheros = true;
            continue;
        }
        if m.is_dir() {
            subdirs.push(hijo);
        } else {
            tiene_ficheros = true;
        }
    }
    if tiene_ficheros {
        return false;
    }

    // Sin ficheros propios: se mira cada rama. Se apunta dónde empiezan las que
    // resulten vacías para poder retirarlas si al final lo vacío es ESTA carpeta.
    let mut todas_vacias = true;
    let mut primera_marca: Option<usize> = None;
    for d in &subdirs {
        let antes = out.len();
        if rama_vacia(d, raiz, pres, out, prof + 1, limite, filtro) {
            if primera_marca.is_none() {
                primera_marca = Some(antes);
            }
        } else {
            todas_vacias = false;
        }
    }
    if !todas_vacias {
        return false;
    }
    if let Some(marca) = primera_marca {
        // Todo lo de dentro está vacío: se apunta esta y se quitan las de dentro,
        // que son la misma cosa vista más abajo.
        out.truncate(marca);
    }
    if dir != raiz {
        out.push(dir.to_string_lossy().to_string());
    }
    true
}

/// Un enlace simbólico que apunta a algo que ya no está.
#[derive(Debug, Clone, Serialize)]
pub struct EnlaceRoto {
    pub ruta: String,
    /// A dónde decía apuntar.
    pub destino: String,
}

/// Enlaces que no llevan a ninguna parte.
///
/// En Unix son enlaces simbólicos rotos (un `ln -s` a un fichero borrado). En
/// **Windows** un acceso directo es un `.lnk` binario y comprobarlo necesita la API
/// del shell, que aquí no está: en ese sistema se devuelve la lista VACÍA con una
/// nota, en vez de señalar todos los `.lnk` como rotos, que sería mentira.
#[cfg(test)]
pub fn enlaces_rotos(raiz: &str, limite: usize) -> Result<Vec<EnlaceRoto>, String> {
    enlaces_rotos_con(raiz, limite, &Filtro::vacio())
}

/// Enlaces rotos, respetando las exclusiones del `filtro`.
pub fn enlaces_rotos_con(
    raiz: &str,
    limite: usize,
    filtro: &Filtro,
) -> Result<Vec<EnlaceRoto>, String> {
    let p = Path::new(raiz);
    let meta = std::fs::symlink_metadata(p).map_err(|e| format!("no se puede leer {raiz}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("{raiz} no es una carpeta"));
    }
    if cfg!(target_os = "windows") {
        return Ok(Vec::new());
    }
    if filtro.excluye(p) {
        return Ok(Vec::new());
    }
    let mut pres = Presupuesto::nuevo();
    let mut out: Vec<EnlaceRoto> = Vec::new();
    let mut pila = vec![p.to_path_buf()];
    while let Some(dir) = pila.pop() {
        if pres.agotado || out.len() >= limite.clamp(1, 5000) {
            break;
        }
        for hijo in hijos_de(&dir, &mut pres, filtro).0 {
            if !pres.gastar(1) {
                break;
            }
            let Ok(m) = std::fs::symlink_metadata(&hijo) else {
                continue;
            };
            if m.file_type().is_symlink() {
                match std::fs::read_link(&hijo) {
                    Ok(destino) => {
                        // `exists()` sigue el enlace: si el destino no está, es roto.
                        if !hijo.exists() {
                            out.push(EnlaceRoto {
                                ruta: hijo.to_string_lossy().to_string(),
                                destino: destino.to_string_lossy().to_string(),
                            });
                        }
                    }
                    Err(_) => continue,
                }
                continue;
            }
            if m.is_dir() {
                pila.push(hijo);
            }
        }
    }
    out.sort_by(|a, b| a.ruta.cmp(&b.ruta));
    Ok(out)
}

/* ── Borrado: papelera o definitivo, con lista blanca ─────────────────────── */

/// Rutas que NUNCA se borran, ni siquiera "solo su contenido": son raíces del
/// sistema o puntos donde vive la sesión en marcha.
///
/// La comparación es por la ruta EXACTA ya resuelta (`canonicalize`): lo que hay
/// DENTRO de una raíz no es ni tu carpeta ni un temporal del sistema, así que la
/// regla del home y de los prefijos permitidos lo rechaza de todas formas (con
/// otro motivo, pero lo rechaza). Estas son las de Linux.
const RAIZES_PROHIBIDAS_UNIX: &[&str] = &[
    "/", "/home", "/usr", "/etc", "/var", "/boot", "/opt", "/root", "/srv", "/bin", "/sbin",
    "/lib", "/lib64", "/tmp", "/var/tmp", "/var/log", "/var/cache", "/proc", "/sys", "/dev",
    "/run", "/media", "/mnt",
];

/// Raíces de macOS, que hay que decir EXPLÍCITAMENTE.
///
/// `/etc`, `/var` y `/tmp` son enlaces a `/private/...`: la ruta canónica de
/// `/etc/hosts` es `/private/etc/hosts`, así que las raíces de la lista de Unix no
/// la cazan. Por eso `/private` está aquí (y con él todo lo que cuelga de ahí,
/// que es donde vive el sistema), junto a las carpetas donde macOS instala
/// aplicaciones y monta discos.
const RAIZES_PROHIBIDAS_MACOS: &[&str] = &[
    "/System", "/Library", "/Applications", "/private", "/Volumes", "/Users",
];

/// Fuera del home solo se permite borrar DENTRO de las rutas de caché y
/// temporales del sistema (con la barra final: `/tmp/lo-que-sea` sí, `/tmp` no).
/// Estas son las de Linux.
const PREFIJOS_PERMITIDOS_UNIX: &[&str] = &[
    "/tmp/",
    "/var/tmp/",
    "/var/cache/",
    "/var/log/",
    "/var/crash/",
    "/var/lib/systemd/coredump/",
    "/var/lib/flatpak/repo/tmp/",
];

/// Los temporales y cachés de macOS, también con la barra final.
///
/// Van los `/private/...` porque eso es lo que devuelve `canonicalize`: en macOS
/// `/tmp` y `/var` son enlaces a `/private/tmp` y `/private/var`. Y
/// `/private/var/folders/...` es el `TMPDIR` de verdad de la sesión (el que usa
/// macOS para los temporales), así que sin esta entrada la limpieza de temporales
/// allí se quedaba sin permiso.
const PREFIJOS_PERMITIDOS_MACOS: &[&str] = &[
    "/private/tmp/",
    "/private/var/tmp/",
    "/private/var/folders/",
    "/private/var/log/",
];

/// Las raíces que no se tocan en el sistema indicado.
///
/// Es PURA a propósito (el entorno, el home y las unidades se pasan como
/// parámetros): así la lógica de macOS y de Windows se prueba desde Linux, que es
/// donde se desarrolla.
fn raices_prohibidas_de(
    so: &str,
    home: &Path,
    entorno: &dyn Fn(&str) -> Option<String>,
    unidades: &[String],
) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    match so {
        "macos" => {
            v.extend(RAIZES_PROHIBIDAS_UNIX.iter().map(|s| (*s).to_string()));
            v.extend(RAIZES_PROHIBIDAS_MACOS.iter().map(|s| (*s).to_string()));
        }
        "windows" => {
            // Las carpetas del sistema NO se escriben a mano: se preguntan al
            // entorno (`SystemRoot`, `ProgramFiles`, `ProgramData`), porque la
            // unidad no tiene por qué ser `C:`.
            for var in ["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
                if let Some(p) = entorno(var) {
                    v.push(p);
                }
            }
            // `C:\Users` y la raíz de la unidad del sistema, para el caso normal.
            if let Some(u) = entorno("SystemDrive") {
                v.push(format!("{u}\\Users"));
                v.push(u);
            }
            // Y la carpeta padre del home: con un perfil redirigido puede estar en
            // otra unidad, y ahí dentro están los perfiles de todos.
            if let Some(padre) = home.parent() {
                v.push(padre.to_string_lossy().to_string());
            }
            // Todas las raíces de unidad que el sistema tenga montadas.
            v.extend(unidades.iter().cloned());
        }
        _ => {
            v.extend(RAIZES_PROHIBIDAS_UNIX.iter().map(|s| (*s).to_string()));
        }
    }
    v
}

/// A un prefijo de Windows que llega sin barra final se le pone: los prefijos se
/// comparan con `starts_with`, así que sin ella `C:\Users\a\Temp2` colaría por
/// `C:\Users\a\Temp`.
fn con_barra_final(p: &str) -> String {
    let mut s = p.to_string();
    if !s.ends_with('\\') && !s.ends_with('/') {
        s.push('\\');
    }
    s
}

/// Los prefijos fuera del home donde SÍ se puede borrar, para el sistema
/// indicado. También es pura (el entorno y el temporal se pasan).
fn prefijos_permitidos_de(
    so: &str,
    entorno: &dyn Fn(&str) -> Option<String>,
    temporal: &Path,
) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    match so {
        "macos" => {
            v.extend(PREFIJOS_PERMITIDOS_UNIX.iter().map(|s| (*s).to_string()));
            v.extend(PREFIJOS_PERMITIDOS_MACOS.iter().map(|s| (*s).to_string()));
        }
        "windows" => {
            // Los temporales de Windows: `%LOCALAPPDATA%\Temp` y el que el sistema
            // dé como `%TEMP%`/`%TMP%` (los dos viven dentro del perfil, pero se
            // dicen porque un perfil redirigido puede estar en otra unidad y la
            // limpieza de temporales tiene que poder borrar ahí).
            let mut candidatos: Vec<String> = Vec::new();
            for var in ["LOCALAPPDATA", "TEMP", "TMP"] {
                if let Some(p) = entorno(var) {
                    candidatos.push(p);
                }
            }
            candidatos.push(temporal.to_string_lossy().to_string());
            if let Some(p) = entorno("LOCALAPPDATA") {
                candidatos.push(format!("{p}\\Temp"));
            }
            for c in candidatos {
                v.push(con_barra_final(&c));
            }
        }
        _ => {
            v.extend(PREFIJOS_PERMITIDOS_UNIX.iter().map(|s| (*s).to_string()));
        }
    }
    v
}

/// Las raíces de ESTE equipo.
fn raices_prohibidas() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    // Las unidades montadas las sabe la capa de plataforma (sysinfo): no se
    // escriben a mano ni se supone que sean `C:`.
    let unidades: Vec<String> = if cfg!(target_os = "windows") {
        crate::plataforma::discos().iter().map(|d| d.punto.clone()).collect()
    } else {
        Vec::new()
    };
    raices_prohibidas_de(crate::plataforma::so(), &home, &|k| std::env::var(k).ok(), &unidades)
}

/// Los prefijos permitidos de ESTE equipo.
fn prefijos_permitidos() -> Vec<String> {
    prefijos_permitidos_de(crate::plataforma::so(), &|k| std::env::var(k).ok(), &std::env::temp_dir())
}

/// Deja una ruta de Windows en la forma en la que se compara con las listas: sin
/// el prefijo `\\?\` que le añade `canonicalize` (y `\\?\UNC\` para las rutas de
/// red), con `\` de separador y en minúsculas (en Windows las rutas no distinguen
/// mayúsculas: `C:\Windows` y `c:\windows` son la misma carpeta).
///
/// NO se quita el separador final: los prefijos permitidos lo llevan a propósito
/// y quitarlo haría que `...\Temp2` colase por `...\Temp`.
fn normalizar_windows(s: &str) -> String {
    let t = s.replace('/', "\\");
    if let Some(resto) = t.strip_prefix(r"\\?\") {
        if let Some(unc) = resto.strip_prefix("UNC\\") {
            return format!(r"\\{unc}").to_lowercase();
        }
        return resto.to_lowercase();
    }
    t.to_lowercase()
}

/// ¿`ruta` está dentro de `base` (o es `base`)? Se compara por componentes, no
/// por texto: `/home/usuarioX` NO está dentro de `/home/usuario`.
fn dentro_de(ruta: &str, base: &str, windows: bool) -> bool {
    let sep = if windows { '\\' } else { '/' };
    match ruta.strip_prefix(base) {
        Some("") => true,
        Some(resto) => resto.starts_with(sep),
        None => false,
    }
}

/// ¿Se puede borrar esta ruta? Es la comprobación de verdad, y es PURA: recibe la
/// ruta ya resuelta y las listas de su sistema, así que la lógica de macOS y de
/// Windows se prueba desde Linux.
///
/// `so` decide dos cosas: qué separador se usa y si las mayúsculas cuentan (en
/// Windows no).
fn comprobar_ruta(
    ruta: &str,
    home: &str,
    so: &str,
    raices: &[String],
    permitidos: &[String],
) -> Result<(), String> {
    let windows = so == "windows";
    let normal = |s: &str| if windows { normalizar_windows(s) } else { s.to_string() };
    let r = normal(ruta);
    let h = normal(home);
    if raices.iter().any(|x| normal(x) == r) {
        // El mensaje lleva la ruta TAL COMO la devolvió el sistema, no la
        // normalizada: es la que la persona reconoce.
        return Err(format!("{ruta} es una raíz del sistema; no se borra"));
    }
    if r == h {
        return Err("esa es tu carpeta personal entera; no se borra".into());
    }
    let en_home = dentro_de(&r, &h, windows);
    let en_permitido = permitidos.iter().any(|pre| r.starts_with(&normal(pre)));
    if !en_home && !en_permitido {
        return Err(format!(
            "{ruta} está fuera de tu carpeta personal y de las rutas de caché permitidas; no se toca"
        ));
    }
    Ok(())
}

/// ¿Se puede borrar este ENLACE? (Para el caso de los enlaces rotos.)
///
/// Un enlace no se sigue NUNCA, así que borrarlo no puede tocar a lo que apunta: lo
/// que hay que comprobar es la CARPETA que lo contiene, que es la que de verdad se
/// modifica. `permitida` rechaza los enlaces a propósito (para no seguir uno que
/// apunte fuera), y por eso esto es una puerta aparte y estrecha.
pub fn permitida_enlace(p: &Path) -> Result<(), String> {
    let padre = p.parent().ok_or_else(|| format!("{} no tiene carpeta", p.display()))?;
    permitida(padre)
}

/// ¿Se puede borrar esta ruta? Devuelve el motivo cuando no.
///
/// La comprobación es sobre la ruta CANÓNICA, así que un enlace no puede usarse
/// para colarse fuera de la lista: si `/home/usuario/enlace` apunta a `/etc`, el
/// canónico es `/etc/...` y se rechaza.
///
/// Las raíces prohibidas y los prefijos permitidos son los DE ESTE SISTEMA (los
/// resuelve `plataforma::so()`): en macOS `/System`, `/Library`, `/private`… y en
/// Windows `C:\Windows`, `C:\Program Files`… con las mayúsculas ignoradas, que es
/// como las trata ese sistema de ficheros.
pub fn permitida(p: &Path) -> Result<(), String> {
    if !p.is_absolute() {
        return Err(format!("{} no es una ruta absoluta", p.display()));
    }
    let meta = std::fs::symlink_metadata(p)
        .map_err(|e| format!("no se puede leer {}: {e}", p.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} es un enlace simbólico; no se borra", p.display()));
    }
    let canon = p
        .canonicalize()
        .map_err(|e| format!("no se puede resolver {}: {e}", p.display()))?;
    let home = dirs::home_dir().ok_or("sin HOME")?;
    comprobar_ruta(
        &canon.to_string_lossy(),
        &home.to_string_lossy(),
        crate::plataforma::so(),
        &raices_prohibidas(),
        &prefijos_permitidos(),
    )
}

pub fn borrar(rutas: &[String], definitivo: bool) -> Result<String, String> {
    if rutas.is_empty() {
        return Err("No hay nada seleccionado que borrar".into());
    }
    let mut hechos = 0usize;
    let mut bytes = 0u64;
    let mut fallos: Vec<String> = Vec::new();

    // Qué es cada cosa se mira ANTES de borrar: después ya no hay nada que mirar
    // (el mensaje de una prueba lo pilló diciendo "a la papelera" de un enlace que
    // acababa de quitar).
    let enlaces: Vec<bool> = rutas
        .iter()
        .map(|r| {
            std::fs::symlink_metadata(r)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        })
        .collect();

    for (r, es_enlace) in rutas.iter().zip(enlaces.iter()) {
        if hechos + fallos.len() >= 5000 {
            fallos.push("se ha parado a los 5000 elementos: vuelve a seleccionar el resto".into());
            break;
        }
        let p = Path::new(r);
        // Un enlace se comprueba por su CARPETA y se quita directamente: la
        // papelera no acepta enlaces (no se pueden seguir) y, además, un enlace roto
        // no tiene nada que recuperar: su destino ya no está.
        let permiso = if *es_enlace { permitida_enlace(p) } else { permitida(p) };
        if let Err(e) = permiso {
            fallos.push(e);
            continue;
        }
        let tam = tamano(p);
        let res = if *es_enlace || definitivo {
            crate::plataforma::papelera::borrar_definitivo(p)
        } else {
            crate::plataforma::papelera::mover(p).map(|_| ())
        };
        match res {
            Ok(()) => {
                hechos += 1;
                bytes += tam;
            }
            Err(e) => fallos.push(e),
        }
    }

    if hechos == 0 {
        return Err(fallos.first().cloned().unwrap_or_else(|| "no se pudo borrar nada".into()));
    }
    // Si todo lo borrado eran enlaces, decir "a la papelera" sería falso: los
    // enlaces se quitan, no se mandan a la papelera (no se pueden seguir).
    let solo_enlaces = !enlaces.is_empty() && enlaces.iter().all(|e| *e);
    let como = if solo_enlaces {
        "quitados (eran enlaces)"
    } else if definitivo {
        "borrados definitivamente"
    } else {
        "movidos a la papelera"
    };
    let mut msg = format!("{hechos} de {} elementos {como}", rutas.len());
    if definitivo {
        msg.push_str(&format!("; {} liberados", legible(bytes)));
    } else {
        msg.push_str(&format!(
            "; {} a la papelera (ese espacio no se libera hasta vaciarla)",
            legible(bytes)
        ));
    }
    if !fallos.is_empty() {
        msg.push_str(&format!(". {} no se pudieron: {}", fallos.len(), fallos[0]));
    }
    Ok(msg)
}

/// Bytes en unidad legible, con punto decimal (la regla del panel).
pub fn legible(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Monta un árbol de prueba con tamaños conocidos y lo devuelve.
    ///
    /// Cada prueba usa SU carpeta (el nombre va en la ruta): `cargo test` corre
    /// las pruebas en paralelo y, compartiendo carpeta, una borraba la que la otra
    /// estaba midiendo.
    fn arbol_de_prueba(caso: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("machinograph-almacen-{caso}"));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("grande")).unwrap();
        std::fs::create_dir_all(raiz.join("pequeno")).unwrap();
        std::fs::create_dir_all(raiz.join("grande/dentro")).unwrap();
        std::fs::write(raiz.join("grande/dentro/uno.bin"), vec![0u8; 4096]).unwrap();
        std::fs::write(raiz.join("grande/dentro/dos.bin"), vec![0u8; 2048]).unwrap();
        std::fs::write(raiz.join("pequeno/tres.bin"), vec![0u8; 512]).unwrap();
        std::fs::write(raiz.join("suelto.bin"), vec![0u8; 256]).unwrap();
        raiz
    }

    #[test]
    fn el_arbol_ordena_por_tamano_y_suma_bien() {
        let raiz = arbol_de_prueba("orden");
        let a = arbol(&raiz.to_string_lossy(), 100).unwrap();
        // 4096 + 2048 + 512 + 256
        assert_eq!(a.bytes, 6912, "{a:?}");
        assert_eq!(a.ficheros, 4);
        assert_eq!(a.dirs, 3);
        // El primero es el más grande, y es una carpeta.
        assert_eq!(a.hijos[0].nombre, "grande");
        assert!(a.hijos[0].es_dir);
        assert_eq!(a.hijos[0].bytes, 6144);
        assert_eq!(a.hijos[0].dirs, 1);
        assert_eq!(a.hijos[1].nombre, "pequeno");
        assert_eq!(a.hijos[2].nombre, "suelto.bin");
        assert!(!a.truncado);
        assert_eq!(a.resto_n, 0);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn con_tope_de_hijos_agrega_el_resto_y_lo_dice() {
        let raiz = arbol_de_prueba("tope");
        let a = arbol(&raiz.to_string_lossy(), 2).unwrap();
        // Se enseñan los dos más grandes; el resto se agrega y se cuenta.
        assert_eq!(a.hijos.len(), 2);
        assert_eq!(a.resto_n, 1);
        assert_eq!(a.resto_bytes, 256);
        // El total sigue siendo el correcto: lo que se agrega no se pierde.
        assert_eq!(a.bytes, 6912);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn los_mas_grandes_van_de_mayor_a_menor() {
        let raiz = arbol_de_prueba("grandes");
        let g = grandes(&raiz.to_string_lossy(), 3).unwrap();
        assert_eq!(g.len(), 3);
        let nombres: Vec<&str> = g.iter().map(|f| f.nombre.as_str()).collect();
        assert_eq!(nombres, vec!["uno.bin", "dos.bin", "tres.bin"]);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn la_busqueda_encuentra_por_nombre_y_no_distingue_mayusculas() {
        let raiz = arbol_de_prueba("buscar");
        let r = buscar(&raiz.to_string_lossy(), "BIN", 10).unwrap();
        // Los cuatro .bin; "dentro" no contiene "bin".
        assert_eq!(r.len(), 4, "{r:?}");
        // Y buscar una carpeta la devuelve sin tamaño medido (None, no 0).
        let r2 = buscar(&raiz.to_string_lossy(), "grande", 10).unwrap();
        assert_eq!(r2.len(), 1);
        assert!(r2[0].es_dir);
        assert_eq!(r2[0].bytes, None);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn el_borrado_no_sale_del_home_ni_de_las_rutas_permitidas() {
        // Raíces del sistema: nunca.
        for r in ["/etc/hostname", "/usr/bin/env", "/home", "/"] {
            assert!(permitida(Path::new(r)).is_err(), "{r} debería rechazarse");
        }
        // Fuera del home y fuera de las rutas de caché: tampoco.
        assert!(permitida(Path::new("/var/lib"),).is_err());
        // Dentro del home: sí (aunque no exista, el error es otro: no se puede leer).
        let home = dirs::home_dir().unwrap();
        let dentro = home.join("cualquier-cosa-temporal");
        let e = permitida(&dentro).unwrap_err();
        assert!(e.contains("no se puede leer"), "{e}");
    }

    /// Un entorno falso, para poder probar la lógica de otro sistema desde Linux.
    /// La clausura se queda con sus datos (una copia): así vale también con una
    /// lista escrita en la propia llamada, sin tener que guardarla aparte.
    fn entorno_de_prueba(pares: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pares: Vec<(String, String)> =
            pares.iter().map(|(n, v)| ((*n).to_string(), (*v).to_string())).collect();
        move |k: &str| pares.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    #[test]
    fn en_linux_las_listas_son_las_de_siempre() {
        // Esta es la máquina del usuario y está verificada: las listas de Linux no
        // cambian. Si alguien las toca, se entera aquí.
        let vacio = entorno_de_prueba(&[]);
        let r = raices_prohibidas_de("linux", Path::new("/home/usuario"), &vacio, &[]);
        assert_eq!(r, RAIZES_PROHIBIDAS_UNIX.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
        let p = prefijos_permitidos_de("linux", &vacio, Path::new("/tmp"));
        assert_eq!(p, PREFIJOS_PERMITIDOS_UNIX.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn en_macos_no_se_tocan_las_raices_y_sus_temporales_si() {
        let home = "/Users/usuario";
        let vacio = entorno_de_prueba(&[]);
        let raices = raices_prohibidas_de("macos", Path::new(home), &vacio, &[]);
        let permitidos = prefijos_permitidos_de("macos", &vacio, Path::new("/var/folders/zz/T"));
        let choca = |r: &str| comprobar_ruta(r, home, "macos", &raices, &permitidos);
        // Raíces de macOS: nunca.
        for r in ["/System", "/Library", "/Applications", "/private", "/Volumes", "/Users"] {
            assert!(choca(r).is_err(), "{r} debería rechazarse");
        }
        // Y lo que cuelga de ellas tampoco: la ruta canónica de `/etc` y de `/var`
        // es `/private/etc` y `/private/var`, así que la lista de Unix no las caza.
        assert!(choca("/private/etc/hosts").is_err());
        assert!(choca("/System/Library/CoreServices/x").is_err());
        assert!(choca("/Library/LaunchDaemons/x.plist").is_err());
        // El temporal DE VERDAD de macOS (su `TMPDIR`, que canonicaliza a
        // `/private/var/folders`) sí se puede limpiar: sin esto, la limpieza de
        // temporales allí se queda sin permiso.
        assert!(choca("/private/var/folders/ab/cd/T/x.log").is_ok());
        assert!(choca("/private/tmp/basura").is_ok());
        // Dentro del home, también (ahí viven las cachés del usuario).
        assert!(choca("/Users/usuario/Library/Caches/x").is_ok());
        // Fuera del home y de los temporales, no.
        assert!(choca("/Users/otro/fichero").is_err());
        assert!(choca("/opt/homebrew/x").is_err());
    }

    #[test]
    fn en_windows_no_se_tocan_las_carpetas_del_sistema_ni_las_unidades() {
        let home = r"C:\Users\usuario";
        let entorno = entorno_de_prueba(&[
            ("SystemRoot", r"C:\Windows"),
            ("windir", r"C:\Windows"),
            ("ProgramFiles", r"C:\Program Files"),
            ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
            ("ProgramData", r"C:\ProgramData"),
            ("SystemDrive", "C:"),
            ("LOCALAPPDATA", r"C:\Users\usuario\AppData\Local"),
            ("TEMP", r"C:\Users\usuario\AppData\Local\Temp"),
        ]);
        let unidades = vec![r"C:\".to_string(), r"D:\".to_string()];
        let raices = raices_prohibidas_de("windows", Path::new(home), &entorno, &unidades);
        let permitidos =
            prefijos_permitidos_de("windows", &entorno, Path::new(r"C:\Users\usuario\AppData\Local\Temp"));
        let choca = |r: &str| comprobar_ruta(r, home, "windows", &raices, &permitidos);
        // `canonicalize` en Windows devuelve las rutas con el prefijo `\\?\` y las
        // mayúsculas que sean: hay que reconocerlas igual.
        for r in [
            r"\\?\C:\Windows",
            r"\\?\C:\Windows\System32\cmd.exe",
            r"c:\WINDOWS\Temp\a.exe",
            r"\\?\C:\Program Files\App\a.exe",
            r"\\?\C:\ProgramData\Microsoft\x",
            r"\\?\C:\Users",
            r"\\?\C:\",
            r"\\?\D:\",
            r"\\?\C:\Users\otro\fichero.txt",
        ] {
            assert!(choca(r).is_err(), "{r} debería rechazarse");
        }
        // Los temporales del perfil sí se pueden limpiar (es lo que evita que la
        // limpieza de temporales se quede sin permiso en Windows), y lo de dentro
        // del home también.
        assert!(choca(r"\\?\C:\Users\usuario\AppData\Local\Temp\basura.tmp").is_ok());
        assert!(choca(r"\\?\C:\Users\usuario\Descargas\x.iso").is_ok());
        // `C:\Windows\Temp` NO cuela por el temporal del usuario, y hacerlo pide
        // ser administrador: se rechaza, que es lo correcto.
        assert!(choca(r"\\?\C:\Windows\Temp\x.exe").is_err());
    }

    #[test]
    fn normalizar_windows_quita_el_prefijo_extendido_y_las_mayusculas() {
        assert_eq!(normalizar_windows(r"\\?\C:\Users\usuario"), r"c:\users\usuario");
        assert_eq!(normalizar_windows(r"\\?\UNC\servidor\recurso\x"), r"\\servidor\recurso\x");
        assert_eq!(normalizar_windows("C:/Users/A"), r"c:\users\a");
        assert_eq!(normalizar_windows(r"C:\"), r"c:\");
    }

    #[test]
    fn el_prefijo_del_home_no_vale_para_una_carpeta_que_solo_empieza_igual() {
        let vacio = entorno_de_prueba(&[]);
        let raices = raices_prohibidas_de("linux", Path::new("/home/usuario"), &vacio, &[]);
        let permitidos = prefijos_permitidos_de("linux", &vacio, Path::new("/tmp"));
        let choca = |r: &str| comprobar_ruta(r, "/home/usuario", "linux", &raices, &permitidos);
        assert!(choca("/home/usuario/x").is_ok());
        assert!(choca("/home/usuarioX/x").is_err());
        // Un prefijo permitido tampoco se estira a una carpeta que solo empieza
        // igual: `/var/tmp/` sí, `/var/tmpX` no.
        assert!(choca("/var/tmp/x").is_ok());
        assert!(choca("/var/tmpX/x").is_err());
    }

    // Prueba de Unix ENTERA: crear un enlace simbólico en Windows exige
    // privilegios (o modo desarrollador) y el runner del CI no los tiene, así que
    // allí el `symlink` falla y no hay nada que comprobar. Se gatea la prueba
    // completa (no solo la creación del enlace) porque sin enlace el resto no
    // tiene sentido.
    #[cfg(unix)]
    #[test]
    fn un_enlace_no_sirve_para_colarse_fuera_del_home() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-enlace");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(&raiz).unwrap();
        std::os::unix::fs::symlink("/etc", raiz.join("puerta")).unwrap();
        // La ruta es /tmp/... (permitida) pero es un enlace: se rechaza sin resolverlo.
        let e = permitida(&raiz.join("puerta")).unwrap_err();
        assert!(e.contains("enlace simbólico"), "{e}");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn los_montajes_reales_no_traen_pseudo_sistemas() {
        let m = montajes();
        assert!(!m.is_empty(), "no se ve ningún disco");
        for x in &m {
            assert!(x.total > 0, "{x:?}");
            assert!(x.usado <= x.total, "{x:?}");
            assert!(!x.punto.is_empty());
            // Los pseudo-sistemas (tmpfs, overlay, proc…) los quita la capa de
            // plataforma: aquí no puede colarse ninguno.
            for pseudo in ["tmpfs", "devtmpfs", "overlay", "squashfs", "proc", "sysfs"] {
                assert_ne!(x.tipo.to_lowercase(), pseudo, "se coló {x:?}");
            }
        }
        // Ordenado por lo que más ocupa: es lo que se quiere ver primero.
        for v in m.windows(2) {
            assert!(v[0].usado >= v[1].usado, "{m:?}");
        }
    }

    #[test]
    fn encuentra_los_ficheros_repetidos_por_contenido() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-duplicados");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("a")).unwrap();
        std::fs::create_dir_all(raiz.join("b")).unwrap();
        // Dos iguales (2048 B), uno distinto del mismo tamaño y uno pequeño que se
        // queda fuera por el mínimo.
        std::fs::write(raiz.join("a/uno.bin"), vec![7u8; 2048]).unwrap();
        std::fs::write(raiz.join("b/dos.bin"), vec![7u8; 2048]).unwrap();
        std::fs::write(raiz.join("b/otro.bin"), vec![9u8; 2048]).unwrap();
        std::fs::write(raiz.join("pequeno.bin"), vec![7u8; 10]).unwrap();

        let d = duplicados(&raiz.to_string_lossy(), 1024, 50).unwrap();
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].bytes, 2048);
        assert_eq!(d[0].rutas.len(), 2);
        assert_eq!(d[0].desperdicio, 2048);
        // El de 10 bytes no entra por el mínimo: no se cuela como duplicado.
        assert!(!d[0].rutas.iter().any(|r| r.contains("pequeno")));

        // Con el mínimo a 0, el pequeño aparece como grupo propio (solo si se
        // repite: aquí hay uno solo, así que no).
        let d2 = duplicados(&raiz.to_string_lossy(), 0, 50).unwrap();
        assert_eq!(d2.len(), 1);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn encuentra_las_carpetas_sin_ningun_fichero() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-vacias");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("vacia/anidada/mas")).unwrap();
        std::fs::create_dir_all(raiz.join("con-algo")).unwrap();
        std::fs::write(raiz.join("con-algo/x.txt"), b"hola").unwrap();

        let v = vacias(&raiz.to_string_lossy(), 100).unwrap();
        // Se enseña la MÁS ALTA de la rama vacía: borrarla se lleva las de dentro.
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].ends_with("vacia"), "{v:?}");
        assert!(!v.iter().any(|x| x.contains("con-algo")));
        // La raíz nunca se propone a sí misma.
        assert!(!v.iter().any(|x| x.ends_with("almacen-vacias")));
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[cfg(unix)]
    #[test]
    fn encuentra_los_enlaces_rotos_y_no_los_buenos() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-enlaces");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(&raiz).unwrap();
        std::fs::write(raiz.join("existe.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(raiz.join("existe.txt"), raiz.join("bueno.lnk")).unwrap();
        std::os::unix::fs::symlink(raiz.join("no-esta.txt"), raiz.join("roto.lnk")).unwrap();

        let e = enlaces_rotos(&raiz.to_string_lossy(), 50).unwrap();
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].ruta.ends_with("roto.lnk"));
        assert!(e[0].destino.ends_with("no-esta.txt"), "{:?}", e[0].destino);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[cfg(unix)]
    #[test]
    fn un_enlace_roto_se_puede_quitar_aunque_la_lista_blanca_rechace_enlaces() {
        let base = std::env::temp_dir().join("machinograph-borrar-enlace");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let roto = base.join("roto.lnk");
        std::os::unix::fs::symlink(base.join("no-esta"), &roto).unwrap();

        // Por la puerta normal NO pasa (es un enlace).
        assert!(permitida(&roto).is_err());
        // Por la de enlaces SÍ: lo que se comprueba es su carpeta, y quitar un
        // enlace no puede tocar a su destino.
        assert!(permitida_enlace(&roto).is_ok());

        let m = borrar(&[roto.to_string_lossy().to_string()], false).unwrap();
        assert!(m.contains("enlaces"), "el mensaje tiene que decir que era un enlace: {m}");
        assert!(!roto.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn los_bytes_legibles_usan_punto_decimal() {
        assert_eq!(legible(512), "512 B");
        assert_eq!(legible(1536), "1.5 KB");
        assert_eq!(legible(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    /// Un filtro con exclusiones concretas, SIN pasar por la base de datos: las
    /// pruebas recorren directorios temporales y no pueden depender de la lista del
    /// usuario (por eso el filtro es un parámetro y no se lee dentro).
    fn filtro_con(patrones: &[&str]) -> Filtro {
        Filtro::con_vigentes(
            patrones.iter().map(|p| exclusiones::preparar(p).unwrap()).collect(),
        )
    }

    #[test]
    fn una_exclusion_no_aparece_ni_suma_y_se_dice_cual_actuo() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-exclusion");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("medida")).unwrap();
        std::fs::create_dir_all(raiz.join("excluida")).unwrap();
        std::fs::write(raiz.join("medida/uno.bin"), vec![0u8; 4096]).unwrap();
        std::fs::write(raiz.join("excluida/dos.bin"), vec![0u8; 8192]).unwrap();

        let patron = raiz.join("excluida").to_string_lossy().to_string();
        let con = arbol_con(&raiz.to_string_lossy(), 100, &filtro_con(&[&patron])).unwrap();

        // (a) La excluida no está entre los hijos ni suma en el total.
        assert_eq!(con.hijos.len(), 1, "{con:?}");
        assert_eq!(con.hijos[0].nombre, "medida");
        assert_eq!(con.bytes, 4096, "{con:?}");
        assert_eq!(con.ficheros, 1, "{con:?}");
        // (b) `excluidos` nombra el patrón que actuó (y solo ese, no la lista
        // entera de configuradas).
        assert_eq!(con.excluidos, vec![patron.clone()], "{con:?}");

        // (c) Sin exclusiones, IDÉNTICO a lo de siempre: mismos bytes, hijos y
        // orden.
        let sin = arbol_con(&raiz.to_string_lossy(), 100, &Filtro::vacio()).unwrap();
        assert_eq!(sin.bytes, 4096 + 8192, "{sin:?}");
        assert_eq!(sin.ficheros, 2, "{sin:?}");
        assert_eq!(sin.hijos.len(), 2, "{sin:?}");
        assert_eq!(sin.hijos[0].nombre, "excluida");
        assert_eq!(sin.hijos[0].bytes, 8192);
        assert!(sin.excluidos.is_empty(), "{sin:?}");

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn la_raiz_excluida_no_se_mide_y_lo_dice() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-raiz-excluida");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(&raiz).unwrap();
        std::fs::write(raiz.join("x.bin"), vec![0u8; 999]).unwrap();

        let patron = raiz.to_string_lossy().to_string();
        let a = arbol_con(&raiz.to_string_lossy(), 100, &filtro_con(&[&patron])).unwrap();
        assert_eq!(a.bytes, 0, "{a:?}");
        assert!(a.hijos.is_empty(), "{a:?}");
        assert_eq!(a.excluidos, vec![patron]);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn un_nombre_suelto_se_salta_en_cualquier_profundidad() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-nombre-suelto");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("proyecto/a/node_modules/pkg")).unwrap();
        std::fs::create_dir_all(raiz.join("proyecto/b/node_modules")).unwrap();
        std::fs::write(raiz.join("proyecto/a/node_modules/pkg/x.js"), vec![0u8; 1000]).unwrap();
        std::fs::write(raiz.join("proyecto/b/node_modules/y.js"), vec![0u8; 2000]).unwrap();
        std::fs::write(raiz.join("proyecto/b/real.bin"), vec![0u8; 512]).unwrap();

        let a = arbol_con(&raiz.to_string_lossy(), 100, &filtro_con(&["node_modules"])).unwrap();
        // Solo cuenta lo que no cuelga de un `node_modules`, a cualquier nivel.
        assert_eq!(a.bytes, 512, "{a:?}");
        assert_eq!(a.hijos.len(), 1, "{a:?}");
        assert_eq!(a.hijos[0].nombre, "proyecto");
        assert_eq!(a.hijos[0].bytes, 512, "los node_modules de dentro tampoco suman");
        assert_eq!(a.excluidos, vec!["node_modules".to_string()], "{a:?}");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn las_herramientas_de_lista_respetan_las_exclusiones() {
        let raiz = std::env::temp_dir().join("machinograph-almacen-listas");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("fuera")).unwrap();
        std::fs::create_dir_all(raiz.join("dentro")).unwrap();
        std::fs::write(raiz.join("fuera/grande.bin"), vec![7u8; 100_000]).unwrap();
        std::fs::write(raiz.join("dentro/pequeno.bin"), vec![7u8; 100]).unwrap();

        let patron = raiz.join("fuera").to_string_lossy().to_string();
        let f = filtro_con(&[patron.as_str()]);

        // Grandes: lo excluido no se lista.
        let g = grandes_con(&raiz.to_string_lossy(), 10, &f).unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
        assert!(g[0].ruta.ends_with("pequeno.bin"), "{g:?}");

        // Búsqueda: no encuentra nada dentro de lo excluido.
        let b = buscar_con(&raiz.to_string_lossy(), "grande", 10, &f).unwrap();
        assert!(b.is_empty(), "{b:?}");

        // Repetidos: la copia visible de un fichero excluido no forma grupo.
        std::fs::write(raiz.join("dentro/igual.bin"), vec![7u8; 100_000]).unwrap();
        let d = duplicados_con(&raiz.to_string_lossy(), 1024, 10, &f).unwrap();
        assert!(d.is_empty(), "con la otra copia excluida no hay grupo: {d:?}");

        // Vacías: una carpeta que SOLO contiene algo excluido NO se propone —no
        // está vacía, y borrarla se llevaría por delante lo excluido—.
        std::fs::create_dir_all(raiz.join("solo-excluido")).unwrap();
        std::fs::write(raiz.join("solo-excluido/x.bin"), b"x").unwrap();
        let patron2 = raiz.join("solo-excluido").to_string_lossy().to_string();
        let v = vacias_con(&raiz.to_string_lossy(), 100, &filtro_con(&[patron2.as_str()])).unwrap();
        assert!(v.is_empty(), "no hay ninguna carpeta realmente vacía: {v:?}");

        let _ = std::fs::remove_dir_all(&raiz);
    }
}
