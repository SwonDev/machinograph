//! Copias de seguridad de los ficheros que la app MODIFICA, y cómo volver atrás.
//!
//! POR QUÉ EXISTE: la app toca ficheros que no son suyos —la configuración de un
//! cliente de IA, una entrada de arranque— y hasta ahora cada sitio se hacía su
//! propia copia (`conexiones.rs` y el autoarranque), sin que nadie pudiera verlas
//! ni restaurarlas desde la interfaz. Esto es el Centro de recuperación de Kudu,
//! con la regla de esta casa: **la copia vive al lado del original** (que es donde
//! uno la busca con un gestor de archivos) y la base de datos guarda el índice.
//!
//! Tres decisiones:
//!
//! 1. **El nombre lleva la fecha** (`models.json.bak-20261003-130501`), así que
//!    dos copias nunca se pisan y se puede decir cuándo se hizo cada una.
//! 2. **Restaurar es reversible**: antes de pisar el original se copia el estado
//!    ACTUAL, de modo que «restaurar» no puede perder lo que había.
//! 3. **Se comprueba releyendo**: restaurar significa que el original queda byte a
//!    byte como la copia; si no, se dice en vez de dar por hecho que fue bien.
use std::path::{Path, PathBuf};

pub use crate::db::CopiaRow;

/// Marca de tiempo para los nombres de copia: `20261003-130501`.
pub fn marca_de_tiempo() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Copia un fichero a su lado y la anota. Devuelve la ruta de la copia.
// Solo lo llama la escritura de ficheros de arranque de Linux (aquí se compila y
// se prueba): macOS y Windows no escriben esos ficheros, así que fuera de Linux no
// hay quien la llame.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn copia_de_seguridad(ruta: &Path) -> Result<PathBuf, String> {
    copia_con_motivo(ruta, "cambio desde Machinograph")
}

/// Igual, diciendo POR QUÉ se hizo la copia (se enseña en el Centro de
/// recuperación: sin el motivo, una lista de rutas no dice nada).
pub fn copia_con_motivo(ruta: &Path, motivo: &str) -> Result<PathBuf, String> {
    if !ruta.is_file() {
        return Err(format!("no hay nada que copiar en {}", ruta.display()));
    }
    // El nombre lleva la fecha, pero la fecha tiene resolución de SEGUNDO: dos
    // copias del mismo fichero en el mismo segundo chocarían y la segunda
    // destruiría a la primera. Pasó de verdad (lo encontró una prueba): al
    // restaurar, la copia de seguridad previa se llamaba igual que la que se iba a
    // restaurar y la dejaba inservible. Por eso se numera si ya existe.
    let marca = marca_de_tiempo();
    let mut destino = PathBuf::from(format!("{}.bak-{marca}", ruta.to_string_lossy()));
    let mut n = 1;
    while destino.exists() {
        destino = PathBuf::from(format!("{}.bak-{marca}-{n}", ruta.to_string_lossy()));
        n += 1;
    }
    std::fs::copy(ruta, &destino).map_err(|e| format!("no se pudo copiar {}: {e}", ruta.display()))?;
    let bytes = std::fs::metadata(&destino).map(|m| m.len() as i64).unwrap_or(0);
    // Anotarla es lo que la hace encontrable; si la base de datos no está, la
    // copia sigue existiendo (y se dice).
    if let Err(e) = crate::db::insert_copia(
        &ruta.to_string_lossy(),
        &destino.to_string_lossy(),
        bytes,
        motivo,
    ) {
        return Err(format!(
            "la copia se hizo en {} pero no se pudo anotar ({e}); búscala por su nombre",
            destino.display()
        ));
    }
    Ok(destino)
}

/// Las copias conocidas, de la más nueva a la más vieja.
pub fn listar(limit: i64) -> Result<Vec<CopiaRow>, String> {
    crate::db::copias(limit).map_err(|e| e.to_string())
}

/// Devuelve un fichero a como estaba en una copia.
///
/// Antes de pisar el original se copia lo que hay AHORA, así que restaurar
/// tampoco es irreversible. Y al final se comprueba releyendo que los dos
/// ficheros coinciden: si no coinciden, se dice.
pub fn restaurar(id: i64) -> Result<String, String> {
    let c = crate::db::copia(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no hay ninguna copia con el id {id}"))?;
    let copia = Path::new(&c.ruta_copia);
    let original = Path::new(&c.ruta_original);
    if !copia.is_file() {
        return Err(format!(
            "la copia ya no está en {} (¿la borraste a mano?)",
            c.ruta_copia
        ));
    }
    // El estado actual, a salvo antes de tocarlo.
    let previa = if original.is_file() {
        Some(copia_con_motivo(original, "antes de restaurar")?)
    } else {
        None
    };
    std::fs::copy(copia, original)
        .map_err(|e| format!("no se pudo restaurar {}: {e}", original.display()))?;

    // Comprobar releyendo: el mismo criterio que en el puente de conexiones.
    let a = std::fs::read(copia).map_err(|e| format!("no se pudo releer la copia: {e}"))?;
    let b = std::fs::read(original).map_err(|e| format!("no se pudo releer el original: {e}"))?;
    if a != b {
        return Err(format!(
            "restaurado, pero la comprobación NO cuadra ({} bytes frente a {}) en {}",
            a.len(),
            b.len(),
            original.display()
        ));
    }
    Ok(match previa {
        Some(p) => format!(
            "Restaurado {} desde la copia y comprobado releyendo. Lo que había antes quedó en {}",
            original.display(),
            p.display()
        ),
        None => format!("Restaurado {} desde la copia y comprobado releyendo.", original.display()),
    })
}

/// Quita una copia del índice y del disco. No toca el original.
pub fn borrar(id: i64) -> Result<String, String> {
    let c = crate::db::copia(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no hay ninguna copia con el id {id}"))?;
    let p = Path::new(&c.ruta_copia);
    if p.is_file() {
        std::fs::remove_file(p).map_err(|e| format!("no se pudo borrar {}: {e}", c.ruta_copia))?;
    }
    crate::db::borrar_copia(id).map_err(|e| e.to_string())?;
    Ok(format!("Copia {} borrada (el original no se ha tocado).", c.ruta_copia))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn copia_y_restaura_de_verdad_en_un_fichero_temporal() {
        let dir = std::env::temp_dir().join("machinograph-copias-prueba");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config.json");
        std::fs::write(&f, b"{\"original\": true}").unwrap();

        // La marca de tiempo tiene la forma esperada (AAAAMMDD-HHMMSS).
        let m = marca_de_tiempo();
        assert_eq!(m.len(), 15, "{m}");
        assert_eq!(&m[8..9], "-");
        assert!(m[0..8].chars().all(|c| c.is_ascii_digit()), "{m}");

        // Con la base de datos caída, la copia se hace igual y se AVISA de que no
        // se pudo anotar (que es distinto de "no se copió").
        match copia_con_motivo(&f, "prueba") {
            Ok(copia) => {
                assert!(copia.exists(), "la copia tiene que existir en el disco");
                assert!(copia.to_string_lossy().contains(".bak-"));
                // Y restaurar deja el original como estaba.
                std::fs::write(&f, b"{\"cambiado\": true}").unwrap();
                let id = crate::db::copias(10)
                    .unwrap()
                    .into_iter()
                    .find(|c| c.ruta_copia == copia.to_string_lossy())
                    .map(|c| c.id);
                if let Some(id) = id {
                    let msg = restaurar(id).expect("debería restaurar");
                    assert!(msg.contains("comprobado releyendo"), "{msg}");
                    assert_eq!(std::fs::read(&f).unwrap(), b"{\"original\": true}");
                    // Y se recogen las filas que esta prueba ha dejado: la base de
                    // datos es la de verdad (la del usuario), y una prueba no debe
                    // ensuciarla. Se borran solo las de ESTE fichero.
                    let original = f.to_string_lossy().to_string();
                    for c in crate::db::copias(1000).unwrap() {
                        if c.ruta_original == original {
                            let _ = crate::db::borrar_copia(c.id);
                        }
                    }
                }
            }
            Err(e) => {
                assert!(e.contains("no se pudo anotar"), "{e}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restaurar_algo_que_no_existe_no_toca_nada() {
        let e = restaurar(999_999).unwrap_err();
        assert!(e.contains("no hay ninguna copia"), "{e}");
    }
}
