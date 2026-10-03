//! El icono de bandeja: abrir la ventana, ver qué modelo está cargado y pararlo.
//!
//! POR QUÉ NO ES SOLO "Abrir" Y "Salir": esta app vigila motores de inferencia que
//! se quedan con la VRAM cogida. El caso real de este equipo es concreto: el
//! `llama-swap` descarga el modelo tras 30 minutos sin peticiones, pero durante esa
//! media hora hay ~13 GiB de VRAM ocupados, y si te vas a jugar lo que quieres es
//! sacarlo YA sin abrir la ventana y sin buscar la sección. Eso es el menú de
//! bandeja: dice qué está cargado y lo para de un clic.
//!
//! La lista se REHACE con cada foto del sistema, no solo al arrancar: un menú que
//! se quedara con el estado de hace una hora diría que hay un modelo cargado
//! cuando ya no lo hay, que es peor que no decir nada.
//!
//! Y se dice de DÓNDE sale el dato: los modelos "cargados" son los que publica el
//! MOTOR en su API. Si ningún motor publica estado, el menú dice que no lo sabe en
//! vez de decir "ninguno".

use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::menu::MenuEvent;
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::Manager;

use crate::types::Snapshot;

/// El icono de bandeja vivo, para poder cambiarle el menú cuando cambie el estado.
static TRAY: std::sync::LazyLock<parking_lot::Mutex<Option<TrayIcon<tauri::Wry>>>> =
std::sync::LazyLock::new(|| parking_lot::Mutex::new(None));

/// La última lista de modelos cargados que se ha pintado en el menú.
///
/// Sirve para NO rehacer el menú en cada foto (cada 2 s): reconstruirlo hace
/// parpadear la bandeja y gasta trabajo para nada si nada ha cambiado. Se compara
/// con la lista nueva y solo se toca si difiere.
static ULTIMO_MENU: std::sync::LazyLock<parking_lot::Mutex<Vec<String>>> =
std::sync::LazyLock::new(|| parking_lot::Mutex::new(Vec::new()));

/// Una acción del menú que no es abrir ni salir.
fn manejar(app: &tauri::AppHandle<tauri::Wry>, accion: &str) {
    match accion {
        "open-machinograph" => {
            for window in app.webview_windows().values() {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        // Parar todos los modelos: la misma acción que usa la interfaz, no una
        // copia. Así la bandeja y la ventana hacen exactamente lo mismo.
        "parar-todos" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::actions::run(
                    app.clone(),
                    crate::actions::ActionJson {
                        kind: "modelo:descargar-todos".into(),
                        // El puerto del llama-swap de este equipo: es el único
                        // motor con API para descargar modelos, y el que sirve.
                        args: serde_json::json!({ "port": 8080 }),
                    },
                )
                .await
                {
                    Ok(mensaje) => eprintln!("bandeja: {mensaje}"),
                    Err(e) => eprintln!("bandeja: no se pudo parar el modelo: {e}"),
                }
            });
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn menu_evento(app: &tauri::AppHandle<tauri::Wry>, ev: MenuEvent) {
    manejar(app, ev.id().0.as_str());
}

/// Los modelos que algún motor tiene cargados AHORA, con el motor del que salen.
///
/// Se lee de la foto (que a su vez lo pregunta a cada motor por su API). Un motor
/// que no publica el estado de sus modelos no aporta nada aquí, y por eso la lista
/// puede venir vacía sin que eso signifique "no hay ninguno": `publica_estado` lo
/// distingue.
fn cargados(servers: &[crate::types::Server]) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut alguno_publica = false;
    for sv in servers {
        for m in &sv.models {
            if m.state.is_empty() {
                continue;
            }
            alguno_publica = true;
            if m.state == "loaded" {
                out.push(format!("{} ({})", if m.label.is_empty() { m.id.clone() } else { m.label.clone() }, sv.name));
            }
        }
    }
    (out, alguno_publica)
}

/// Rehace el menú si la lista de cargados ha cambiado.
pub fn actualizar(s: &Snapshot) {
    let (nuevos, alguno_publica) = cargados(&s.servers);
    let mut ultimo = ULTIMO_MENU.lock();
    if *ultimo == nuevos {
        return;
    }
    *ultimo = nuevos.clone();
    drop(ultimo);

    let guardia = TRAY.lock();
    let Some(tray) = guardia.as_ref() else { return };
    let Ok(menu) = construir_menu(tray.app_handle(), &nuevos, alguno_publica) else {
        return;
    };
    let _ = tray.set_menu(Some(menu));

    // El tooltip también dice qué hay cargado: es lo que se ve sin abrir el menú.
    let texto = if nuevos.is_empty() {
        "Machinograph".to_string()
    } else {
        format!("Machinograph · {}", nuevos.join(", "))
    };
    let _ = tray.set_tooltip(Some(&texto));
}

fn construir_menu(
    app: &tauri::AppHandle<tauri::Wry>,
    cargados: &[String],
    alguno_publica: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    let abrir = MenuItemBuilder::with_id("open-machinograph", "Abrir Machinograph").build(app)?;
    menu.append(&abrir)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    if cargados.is_empty() {
        // Tres estados, no dos: "ningún motor publica el estado" NO es "no hay
        // ningún modelo cargado", y confundirlos es lo que haría pensar que la
        // VRAM está libre cuando no se sabe.
        let texto = if alguno_publica {
            "Ningún modelo cargado"
        } else {
            "Ningún motor publica el estado"
        };
        let vacio = MenuItemBuilder::with_id("sin-modelos", texto).enabled(false).build(app)?;
        menu.append(&vacio)?;
    } else {
        let cabecera = MenuItemBuilder::with_id("cargados", "Cargados ahora").enabled(false).build(app)?;
        menu.append(&cabecera)?;
        for (i, nombre) in cargados.iter().enumerate() {
            // Deshabilitado a propósito: es un DATO, no una acción. El clic va en
            // el botón de parar, que es el único que hace algo.
            let item = MenuItemBuilder::with_id(format!("modelo-{i}"), format!("  {nombre}"))
                .enabled(false)
                .build(app)?;
            menu.append(&item)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        let parar = MenuItemBuilder::with_id("parar-todos", "Liberar la VRAM (parar todos)").build(app)?;
        menu.append(&parar)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let salir = MenuItemBuilder::with_id("quit", "Salir").build(app)?;
    menu.append(&salir)?;
    Ok(menu)
}

pub fn setup(app: &mut tauri::App<tauri::Wry>) -> anyhow::Result<()> {
    // El icono va EMBEBIDO en el binario, no leído del disco: con
    // `Image::from_path("src-tauri/icons/icon-256.png")` la ruta era relativa al
    // directorio de trabajo, así que al lanzar la app desde el menú (cwd = $HOME)
    // el `setup` fallaba con "No existe el fichero o el directorio" y el icono de
    // bandeja no aparecía nunca.
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;

    // El menú inicial no conoce ningún modelo todavía (la primera foto aún no ha
    // llegado): dice que no lo sabe, que es la verdad en este momento.
    let menu = construir_menu(app.handle(), &[], false)?;

    let tray = TrayIconBuilder::new()
        .icon(icon.to_owned())
        .tooltip("Machinograph")
        .menu(&menu)
        .on_menu_event(|app: &tauri::AppHandle<tauri::Wry>, ev: MenuEvent| {
            menu_evento(app, ev);
        })
        .build(app)?;

    tray.set_visible(true)?;
    { let mut slot = TRAY.lock();
        *slot = Some(tray);
    }
    Ok(())
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::types::{Server, ServerModel};

    fn servidor(models: Vec<ServerModel>) -> Server {
        Server {
            id: "llama-swap:8080".into(),
            name: "Llama-Swap (local)".into(),
            kind: "llama-swap".into(),
            port: 8080,
            state: "active".into(),
            process_active: true,
            version: None,
            pid: Some(1),
            proc_uptime_secs: Some(10),
            models,
            error: None,
        }
    }

    fn modelo(id: &str, state: &str) -> ServerModel {
        ServerModel {
            id: id.into(),
            label: id.into(),
            state: state.into(),
            quant: None,
            size_mb: None,
        }
    }

    /// Lo que el menú enseña: solo los que el motor dice que están CARGADOS, con
    /// su motor al lado para saber a quién hay que pedirle que los suelte.
    #[test]
    fn el_menu_enseña_los_cargados_y_su_motor() {
        let servers = vec![servidor(vec![
            modelo("modelo-27b", "loaded"),
            modelo("mimo-9b", "unloaded"),
            modelo("modelo-8b", "loaded"),
        ])];
        let (cargados, publica) = cargados(&servers);
        assert!(publica);
        assert_eq!(cargados.len(), 2, "solo los cargados: {cargados:?}");
        assert!(cargados[0].contains("modelo-27b"));
        assert!(cargados[0].contains("Llama-Swap"), "tiene que decir de qué motor: {}", cargados[0]);
    }

    /// "No publica el estado" NO es "no hay ninguno cargado". Confundirlos haría
    /// creer que la VRAM está libre cuando en realidad no se sabe.
    #[test]
    fn no_publicar_estado_no_es_no_haber_cargado_nada() {
        let servers = vec![servidor(vec![modelo("modelo-27b", "")])];
        let (cargados, publica) = cargados(&servers);
        assert!(cargados.is_empty());
        assert!(!publica, "con el estado vacío, ningún motor publica");
    }

    /// Sin servidores no hay nada cargado y tampoco nadie que lo publique.
    #[test]
    fn sin_servidores_no_hay_nada_que_enseñar() {
        let (cargados, publica) = cargados(&[]);
        assert!(cargados.is_empty());
        assert!(!publica);
    }
}
