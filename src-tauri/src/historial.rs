//! Histórico del uso de disco: una medida por carpeta y día, y la comparación
//! entre dos medidas para saber qué ha crecido y qué ha bajado.
//!
//! POR QUÉ EXISTE: el analizador (`almacen.rs`) dice lo que ocupa el disco AHORA,
//! pero no si eso ha cambiado desde la semana pasada. Kudu tiene «storage history
//! and growth comparisons»; aquí se hace con una diferencia: cada punto es una
//! MEDIDA REAL del analizador (con su presupuesto y sus exclusiones), no una
//! estimación, y cada cifra dice de cuándo es y si quedó completa.
//!
//! Tres decisiones:
//!
//! 1. **Un punto por día.** Guardar cada análisis haría crecer la tabla sin fin y
//!    comparar dos medidas del mismo día no dice nada. Se conserva el más reciente
//!    de cada día y hasta `db::HISTORIAL_DIAS` días (ver `guardar_instantanea`).
//! 2. **La comparación es una función PURA.** `comparar` no toca el disco ni la
//!    base: recibe dos medidas y devuelve el crecimiento. Eso permite probarla con
//!    casos que en disco serían incómodos (un hijo que BAJA, una medida parcial,
//!    una sola medida).
//! 3. **Lo parcial se dice.** Una medida puede quedarse corta (presupuesto) o no
//!    incluir todo (exclusiones, hijos que el analizador no listó); compararla
//!    como si fuera completa daría un crecimiento que no es el real. El resultado
//!    lleva `parcial`/`motivo` y quien lo pinte tiene que decirlo.
//!
//! Nada de esto escribe fuera de la carpeta de datos: las medidas van a la misma
//! base SQLite que el resto del histórico.
use crate::almacen::Arbol;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Un hijo directo de la carpeta medida, tal como se guarda.
///
/// Solo se guardan los hijos que el analizador DEVOLVIÓ (los más grandes, según
/// su `max_hijos`); si dejó alguno fuera, `Instantanea::resto_n` lo dice. No se
/// vuelve a recorrer el árbol: medir dos veces la misma carpeta sería hacer el
/// trabajo dos veces y con presupuestos distintos.
#[derive(Debug, Clone, Serialize)]
pub struct HijoInstantanea {
    pub ruta: String,
    pub nombre: String,
    pub bytes: u64,
    pub ficheros: u64,
    pub dirs: u64,
}

/// Una medida de una carpeta en un momento concreto.
///
/// Los tres campos que describen su calidad (`truncado`, `excluidos`, `resto_n`)
/// se guardan en crudo a propósito: así la interfaz puede decir POR QUÉ una
/// comparación no es de fiar, en vez de un «parcial» sin explicación.
#[derive(Debug, Clone, Serialize)]
pub struct Instantanea {
    pub ts: i64,
    pub ruta: String,
    pub bytes: u64,
    pub ficheros: u64,
    pub dirs: u64,
    /// El recorrido se agotó (entradas o tiempo): el total puede quedarse corto.
    pub truncado: bool,
    /// Patrones de exclusión que dejaron algo FUERA de esta medida (solo los que
    /// actuaron de verdad).
    pub excluidos: Vec<String>,
    /// Hijos directos que el analizador no listó (se quedaron fuera del top). No
    /// afecta al total de la carpeta, pero sí a la lista de «los que más crecen».
    pub resto_n: u64,
    pub resto_bytes: u64,
    pub hijos: Vec<HijoInstantanea>,
}

impl Instantanea {
    /// Convierte una medida del analizador en una instantánea guardable.
    ///
    /// `ts` se pasa aparte para que la conversión siga siendo pura (las pruebas
    /// pueden fijar el momento).
    pub fn desde_arbol(a: &Arbol, ts: i64) -> Self {
        Self {
            ts,
            ruta: a.ruta.clone(),
            bytes: a.bytes,
            ficheros: a.ficheros,
            dirs: a.dirs,
            truncado: a.truncado,
            excluidos: a.excluidos.clone(),
            resto_n: a.resto_n,
            resto_bytes: a.resto_bytes,
            hijos: a
                .hijos
                .iter()
                .map(|n| HijoInstantanea {
                    ruta: n.ruta.clone(),
                    nombre: n.nombre.clone(),
                    bytes: n.bytes,
                    ficheros: n.ficheros,
                    dirs: n.dirs,
                })
                .collect(),
        }
    }

    /// ¿El TOTAL de esta medida puede no ser el de la carpeta entera? Sí si el
    /// recorrido se cortó o si una exclusión dejó algo fuera. (`resto_n` es otra
    /// cosa: solo afecta a la lista de hijos.)
    pub fn es_parcial(&self) -> bool {
        self.truncado || !self.excluidos.is_empty()
    }

    /// ¿Están TODOS los hijos directos en `hijos`?
    pub fn hijos_completos(&self) -> bool {
        self.resto_n == 0
    }
}

/// Un hijo en la comparación. `pct` es `None` cuando antes estaba a cero: dividir
/// entre cero no da un porcentaje, y un `+∞ %` o un `100 %` inventado serían
/// mentira. `nuevo`/`desaparecido` dicen que el hijo no estaba en una de las dos
/// medidas (no que midiera cero).
#[derive(Debug, Clone, Serialize)]
pub struct CrecimientoHijo {
    pub ruta: String,
    pub nombre: String,
    pub antes: u64,
    pub ahora: u64,
    pub delta: i64,
    pub pct: Option<f64>,
    pub nuevo: bool,
    pub desaparecido: bool,
}

/// Lo que ha cambiado una carpeta entre dos medidas.
#[derive(Debug, Clone, Serialize)]
pub struct Crecimiento {
    pub ruta: String,
    pub antes_ts: i64,
    pub ahora_ts: i64,
    /// Diferencia real entre las dos medidas, en segundos. La interfaz la usa
    /// para escribir «hace N días» sin recalcularlo a partir del reloj de ahora.
    pub segundos: i64,
    pub antes_bytes: u64,
    pub ahora_bytes: u64,
    pub delta_bytes: i64,
    pub antes_ficheros: u64,
    pub ahora_ficheros: u64,
    pub delta_ficheros: i64,
    /// El delta TOTAL puede no ser el real: alguna de las dos medidas quedó
    /// incompleta. El `motivo` dice por qué.
    pub parcial: bool,
    pub motivo: Option<String>,
    /// No se guardaron todos los hijos de alguna de las dos medidas: la lista de
    /// abajo puede tener altas o bajas que en realidad son hijos que cambiaron de
    /// puesto, no que aparecieron o desaparecieron.
    pub hijos_parcial: bool,
    /// El mayor de los `resto_n` de las dos medidas: cuántos hijos faltan, como
    /// mínimo, en una de ellas.
    pub hijos_faltan: u64,
    /// De mayor a menor crecimiento. Incluye los que BAJAN (con delta negativo):
    /// lo que se ha liberado es información tan útil como lo que ha crecido.
    pub hijos: Vec<CrecimientoHijo>,
}

/// Compara la última medida con la anterior. `v` llega de la más reciente a la
/// más antigua (como la devuelve la base de datos).
///
/// Con menos de dos medidas devuelve `None` a propósito: un «+0 B» ahí se leería
/// como «no ha crecido», y lo que pasa es que no hay con qué comparar.
pub fn comparar_ultimas(v: &[Instantanea]) -> Option<Crecimiento> {
    if v.len() < 2 {
        return None;
    }
    Some(comparar(&v[1], &v[0]))
}

/// El crecimiento de `antes` a `ahora`. Función pura: no toca disco ni base.
pub fn comparar(antes: &Instantanea, ahora: &Instantanea) -> Crecimiento {
    // Los hijos se emparejan por RUTA, no por nombre: es la identidad estable
    // entre dos pases. Un hijo que se renombra aparece como uno nuevo y otro
    // desaparecido, y eso es exactamente lo que ha pasado.
    let mut mapa: BTreeMap<&str, (Option<&HijoInstantanea>, Option<&HijoInstantanea>)> =
        BTreeMap::new();
    for h in &antes.hijos {
        mapa.entry(h.ruta.as_str()).or_default().0 = Some(h);
    }
    for h in &ahora.hijos {
        mapa.entry(h.ruta.as_str()).or_default().1 = Some(h);
    }

    let mut hijos: Vec<CrecimientoHijo> = Vec::new();
    for (ruta, (a, b)) in mapa {
        let b_antes = a.map(|x| x.bytes).unwrap_or(0);
        let b_ahora = b.map(|x| x.bytes).unwrap_or(0);
        if b_antes == b_ahora {
            // Sin cambio no es «lo que más ha crecido»: se omite para no llenar la
            // lista de ruido.
            continue;
        }
        let delta = b_ahora as i64 - b_antes as i64;
        hijos.push(CrecimientoHijo {
            ruta: ruta.to_string(),
            nombre: b
                .or(a)
                .map(|x| x.nombre.clone())
                .unwrap_or_else(|| ruta.to_string()),
            antes: b_antes,
            ahora: b_ahora,
            delta,
            pct: (b_antes > 0).then(|| delta as f64 / b_antes as f64 * 100.0),
            nuevo: a.is_none(),
            desaparecido: b.is_none(),
        });
    }

    // De mayor a menor delta: arriba lo que más ha subido y abajo lo que más ha
    // bajado. El desempate por nombre hace el orden determinista (dos medidas
    // iguales no pueden salir en orden distinto en dos ejecuciones).
    hijos.sort_by(|x, y| y.delta.cmp(&x.delta).then_with(|| x.nombre.cmp(&y.nombre)));

    Crecimiento {
        ruta: ahora.ruta.clone(),
        antes_ts: antes.ts,
        ahora_ts: ahora.ts,
        segundos: ahora.ts - antes.ts,
        antes_bytes: antes.bytes,
        ahora_bytes: ahora.bytes,
        delta_bytes: ahora.bytes as i64 - antes.bytes as i64,
        antes_ficheros: antes.ficheros,
        ahora_ficheros: ahora.ficheros,
        delta_ficheros: ahora.ficheros as i64 - antes.ficheros as i64,
        parcial: antes.es_parcial() || ahora.es_parcial(),
        motivo: motivo_parcial(antes, ahora),
        hijos_parcial: !antes.hijos_completos() || !ahora.hijos_completos(),
        hijos_faltan: antes.resto_n.max(ahora.resto_n),
        hijos,
    }
}

/// Por qué una comparación no es de fiar, en una frase. `None` si las dos medidas
/// están completas.
fn motivo_parcial(antes: &Instantanea, ahora: &Instantanea) -> Option<String> {
    let mut partes: Vec<String> = Vec::new();
    match (antes.truncado, ahora.truncado) {
        (true, true) => partes.push("las dos medidas se cortaron por presupuesto".into()),
        (true, false) => partes.push("la medida anterior se cortó por presupuesto".into()),
        (false, true) => partes.push("la última medida se cortó por presupuesto".into()),
        _ => {}
    }
    // La unión de las dos listas: una exclusión que actuó en cualquiera de los
    // dos pases ya deja la comparación tocada.
    let mut excl: BTreeSet<&str> = BTreeSet::new();
    for p in antes.excluidos.iter().chain(ahora.excluidos.iter()) {
        excl.insert(p.as_str());
    }
    if !excl.is_empty() {
        partes.push(format!(
            "hay {} exclusión(es) actuando ({})",
            excl.len(),
            excl.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    (!partes.is_empty()).then(|| partes.join("; "))
}

/* ── La medida diaria en segundo plano ────────────────────────────────────── */

/// Hijos de primer nivel que se guardan en la medida diaria. Es el mismo orden de
/// magnitud que el analizador usa en la interfaz: con menos, «lo que más ha
/// crecido» se quedaría sin candidatos.
const HIJOS_DIARIOS: usize = 400;

/// Las carpetas que se miden una vez al día: el hogar y la carpeta de modelos
/// principal (`~/models`, la del inventario).
///
/// Se miden las dos porque los modelos son lo que más crece y su crecimiento
/// quedaría diluido en el total del hogar. Si la carpeta de modelos no existe, no
/// se inventa una ruta: se mide solo el hogar.
fn raices_diarias() -> Vec<String> {
    let r = crate::plataforma::rutas();
    let mut v = vec![r.home.to_string_lossy().to_string()];
    let modelos = r.home.join("models");
    if modelos.is_dir() {
        v.push(modelos.to_string_lossy().to_string());
    }
    v
}

/// Mide en segundo plano las raíces diarias y guarda su instantánea.
///
/// Devuelve un resumen para dejar rastro en stderr, o `None` si no tocaba (el
/// ajuste está apagado o ya se midió hoy) o no se pudo medir nada.
///
/// Se marca el día aunque alguna medición falle: si no, una carpeta que ya no
/// existe haría repetir el recorrido entero en cada vuelta del bucle.
pub fn diaria() -> Option<String> {
    if crate::db::setting_int("historial_activo", 1) == 0 {
        return None;
    }
    let hoy = chrono::Local::now().format("%Y-%m-%d").to_string();
    if matches!(crate::db::historial_ultimo_dia(), Ok(Some(d)) if d == hoy) {
        return None;
    }
    let filtro = crate::almacen::Filtro::reales();
    let mut hechas: Vec<String> = Vec::new();
    for raiz in raices_diarias() {
        match crate::almacen::arbol_con(&raiz, HIJOS_DIARIOS, &filtro) {
            Ok(a) => {
                let inst = Instantanea::desde_arbol(&a, crate::db::now_ts());
                match crate::db::guardar_instantanea(&inst) {
                    Ok(()) => hechas.push(raiz),
                    Err(e) => eprintln!("no se pudo guardar la medida diaria de {raiz}: {e}"),
                }
            }
            Err(e) => eprintln!("no se pudo medir {raiz} para el histórico de disco: {e}"),
        }
    }
    if let Err(e) = crate::db::marcar_historial_dia(&hoy) {
        eprintln!("no se pudo anotar el día del histórico de disco: {e}");
    }
    (!hechas.is_empty()).then(|| format!("histórico de disco: medidas {}", hechas.join(", ")))
}

/* ── Pruebas ──────────────────────────────────────────────────────────────── */

#[cfg(test)]
mod pruebas {
    use super::*;

    fn hijo(nombre: &str, bytes: u64) -> HijoInstantanea {
        HijoInstantanea {
            ruta: format!("/home/x/{nombre}"),
            nombre: nombre.into(),
            bytes,
            ficheros: 1,
            dirs: 0,
        }
    }

    /// Una medida de prueba: la raíz más los hijos que se le pasen.
    fn medida(ts: i64, bytes: u64, hijos: &[(&str, u64)]) -> Instantanea {
        Instantanea {
            ts,
            ruta: "/home/x".into(),
            bytes,
            ficheros: 10,
            dirs: 2,
            truncado: false,
            excluidos: vec![],
            resto_n: 0,
            resto_bytes: 0,
            hijos: hijos.iter().map(|(n, b)| hijo(n, *b)).collect(),
        }
    }

    /// Con una sola medida no hay comparación: `None`, nunca un «+0 B» que se
    /// leería como «no ha crecido».
    #[test]
    fn una_sola_medida_no_da_comparacion() {
        assert!(comparar_ultimas(&[]).is_none());
        assert!(comparar_ultimas(&[medida(100, 10, &[("a", 10)])]).is_none());
        assert!(comparar_ultimas(&[medida(200, 12, &[]), medida(100, 10, &[])]).is_some());
    }

    /// La comparación da el delta del total y, ordenados, los hijos que han
    /// crecido y los que han bajado.
    #[test]
    fn la_comparacion_da_el_delta_y_los_hijos_que_cambian() {
        let antes = medida(100, 1_000, &[("grande", 600), ("pequena", 400)]);
        let ahora = medida(200, 1_300, &[("grande", 900), ("pequena", 300), ("nueva", 100)]);

        let c = comparar(&antes, &ahora);

        assert_eq!(c.delta_bytes, 300);
        assert_eq!(c.segundos, 100);
        assert!(!c.parcial, "las dos medidas están completas");
        assert!(c.motivo.is_none());

        // De mayor a menor delta: grande (+300), nueva (+100) y pequena (-100).
        let nombres: Vec<&str> = c.hijos.iter().map(|h| h.nombre.as_str()).collect();
        assert_eq!(nombres, vec!["grande", "nueva", "pequena"]);

        let grande = &c.hijos[0];
        assert_eq!((grande.delta, grande.pct), (300, Some(50.0)));
        assert!(!grande.nuevo && !grande.desaparecido);

        // Un hijo que no ha cambiado no aparece: no es «lo que más ha crecido».
        assert!(c.hijos.iter().all(|h| h.delta != 0));
    }

    /// Un hijo que BAJA sale con su delta negativo y su porcentaje: lo que se ha
    /// liberado es información igual de útil que lo que ha crecido.
    #[test]
    fn un_hijo_que_baja_sale_con_delta_negativo() {
        let antes = medida(100, 1_000, &[("cachés", 800), ("datos", 200)]);
        let ahora = medida(200, 500, &[("cachés", 300), ("datos", 200)]);

        let c = comparar(&antes, &ahora);

        assert_eq!(c.delta_bytes, -500);
        let bajado = c.hijos.iter().find(|h| h.nombre == "cachés").unwrap();
        assert_eq!(bajado.delta, -500);
        assert_eq!(bajado.pct, Some(-62.5));
        // El que no cambió no está en la lista.
        assert_eq!(c.hijos.len(), 1);
    }

    /// Un hijo que solo está en una de las dos medidas se dice como nuevo o
    /// desaparecido, y sin porcentaje inventado cuando antes era cero.
    #[test]
    fn un_hijo_nuevo_o_desaparecido_se_dice_y_no_se_inventa_el_porcentaje() {
        let antes = medida(100, 500, &[("viejo", 500)]);
        let ahora = medida(200, 700, &[("recien-llegado", 700)]);

        let c = comparar(&antes, &ahora);
        let nombres: Vec<&str> = c.hijos.iter().map(|h| h.nombre.as_str()).collect();
        assert_eq!(nombres, vec!["recien-llegado", "viejo"]);

        let nuevo = &c.hijos[0];
        assert!(nuevo.nuevo && !nuevo.desaparecido);
        assert_eq!(nuevo.pct, None, "de cero no sale un porcentaje: no se inventa");

        let ido = &c.hijos[1];
        assert!(ido.desaparecido && !ido.nuevo);
        assert_eq!(ido.delta, -500);
        assert_eq!(ido.pct, Some(-100.0));
    }

    /// Una medida parcial (presupuesto agotado o exclusiones) marca la
    /// comparación y dice POR QUÉ: presentarla como completa daría un crecimiento
    /// que no es el real.
    #[test]
    fn las_medidas_parciales_se_marcan_y_se_explican() {
        let antes = medida(100, 1_000, &[("a", 1_000)]);
        let mut ahora = medida(200, 1_200, &[("a", 1_200)]);
        ahora.truncado = true;
        ahora.excluidos = vec!["${HOME}/VMs".into(), "*.iso".into()];

        let c = comparar(&antes, &ahora);

        assert!(c.parcial);
        let m = c.motivo.unwrap();
        assert!(m.contains("presupuesto"), "{m}");
        assert!(m.contains("${HOME}/VMs"), "{m}");
        assert!(m.contains("*.iso"), "{m}");
    }

    /// Que falten hijos por listar NO invalida el total (el recorrido midió todo);
    /// solo avisa de que la lista de hijos puede tener altas o bajas falsas.
    #[test]
    fn los_hijos_incompletos_se_marcan_sin_tocar_el_total() {
        let antes = medida(100, 1_000, &[("a", 600)]);
        let mut ahora = medida(200, 1_100, &[("a", 700)]);
        ahora.resto_n = 12;
        ahora.resto_bytes = 400;

        let c = comparar(&antes, &ahora);

        assert!(!c.parcial, "el total está medido: no es una medida parcial");
        assert!(c.hijos_parcial);
        assert_eq!(c.hijos_faltan, 12);
    }

    /// La conversión desde el analizador conserva lo que hace falta para saber si
    /// la medida es completa, sin volver a recorrer el árbol.
    #[test]
    fn desde_arbol_conserva_la_calidad_de_la_medida() {
        let arbol = Arbol {
            ruta: "/tmp/x".into(),
            bytes: 42,
            ficheros: 3,
            dirs: 1,
            hijos: vec![crate::almacen::Nodo {
                ruta: "/tmp/x/a".into(),
                nombre: "a".into(),
                bytes: 42,
                ficheros: 3,
                dirs: 0,
                es_dir: true,
                modificado: None,
            }],
            resto_n: 2,
            resto_bytes: 7,
            truncado: true,
            omitidos: 0,
            entradas: 9,
            ms: 1,
            excluidos: vec!["*.tmp".into()],
        };

        let i = Instantanea::desde_arbol(&arbol, 999);

        assert_eq!(i.ts, 999);
        assert!(i.es_parcial());
        assert!(!i.hijos_completos());
        assert_eq!(i.resto_n, 2);
        assert_eq!(i.hijos.len(), 1);
        assert_eq!(i.hijos[0].bytes, 42);
    }
}
