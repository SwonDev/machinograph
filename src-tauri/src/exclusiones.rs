//! Exclusiones: lo que NO se mide y NO se borra, y por qué.
//!
//! POR QUÉ EXISTE (y de dónde viene la idea): Kudu tiene una lista de exclusiones
//! globales que respetan TODAS sus herramientas —el limpiador, los ficheros
//! grandes, los repetidos, las carpetas vacías y los accesos directos—, y su
//! desinstalador también (lo arreglaron en su 3.5, issues #484 y #492). En este
//! panel faltaba: no había forma de decir «mis máquinas virtuales no me las
//! cuentes ni me las toques» salvo no mirar esa carpeta.
//!
//! Aquí la lista es UNA y la respetan el analizador de disco (árbol, grandes,
//! repetidos, vacías, enlaces y búsqueda), el escaneo de limpieza y el borrado.
//! Y con una regla que Kudu no cumple y aquí sí: **cuando algo se deja fuera, se
//! dice cuál es la exclusión que lo dejó fuera**. Un total que encoge en silencio
//! es peor que un total grande: parece que tienes menos de lo que tienes.
//!
//! LENGUAJE DE LAS RUTAS: el mismo del catálogo de limpieza (`${HOME}`,
//! `${CACHE}`, `${CONFIG}`, `${LOCALAPPDATA}`, `${TEMP}`…) porque el usuario ya lo
//! tiene delante en Optimización, y `~` al principio como en cualquier terminal.
//! Además se puede escribir un PATRÓN con comodines (`*.iso`) o un nombre suelto
//! (`node_modules`), que vale para cualquier carpeta con ese nombre.
//!
//! POR QUÉ NO HAY LISTA POR DEFECTO: la promesa del analizador es «esto es lo que
//! ocupa tu disco». Traer de fábrica carpetas ocultas haría que los totales no
//! cuadraran con lo que enseña el gestor de archivos y nadie sabría por qué. La
//! lista empieza VACÍA y la llena quien la quiere usar; lo que sí está de fábrica
//! es la protección de las raíces del sistema, que es otra cosa y ya existía
//! (`almacen::permitida`).

use std::path::PathBuf;

/// Una exclusión vigente: el texto que escribió el usuario y lo que significa ya
/// resuelto, para no expandir el mismo patrón en cada fichero de un recorrido de
/// un millón de entradas.
#[derive(Debug, Clone)]
pub struct Vigente {
    /// Tal cual lo escribió el usuario (lo que se enseña).
    pub patron: String,
    /// Ruta absoluta ya expandida (`~`/`${HOME}` resueltos), o `None` si el patrón
    /// es un glob o un nombre suelto que no se ancla a una carpeta.
    pub base: Option<PathBuf>,
    /// El glob, si el patrón lleva comodines o es un nombre suelto.
    pub globo: Option<glob::Pattern>,
    /// Si el patrón no lleva separador, vale para cualquier componente del camino.
    pub por_componente: bool,
}

impl Vigente {
    /// Cómo se enseña en la interfaz: el patrón y, si se resolvió, a qué carpeta
    /// apunta. Sin esto, «${HOME}/VMs» no se entiende.
    pub fn descripcion(&self) -> String {
        match &self.base {
            Some(b) => format!("{} → {}", self.patron, b.display()),
            None => self.patron.clone(),
        }
    }
}

/// ¿Este sistema compara rutas sin distinguir mayúsculas? En Windows y macOS el
/// sistema de ficheros NO distingue (`Informes` y `informes` son la misma
/// carpeta), así que una exclusión escrita con otra caja tiene que valer igual.
/// Se pasa como parámetro a `coincide` para poder probar los dos modos en Linux.
pub fn sin_distinguir_caja() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

/// Normaliza una ruta SIN tocar el disco: quita `.`, resuelve `..` por texto y
/// unifica los separadores. No sigue enlaces a propósito (sería una llamada al
/// sistema por cada fichero de un recorrido, y para decidir una exclusión no hace
/// falta: las rutas que se comparan salen del propio recorrido, ya resueltas).
pub fn normalizar(ruta: &str) -> String {
    let unificada = ruta.replace('\\', "/");
    let mut partes: Vec<&str> = Vec::new();
    let mut absoluta = false;
    let mut prefijo = String::new();
    for (i, p) in unificada.split('/').enumerate() {
        if i == 0 && p.is_empty() {
            absoluta = true;
            continue;
        }
        // En Windows la raíz es `C:`; se conserva como primer trozo.
        if i == 0 && p.len() == 2 && p.ends_with(':') {
            prefijo = format!("{p}/");
            continue;
        }
        match p {
            "" | "." => {}
            ".." => {
                partes.pop();
            }
            otro => partes.push(otro),
        }
    }
    let cuerpo = partes.join("/");
    let mut s = format!("{prefijo}{cuerpo}");
    if absoluta {
        s = format!("/{s}");
    }
    if s.is_empty() {
        s.push('.');
    }
    s
}

/// Expande una exclusión escrita a mano: `~` y `${VARIABLE}` como en el catálogo.
/// Se reutiliza `limpieza::expandir` para que las variables sean EXACTAMENTE las
/// mismas que las de las reglas (si allí se añade una, aquí ya funciona).
fn expandir(patron: &str) -> String {
    let p = patron.trim();
    let con_home = match p.strip_prefix("~/") {
        Some(resto) => format!("${{HOME}}/{resto}"),
        // `~` a secas: la carpeta personal.
        None if p == "~" => "${HOME}".to_string(),
        None => p.to_string(),
    };
    crate::plataforma::expandir_plantilla(&con_home)
}

/// Prepara una exclusión para usarla. Puro salvo por leer el entorno de las
/// variables (`${HOME}` y compañía), que es lo que hace `expandir`.
pub fn preparar(patron: &str) -> Result<Vigente, String> {
    let p = patron.trim();
    if p.is_empty() {
        return Err("una exclusión no puede estar vacía".into());
    }
    let con_comodines = p.contains('*') || p.contains('?') || p.contains('[');
    let por_componente = !p.contains('/') && !p.contains('\\') && p != "~";
    if con_comodines || por_componente {
        let globo = glob::Pattern::new(&expandir(p))
            .map_err(|e| format!("«{p}» no es un patrón válido: {e}"))?;
        return Ok(Vigente {
            patron: p.to_string(),
            base: None,
            globo: Some(globo),
            por_componente,
        });
    }
    // Sin comodines y con separador: es una CARPETA. Vale ella y todo lo de dentro.
    let expandida = expandir(p);
    let abs = PathBuf::from(&expandida);
    if !abs.is_absolute() {
        return Err(format!(
            "«{p}» no es una ruta absoluta: escribe una carpeta completa (por ejemplo ${{HOME}}/VMs) o un patrón (*.iso)"
        ));
    }
    Ok(Vigente {
        patron: p.to_string(),
        base: Some(PathBuf::from(normalizar(&expandida))),
        globo: None,
        por_componente: false,
    })
}

/// ¿Coincide esta ruta con esta exclusión? `None` si no.
///
/// Reglas, en orden (y todas se prueban):
/// 1. Una carpeta excluida excluye lo de dentro, con el separador como frontera
///    (si no, excluir `/home/x/VMs` excluiría `/home/x/VMs2`, que es otra carpeta).
/// 2. Un patrón con comodines se prueba contra la ruta completa y contra el
///    nombre del fichero.
/// 3. Un nombre suelto (`node_modules`) vale para cualquier componente del camino.
pub fn coincide_con(ruta: &str, v: &Vigente, caja_sensible: bool) -> bool {
    let r = normalizar(ruta);
    let (r, patron_base) = if caja_sensible {
        (
            r,
            v.base.as_ref().map(|b| normalizar(&b.to_string_lossy())),
        )
    } else {
        (
            r.to_lowercase(),
            v.base.as_ref().map(|b| normalizar(&b.to_string_lossy()).to_lowercase()),
        )
    };
    if let Some(base) = patron_base {
        if r == base {
            return true;
        }
        return r.starts_with(&format!("{base}/"));
    }
    let Some(globo) = &v.globo else {
        return false;
    };
    let patron = if caja_sensible {
        globo.to_string()
    } else {
        globo.to_string().to_lowercase()
    };
    if let Ok(g) = glob::Pattern::new(&patron) {
        if v.por_componente {
            // Cualquier trozo del camino, no solo el final: `node_modules` tiene
            // que valer también para `x/node_modules/y/fichero`.
            return r.split('/').any(|c| g.matches(c));
        }
        if g.matches(&r) {
            return true;
        }
        if let Some(nombre) = r.rsplit('/').next() {
            return g.matches(nombre);
        }
    }
    false
}

/// La primera exclusión que cubre esta ruta, si hay alguna. Se devuelve la
/// exclusión (no un `bool`) porque quien llama tiene que poder decir CUÁL fue.
pub fn excluida_con<'a>(
    ruta: &str,
    vigentes: &'a [Vigente],
    caja_sensible: bool,
) -> Option<&'a Vigente> {
    vigentes.iter().find(|v| coincide_con(ruta, v, caja_sensible))
}

/* ── Persistencia (SQLite) ────────────────────────────────────────────────── */

/// Las exclusiones guardadas, más recientes primero. Si la base de datos no
/// responde, devuelve vacío: no poder leerlas deja el panel como estaba antes de
/// que existieran (nada excluido), que es la opción segura.
pub fn vigentes() -> Vec<Vigente> {
    let filas = match crate::db::exclusiones_listar() {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for (patron, _ts) in filas {
        if let Ok(v) = preparar(&patron) {
            out.push(v);
        }
    }
    out
}

/// Añade una exclusión. Valida ANTES de guardar (un patrón roto no llega a la
/// base de datos) y no deja duplicados.
pub fn anadir(patron: &str) -> Result<String, String> {
    let v = preparar(patron)?;
    if crate::db::exclusiones_listar()
        .map(|f| f.iter().any(|(p, _)| p == &v.patron))
        .unwrap_or(false)
    {
        return Err(format!("«{}» ya estaba en la lista", v.patron));
    }
    crate::db::exclusiones_anadir(&v.patron).map_err(|e| format!("no se pudo guardar: {e}"))?;
    Ok(match &v.base {
        Some(b) => format!("Excluido «{}» ({})", v.patron, b.display()),
        None => format!("Excluido el patrón «{}»", v.patron),
    })
}

pub fn quitar(patron: &str) -> Result<String, String> {
    let n = crate::db::exclusiones_quitar(patron.trim())
        .map_err(|e| format!("no se pudo quitar: {e}"))?;
    if n == 0 {
        return Err(format!("«{}» no estaba en la lista", patron.trim()));
    }
    Ok(format!("Quitada la exclusión «{}»", patron.trim()))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn v(p: &str) -> Vigente {
        preparar(p).unwrap()
    }

    #[test]
    fn una_carpeta_excluida_excluye_lo_de_dentro_pero_no_a_sus_vecinas() {
        let e = v("/home/x/VMs");
        assert!(coincide_con("/home/x/VMs", &e, true));
        assert!(coincide_con("/home/x/VMs/disco.qcow2", &e, true));
        assert!(coincide_con("/home/x/VMs/a/b/c", &e, true));
        // La frontera es el separador: `VMs2` es OTRA carpeta.
        assert!(!coincide_con("/home/x/VMs2", &e, true));
        assert!(!coincide_con("/home/x/VMs2/disco.qcow2", &e, true));
        // Y la carpeta de arriba no está excluida.
        assert!(!coincide_con("/home/x", &e, true));
    }

    #[test]
    fn las_rutas_se_normalizan_antes_de_comparar() {
        let e = v("/home/x/VMs");
        assert!(coincide_con("/home/x/./VMs//disco.qcow2", &e, true));
        assert!(coincide_con("/home/x/otra/../VMs/disco.qcow2", &e, true));
        // La barra final no cambia nada.
        assert!(coincide_con("/home/x/VMs/", &e, true));
        // Y `..` no puede sacar de la exclusión a lo que está dentro.
        assert!(!coincide_con("/home/x/VMs/../otra.txt", &e, true));
    }

    #[test]
    fn los_comodines_valen_para_el_nombre_y_para_la_ruta() {
        let iso = v("*.iso");
        assert!(coincide_con("/datos/ubuntu.iso", &iso, true));
        assert!(coincide_con("/datos/sub/windows.iso", &iso, true));
        assert!(!coincide_con("/datos/ubuntu.img", &iso, true));
        // Con ruta delante, el glob se prueba contra la ruta entera.
        let tmp = v("/var/tmp/*");
        assert!(coincide_con("/var/tmp/algo", &tmp, true));
        assert!(!coincide_con("/var/tmp", &tmp, true));
    }

    #[test]
    fn un_nombre_suelto_vale_para_cualquier_componente() {
        let nm = v("node_modules");
        assert!(coincide_con("/proyectos/a/node_modules", &nm, true));
        assert!(coincide_con("/proyectos/a/node_modules/x/y.js", &nm, true));
        // También si está en medio del camino.
        assert!(coincide_con("/p/node_modules/a/b", &nm, true));
        assert!(!coincide_con("/p/nodo_modules/a", &nm, true));
    }

    #[test]
    fn en_windows_y_macos_la_caja_no_cuenta_pero_en_linux_si() {
        let e = v("/home/x/Informes");
        // Como en Windows/macOS (sin distinguir caja).
        assert!(coincide_con("/home/x/informes/2026.txt", &e, false));
        // Como en Linux.
        assert!(!coincide_con("/home/x/informes/2026.txt", &e, true));
        // Y la función que dice cómo es este sistema concuerda con el `cfg`.
        assert_eq!(sin_distinguir_caja(), cfg!(any(target_os = "windows", target_os = "macos")));
    }

    #[test]
    fn una_exclusion_escrita_mal_se_rechaza_con_el_motivo() {
        assert!(preparar("").is_err());
        assert!(preparar("   ").is_err());
        let e = preparar("carpeta/relativa").unwrap_err();
        assert!(e.contains("absoluta"), "{e}");
        // Un patrón con comodines SÍ puede ser relativo (es un nombre, no una ruta).
        assert!(preparar("*.log").is_ok());
    }

    #[test]
    fn la_descripcion_dice_a_que_carpeta_apunta_la_exclusion() {
        let d = v("/home/x/VMs").descripcion();
        assert!(d.contains("/home/x/VMs"), "{d}");
        // Un patrón no se resuelve a ninguna carpeta: se enseña tal cual.
        assert_eq!(v("*.iso").descripcion(), "*.iso");
    }

    #[test]
    fn la_casa_del_usuario_se_expande() {
        // `~` y `${HOME}` tienen que dar lo mismo (y apuntar al home de verdad).
        let con_tilde = preparar("~/VMs").unwrap();
        let con_var = preparar("${HOME}/VMs").unwrap();
        assert_eq!(con_tilde.base, con_var.base);
        let home = crate::plataforma::rutas().home;
        assert!(con_tilde.base.unwrap().starts_with(home));
    }

    #[test]
    fn gana_la_primera_exclusion_y_se_dice_cual() {
        let lista = vec![v("*.iso"), v("/home/x/VMs")];
        let cual = excluida_con("/home/x/VMs/ubuntu.iso", &lista, true).unwrap();
        assert_eq!(cual.patron, "*.iso");
        assert!(excluida_con("/home/x/otra.txt", &lista, true).is_none());
    }
}
