//! Chequeo de salud del entorno: "¿está todo como debería?".
//!
//! Existe porque Machinograph ha ido aprendiendo cosas de esta máquina que, cuando
//! fallan, fallan **en silencio**: una instalación de llama.cpp que no calcula el
//! contexto (devuelve `-c 0`), modelos ternarios que solo lee el fork, el reloj de
//! memoria clavado en el mínimo, un cliente apuntando a un endpoint que ya no
//! existe… Ninguna de esas cosas da un error visible; simplemente todo va mal o
//! no va. Aquí se comprueban y se dice qué hacer.
//!
//! Todo son lecturas locales: no carga modelos, no usa la GPU y no necesita
//! privilegios (la única que mira root es la del reinicio de GPU, y solo para
//! saber si se podría).
use serde::Serialize;

// `Copy` porque el estado es un valor diminuto que se usa varias veces al montar
// cada comprobación (y moverlo obligaba a clonarlo o a reordenar el código).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Estado {
    /// Todo correcto.
    Ok,
    /// Funciona, pero conviene saberlo.
    Aviso,
    /// Hay un problema que explica que algo no funcione o vaya lento.
    Problema,
    /// No se ha podido comprobar (y se dice por qué, no se asume).
    Desconocido,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comprobacion {
    pub id: String,
    pub titulo: String,
    pub estado: Estado,
    /// Qué se ha encontrado, en una frase.
    pub detalle: String,
    /// Qué hacer si hay algo que hacer.
    pub como_arreglarlo: Option<String>,
}

impl Comprobacion {
    fn nueva(
        id: &str,
        titulo: &str,
        estado: Estado,
        detalle: String,
        arreglo: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            titulo: titulo.into(),
            estado,
            detalle,
            como_arreglarlo: arreglo,
        }
    }
}

fn gb(bytes: f64) -> f64 {
    (bytes / 1_073_741_824.0 * 10.0).round() / 10.0
}

/// Todas las comprobaciones. Es síncrono y rápido (todo son ficheros y sysfs).
pub fn comprobar() -> Vec<Comprobacion> {
    let mut out = Vec::new();

    /* ── Runtimes de llama.cpp ───────────────────────────────────────────── */
    let runtimes = crate::perf::runtimes();
    let con_fit = runtimes.iter().filter(|r| r.fit.is_some()).count();
    if runtimes.is_empty() {
        out.push(Comprobacion::nueva(
            "llama-cpp",
            "Instalaciones de llama.cpp",
            Estado::Problema,
            "No se ha encontrado ninguna (ni `llama-fit-params` ni `llama-bench`).".into(),
            Some("Instala llama.cpp, o dile a Machinograph dónde está con la variable MACHINOGRAPH_LLAMA_DIRS.".into()),
        ));
    } else {
        // Ojo con esto: tener varias instalaciones no es un problema por sí solo,
        // pero SÍ lo es que la que se use no sepa calcular el contexto.
        out.push(Comprobacion::nueva(
            "llama-cpp",
            "Instalaciones de llama.cpp",
            if con_fit >= 2 { Estado::Aviso } else { Estado::Ok },
            format!(
                "{con_fit} con planificador de encaje: {}.",
                runtimes
                    .iter()
                    .map(|r| r.nombre.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            (con_fit >= 2).then(|| {
                "Hay más de una. Machinograph las prueba y usa la que sabe leer cada modelo, pero conviene saber cuál sirve para qué.".into()
            }),
        ));
    }

    /* ── El encaje, y si algún modelo no cabe ────────────────────────────── */
    match crate::db::fits() {
        Ok(fits) if fits.is_empty() => out.push(Comprobacion::nueva(
            "encaje",
            "Encaje de los modelos",
            Estado::Desconocido,
            "Todavía no se ha calculado (se hace solo al arrancar y cada 10 minutos).".into(),
            None,
        )),
        Ok(fits) => {
            let errores: Vec<&crate::db::FitRow> =
                fits.iter().filter(|f| f.encaje == "Error").collect();
            let sin_contexto: Vec<&crate::db::FitRow> =
                fits.iter().filter(|f| f.ctx_max <= 0).collect();
            let no_caben: Vec<&crate::db::FitRow> =
                fits.iter().filter(|f| f.encaje == "NoCabe").collect();
            let estado = if !sin_contexto.is_empty() {
                Estado::Problema
            } else if !errores.is_empty() || !no_caben.is_empty() {
                Estado::Aviso
            } else {
                Estado::Ok
            };
            let mut detalle = format!("{} modelo(s) con encaje calculado", fits.len());
            if !sin_contexto.is_empty() {
                detalle.push_str(&format!(
                    "; {} sin contexto (un encaje sin contexto no vale)",
                    sin_contexto.len()
                ));
            }
            if !errores.is_empty() {
                detalle.push_str(&format!("; {} con error", errores.len()));
            }
            if !no_caben.is_empty() {
                detalle.push_str(&format!("; {} no caben enteros", no_caben.len()));
            }
            detalle.push('.');
            // El remedio, cuando hay algo que arreglar. Antes esta comprobación
            // decía que había un problema y no decía QUÉ HACER, y la propia prueba
            // del diagnóstico lo cazó ("encaje dice que hay un problema y no dice
            // cómo arreglarlo"): un aviso sin salida deja al usuario mirando el
            // panel. El remedio nombra dónde se actúa y con qué.
            let remedio = if !sin_contexto.is_empty() {
                Some(
                    "El planificador devolvió «0 de contexto», que no es un encaje: ese binario no sabe leer el modelo. \
                     Prueba otro runtime en Rendimiento → Encaje, y si ninguno vale, el fichero puede estar incompleto."
                        .to_string(),
                )
            } else if !errores.is_empty() {
                Some(
                    "Ningún runtime instalado supo leer esos ficheros. En Rendimiento → Encaje se puede probar otro runtime a mano; \
                     los modelos ternarios (PQ2_0, PTQ1_0) solo los lee el fork, no el llama.cpp oficial."
                        .to_string(),
                )
            } else if !no_caben.is_empty() {
                Some(
                    "No caben enteros en la GPU. No es un fallo: se pueden servir con capas en CPU (más lento) o recalcular el encaje \
                     con menos contexto desde Rendimiento → Encaje."
                        .to_string(),
                )
            } else {
                None
            };
            out.push(Comprobacion::nueva(
                "encaje",
                "Encaje de los modelos",
                estado,
                detalle,
                remedio,
            ));
        }
        Err(e) => out.push(Comprobacion::nueva(
            "encaje",
            "Encaje de los modelos",
            Estado::Desconocido,
            format!("No se pudo leer su tabla: {e}"),
            None,
        )),
    }

    /* ── El reloj de memoria de la GPU (el fallo silencioso) ─────────────── */
    match crate::gpu::estado_mclk() {
        Some(e) => {
            // Tres niveles, porque el mismo síntoma tiene dos lecturas distintas:
            //   * el mínimo CON carga clara  -> problema (es el fallo, y va lento)
            //   * el mínimo con algo de carga -> aviso: puede estar clavándose, y
            //     mirar Machinograph justo cuando acabas de salir de un juego es el
            //     momento típico en que la carga ya ha bajado pero el reloj sigue
            //     abajo. Si no avisara aquí, diría "todo bien" con el reloj
            //     clavado.
            //   * en reposo de verdad -> normal: la tarjeta no necesita subir.
            let minimo_mhz = e.niveles.iter().map(|n| n.mhz).min().unwrap_or(0);
            let carga = e.gpu_busy.max(e.mem_busy);
            let en_minimo = e.activo_mhz == minimo_mhz;
            let estado = if e.degradado {
                Estado::Problema
            } else if en_minimo && carga >= 5 {
                Estado::Aviso
            } else {
                Estado::Ok
            };
            let detalle = format!(
                "{} MHz de {} posibles, con la GPU al {} %{}.",
                e.activo_mhz,
                e.max_mhz,
                carga,
                if en_minimo && carga >= 5 {
                    " y el reloj en el nivel mínimo"
                } else {
                    ""
                }
            );
            out.push(Comprobacion::nueva(
                "mclk",
                "Reloj de memoria de la GPU",
                estado,
                detalle,
                (estado != Estado::Ok).then(|| {
                    "Prueba «Arreglar el reloj» en el Panel: cicla la pantalla y lo recupera sin privilegios, sin perder la VRAM y sin cortar lo que estés generando. Si con eso no sube, el reinicio de GPU (que sí pierde la VRAM)."
                        .into()
                }),
            ));
        }
        None => out.push(Comprobacion::nueva(
            "mclk",
            "Reloj de memoria de la GPU",
            Estado::Desconocido,
            // La vigilancia del MCLK es de las GPU AMD en Linux (sysfs + amd-smi).
            // Fuera de ahí el concepto no existe, y decir «no hay GPU visible» haría
            // pensar que falta una tarjeta en vez de que esta comprobación no aplica.
            if crate::plataforma::so() == "linux" {
                "No hay una GPU amdgpu visible por sysfs.".to_string()
            } else {
                format!(
                    "Esta vigilancia es específica de las GPU AMD en Linux (el reloj de memoria se lee de sysfs); {} no expone ese dato.",
                    crate::plataforma::nombre_so()
                )
            },
            None,
        )),
    }

    /* ── llmfit (el catálogo y las recomendaciones) ──────────────────────── */
    match crate::llmfit::binario() {
        Some(_) => {
            let v = crate::llmfit::version().unwrap_or_else(|| "versión desconocida".into());
            out.push(Comprobacion::nueva(
                "llmfit",
                "llmfit (catálogo de modelos)",
                Estado::Ok,
                format!("Instalado: {v}."),
                None,
            ));
        }
        None => out.push(Comprobacion::nueva(
            "llmfit",
            "llmfit (catálogo de modelos)",
            Estado::Aviso,
            "No está instalado, así que la sección Recomendados no puede dar nada.".into(),
            Some("https://github.com/AlexsJones/llmfit — es una herramienta aparte (MIT).".into()),
        )),
    }

    /* ── Modelos: cuántos y cuánto ocupan ────────────────────────────────── */
    let inv = crate::inventario::inventario();
    if inv.is_empty() {
        out.push(Comprobacion::nueva(
            "modelos",
            "Modelos en el equipo",
            Estado::Aviso,
            "No se ha encontrado ninguno en las carpetas conocidas.".into(),
            Some("Se miran ~/models, ~/.lmstudio, ~/ComfyUI, piper y Coqui TTS. Con MACHINOGRAPH_MODEL_DIRS=ruta,familia se añaden más.".into()),
        ));
    } else {
        let bytes: i64 = inv.iter().map(|m| m.tamano_bytes).sum();
        let familias: std::collections::BTreeSet<&str> =
            inv.iter().map(|m| m.familia.as_str()).collect();
        out.push(Comprobacion::nueva(
            "modelos",
            "Modelos en el equipo",
            Estado::Ok,
            format!(
                "{} ficheros, {} GB, en {} familias ({}).",
                inv.len(),
                gb(bytes as f64),
                familias.len(),
                familias.into_iter().collect::<Vec<_>>().join(", ")
            ),
            None,
        ));
    }

    /* ── Clientes de IA: ¿apuntan a algo que existe? ─────────────────────── */
    let clientes = crate::conexiones::clientes();
    let existentes: Vec<&crate::conexiones::Cliente> = clientes.iter().filter(|c| c.existe).collect();
    let locales = existentes.iter().filter(|c| c.apunta_local).count();
    out.push(Comprobacion::nueva(
        "clientes",
        "Clientes de IA configurados",
        if existentes.is_empty() { Estado::Aviso } else { Estado::Ok },
        format!(
            "{} con configuración, {} apuntando a un endpoint de esta máquina.",
            existentes.len(),
            locales
        ),
        None,
    ));

    /* ── Espacio en disco ────────────────────────────────────────────────── */
    let (_, disco, _) = crate::system::build();
    out.push(Comprobacion::nueva(
        "disco",
        "Espacio en disco",
        if disco.pct >= 90.0 { Estado::Problema } else if disco.pct >= 75.0 { Estado::Aviso } else { Estado::Ok },
        format!(
            "{:.1} GB libres de {:.1} GB ({:.0} % usado) en {}.",
            disco.free_gb, disco.total_gb, disco.pct, disco.mount
        ),
        (disco.pct >= 90.0).then(|| {
            "Los modelos ocupan decenas de GB: mira qué puedes borrar en la sección Modelos (va a la papelera)."
                .into()
        }),
    ));

    out
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn comprueba_todo_y_sin_repetir_ids() {
        let c = comprobar();
        assert!(c.len() >= 6, "se esperaban varias comprobaciones, hay {}", c.len());
        let ids: std::collections::BTreeSet<&str> = c.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids.len(), c.len(), "hay identificadores repetidos");
        for x in &c {
            assert!(!x.titulo.is_empty());
            assert!(!x.detalle.is_empty(), "{} sin detalle", x.id);
            // Si hay problema, tiene que haber algo que hacer con él.
            if x.estado == Estado::Problema {
                assert!(
                    x.como_arreglarlo.is_some(),
                    "{} dice que hay un problema y no dice cómo arreglarlo",
                    x.id
                );
            }
        }
    }

    #[test]
    fn detecta_lo_que_hay_en_esta_maquina() {
        // Contra el equipo real: aquí hay GPU amdgpu, modelos y clientes, así que
        // esas comprobaciones no pueden salir como "desconocido".
        let c = comprobar();
        for x in &c {
            let marca = match x.estado {
                Estado::Ok => "OK      ",
                Estado::Aviso => "AVISO   ",
                Estado::Problema => "PROBLEMA",
                Estado::Desconocido => "?       ",
            };
            println!("{marca} {:<34} {}", x.titulo, x.detalle);
            if let Some(a) = &x.como_arreglarlo {
                println!("         -> {a}");
            }
        }
        let busca = |id: &str| c.iter().find(|x| x.id == id).unwrap();
        assert_ne!(
            busca("mclk").estado,
            Estado::Desconocido,
            "esta máquina tiene GPU amdgpu"
        );
        assert_ne!(busca("modelos").estado, Estado::Desconocido);
        assert_ne!(busca("disco").estado, Estado::Desconocido);
        assert_ne!(busca("llama-cpp").estado, Estado::Desconocido);
        // Y las familias reales tienen que aparecer en el detalle.
        let m = &busca("modelos").detalle;
        assert!(m.contains("GB"), "detalle de modelos: {m}");
    }
}
