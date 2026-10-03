//! La papelera del sistema: mover a ella, vaciarla y listarla.
//!
//! DOS IMPLEMENTACIONES, y no por capricho:
//!
//! * **Linux**: la implementación de casa (especificación freedesktop: `rename` a
//!   `~/.local/share/Trash/files` + un `.trashinfo` con la ruta original). Se
//!   queda la nuestra porque la del crate `trash` documenta **UB potencial** en
//!   Linux (usa `getmntent`, que no es reentrante) y aquí se llama desde los
//!   comandos de Tauri, que corren en varios hilos. Además ya está probada en
//!   esta máquina, con su cruce de sistemas de ficheros incluido.
//! * **macOS y Windows**: los hace el crate `trash` (NSFileManager y
//!   `IFileOperation`), que es exactamente lo que hay que usar ahí y lo que sería
//!   temerario reescribir a mano sin poder probarlo.
//!
//! La fecha del `.trashinfo` se escribe con `chrono` en hora LOCAL, que es lo que
//! pide la especificación; antes se calculaba a mano y en UTC.
use std::path::Path;

/// Mueve un fichero o una carpeta a la papelera y devuelve el mensaje que se
/// enseña (dice que se puede recuperar).
///
/// No valida el origen: quién puede borrar qué es una decisión de cada llamante
/// (`inventario` exige que sea un modelo; `almacen` exige que esté dentro del home
/// o de una raíz permitida).
pub fn mover(p: &Path) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(p).map_err(|_| format!("no existe {}", p.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} es un enlace simbólico; no se mueve", p.display()));
    }
    mover_impl(p)?;
    let base = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string_lossy().to_string());
    Ok(format!(
        "'{base}' movido a la papelera (se puede recuperar desde el gestor de archivos)."
    ))
}

/// Borra un fichero o una carpeta DE VERDAD (sin papelera).
///
/// Es igual en los tres sistemas: `remove_file` / `remove_dir_all` de la std.
/// Solo lo llaman la limpieza de basura (cachés y temporales, regenerables por
/// definición) y `mover` como último paso cuando no ha podido renombrar entre
/// montajes. Nunca decide desde aquí si una ruta se puede borrar: eso lo valida
/// cada llamante con su propia lista.
pub fn borrar_definitivo(p: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(p)
        .map_err(|e| format!("no se pudo leer {}: {e}", p.display()))?;
    if meta.file_type().is_symlink() {
        return std::fs::remove_file(p).map_err(|e| format!("no se pudo borrar {}: {e}", p.display()));
    }
    if meta.is_dir() {
        std::fs::remove_dir_all(p).map_err(|e| format!("no se pudo borrar {}: {e}", p.display()))
    } else {
        std::fs::remove_file(p).map_err(|e| format!("no se pudo borrar {}: {e}", p.display()))
    }
}

/// Cuántos elementos hay en la papelera y cuánto ocupan (lo que se liberaría al
/// vaciarla). `None` cuando el sistema no deja contarlo.
pub fn resumen() -> Option<(u64, u64)> {
    #[cfg(target_os = "windows")]
    {
        // En Windows la papelera no es un directorio: se pregunta al sistema, que
        // es quien sabe dónde está cada elemento y cuánto ocupa.
        let items = trash::os_limited::list().ok()?;
        let mut total = 0u64;
        for it in &items {
            if let Ok(m) = trash::os_limited::metadata(it) {
                if let trash::TrashItemSize::Bytes(b) = m.size {
                    total += b;
                }
            }
        }
        Some((items.len() as u64, total))
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Linux y macOS: la papelera SÍ es un directorio del home (`Trash/files`
        // y `~/.Trash`), así que se cuenta y se mide ahí. Es exactamente lo que
        // enseña el gestor de archivos y no depende de la API del crate.
        let dir = if cfg!(target_os = "macos") {
            super::rutas().papelera()
        } else {
            super::rutas().papelera().join("files")
        };
        let mut n = 0u64;
        let mut bytes = 0u64;
        for e in std::fs::read_dir(&dir).ok()?.flatten() {
            // En macOS la papelera es el directorio `~/.Trash` ENTERO y Finder deja
            // su propio `.DS_Store` dentro: contarlo sería contar una entrada que
            // nadie ha mandado a la papelera.
            if cfg!(target_os = "macos") && e.file_name().to_string_lossy() == ".DS_Store" {
                continue;
            }
            n += 1;
            bytes += crate::almacen::tamano(&e.path());
        }
        Some((n, bytes))
    }
}

/// Vacía la papelera. Es una de las acciones que MÁS espacio libera de golpe (lo
/// que se mandó allí no ocupa espacio libre hasta que se vacía, y la interfaz lo
/// dice así).
pub fn vaciar() -> Result<String, String> {
    let (n, bytes) = resumen().ok_or("este sistema no deja contar la papelera")?;
    if n == 0 {
        return Ok("La papelera ya estaba vacía.".into());
    }
    let restos = vaciar_impl()?;
    let mut msg = format!(
        "Papelera vaciada: {n} elementos, {} liberados.",
        crate::almacen::legible(bytes)
    );
    if restos > 0 {
        // Los restos se cuentan aparte porque NO son elementos que el gestor
        // enseñe: son fichas sin fichero o ficheros sin ficha que quedaron de algo
        // a medias. Si no se limpiaran, se quedarían ahí para siempre y el tamaño
        // de la papelera no volvería a cero.
        msg.push_str(&format!(
            " Además se limpiaron {restos} resto(s) sin pareja (fichas o ficheros huérfanos) que quedaban de antes."
        ));
    }
    Ok(msg)
}

/* ── Linux: especificación freedesktop ────────────────────────────────────── */

#[cfg(target_os = "linux")]
fn mover_impl(p: &Path) -> Result<(), String> {
    let r = super::rutas();
    let trash = r.papelera();
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files).map_err(|e| format!("no se pudo preparar la papelera: {e}"))?;
    std::fs::create_dir_all(&info).map_err(|e| format!("no se pudo preparar la papelera: {e}"))?;

    let base = p.file_name().ok_or("ruta sin nombre")?.to_string_lossy().to_string();
    // Si ya hay uno con ese nombre en la papelera, se numera como manda el spec.
    let mut destino = files.join(&base);
    let mut n = 1;
    while destino.exists() {
        destino = files.join(format!("{base}.{n}"));
        n += 1;
    }

    match std::fs::rename(p, &destino) {
        Ok(()) => {}
        // EXDEV (18) = otro sistema de ficheros: un `rename` no cruza montajes.
        // Se copia y se borra el original, que es lo que haría cualquier gestor.
        Err(e) if e.raw_os_error() == Some(18) => {
            copiar_recursivo(p, &destino)?;
            borrar_definitivo(p)?;
        }
        Err(e) => return Err(format!("no se pudo mover a la papelera: {e}")),
    }

    let nombre_final = destino.file_name().unwrap_or_default().to_string_lossy().to_string();
    let escapada = p.to_string_lossy().replace('%', "%25").replace(' ', "%20");
    let info_txt = format!(
        "[Trash Info]\nPath={escapada}\nDeletionDate={}\n",
        fecha_iso_local()
    );
    std::fs::write(info.join(format!("{nombre_final}.trashinfo")), info_txt)
        .map_err(|e| format!("movido a la papelera, pero no se pudo anotar la ruta original: {e}"))?;
    Ok(())
}

/// Copia recursiva para el caso de cruzar sistemas de ficheros. Los enlaces se
/// recrean como enlaces, no se siguen (si se siguiera, una caché con un enlace a
/// `/` copiaría el disco entero).
#[cfg(target_os = "linux")]
fn copiar_recursivo(origen: &Path, destino: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(origen)
        .map_err(|e| format!("no se pudo leer {}: {e}", origen.display()))?;
    if meta.file_type().is_symlink() {
        let objetivo = std::fs::read_link(origen)
            .map_err(|e| format!("no se pudo leer el enlace {}: {e}", origen.display()))?;
        std::os::unix::fs::symlink(objetivo, destino)
            .map_err(|e| format!("no se pudo recrear el enlace {}: {e}", destino.display()))
    } else if meta.is_dir() {
        std::fs::create_dir_all(destino)
            .map_err(|e| format!("no se pudo crear {}: {e}", destino.display()))?;
        let entradas = std::fs::read_dir(origen)
            .map_err(|e| format!("no se pudo leer {}: {e}", origen.display()))?;
        for e in entradas {
            let e = e.map_err(|e| format!("no se pudo leer {}: {e}", origen.display()))?;
            copiar_recursivo(&e.path(), &destino.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(origen, destino)
            .map(|_| ())
            .map_err(|e| format!("no se pudo copiar {}: {e}", origen.display()))
    }
}

#[cfg(target_os = "linux")]
pub fn fecha_iso_local() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/* ── macOS y Windows: lo hace el crate `trash` ────────────────────────────── */

#[cfg(not(target_os = "linux"))]
fn mover_impl(p: &Path) -> Result<(), String> {
    trash::delete(p).map_err(|e| format!("no se pudo mover a la papelera: {e}"))
}

// En Linux lo usa `mover_impl` para escribir el `.trashinfo`; fuera de Linux la
// papelera la mueve el crate `trash` (aquí se compila y se prueba), así que solo
// lo llaman las pruebas.
#[cfg(not(target_os = "linux"))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn fecha_iso_local() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/* ── Vaciar ───────────────────────────────────────────────────────────────── */

/// Cruza lo que hay en `files/` con lo que hay en `info/` y dice qué sobra.
///
/// PURA y con prueba propia: es LA REGLA, no el acceso a disco. Devuelve
/// `(ficheros sin ficha, fichas sin fichero)`.
///
/// POR QUÉ HACE FALTA: el crate `trash` vacía lo que puede LISTAR, y solo lista lo
/// que tiene ficha buena. Un `.trashinfo` cuyo fichero ya no está se queda para
/// siempre (y el gestor de archivos lo enseña como una entrada fantasma), y un
/// fichero sin `.trashinfo` no se puede restaurar ni purgar, pero **sí cuenta** en
/// el tamaño de la papelera: sin esto, vaciar no dejaba la papelera vacía y la
/// resta no cuadraba. Lo arregló Kudu en su 3.5 (issue #487).
///
/// `fichas` son los nombres de `info/` CON su `.trashinfo`.
// Solo lo usa la limpieza de la papelera de Linux (aquí se compila y se prueba):
// en macOS y Windows la papelera la vacía el crate `trash`, sin fichas propias.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn huerfanos(files: &[String], fichas: &[String]) -> (Vec<String>, Vec<String>) {
    let bases: Vec<&str> = fichas
        .iter()
        .map(|f| f.strip_suffix(".trashinfo").unwrap_or(f))
        .collect();
    let sin_ficha: Vec<String> = files
        .iter()
        .filter(|f| !bases.contains(&f.as_str()))
        .cloned()
        .collect();
    let sin_fichero: Vec<String> = fichas
        .iter()
        .filter(|f| {
            let b = f.strip_suffix(".trashinfo").unwrap_or(f);
            !files.iter().any(|x| x == b)
        })
        .cloned()
        .collect();
    (sin_ficha, sin_fichero)
}

/// Vacía la papelera. `Ok(restos)` = cuántas fichas o ficheros huérfanos se
/// limpiaron de paso (ver `huerfanos`).
#[cfg(target_os = "linux")]
fn vaciar_impl() -> Result<usize, String> {
    let items = trash::os_limited::list().map_err(|e| format!("no se pudo leer la papelera: {e}"))?;
    trash::os_limited::purge_all(items).map_err(|e| format!("no se pudo vaciar la papelera: {e}"))?;
    Ok(limpiar_huerfanos_linux())
}

/// Quita lo que `purge_all` no ve: fichas sin fichero y ficheros sin ficha.
#[cfg(target_os = "linux")]
fn limpiar_huerfanos_linux() -> usize {
    let trash = super::rutas().papelera();
    limpiar_huerfanos_en(&trash.join("files"), &trash.join("info"))
}

/// La parte con disco, con las carpetas como PARÁMETRO: así se puede probar en un
/// directorio temporal sin tocar la papelera de verdad, que es dato del usuario.
#[cfg(target_os = "linux")]
fn limpiar_huerfanos_en(files: &std::path::Path, info: &std::path::Path) -> usize {
    let nombres = |d: &std::path::Path| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|it| {
                it.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let (sin_ficha, sin_fichero) = huerfanos(&nombres(files), &nombres(info));
    let mut restos = 0usize;
    for n in &sin_ficha {
        // Un fichero sin ficha no se puede restaurar (no sabemos de dónde vino) y
        // no se puede purgar por la API: está en la papelera y se va con ella.
        if borrar_definitivo(&files.join(n)).is_ok() {
            restos += 1;
        }
    }
    for n in &sin_fichero {
        if std::fs::remove_file(info.join(n)).is_ok() {
            restos += 1;
        }
    }
    restos
}

#[cfg(target_os = "windows")]
fn vaciar_impl() -> Result<usize, String> {
    let items = trash::os_limited::list().map_err(|e| format!("no se pudo leer la papelera: {e}"))?;
    trash::os_limited::purge_all(items).map_err(|e| format!("no se pudo vaciar la papelera: {e}"))?;
    // En Windows la papelera no es un directorio: la gestiona el sistema y no hay
    // fichas que se puedan quedar sueltas.
    Ok(0)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn vaciar_impl() -> Result<usize, String> {
    let dir = super::rutas().papelera();
    let mut restos = 0usize;
    for e in std::fs::read_dir(&dir)
        .map_err(|e| format!("no se pudo leer {}: {e}", dir.display()))?
        .flatten()
    {
        // En macOS `~/.Trash` ES la papelera, así que vaciarla es borrar lo que
        // hay dentro: no hay ficheros «de más» que limpiar aparte. Lo que sí se
        // quita es el `.DS_Store` de Finder, que volvería a aparecer solo.
        borrar_definitivo(&e.path())?;
        restos += 1;
    }
    // Lo que se ha borrado son los elementos de verdad, no restos: se contaron ya
    // en `resumen()`, así que aquí no se informa de ellos otra vez.
    Ok(0)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn un_enlace_simbolico_no_se_mueve() {
        let dir = std::env::temp_dir().join("machinograph-papelera-prueba");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.txt");
        std::fs::write(&real, b"hola").unwrap();
        #[cfg(unix)]
        {
            let enlace = dir.join("enlace.txt");
            std::os::unix::fs::symlink(&real, &enlace).unwrap();
            let e = mover(&enlace).unwrap_err();
            assert!(e.contains("enlace simbólico"), "{e}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn borrar_definitivo_quita_fichero_y_carpeta() {
        let dir = std::env::temp_dir().join("machinograph-borrar-prueba");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.txt"), b"x").unwrap();
        borrar_definitivo(&dir.join("sub/a.txt")).unwrap();
        assert!(!dir.join("sub/a.txt").exists());
        borrar_definitivo(&dir.join("sub")).unwrap();
        assert!(!dir.join("sub").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_fecha_de_la_papelera_esta_bien_formada() {
        // Formato que exige `.trashinfo`: YYYY-MM-DDTHH:MM:SS
        let f = fecha_iso_local();
        assert_eq!(f.len(), 19, "{f}");
        assert_eq!(&f[4..5], "-");
        assert_eq!(&f[10..11], "T");
        assert_eq!(&f[13..14], ":");
    }

    #[test]
    fn mover_de_verdad_lleva_el_fichero_a_la_papelera_del_sistema() {
        // Ruta fuera de cualquier lista blanca: aquí se prueba `mover` a secas.
        let dir = std::env::temp_dir().join("machinograph-papelera-real");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("prueba-machinograph.txt");
        std::fs::write(&f, b"contenido de prueba").unwrap();
        let msg = mover(&f).expect("debería moverse a la papelera");
        assert!(msg.contains("papelera"), "{msg}");
        assert!(!f.exists(), "el original ya no está");
        // Y la papelera crece: es la prueba de que ha ido A la papelera y no a
        // cualquier sitio.
        let (n, bytes) = resumen().expect("se puede contar la papelera");
        assert!(n >= 1 && bytes >= 18, "{n} elementos, {bytes} bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn los_huerfanos_se_detectan_por_su_pareja() {
        let files = vec!["a.txt".to_string(), "b.bin".to_string(), "carpeta".to_string()];
        // `a` tiene ficha, `b` no; y sobra la ficha de `z`.
        let fichas = vec!["a.txt.trashinfo".to_string(), "z.bin.trashinfo".to_string()];
        let (sin_ficha, sin_fichero) = huerfanos(&files, &fichas);
        assert_eq!(sin_ficha, vec!["b.bin".to_string(), "carpeta".to_string()]);
        assert_eq!(sin_fichero, vec!["z.bin.trashinfo".to_string()]);

        // Un fichero que se llame `x.trashinfo` NO es huérfano: su ficha es
        // `x.trashinfo.trashinfo`, y ahí la comparación tiene que cuadrar.
        let (s1, s2) = huerfanos(
            &["x.trashinfo".to_string()],
            &["x.trashinfo.trashinfo".to_string()],
        );
        assert!(s1.is_empty() && s2.is_empty(), "{s1:?} {s2:?}");

        // Dos conjuntos vacíos no dan nada (y una carpeta recién creada tampoco).
        let (v1, v2) = huerfanos(&[], &[]);
        assert!(v1.is_empty() && v2.is_empty());
    }

    /// La parte con disco, en un directorio temporal: comprueba que se limpia SOLO
    /// lo que no tiene pareja y que lo bueno sigue donde estaba.
    #[cfg(target_os = "linux")]
    #[test]
    fn la_limpieza_de_huerfanos_respeta_lo_que_tiene_pareja() {
        let dir = std::env::temp_dir().join("machinograph-papelera-huerfanos");
        let _ = std::fs::remove_dir_all(&dir);
        let files = dir.join("files");
        let info = dir.join("info");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::create_dir_all(&info).unwrap();
        // Pareja buena.
        std::fs::write(files.join("bueno.txt"), b"contenido").unwrap();
        std::fs::write(info.join("bueno.txt.trashinfo"), b"[Trash Info]\n").unwrap();
        // Fichero sin ficha (no se puede restaurar y no se puede purgar).
        std::fs::write(files.join("sin-ficha.txt"), b"x").unwrap();
        // Ficha sin fichero (entrada fantasma en el gestor).
        std::fs::write(info.join("fantasma.txt.trashinfo"), b"[Trash Info]\n").unwrap();

        let restos = limpiar_huerfanos_en(&files, &info);
        assert_eq!(restos, 2, "tenían que limpiarse los dos restos");
        assert!(files.join("bueno.txt").exists(), "el que tenía pareja se queda");
        assert!(info.join("bueno.txt.trashinfo").exists());
        assert!(!files.join("sin-ficha.txt").exists());
        assert!(!info.join("fantasma.txt.trashinfo").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
