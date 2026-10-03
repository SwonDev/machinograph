//! CPU, memoria, swap, arranque del equipo y uso del disco donde vives.
//!
//! POR QUÉ ESTE FICHERO ES TAN CORTO AHORA: leía `/proc/stat`, `/proc/meminfo`,
//! `/proc/swaps`, `/proc/uptime` y ejecutaba `df` con `LC_ALL=C` para que el
//! encabezado no saliera en español. Nada de eso existe en macOS ni en Windows, así
//! que el panel entero se caía en cuanto salías de Linux. Todo eso vive ahora en
//! `plataforma` (que usa `sysinfo` y funciona en los tres) y aquí solo queda
//! traducir a los tipos de la foto y cachear el disco, que es lo único que tiene
//! sentido cachear: preguntar por el uso de un disco es caro y no cambia de una
//! foto a la siguiente.
use crate::plataforma;
use crate::types::{DiskUsage, Mem, System};
use parking_lot::Mutex;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Bytes a GiB. El nombre del campo (`total_gb`) viene de cuando se leía `df` en
/// MiB y se dividía entre 1024; se conserva el número EXACTO que ya se enseñaba
/// para no cambiar lo que el usuario tiene delante.
fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

/// Uso del sistema de ficheros donde vive el usuario.
///
/// Se pregunta por el HOME, no por `/`: en un Fedora atómico (Bazzite) `/` es la
/// imagen de solo lectura (composefs, 45 MB, 100% usada), no el disco real. Antes
/// esto salía mal por tres motivos a la vez —idioma del `df`, la raíz equivocada y
/// las columnas cruzadas—, y ahora lo resuelve `plataforma::disco_de`, que elige
/// el punto de montaje MÁS ESPECÍFICO que contiene esa carpeta.
fn medir_disco() -> DiskUsage {
    let r = plataforma::rutas();
    match plataforma::disco_de(&r.home) {
        Some(d) => DiskUsage {
            total_gb: gib(d.total),
            used_gb: gib(d.usado),
            free_gb: gib(d.libre),
            pct: d.uso_pct,
            mount: d.punto,
        },
        // Sin dato: el punto de montaje se dice igual (para saber QUÉ no se pudo
        // medir), pero los números quedan en 0 y la interfaz los enseña como "—".
        None => DiskUsage {
            mount: r.home.to_string_lossy().to_string(),
            ..Default::default()
        },
    }
}

static CACHE_DISCO: LazyLock<Mutex<Option<(Instant, DiskUsage)>>> = LazyLock::new(|| Mutex::new(None));
const TTL_DISCO: Duration = Duration::from_secs(15);

fn disco() -> DiskUsage {
    {
        let g = CACHE_DISCO.lock();
        if let Some((cuando, d)) = g.as_ref() {
            if cuando.elapsed() < TTL_DISCO {
                return d.clone();
            }
        }
    }
    let d = medir_disco();
    let mut g = CACHE_DISCO.lock();
    *g = Some((Instant::now(), d.clone()));
    d
}

pub fn build() -> (System, DiskUsage, i64) {
    let r = plataforma::resumen();
    (
        System {
            cpu_pct: r.cpu_pct,
            load1: r.load1,
            load5: r.load5,
            load15: r.load15,
            cores: r.cores,
            mem: Mem {
                total_mb: r.mem_total_mb,
                used_mb: r.mem_usado_mb,
                free_mb: r.mem_libre_mb,
                avail_mb: r.mem_disp_mb,
                pct: r.mem_pct,
            },
            swap: Mem {
                total_mb: r.swap_total_mb,
                used_mb: r.swap_usado_mb,
                free_mb: r.swap_libre_mb,
                // Para la swap, «disponible» y «libre» son lo mismo: no hay caché
                // que la ocupe, así que se repite el mismo dato en vez de inventar.
                avail_mb: r.swap_libre_mb,
                pct: r.swap_pct,
            },
        },
        disco(),
        plataforma::uptime(),
    )
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn el_disco_del_home_tiene_datos_coherentes() {
        let d = medir_disco();
        assert!(d.total_gb > 0.0, "{d:?}");
        assert!(d.used_gb <= d.total_gb, "{d:?}");
        assert!(d.free_gb <= d.total_gb, "{d:?}");
        assert!((0.0..=100.0).contains(&d.pct), "{d:?}");
        assert!(!d.mount.is_empty());
    }

    #[test]
    fn la_foto_trae_cpu_memoria_y_arranque() {
        let (s, d, uptime) = build();
        assert!(s.cores >= 1, "{s:?}");
        assert!(s.mem.total_mb > 0.0, "{s:?}");
        assert!(s.mem.used_mb <= s.mem.total_mb, "{s:?}");
        assert!(uptime > 0);
        assert!(!d.mount.is_empty());
    }

    #[test]
    fn el_uso_del_disco_se_reutiliza_unos_segundos() {
        // Dos llamadas seguidas tienen que dar EXACTAMENTE lo mismo: si no, el
        // sondeo del disco se estaría repitiendo en cada foto.
        let a = disco();
        let b = disco();
        assert_eq!(a.total_gb, b.total_gb);
        assert_eq!(a.mount, b.mount);
    }
}
