//! La limpieza programada, y cómo ponerla en el planificador del sistema.
//!
//! DOS CAMINOS, y los dos existen por un motivo distinto:
//!
//! 1. **Mientras Machinograph está abierto**, comprueba cada minuto si toca y, si toca,
//!    **MIDE** la basura y deja el resultado anotado. **NO borra nada.**
//! 2. **Con la app cerrada**, se le entrega la tarea al planificador del sistema
//!    (systemd --user, launchd o el Programador de tareas de Windows) llamando al
//!    CLI: `machinograph --cli limpiar --json`. Aquí no se finge un servicio propio.
//!
//! Y una regla que viene de Kudu y que aquí se copia a propósito: **una ejecución
//! programada NUNCA borra por su cuenta.** Un borrado que nadie ha mirado, a las
//! tres de la mañana, es exactamente la clase de cosa que este programa no hace: lo
//! que se programa mide y avisa, y borrar lo decide una persona. Kudu lo arregló en
//! su 3.5 («never auto-apply cache resets or native maintenance») después de que
//! pasara.
use serde::Serialize;

pub use crate::db::Programacion;

/// ¿Toca ejecutarla AHORA?
///
/// Pura y con la hora por parámetro para poder probarla: nada de mirar el reloj por
/// dentro. `ahora` es la hora local, y el día de la última ejecución también es
/// local (si se comparara en UTC, una limpieza de las 23:30 contaría como del día
/// siguiente y se repetiría).
pub fn toca(p: &Programacion, ahora: chrono::DateTime<chrono::Local>) -> bool {
    if !p.activa {
        return false;
    }
    let hoy = ahora.format("%Y-%m-%d").to_string();
    if p.ultima.as_deref() == Some(hoy.as_str()) {
        return false;
    }
    ahora.hour() == p.hora && ahora.minute() == p.minuto
}

use chrono::Timelike;

/// Ejecuta la comprobación programada si toca. Devuelve el mensaje para el registro
/// de acciones (o `None` si no tocaba).
///
/// MIDE, no borra: el resultado se queda anotado y la interfaz lo enseña. Es la
/// diferencia entre «tener el equipo limpio» y «que alguien decida qué se tira».
pub fn revisar() -> Option<String> {
    let p = match crate::db::programacion() {
        Ok(p) => p,
        Err(e) => return Some(format!("no se pudo leer la limpieza programada: {e}")),
    };
    let ahora = chrono::Local::now();
    if !toca(&p, ahora) {
        return None;
    }
    let hoy = ahora.format("%Y-%m-%d").to_string();
    let cats = if p.categorias.is_empty() { None } else { Some(p.categorias.clone()) };
    let msg = match crate::limpieza::escanear(cats) {
        Ok(e) => {
            format!(
                "limpieza programada ({}): {} objetivos, {} recuperables. NO se ha borrado nada: \
                 míralo en Optimización y decide tú.",
                ahora.format("%H:%M"),
                e.objetivos.len(),
                crate::almacen::legible(e.bytes)
            )
        }
        Err(e) => format!("la limpieza programada no pudo medir la basura: {e}"),
    };
    if let Err(e) = crate::db::marcar_programacion_hecha(&hoy) {
        return Some(format!("{msg} (y no se pudo anotar el día: {e})"));
    }
    Some(msg)
}

/// Una receta para el planificador del sistema: un fichero (o un comando) y cómo se
/// activa. Se enseña **para copiar**, no se escribe sola: tocar el planificador del
/// sistema es una decisión del usuario y estas cosas se activan a mano.
#[derive(Debug, Clone, Serialize)]
pub struct Receta {
    pub titulo: String,
    pub destino: String,
    pub contenido: String,
    pub instrucciones: String,
}

/// Arma la receta de este sistema para que la limpieza se haga con la app cerrada.
///
/// El comando que se programa es el CLI con `--json`, que es exactamente lo que
/// haría la app: la misma medición, sin borrar nada. Si el usuario quiere que
/// además limpie, se le dice aquí qué hay que añadir (`--aplicar`) para que la
/// decisión esté en su mano y a la vista.
pub fn recetas(p: &Programacion) -> Vec<Receta> {
    let exe = std::env::current_exe()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|_| "machinograph".to_string());
    let hhmm = format!("{:02}:{:02}", p.hora, p.minuto);
    let cats = if p.categorias.is_empty() {
        String::new()
    } else {
        p.categorias.iter().map(|c| format!(" --categoria {c}")).collect::<String>()
    };
    let comando = format!("{exe} --cli limpiar --json{cats}");
    let limpiar_de_verdad = format!("{exe} --cli limpiar --aplicar{cats}");

    let mut v = Vec::new();

    #[cfg(target_os = "linux")]
    {
        v.push(Receta {
            titulo: "systemd (usuario)".into(),
            destino: "~/.config/systemd/user/machinograph-limpieza.service y .timer".into(),
            contenido: format!(
                "# machinograph-limpieza.service\n[Unit]\nDescription=Machinograph: medir la basura del disco\n\n[Service]\nType=oneshot\nExecStart={comando}\n\n# machinograph-limpieza.timer\n[Unit]\nDescription=Machinograph: limpieza programada a las {hhmm}\n\n[Timer]\nOnCalendar=*-*-* {hhmm}:00\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
            ),
            instrucciones: "Guárdalos en ~/.config/systemd/user/ y luego:\n  systemctl --user daemon-reload\n  systemctl --user enable --now machinograph-limpieza.timer\n(y `systemctl --user list-timers` para ver cuándo toca)".into(),
        });
    }
    #[cfg(target_os = "macos")]
    {
        v.push(Receta {
            titulo: "launchd (LaunchAgent)".into(),
            destino: "~/Library/LaunchAgents/dev.machinograph.panel.limpieza.plist".into(),
            contenido: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Label</key><string>dev.machinograph.panel.limpieza</string>\n\t<key>ProgramArguments</key>\n\t<array><string>{exe}</string><string>--cli</string><string>limpiar</string><string>--json</string></array>\n\t<key>StartCalendarInterval</key>\n\t<dict><key>Hour</key><integer>{}</integer><key>Minute</key><integer>{}</integer></dict>\n</dict>\n</plist>\n",
                p.hora, p.minuto
            ),
            instrucciones: format!(
                "Guárdalo en ~/Library/LaunchAgents/ y luego:\n  launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.machinograph.panel.limpieza.plist"
            ),
        });
    }
    #[cfg(target_os = "windows")]
    {
        v.push(Receta {
            titulo: "Programador de tareas".into(),
            destino: "Programador de tareas de Windows".into(),
            contenido: format!(
                "schtasks /Create /TN \"Machinograph: limpieza\" /SC DAILY /ST {hhmm} /TR \"{comando}\""
            ),
            instrucciones:
                "Pégalo en un símbolo del sistema (como administrador si quieres que corra siempre). Para verlo: schtasks /Query /TN \"Machinograph: limpieza\"".into(),
        });
    }

    // Y SIEMPRE, en cualquier sistema, la vía manual: qué hay que añadir para que
    // además borre. Se dice explícitamente para que nadie programe un borrado sin
    // saberlo.
    v.push(Receta {
        titulo: "¿Que además limpie?".into(),
        destino: "—".into(),
        contenido: limpiar_de_verdad,
        instrucciones: "Cambia `--json` por `--aplicar` en el comando programado. Lo que necesita root o \
                        tiene su propio comando se sigue saltando: eso hay que hacerlo a mano."
            .into(),
    });
    v
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use chrono::TimeZone;

    fn a_las(h: u32, m: u32) -> chrono::DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(2026, 10, 3, h, m, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn solo_toca_a_su_hora_y_una_vez_al_dia() {
        let p = Programacion { activa: true, hora: 3, minuto: 30, ..Default::default() };
        assert!(toca(&p, a_las(3, 30)));
        assert!(!toca(&p, a_las(3, 31)));
        assert!(!toca(&p, a_las(4, 30)));
        // Si ya se hizo hoy, no se repite (aunque se reinicie la app mil veces).
        let hecho = Programacion { ultima: Some("2026-10-03".into()), ..p.clone() };
        assert!(!toca(&hecho, a_las(3, 30)));
        // Y mañana sí.
        let manana = chrono::Local.with_ymd_and_hms(2026, 10, 4, 3, 30, 0).single().unwrap();
        assert!(toca(&hecho, manana));
    }

    #[test]
    fn desactivada_no_toca_nunca() {
        let p = Programacion { activa: false, hora: 3, minuto: 30, ..Default::default() };
        assert!(!toca(&p, a_las(3, 30)));
    }

    #[test]
    fn la_receta_lleva_el_comando_con_su_hora_y_sus_categorias() {
        let p = Programacion {
            activa: true,
            hora: 4,
            minuto: 5,
            categorias: vec!["apps".into(), "sistema".into()],
            ultima: None,
        };
        let r = recetas(&p);
        assert!(!r.is_empty());
        // La receta de arranque y la vía manual están SIEMPRE, en los tres sistemas;
        // lo que cambia es el mecanismo (systemd, launchd o el Programador de
        // tareas), así que su texto se comprueba en su propio sistema.
        let principal = r.iter().find(|x| x.titulo != "¿Que además limpie?").unwrap();
        assert!(!principal.instrucciones.is_empty());
        assert!(!principal.contenido.is_empty());
        // La vía que SÍ borra se dice explícitamente, para que nadie programe un
        // borrado sin saberlo. Esto es común a los tres sistemas.
        let borra = r.iter().find(|x| x.titulo == "¿Que además limpie?").unwrap();
        assert!(borra.contenido.contains("--aplicar"));

        // El comando programado, con las categorías dentro. La prueba afirmaba el
        // texto de systemd, que solo existe en Linux; en cada sistema se comprueba
        // su receta. En macOS el LaunchAgent pasa los argumentos uno a uno, así que
        // no contiene la línea entera.
        #[cfg(target_os = "linux")]
        {
            assert!(
                principal.contenido.contains("--cli limpiar --json"),
                "{}",
                principal.contenido
            );
            assert!(principal.contenido.contains("--categoria apps"), "{}", principal.contenido);
        }
        #[cfg(target_os = "windows")]
        {
            assert!(
                principal.contenido.contains("--cli limpiar --json"),
                "{}",
                principal.contenido
            );
            assert!(principal.contenido.contains("--categoria apps"), "{}", principal.contenido);
        }
        #[cfg(target_os = "macos")]
        {
            assert!(
                principal.contenido.contains("<string>limpiar</string>"),
                "{}",
                principal.contenido
            );
            assert!(
                principal.contenido.contains("<string>--json</string>"),
                "{}",
                principal.contenido
            );
        }
    }
}
