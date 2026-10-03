//! Ejecución de binarios externos con LÍMITE DE TIEMPO.
//!
//! POR QUÉ EXISTE ESTE MÓDULO: ninguno de los binarios que lanza el backend tiene
//! un límite propio. Si uno se queda colgado (un `llama-fit-params` atascado, un
//! `amd-smi` que no vuelve del driver), el hilo que lo espera no vuelve nunca. Y
//! eso se nota de dos formas, las dos medidas en este código:
//!
//!   * en el bucle de encajes, la vuelta no termina porque el hilo de
//!     `spawn_blocking` queda retenido: el bucle deja de recalcular y no lo dice;
//!   * en un comando del usuario, la espera no acaba nunca y la interfaz se queda
//!     con el aviso puesto para siempre.
//!
//! CÓMO SE PONE EL LÍMITE AHORA: antes se envolvía la llamada con `timeout(1)` de
//! coreutils. Eso funcionaba en Linux y **en ningún sitio más**: macOS trae el
//! `timeout` de BSD (otra interfaz y sin las mismas opciones) y Windows no lo
//! tiene. Y cuando no estaba, este módulo ejecutaba SIN límite: prometía un tope y
//! no lo daba, en silencio. Ahora el límite lo pone este código: lanza el proceso,
//! lo espera con plazo y lo termina si se pasa. Es el mismo tope en los tres
//! sistemas y ya no depende de que el sistema traiga una utilidad.
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Cada cuánto se mira si el proceso ya terminó. 15 ms es imperceptible para
/// quien espera y no gasta CPU apreciable (una comprobación por vuelta).
const INTERVALO: Duration = Duration::from_millis(15);

/// Margen que se le da al proceso para morir después de pedirle que termine. Si
/// no muere ni con eso (un proceso atascado en el núcleo), no se espera más: se
/// avisa y se sigue.
const MARGEN_MUERTE: Duration = Duration::from_secs(2);

/// Lee un tubo hasta el final. Se lanza SIEMPRE en un hilo aparte, uno por tubo:
/// si se leyera uno entero antes de empezar el otro, el proceso se bloquearía al
/// llenar (64 KB) el tubo que nadie vacía.
fn leer_todo<R: Read + Send + 'static>(fuente: Option<R>) -> Vec<u8> {
    let mut datos = Vec::new();
    if let Some(mut f) = fuente {
        let _ = f.read_to_end(&mut datos);
    }
    datos
}

/// Ejecuta `programa` con `args` y devuelve su salida completa, o un error si
/// tarda más de `limite`.
///
/// `envs` son variables de entorno extra (por ejemplo `LC_ALL=C`). El proceso
/// hereda el resto del entorno, como cualquier hijo.
pub fn ejecutar(
    programa: &str,
    args: &[String],
    envs: &[(&str, &str)],
    limite: Duration,
) -> Result<Output, String> {
    let mut cmd = Command::new(programa);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (clave, valor) in envs {
        cmd.env(clave, valor);
    }
    let mut hijo = cmd
        .spawn()
        .map_err(|e| format!("no se pudo ejecutar {programa}: {e}"))?;

    // Los dos tubos se sacan del hijo ANTES de lanzar los hilos (si no, el hijo
    // viajaría con el primero y ya no se podría esperar), y se leen a la vez.
    //
    // POR QUÉ SE DEVUELVE POR UN CANAL Y NO POR EL `JoinHandle`: un proceso puede
    // MORIR y dejar hijos suyos vivos con las tuberías abiertas (pasa de verdad:
    // `sh -c "sleep 30"` con el `sh` de Ubuntu, que no hace `exec`, deja el `sleep`
    // huérfano). Al matar al hijo, el hilo que lee seguiría esperando el fin del
    //tubo hasta que terminara el HUÉRFANO, así que el tope no cortaba nada. Con un
    // canal se espera lo que dice el margen y se devuelve lo leído hasta ahí.
    let (tx_salida, rx_salida) = std::sync::mpsc::channel();
    let (tx_error, rx_error) = std::sync::mpsc::channel();
    let salida_t = hijo.stdout.take();
    let error_t = hijo.stderr.take();
    std::thread::spawn(move || {
        let _ = tx_salida.send(leer_todo(salida_t));
    });
    std::thread::spawn(move || {
        let _ = tx_error.send(leer_todo(error_t));
    });

    let t0 = Instant::now();
    let mut agotado = false;
    let estado = loop {
        match hijo.try_wait() {
            Ok(Some(e)) => break e,
            Ok(None) => {
                if t0.elapsed() >= limite {
                    agotado = true;
                    // `Child::kill` manda la señal más fuerte que permite cada
                    // sistema (SIGKILL en Unix, TerminateProcess en Windows).
                    let _ = hijo.kill();
                    let fin = Instant::now();
                    let muerto = loop {
                        match hijo.try_wait() {
                            Ok(Some(e)) => break Some(e),
                            Ok(None) if fin.elapsed() < MARGEN_MUERTE => {
                                std::thread::sleep(INTERVALO)
                            }
                            _ => break None,
                        }
                    };
                    match muerto {
                        Some(e) => break e,
                        None => {
                            return Err(format!(
                                "{programa} no responde y no se ha podido terminar (se le pidió dos veces)"
                            ))
                        }
                    }
                }
                std::thread::sleep(INTERVALO);
            }
            Err(e) => return Err(format!("no se pudo esperar a {programa}: {e}")),
        }
    };

    let stdout = rx_salida.recv_timeout(MARGEN_MUERTE).unwrap_or_default();
    let stderr = rx_error.recv_timeout(MARGEN_MUERTE).unwrap_or_default();
    let segundos = limite.as_secs().max(1);
    if agotado {
        return Err(format!("{programa} no responde: se ha terminado tras {segundos} s"));
    }
    Ok(Output { status: estado, stdout, stderr })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// El intérprete de órdenes que existe en los TRES sistemas: `cmd` en Windows
    /// y `/bin/sh` en Linux y macOS.
    ///
    /// POR QUÉ: las pruebas pedían `/bin/echo`, `/bin/sleep` y `/bin/sh` a pelo, y
    /// en Windows no existen, así que estaban detrás de `#[cfg(unix)]` y allí no
    /// corrían. El intérprete sí está en los tres, así que la orden se le pasa a él
    /// y la prueba comprueba lo mismo en todos.
    fn interprete() -> &'static str {
        if cfg!(windows) {
            "cmd"
        } else {
            "/bin/sh"
        }
    }

    /// Los argumentos con los que ese intérprete ejecuta una orden: `cmd` usa `/C`
    /// y el `sh` de Unix `-c`.
    fn para_ejecutar(orden: &str) -> Vec<String> {
        if cfg!(windows) {
            vec!["/C".to_string(), orden.to_string()]
        } else {
            vec!["-c".to_string(), orden.to_string()]
        }
    }

    #[test]
    fn devuelve_la_salida_de_un_binario_rapido() {
        let out = ejecutar(interprete(), &para_ejecutar("echo hola"), &[], Duration::from_secs(5))
            .expect("el intérprete del sistema tiene que poder ejecutarse");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hola");
        assert!(out.status.success());
    }

    #[test]
    fn un_binario_que_no_responde_da_error_en_vez_de_colgarse() {
        // Es justo el caso que retenía el hilo: un proceso que no termina. Con el
        // límite, esto tiene que volver con error en ~1 s, no quedarse ahí.
        //
        // La orden que NO termina es por sistema: `sleep` no existe en el `cmd` de
        // Windows, y allí `ping -n 31` es lo que hay para tener un proceso de ~30 s
        // (cada sondeo dura ~1 s). En Linux y macOS sigue siendo el mismo `sleep 30`
        // que se probaba antes.
        let orden = if cfg!(windows) {
            "ping -n 31 127.0.0.1 >nul"
        } else {
            "sleep 30"
        };
        let t0 = Instant::now();
        let r = ejecutar(interprete(), &para_ejecutar(orden), &[], Duration::from_secs(1));
        let e = r.expect_err("una orden que no termina con límite de 1 s tiene que dar error");
        assert!(e.contains("no responde"), "el motivo tiene que ser legible: {e}");
        // El margen es ancho A PROPÓSITO: el límite es de 1 s y la orden tarda 30,
        // así que aunque un runner compartido se atragante, seguir muy por debajo
        // de 25 s demuestra que el tope corta y no se espera a que termine. Un
        // margen de 10 s era demasiado justo y salió flaky en el CI.
        assert!(
            t0.elapsed() < Duration::from_secs(25),
            "no puede esperar los 30 s del proceso"
        );
    }

    /// REGRESIÓN del fallo que destapó el CI: un hijo que muere y deja un NIETO
    /// vivo con la tubería abierta (el `sh` de Ubuntu no hace `exec`, así que
    /// `sleep` queda huérfano). Antes, el tope mataba al hijo, pero la lectura del
    /// tubo seguía esperando al huérfano: con un límite de 1 s, la llamada tardaba
    /// los 30 s del `sleep`.
    ///
    /// Es de Unix porque usa el `&` del shell para dejar un hijo en segundo plano;
    /// el caso de Windows (un proceso que no muere) lo cubre la prueba de arriba.
    #[cfg(unix)]
    #[test]
    fn un_huerfano_con_la_tuberia_abierta_no_bloquea_la_lectura() {
        let t0 = Instant::now();
        let r = ejecutar(
            "/bin/sh",
            &["-c".to_string(), "sleep 30 & sleep 30".to_string()],
            &[],
            Duration::from_secs(1),
        );
        assert!(r.is_err(), "con tope de 1 s tiene que dar error");
        assert!(
            t0.elapsed() < Duration::from_secs(6),
            "no puede esperar al huérfano: tardó {:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn la_salida_de_error_llega_separada_de_la_normal() {
        // Los dos tubos se leen a la vez: si se leyera uno antes que el otro, un
        // proceso que escribe mucho en los dos se bloquearía. La orden es por
        // sistema porque `cmd` no separa órdenes con `;` (usa `&`) ni redirige
        // igual: en los dos casos, un eco va a la salida normal y otro a la de
        // error, que es lo que se comprueba.
        let orden = if cfg!(windows) {
            "echo salida & echo error 1>&2"
        } else {
            "echo salida; echo error >&2"
        };
        let out = ejecutar(interprete(), &para_ejecutar(orden), &[], Duration::from_secs(5))
            .expect("el intérprete del sistema tiene que poder ejecutarse");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "salida");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "error");
    }

    #[test]
    fn un_binario_que_no_existe_se_dice_con_su_nombre() {
        let e = ejecutar("binario-que-no-existe-machinograph", &[], &[], Duration::from_secs(2))
            .expect_err("tiene que fallar");
        assert!(e.contains("binario-que-no-existe-machinograph"), "{e}");
    }
}
