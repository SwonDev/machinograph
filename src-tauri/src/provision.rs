//! Autoinstalación, verificación y autorreparación de lo que Machinograph necesita.
//!
//! POR QUÉ EXISTE: hasta ahora, para descargar un modelo la app exigía tener
//! `llmfit` instalado y, si no estaba, MANDABA AL USUARIO A UNA WEB; y para medir
//! tokens/s de verdad y calcular el encaje hacían falta los binarios de
//! llama.cpp, que la app solo sabía BUSCAR. Eso es fricción: el usuario tiene que
//! salir, averiguar qué bajar para su sistema y su arquitectura, bajarlo,
//! descomprimirlo y ponerlo en el PATH. Aquí eso lo hace la app, sin permisos de
//! administrador y sin pedir nada.
//!
//! CÓMO, Y POR QUÉ ASÍ:
//!
//!  * **El asset no se adivina: se resuelve.** Se pregunta a la API de releases
//!    del proyecto y se elige por (sistema, arquitectura) con una función PURA que
//!    se prueba con el JSON real como fixture. Nunca hay una URL con un tag fijo
//!    escrito a mano: el tag cambia cada semana y una URL fija caduca.
//!  * **Nada se instala sin verificar.** GitHub publica el sha256 de cada asset
//!    (campo `digest`), y llmfit publica además un fichero `.sha256` al lado. Se
//!    comprueba ANTES de extraer y, si no cuadra, no se extrae nada: un binario
//!    a medias es peor que no tenerlo. Un asset sin hash conocido no se instala:
//!    no se puede afirmar que sea el que se publicó.
//!  * **Nada se instala fuera de la carpeta de datos del usuario.** No hace falta
//!    root, y por eso el destructor de caminos comprueba que el destino cuelga de
//!    <datos>/machinograph antes de escribir. Una ruta de fuera se rechaza.
//!  * **Un binario que no arranca está roto.** El último paso no es "el fichero
//!    existe": es EJECUTARLO (`--version`) con su límite de tiempo. Si no
//!    responde o falla, se borra lo instalado y se dice. En el arranque siguiente
//!    (o con «comprobar ahora») se vuelve a bajar solo: autorreparación.
//!  * **Se ve mientras pasa.** Descargar es una acción de red, así que la app dice
//!    qué baja, cuánto lleva (porcentaje y velocidad medidos), cuánto queda y deja
//!    cancelar. Nada de descargas silenciosas.
//!  * **Lo que no se puede instalar sola, se dice.** `amd-smi` viene con ROCm y
//!    necesita root: no se finge que se instala. Se DETECTA, y si falta se enseña
//!    su motivo y el comando exacto del gestor de paquetes de ESTE sistema.
//!
//! Lo que no se puede hacer se dice con su motivo; lo medido se dice medido. Es la
//! regla del proyecto (DESIGN.md) y aquí se aplica a cada paso.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::plataforma;
use crate::proceso;

/// El evento con el que la interfaz se entera de qué se está instalando.
pub const EVENTO: &str = "ai:provision";

/// Límite para preguntar la versión a un binario. Es corto a propósito: un binario
/// que no contesta "qué eres" en 10 s está roto (una biblioteca que falta, un
/// fichero truncado) y no puede retener la comprobación.
const LIMITE_VERSION: Duration = Duration::from_secs(10);

/// Límite para extraer. Un tar.gz de 20 MB se extrae en menos de un segundo; 300 s
/// es holgado incluso en un disco lento, y sobre todo EXISTE: si `tar` se cuelga,
/// esto vuelve con un motivo en vez de quedarse ahí.
const LIMITE_TAR: Duration = Duration::from_secs(300);

/// Cada cuánto, como mucho, se emite una actualización de progreso. El bucle lee
/// de 64 KB en 64 KB y en una conexión rápida son cientos de avisos por segundo:
/// sin freno, la interfaz repintaría más de lo que puede leer.
const INTERVALO_AVISO: Duration = Duration::from_millis(120);

/* ── SHA-256 (a mano, sin dependencias nuevas) ────────────────────────────── */

/// SHA-256 mínimo.
///
/// POR QUÉ ESCRITO AQUÍ: la verificación de las descargas necesita sha256 (es lo
/// que publican las releases de GitHub) y el proyecto se ha propuesto no añadir
/// dependencias para esto. No se usa para firmar ni para autenticar nada: solo
/// para comprobar que el fichero bajado es el que se publicó. Las pruebas lo
/// contrastan con los vectores de la FIPS 180-4 y con la salida real de
/// `sha256sum` sobre un asset de verdad.
mod sha256 {
    use std::io::Read;

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    pub struct Sha256 {
        h: [u32; 8],
        buf: [u8; 64],
        usado: usize,
        total: u64,
    }

    impl Sha256 {
        pub fn nuevo() -> Self {
            Sha256 {
                h: [
                    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                    0x5be0cd19,
                ],
                buf: [0u8; 64],
                usado: 0,
                total: 0,
            }
        }

        pub fn actualizar(&mut self, mut datos: &[u8]) {
            self.total = self.total.wrapping_add(datos.len() as u64);
            if self.usado > 0 {
                let falta = 64 - self.usado;
                let toma = falta.min(datos.len());
                self.buf[self.usado..self.usado + toma].copy_from_slice(&datos[..toma]);
                self.usado += toma;
                datos = &datos[toma..];
                if self.usado == 64 {
                    let b = self.buf;
                    self.comprimir(&b);
                    self.usado = 0;
                }
            }
            while datos.len() >= 64 {
                let mut b = [0u8; 64];
                b.copy_from_slice(&datos[..64]);
                self.comprimir(&b);
                datos = &datos[64..];
            }
            if !datos.is_empty() {
                self.buf[..datos.len()].copy_from_slice(datos);
                self.usado = datos.len();
            }
        }

        fn comprimir(&mut self, bloque: &[u8; 64]) {
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    bloque[4 * i],
                    bloque[4 * i + 1],
                    bloque[4 * i + 2],
                    bloque[4 * i + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let (mut a, mut b, mut c, mut d) = (self.h[0], self.h[1], self.h[2], self.h[3]);
            let (mut e, mut f, mut g, mut hh) = (self.h[4], self.h[5], self.h[6], self.h[7]);
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = hh
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            self.h[0] = self.h[0].wrapping_add(a);
            self.h[1] = self.h[1].wrapping_add(b);
            self.h[2] = self.h[2].wrapping_add(c);
            self.h[3] = self.h[3].wrapping_add(d);
            self.h[4] = self.h[4].wrapping_add(e);
            self.h[5] = self.h[5].wrapping_add(f);
            self.h[6] = self.h[6].wrapping_add(g);
            self.h[7] = self.h[7].wrapping_add(hh);
        }

        pub fn finalizar(mut self) -> [u8; 32] {
            // Los bits se capturan ANTES del relleno: el relleno no cuenta.
            let bits = self.total.wrapping_mul(8);
            self.buf[self.usado] = 0x80;
            self.usado += 1;
            if self.usado > 56 {
                for b in self.buf.iter_mut().skip(self.usado) {
                    *b = 0;
                }
                let b = self.buf;
                self.comprimir(&b);
                self.usado = 0;
            }
            for b in self.buf.iter_mut().take(56).skip(self.usado) {
                *b = 0;
            }
            self.buf[56..64].copy_from_slice(&bits.to_be_bytes());
            let b = self.buf;
            self.comprimir(&b);
            let mut out = [0u8; 32];
            for i in 0..8 {
                out[4 * i..4 * i + 4].copy_from_slice(&self.h[i].to_be_bytes());
            }
            out
        }
    }

    fn a_hex(b: &[u8]) -> String {
        let mut s = String::with_capacity(b.len() * 2);
        for x in b {
            s.push_str(&format!("{x:02x}"));
        }
        s
    }

    /// sha256 de unos bytes, en hexadecimal minúscula.
    ///
    /// Existe SOLO para las pruebas (de ahí el `cfg(test)`): la verificación real
    /// hashea ficheros con `de_archivo`; esto permite contrastar el resultado
    /// contra los vectores de la FIPS 180-4 sin tocar el disco.
    #[cfg(test)]
    pub fn de_bytes(datos: &[u8]) -> String {
        let mut h = Sha256::nuevo();
        h.actualizar(datos);
        a_hex(&h.finalizar())
    }

    /// sha256 de un fichero, leído a trozos (no se carga entero en memoria).
    pub fn de_archivo(p: &std::path::Path) -> Result<String, String> {
        let mut f = std::fs::File::open(p).map_err(|e| format!("no se pudo abrir {p:?}: {e}"))?;
        let mut h = Sha256::nuevo();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf).map_err(|e| format!("no se pudo leer {p:?}: {e}"))?;
            if n == 0 {
                break;
            }
            h.actualizar(&buf[..n]);
        }
        Ok(a_hex(&h.finalizar()))
    }
}

use sha256::de_archivo as sha256_archivo;

/* ── El catálogo de herramientas ──────────────────────────────────────────── */

/// Por dónde se publica una herramienta.
#[derive(Debug, Clone)]
pub enum Origen {
    /// Releases de GitHub. `canal` dice si se usa la última publicada (`latest`)
    /// o la nocturna más nueva (los tags `bNNNNN` de llama.cpp, que son
    /// «prerelease» y por eso `/releases/latest` NO los devuelve).
    Github {
        repo: &'static str,
        canal: Canal,
        colocacion: Colocacion,
    },
    /// No se puede instalar sola. Se dice con su motivo y el comando del gestor
    /// de paquetes de este sistema (no se inventa uno genérico).
    Manual {
        motivo: &'static str,
        paquete: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canal {
    Ultima,
    Nocturna,
}

/// Dónde y cómo queda lo instalado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colocacion {
    /// El ejecutable se copia a `<datos>/machinograph/bin` (herramientas de un binario).
    BinDir,
    /// El árbol extraído se guarda entero en `<datos>/machinograph/llama/<tag>/`, porque
    /// los ejecutables de llama.cpp necesitan sus `.so` al lado (`$ORIGIN`).
    ArbolLlama,
}

#[derive(Debug, Clone)]
pub struct Herramienta {
    pub id: &'static str,
    /// Cómo se llama para leerlo.
    pub nombre: &'static str,
    /// Para qué la usa Machinograph. Se enseña tal cual: sin esto, una lista de binarios
    /// no dice nada.
    pub para_que: &'static str,
    pub imprescindible: bool,
    /// Ejecutables que tiene que haber. El primero se usa para leer la versión.
    pub binarios: &'static [&'static str],
    /// La bandera con la que se comprueba que el binario ARRANCA (tiene que salir
    /// con código 0). Para llama.cpp y amd-smi es `--help` A PROPÓSITO: sus builds
    /// viejas no aceptan `--version` (sale 1 con «invalid parameter»), y un binario
    /// que mide perfectamente no está roto por eso. Se comprobó en esta máquina:
    /// el `llama-bench` de `~/.local/bin` rechaza `--version` y responde a `--help`.
    pub comprobar: &'static [&'static str],
    /// De dónde se lee la versión, si el binario la publica. Puede fallar sin que
    /// el binario esté roto: entonces la versión queda vacía y se dice, en vez de
    /// inventarla.
    pub version: Option<&'static [&'static str]>,
    pub origen: Origen,
}

/// El catálogo. Es una constante: lo que Machinograph usa no lo decide la interfaz.
const CATALOGO: &[Herramienta] = &[
    Herramienta {
        id: "llmfit",
        nombre: "llmfit",
        para_que: "Descargar modelos, recomendarlos según tu hardware y estimar la velocidad",
        imprescindible: true,
        binarios: &["llmfit"],
        comprobar: &["--version"],
        version: Some(&["--version"]),
        origen: Origen::Github {
            repo: "AlexsJones/llmfit",
            canal: Canal::Ultima,
            colocacion: Colocacion::BinDir,
        },
    },
    Herramienta {
        id: "llama.cpp",
        nombre: "llama.cpp (medir y encajar)",
        para_que: "Medir tokens/s de verdad y calcular qué contexto te cabe (llama-bench y llama-fit-params)",
        imprescindible: false,
        binarios: &["llama-bench", "llama-fit-params"],
        comprobar: &["--help"],
        version: Some(&["--version"]),
        origen: Origen::Github {
            repo: "ggml-org/llama.cpp",
            canal: Canal::Nocturna,
            colocacion: Colocacion::ArbolLlama,
        },
    },
    Herramienta {
        id: "amd-smi",
        nombre: "amd-smi",
        para_que: "Leer temperatura, potencia y memoria de la GPU AMD",
        imprescindible: false,
        binarios: &["amd-smi"],
        // amd-smi no tiene `--version`: hay que pasarle un subcomando. `--help`
        // responde y es lo único que hace falta para saber que arranca.
        comprobar: &["--help"],
        version: None,
        origen: Origen::Manual {
            motivo: "Viene con el paquete de ROCm de tu sistema y se instala con permisos de \
                     administrador: Machinograph no puede hacerlo sola sin pedir root.",
            paquete: "amdsmi",
        },
    },
];

/// El catálogo para ESTE sistema. `amd-smi` solo existe en Linux (en macOS y
/// Windows la GPU se lee por otras vías), así que ahí no se enseña: una fila que
/// no puede existir sería ruido.
pub fn catalogo() -> Vec<Herramienta> {
    CATALOGO
        .iter()
        .filter(|h| h.id != "amd-smi" || plataforma::so() == "linux")
        .cloned()
        .collect()
}

/// Una herramienta del catálogo por su id.
///
/// Existe SOLO para las pruebas (de ahí el `cfg(test)`): el código de producción
/// recorre el catálogo entero con `catalogo()`, y la elección del asset se prueba
/// aquí contra el catálogo real. Su único consumidor fuera de las pruebas es
/// `canal_de`, que también es solo de pruebas.
#[cfg(test)]
pub fn herramienta(id: &str) -> Option<Herramienta> {
    catalogo().into_iter().find(|h| h.id == id)
}

/* ── Dónde vive todo ──────────────────────────────────────────────────────── */

/// La raíz que gestiona la app: `<datos>/machinograph`.
///
/// `datos` lo resuelve `plataforma::rutas()` (XDG en Linux, `~/Library` en macOS,
/// `%LOCALAPPDATA%` en Windows), así que esto nunca cae fuera de la carpeta del
/// usuario ni necesita permisos.
pub fn raiz_gestion() -> PathBuf {
    plataforma::rutas().datos.join("machinograph")
}

fn dir_bin_en(raiz: &Path) -> PathBuf {
    raiz.join("bin")
}

fn dir_llama(raiz: &Path) -> PathBuf {
    raiz.join("llama")
}

fn dir_tmp(raiz: &Path) -> PathBuf {
    raiz.join("tmp")
}

/// El nombre del ejecutable en este sistema (en Windows llevan `.exe`).
fn exe_nombre(nombre: &str) -> String {
    if cfg!(windows) {
        format!("{nombre}.exe")
    } else {
        nombre.to_string()
    }
}

/// ¿Esta ruta cuelga del directorio que gestiona la app? Es PURO (se le da la
/// raíz) para poder probar el rechazo sin tocar el disco.
pub fn es_ruta_gestionada_en(raiz: &Path, p: &Path) -> bool {
    p.starts_with(raiz) && p != raiz
}

/// Rechaza cualquier destino de instalación que no cuelgue de la carpeta de datos.
///
/// POR QUÉ: la promesa es «nada fuera de tu carpeta de datos y sin root». Si un
/// día alguien compone mal una ruta (o un nombre de asset malicioso trae `../`),
/// esto lo corta ANTES de escribir nada.
fn validar_destino(raiz: &Path, destino: &Path) -> Result<(), String> {
    if !raiz.is_absolute() {
        return Err(format!(
            "la raíz de instalación tiene que ser absoluta, y es {raiz:?}"
        ));
    }
    if !es_ruta_gestionada_en(raiz, destino) {
        return Err(format!(
            "se ha intentado instalar en {destino:?}, que está fuera de la carpeta de datos de la \
             aplicación ({raiz:?}): no se escribe ahí"
        ));
    }
    Ok(())
}

/* ── Elegir el asset (puro y probado con el JSON real) ────────────────────── */

/// Un asset de una release de GitHub, con lo que hace falta para instalarlo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Asset {
    pub nombre: String,
    pub url: String,
    pub tamano: u64,
    /// `sha256:<hex>` tal cual lo publica la API, o `None` si no lo trae.
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AssetCrudo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReleaseCruda {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<AssetCrudo>,
}

/// Una release ya normalizada.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// El asset que corresponde a (sistema, arquitectura), si lo hay.
    pub fn elegir(&self, id: &str, so: &str, arq: &str) -> Option<Asset> {
        let (nombres, ext) = nombres_candidatos(id, &self.tag, so, arq)?;
        for n in nombres {
            let esperado = format!("{n}.{ext}");
            if let Some(a) = self.assets.iter().find(|a| a.nombre == esperado) {
                return Some(a.clone());
            }
        }
        None
    }

    /// El asset `.sha256` que acompaña a otro, si el proyecto lo publica.
    ///
    /// llmfit lo publica; llama.cpp no. Cuando está, es una SEGUNDA fuente del
    /// mismo hash, así que se contrasta con el `digest` de la API: si los dos no
    /// dicen lo mismo, algo va mal y no se instala.
    pub fn sidecar(&self, asset: &Asset) -> Option<Asset> {
        let nombre = format!("{}.sha256", asset.nombre);
        self.assets
            .iter()
            .find(|a| a.nombre == nombre)
            .cloned()
    }
}

/// Los nombres de asset que valen para (sistema, arquitectura), en orden de
/// preferencia, y la extensión.
///
/// POR QUÉ EN ORDEN: en Linux x64, llmfit publica glibc (`gnu`) y musl; en un
/// sistema normal el bueno es glibc, y si algún día solo estuviera el musl, se
/// usaría ese. La preferencia es explícita y probada, no "lo que salga primero".
fn nombres_candidatos(id: &str, tag: &str, so: &str, arq: &str) -> Option<(Vec<String>, &'static str)> {
    let ext = if so == "windows" { "zip" } else { "tar.gz" };
    match id {
        // llmfit nombra los assets por target triple de Rust.
        "llmfit" => {
            let triples: &[&str] = match (so, arq) {
                ("linux", "x64") => &["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"],
                ("linux", "arm64") => &["aarch64-unknown-linux-gnu", "aarch64-unknown-linux-musl"],
                ("macos", "x64") => &["x86_64-apple-darwin"],
                ("macos", "arm64") => &["aarch64-apple-darwin"],
                ("windows", "x64") => &["x86_64-pc-windows-msvc"],
                ("windows", "arm64") => &["aarch64-pc-windows-msvc"],
                _ => return None,
            };
            Some((
                triples.iter().map(|t| format!("llmfit-{tag}-{t}")).collect(),
                ext,
            ))
        }
        // llama.cpp nombra por plataforma, y el tag ya es `bNNNNN`.
        "llama.cpp" => {
            let plataforma = match (so, arq) {
                ("linux", "x64") => "ubuntu-x64",
                ("linux", "arm64") => "ubuntu-arm64",
                ("macos", "x64") => "macos-x64",
                ("macos", "arm64") => "macos-arm64",
                ("windows", "x64") => "win-cpu-x64",
                ("windows", "arm64") => "win-cpu-arm64",
                _ => return None,
            };
            // Solo la build de CPU: los assets `-cuda-`, `-vulkan-`, `-rocm-`… son
            // para quien ya tiene ese runtime, y elegirlos a ciegas daría un
            // binario que no arranca en la mayoría de equipos. La coincidencia es
            // EXACTA, así que las variantes no se cuelan.
            Some((vec![format!("llama-{tag}-bin-{plataforma}")], ext))
        }
        _ => None,
    }
}

/// La arquitectura de este equipo, en los nombres que usan los assets.
pub fn arq() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        otra => otra,
    }
}

/// El canal (última/nocturna) con el que se publica una herramienta del catálogo.
///
/// Existe SOLO para las pruebas (de ahí el `cfg(test)`): es el envoltorio de
/// `herramienta()` que usa `elegir_asset` para reproducir, desde la prueba, qué
/// canal elegiría cada herramienta. La instalación real lee el canal del catálogo
/// directamente.
#[cfg(test)]
fn canal_de(id: &str) -> Canal {
    herramienta(id).map_or(Canal::Ultima, |h| match h.origen {
        Origen::Github { canal, .. } => canal,
        Origen::Manual { .. } => Canal::Ultima,
    })
}

/// Convierte el JSON de la API en una release.
///
/// Acepta las dos formas que se piden: el OBJETO de `/releases/latest` y la LISTA
/// de `/releases`. En la lista, para el canal nocturno se elige la de mayor número
/// de build: GitHub las ordena por fecha, pero el número no depende de eso.
pub fn release_de(json: &str, canal: Canal) -> Option<Release> {
    let valor: serde_json::Value = serde_json::from_str(json).ok()?;
    let crudas: Vec<ReleaseCruda> = match valor {
        serde_json::Value::Array(a) => a
            .into_iter()
            .filter_map(|x| serde_json::from_value(x).ok())
            .collect(),
        v => vec![serde_json::from_value(v).ok()?],
    };
    let elegida = match canal {
        Canal::Ultima => crudas.into_iter().find(|r| !r.assets.is_empty()),
        Canal::Nocturna => crudas
            .into_iter()
            .filter_map(|r| build_nocturno(&r.tag_name).map(|n| (n, r)))
            .max_by_key(|(n, _)| *n)
            .map(|(_, r)| r),
    }?;
    let assets = elegida
        .assets
        .into_iter()
        .filter(|a| !a.name.is_empty())
        .map(|a| Asset {
            nombre: a.name,
            url: a.browser_download_url,
            tamano: a.size,
            digest: a.digest,
        })
        .collect();
    Some(Release {
        tag: elegida.tag_name,
        assets,
    })
}

/// El número de build de un tag nocturno (`b11375` -> 11375). `None` si el tag no
/// tiene esa forma (así una release «de verdad» como `v0.5.0` no se confunde con
/// una nocturna).
fn build_nocturno(tag: &str) -> Option<u64> {
    let n = tag.strip_prefix('b')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// ELECCIÓN PURA Y PROBADA: dado el JSON de la API y un (sistema, arquitectura),
/// devuelve el asset o `None`. No toca el disco ni la red; se prueba con el JSON
/// real como fixture para las seis combinaciones.
///
/// Existe SOLO para las pruebas (de ahí el `cfg(test)`): la instalación real llama
/// a `release_de` y a `Release::elegir` por separado; esta puerta junta las dos
/// para probar la elección completa contra el fixture.
#[cfg(test)]
pub fn elegir_asset(id: &str, json: &str, so: &str, arq: &str) -> Option<Asset> {
    release_de(json, canal_de(id))?.elegir(id, so, arq)
}

/// El `sha256:<hex>` de la API, normalizado a hexadecimal minúscula.
pub fn hex_del_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest).trim();
    let valido = hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit());
    valido.then(|| hex.to_ascii_lowercase())
}

/// El hash del fichero `.sha256` que publica un proyecto.
///
/// El formato es el de `sha256sum`: `<hex>  <nombre-del-fichero>`. Se lee solo el
/// primer campo y se exige que sea un sha256 completo: un fichero de sumas
/// truncado o con otra cosa dentro no vale como verificación.
pub fn hash_de_sha256(texto: &str) -> Option<String> {
    let primero = texto.split_whitespace().next()?;
    hex_del_digest(primero)
}

/// Comprueba el sha256 de un fichero contra el esperado.
pub fn verificar_sha256(archivo: &Path, esperado_hex: &str) -> Result<String, String> {
    let real = sha256_archivo(archivo)?;
    let esperado = esperado_hex.to_ascii_lowercase();
    if real != esperado {
        return Err(format!(
            "el sha256 de {archivo:?} no coincide: se esperaba {esperado} y ha salido {real}. No se \
             extrae nada"
        ));
    }
    Ok(real)
}

/* ── Detección ────────────────────────────────────────────────────────────── */

/// En qué punto está una herramienta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Estado {
    /// Está y arranca.
    Listo,
    /// No está, y la app puede instalarla sola.
    Falta,
    /// Se está instalando ahora mismo.
    Descargando,
    /// Está, pero no arranca (borrada a medias, truncada, sin sus bibliotecas).
    Roto,
    /// No está y no se puede instalar sola (necesita root o un paquete del sistema).
    NoInstalable,
}

/// Qué hacer con una herramienta, según su estado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Decision {
    Saltar,
    Instalar,
    Reparar,
}

/// LA DECISIÓN DE «ROTO -> VOLVER A BAJAR», separada para probarla sin red.
pub fn decidir(estado: Estado, instalable: bool, forzar: bool) -> Decision {
    if !instalable {
        return Decision::Saltar;
    }
    match estado {
        Estado::Roto => Decision::Reparar,
        Estado::Falta => Decision::Instalar,
        Estado::Listo if forzar => Decision::Instalar,
        _ => Decision::Saltar,
    }
}

/// Lo que la interfaz enseña de cada herramienta.
#[derive(Debug, Clone, Serialize)]
pub struct EstadoHerramienta {
    pub id: String,
    pub nombre: String,
    pub para_que: String,
    pub imprescindible: bool,
    /// `false` = no se puede instalar sola (hay motivo y comando).
    pub instalable: bool,
    pub estado: Estado,
    /// De dónde sale: la URL del proyecto o el paquete del sistema.
    pub origen: String,
    pub ruta: Option<String>,
    pub version: Option<String>,
    pub motivo_manual: Option<String>,
    pub comando_manual: Option<String>,
    pub detalle: Option<String>,
}

/// Las carpetas donde se busca un ejecutable, EN ORDEN.
///
/// 1. Lo que ha instalado la app (`<datos>/machinograph/bin` y los árboles de llama.cpp):
///    es lo suyo y manda sobre lo que haya por ahí.
/// 2. Los sitios que ya miraba `perf::runtimes()`: `~/.local/bin`, `~/.cache/llmfit`
///    y lo que diga `MACHINOGRAPH_LLAMA_DIRS`.
/// 3. El PATH (una app de escritorio no siempre lo hereda completo).
fn candidatos_bin(raiz: &Path, nombre: &str) -> Vec<PathBuf> {
    let exe = exe_nombre(nombre);
    let mut v: Vec<PathBuf> = Vec::new();
    v.push(dir_bin_en(raiz).join(&exe));
    for d in dirs_llama_en(raiz) {
        v.push(d.join(&exe));
    }
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".local").join("bin").join(&exe));
        v.push(home.join(".cache").join("llmfit").join(&exe));
    }
    if let Ok(extra) = std::env::var("MACHINOGRAPH_LLAMA_DIRS") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        v.extend(
            extra
                .split(sep)
                .filter(|s| !s.trim().is_empty())
                .map(|s| PathBuf::from(s).join(&exe)),
        );
    }
    for d in path_dirs() {
        v.push(d.join(&exe));
    }
    v
}

fn path_dirs() -> Vec<PathBuf> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    std::env::var("PATH")
        .map(|p| {
            p.split(sep)
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// El primer candidato que existe de verdad. Separado para poder probar el orden.
fn primer_existente(candidatos: &[PathBuf]) -> Option<PathBuf> {
    candidatos.iter().find(|p| p.is_file()).cloned()
}

fn encontrar(raiz: &Path, nombre: &str) -> Option<PathBuf> {
    primer_existente(&candidatos_bin(raiz, nombre))
}

/// Los árboles de llama.cpp que ha instalado la app, el más nuevo primero.
///
/// Se busca el directorio que CONTIENE los ejecutables, no el que se extrae: el
/// tar.gz trae un nivel de más (`llama-b11375/…`), y las bibliotecas `.so` tienen
/// que quedar al lado del binario.
pub fn dirs_llama() -> Vec<PathBuf> {
    dirs_llama_en(&raiz_gestion())
}

fn dirs_llama_en(raiz: &Path) -> Vec<PathBuf> {
    let mut out: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let base = dir_llama(raiz);
    let niveles: Vec<PathBuf> = match std::fs::read_dir(&base) {
        Ok(entradas) => entradas
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => return Vec::new(),
    };
    let mut candidatos: Vec<PathBuf> = Vec::new();
    for n1 in niveles {
        candidatos.push(n1.clone());
        if let Ok(hijos) = std::fs::read_dir(&n1) {
            candidatos.extend(hijos.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    for c in candidatos {
        let tiene = ["llama-bench", "llama-fit-params"]
            .iter()
            .any(|n| c.join(exe_nombre(n)).is_file());
        if !tiene {
            continue;
        }
        let mtime = std::fs::metadata(&c)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        out.push((mtime, c));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, p)| p).collect()
}

/// Ejecuta un binario con sus argumentos de sonda.
///
/// Error si no se puede lanzar o si sale distinto de 0: eso es lo que significa
/// «roto». Se devuelve la salida (stdout y, si está vacía, stderr: `llama-bench`
/// escribe por stderr).
fn ejecutar_bin(bin: &Path, args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let salida = proceso::ejecutar(&bin.to_string_lossy(), &args, &[], LIMITE_VERSION)?;
    if !salida.status.success() {
        return Err(format!(
            "{} {} terminó con {}: {}",
            bin.display(),
            args.join(" "),
            salida.status,
            String::from_utf8_lossy(&salida.stderr).trim()
        ));
    }
    let out = String::from_utf8_lossy(&salida.stdout).trim().to_string();
    if !out.is_empty() {
        return Ok(out);
    }
    Ok(String::from_utf8_lossy(&salida.stderr).trim().to_string())
}

/// La versión de un binario que YA ha arrancado.
///
/// Se prueba su bandera de versión y, si el binario no la acepta (builds viejas de
/// llama.cpp), se devuelve `None`: eso NO es estar roto, es que esa build no
/// publica su versión de esa forma. Inventarla sería mentir.
fn leer_version(h: &Herramienta, bin: &Path, salida_arranque: &str) -> Option<String> {
    match h.version {
        None => None,
        Some(v) if v == h.comprobar => version_de(h.id, salida_arranque),
        Some(v) => ejecutar_bin(bin, v).ok().and_then(|s| version_de(h.id, &s)),
    }
}

/// La versión, leída de lo que imprime `--version`.
///
/// Los formatos son los REALES de cada herramienta y se probaron con sus binarios:
/// llmfit dice `llmfit 1.1.16`; los de llama.cpp dicen
/// `version: 0.5.0-dev (build 11375, commit 436f6f89e)`. Si el formato cambia, se
/// devuelve `None` (no un número inventado).
pub fn version_de(id: &str, salida: &str) -> Option<String> {
    if id == "llmfit" {
        let linea = salida.lines().find(|l| l.trim_start().starts_with("llmfit"))?;
        return linea.split_whitespace().nth(1).map(str::to_string);
    }
    // llama.cpp: `version: X (build NNNNN, commit …)`.
    let linea = salida.lines().find(|l| l.contains("version:"))?;
    let build = linea.split("build ").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?;
    (!build.is_empty()).then(|| format!("b{build}"))
}

/// El comando con el que se instala un paquete en ESTE sistema.
///
/// Se elige por el gestor que hay de verdad en el PATH, no por lo que diga el
/// sistema: un mismo Linux puede llevar dnf, apt, pacman, brew o zypper; macOS
/// lleva brew; y Windows, winget o choco. Así el comando que se copia es el que
/// funciona en esta máquina, no uno genérico que habría que adaptar.
pub fn comando_instalacion(paquete: &str) -> String {
    comando_para(paquete, gestor_paquetes())
}

fn gestor_paquetes() -> Option<&'static str> {
    // El orden importa poco (casi siempre hay uno), pero el de Linux va primero
    // porque es donde vive el caso que hoy hace falta: `amd-smi` viene con ROCm.
    ["rpm-ostree", "dnf", "apt", "pacman", "zypper", "brew", "winget", "choco"]
        .into_iter()
        .find(|g| path_dirs().iter().any(|d| d.join(exe_nombre(g)).is_file()))
}

/// Separado para probarlo sin depender del sistema donde corre la prueba.
pub fn comando_para(paquete: &str, gestor: Option<&str>) -> String {
    match gestor {
        Some("rpm-ostree") => format!("rpm-ostree install {paquete} (necesita reiniciar)"),
        Some("dnf") => format!("sudo dnf install {paquete}"),
        Some("apt") => format!("sudo apt install {paquete}"),
        Some("pacman") => format!("sudo pacman -S {paquete}"),
        Some("zypper") => format!("sudo zypper install {paquete}"),
        Some("brew") => format!("brew install {paquete}"),
        Some("winget") => format!("winget install {paquete}"),
        Some("choco") => format!("choco install {paquete}"),
        // Sin gestor conocido no se inventa un comando: se dice de dónde sale el
        // paquete y que lo instale el usuario con el suyo.
        _ => format!("instala el paquete «{paquete}» con el gestor de tu sistema (viene con ROCm)"),
    }
}

/// El estado de una herramienta en un directorio dado (sin red, sin escribir).
fn estado_de(h: &Herramienta, raiz: &Path) -> EstadoHerramienta {
    let (origen, instalable, motivo, comando) = match &h.origen {
        Origen::Github { repo, .. } => (
            format!("https://github.com/{repo}/releases"),
            true,
            None,
            None,
        ),
        Origen::Manual { motivo, paquete } => (
            format!("paquete del sistema ({paquete})"),
            false,
            Some(motivo.to_string()),
            Some(comando_instalacion(paquete)),
        ),
    };
    let mut base = EstadoHerramienta {
        id: h.id.to_string(),
        nombre: h.nombre.to_string(),
        para_que: h.para_que.to_string(),
        imprescindible: h.imprescindible,
        instalable,
        estado: Estado::Falta,
        origen,
        ruta: None,
        version: None,
        motivo_manual: motivo,
        comando_manual: comando,
        detalle: None,
    };

    // Todos los ejecutables tienen que estar: para medir hacen falta los dos
    // (`llama-bench` y `llama-fit-params`), y tener solo uno no es "listo".
    let mut encontrados = Vec::new();
    for nombre in h.binarios {
        match encontrar(raiz, nombre) {
            Some(p) => encontrados.push((nombre, p)),
            None => {
                base.estado = if instalable {
                    Estado::Falta
                } else {
                    Estado::NoInstalable
                };
                return base;
            }
        }
    }

    // Están todos los ficheros; ahora hay que ver si ARRANCAN. Se usa la bandera
    // de sonda de CADA herramienta: `--version` no vale para todas.
    let (_, principal) = &encontrados[0];
    base.ruta = Some(principal.to_string_lossy().to_string());
    match ejecutar_bin(principal, h.comprobar) {
        Ok(salida) => {
            base.version = leer_version(h, principal, &salida);
            base.estado = Estado::Listo;
        }
        Err(e) => {
            base.estado = Estado::Roto;
            base.detalle = Some(e);
        }
    }
    base
}

/// El estado de todas las herramientas, con lo que esté en curso marcado como tal.
pub fn estados() -> Vec<EstadoHerramienta> {
    let raiz = raiz_gestion();
    let activo = en_curso();
    catalogo()
        .iter()
        .map(|h| {
            let mut e = estado_de(h, &raiz);
            if let Some(p) = &activo {
                if p.herramienta == h.nombre {
                    e.estado = Estado::Descargando;
                    e.detalle = Some(p.linea.clone());
                }
            }
            e
        })
        .collect()
}

/* ── El ajuste `auto_provision` ───────────────────────────────────────────── */

/// La preferencia se guarda en un JSON dentro de la carpeta de datos de la app
/// (`<datos>/machinograph/provision.json`), que es donde vive todo lo que gestiona este
/// módulo. No se usa la tabla `settings` del backend porque esa solo acepta los
/// ajustes NUMÉRICOS declarados en `db::AJUSTES`, y esto es un sí/no que decide
/// este módulo. La app lo dice en Ajustes: se escribe en tu carpeta de datos.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct Config {
    auto_provision: bool,
}

impl Default for Config {
    fn default() -> Self {
        // Por defecto SÍ: el objetivo es no generar fricción.
        Config { auto_provision: true }
    }
}

fn leer_config_en(raiz: &Path) -> Config {
    let p = raiz.join("provision.json");
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        // Un fichero ilegible o a medias no puede dejar la app sin autoinstalarse:
        // se vuelve al valor por defecto.
        .unwrap_or_default()
}

fn escribir_config_en(raiz: &Path, c: &Config) -> Result<(), String> {
    std::fs::create_dir_all(raiz).map_err(|e| format!("no se pudo crear {raiz:?}: {e}"))?;
    let p = raiz.join("provision.json");
    let texto = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
    std::fs::write(&p, texto).map_err(|e| format!("no se pudo escribir {p:?}: {e}"))
}

/// ¿Está activada la instalación automática? Por defecto sí.
pub fn auto_activo() -> bool {
    leer_config_en(&raiz_gestion()).auto_provision
}

pub fn fijar_auto(activo: bool) -> Result<bool, String> {
    escribir_config_en(&raiz_gestion(), &Config { auto_provision: activo })?;
    Ok(activo)
}

/* ── Progreso, cancelación y estado global ────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Fase {
    Preparando,
    Descargando,
    Verificando,
    Extrayendo,
    Terminada,
    Cancelada,
    Fallida,
}

impl Fase {
    fn en_curso(self) -> bool {
        matches!(self, Fase::Preparando | Fase::Descargando | Fase::Verificando | Fase::Extrayendo)
    }
}

/// Lo que la interfaz enseña de la instalación en curso (o de la última).
#[derive(Debug, Clone, Serialize)]
pub struct Progreso {
    pub herramienta: String,
    pub fase: Fase,
    /// La última línea, para poder leerla y comprobarla.
    pub linea: String,
    pub pct: Option<f64>,
    pub bajado_bytes: u64,
    pub total_bytes: u64,
    /// Medidos aquí, con dos lecturas separadas en el tiempo.
    pub b_s: Option<f64>,
    pub eta_s: Option<f64>,
    pub error: Option<String>,
}

impl Progreso {
    fn nuevo(herramienta: &str, fase: Fase, linea: &str) -> Self {
        Progreso {
            herramienta: herramienta.to_string(),
            fase,
            linea: linea.to_string(),
            pct: None,
            bajado_bytes: 0,
            total_bytes: 0,
            b_s: None,
            eta_s: None,
            error: None,
        }
    }
}

/// Lo que se manda con cada aviso: lo que está pasando y cómo acabó lo anterior.
#[derive(Debug, Clone, Serialize)]
pub struct Aviso {
    pub en_curso: Option<Progreso>,
    pub ultimo: Option<Progreso>,
}

struct Global {
    activo: Option<Progreso>,
    cancelar: Arc<AtomicBool>,
    ocupado: bool,
}

static GLOBAL: LazyLock<Mutex<Global>> = LazyLock::new(|| {
    Mutex::new(Global {
        activo: None,
        cancelar: Arc::new(AtomicBool::new(false)),
        ocupado: false,
    })
});

/// La instalación en curso, si la hay.
pub fn en_curso() -> Option<Progreso> {
    GLOBAL.lock().activo.clone()
}

/// Deja constancia de un aviso en el estado global (para `en_curso`).
fn registrar(a: &Aviso) {
    GLOBAL.lock().activo = a.en_curso.clone();
}

/// Cancela la instalación en curso.
///
/// La descarga se corta donde está (el bucle comprueba la bandera en cada trozo) y
/// el fichero a medias se borra: no queda basura ni un binario corrupto.
pub fn cancelar() -> Result<String, String> {
    let g = GLOBAL.lock();
    if !g.ocupado {
        return Ok("No hay ninguna instalación en curso.".into());
    }
    g.cancelar.store(true, Ordering::SeqCst);
    Ok("Se ha pedido cancelar: la instalación en curso se corta en el siguiente paso.".into())
}

/* ── Descarga, extracción e instalación ───────────────────────────────────── */

fn cliente() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("machinograph/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        // Sin límite TOTAL: una descarga de 20 MB puede tardar en una conexión
        // lenta, y matarla por tardar sería justo lo contrario de lo que se quiere.
        .build()
        .map_err(|e| format!("no se pudo preparar el cliente HTTP: {e}"))
}

fn traer_texto(cli: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let resp = cli
        .get(url)
        .send()
        .map_err(|e| format!("no se pudo consultar {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "{url} respondió {}: la API de releases no dio los datos de la versión",
            resp.status()
        ));
    }
    resp.text().map_err(|e| format!("no se pudo leer la respuesta de {url}: {e}"))
}

fn avisar(publicar: &mut dyn FnMut(&Aviso), p: Progreso) {
    publicar(&Aviso {
        en_curso: p.fase.en_curso().then(|| p),
        ultimo: None,
    });
}

/// Baja un fichero a `destino` con progreso medido y cancelación.
#[allow(clippy::too_many_arguments)]
fn bajar(
    cli: &reqwest::blocking::Client,
    url: &str,
    destino: &Path,
    herramienta: &str,
    descripcion: &str,
    fuente: &str,
    cancel: &AtomicBool,
    publicar: &mut dyn FnMut(&Aviso),
) -> Result<(), String> {
    let mut resp = cli
        .get(url)
        .send()
        .map_err(|e| format!("no se pudo empezar la descarga de {descripcion}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "{url} respondió {}: no se puede descargar {descripcion}",
            resp.status()
        ));
    }
    let total = resp.content_length().unwrap_or(0);
    if let Some(dir) = destino.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {dir:?}: {e}"))?;
    }
    let mut f = std::fs::File::create(destino)
        .map_err(|e| format!("no se pudo crear {destino:?}: {e}"))?;

    let mut buf = [0u8; 64 * 1024];
    let mut bajado: u64 = 0;
    let mut t_ult = Instant::now();
    let mut b_ult: u64 = 0;
    let mut b_s: Option<f64> = None;
    let mut eta: Option<f64> = None;

    loop {
        if cancel.load(Ordering::SeqCst) {
            drop(f);
            let _ = std::fs::remove_file(destino);
            return Err("cancelado por el usuario".into());
        }
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("se cortó la descarga de {descripcion}: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])
            .map_err(|e| format!("no se pudo escribir {destino:?}: {e}"))?;
        bajado += n as u64;

        let ahora = Instant::now();
        let dt = ahora.duration_since(t_ult).as_secs_f64();
        if dt >= INTERVALO_AVISO.as_secs_f64() || (total > 0 && bajado >= total) {
            // La velocidad se MIDE con dos lecturas separadas en el tiempo; nada de
            // un número de adorno.
            if dt > 0.0 {
                let instantanea = (bajado - b_ult) as f64 / dt;
                if instantanea > 0.0 {
                    b_s = Some(instantanea);
                }
            }
            if let (Some(v), true) = (b_s, total > bajado) {
                eta = Some((total - bajado) as f64 / v);
            }
            let pct = (total > 0).then(|| (bajado as f64 * 100.0 / total as f64).min(100.0));
            avisar(
                publicar,
                Progreso {
                    herramienta: herramienta.to_string(),
                    fase: Fase::Descargando,
                    linea: format!(
                        "Bajando {descripcion} ({:.1} MB) desde {fuente}",
                        total as f64 / 1_048_576.0
                    ),
                    pct,
                    bajado_bytes: bajado,
                    total_bytes: total,
                    b_s,
                    eta_s: eta,
                    error: None,
                },
            );
            t_ult = ahora;
            b_ult = bajado;
        }
    }
    f.flush().map_err(|e| format!("no se pudo cerrar {destino:?}: {e}"))?;
    if total > 0 && bajado != total {
        let _ = std::fs::remove_file(destino);
        return Err(format!(
            "la descarga de {descripcion} quedó a medias ({bajado} de {total} bytes): se descarta"
        ));
    }
    Ok(())
}

/// Extrae con el `tar` DEL SISTEMA (no se añade ningún crate para esto).
///
/// En Linux y macOS es GNU/BSD tar y en Windows 10+ es bsdtar, que además sabe con
/// los `.zip` que publica llmfit para Windows. `-z` es para los `.tar.gz`.
fn extraer(archivo: &Path, destino: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destino).map_err(|e| format!("no se pudo crear {destino:?}: {e}"))?;
    let zip = archivo
        .extension()
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);
    let mut args: Vec<String> = vec![if zip { "-xf" } else { "-xzf" }.to_string()];
    args.push(archivo.to_string_lossy().to_string());
    args.push("-C".to_string());
    args.push(destino.to_string_lossy().to_string());
    let salida = proceso::ejecutar("tar", &args, &[], LIMITE_TAR)?;
    if !salida.status.success() {
        return Err(format!(
            "tar no pudo extraer {}: {}",
            archivo.display(),
            String::from_utf8_lossy(&salida.stderr).trim()
        ));
    }
    Ok(())
}

/// Busca el ejecutable dentro de lo extraído (el tar.gz trae un nivel de carpeta de
/// más). Se queda con el más superficial para no acabar en un ejemplo o una prueba.
fn buscar_ejecutable(raiz: &Path, nombre: &str) -> Option<PathBuf> {
    let mut cola: VecDeque<(PathBuf, usize)> = VecDeque::new();
    cola.push_back((raiz.to_path_buf(), 0));
    let mut encontrados: Vec<(usize, PathBuf)> = Vec::new();
    while let Some((dir, prof)) = cola.pop_front() {
        if prof > 4 {
            continue;
        }
        let Ok(entradas) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entradas.flatten() {
            let p = e.path();
            if p.is_dir() {
                cola.push_back((p, prof + 1));
            } else if p.file_name().map(|n| n == nombre).unwrap_or(false) {
                encontrados.push((prof, p));
            }
        }
    }
    encontrados.sort();
    encontrados.into_iter().map(|(_, p)| p).next()
}

#[cfg(unix)]
fn marcar_ejecutable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(p)
        .map_err(|e| format!("no se pudo leer los permisos de {p:?}: {e}"))?
        .permissions();
    perm.set_mode(perm.mode() | 0o755);
    std::fs::set_permissions(p, perm).map_err(|e| format!("no se pudo marcar {p:?} como ejecutable: {e}"))
}

#[cfg(not(unix))]
fn marcar_ejecutable(_p: &Path) -> Result<(), String> {
    // En Windows la ejecutabilidad no es un bit del fichero.
    Ok(())
}

/// Borra lo que la app instaló de una herramienta (solo dentro de su carpeta).
fn limpiar_instalado(h: &Herramienta, raiz: &Path) -> Result<(), String> {
    match h.origen {
        Origen::Github { colocacion: Colocacion::BinDir, .. } => {
            let p = dir_bin_en(raiz).join(exe_nombre(h.binarios[0]));
            validar_destino(raiz, &p)?;
            if p.exists() {
                std::fs::remove_file(&p).map_err(|e| format!("no se pudo borrar {p:?}: {e}"))?;
            }
            Ok(())
        }
        Origen::Github { colocacion: Colocacion::ArbolLlama, .. } => {
            let base = dir_llama(raiz);
            validar_destino(raiz, &base)?;
            if base.exists() {
                std::fs::remove_dir_all(&base)
                    .map_err(|e| format!("no se pudo borrar {base:?}: {e}"))?;
            }
            Ok(())
        }
        Origen::Manual { .. } => Ok(()),
    }
}

/// Instala (o reinstala) UNA herramienta. Devuelve un mensaje legible con lo que
/// pasó, incluido dónde quedó y qué versión responde.
fn instalar_una(
    h: &Herramienta,
    raiz: &Path,
    cancel: &AtomicBool,
    publicar: &mut dyn FnMut(&Aviso),
) -> Result<String, String> {
    let (repo, canal, colocacion) = match h.origen {
        Origen::Github { repo, canal, colocacion } => (repo, canal, colocacion),
        Origen::Manual { motivo, .. } => {
            return Err(format!("{motivo} Se instala a mano."));
        }
    };

    avisar(
        publicar,
        Progreso::nuevo(
            h.nombre,
            Fase::Preparando,
            &format!("Buscando la última versión de {repo}"),
        ),
    );

    let cli = cliente()?;
    let url = match canal {
        // El tag de la nocturna cambia cada día: se pide la LISTA y se elige la de
        // mayor número de build. `latest` no vale para llama.cpp porque sus
        // nocturnas son «prerelease».
        Canal::Ultima => format!("https://api.github.com/repos/{repo}/releases/latest"),
        Canal::Nocturna => format!("https://api.github.com/repos/{repo}/releases?per_page=30"),
    };
    let json = traer_texto(&cli, &url)?;
    let release = release_de(&json, canal)
        .ok_or_else(|| format!("no se pudo leer la release de {repo}: formato inesperado"))?;
    let asset = release.elegir(h.id, plataforma::so(), arq()).ok_or_else(|| {
        format!(
            "la versión {} de {repo} no publica un binario para {} {}",
            release.tag,
            plataforma::so(),
            arq()
        )
    })?;

    // Dos fuentes del mismo hash: el `digest` de la API y, si existe, el fichero
    // `.sha256` que publica el proyecto. Si las dos no coinciden, no se instala.
    let mut esperado = asset.digest.as_deref().and_then(hex_del_digest);
    if let Some(sidecar) = release.sidecar(&asset) {
        let texto = traer_texto(&cli, &sidecar.url)?;
        let del_fichero = hash_de_sha256(&texto).ok_or_else(|| {
            format!(
                "el fichero {} no trae un sha256 legible: no se puede verificar",
                sidecar.nombre
            )
        })?;
        if let Some(d) = &esperado {
            if *d != del_fichero {
                return Err(format!(
                    "el hash de la API ({d}) y el de {} ({del_fichero}) no coinciden: no se instala",
                    sidecar.nombre
                ));
            }
        }
        esperado = Some(del_fichero);
    }
    let Some(esperado) = esperado else {
        return Err(format!(
            "{} no publica el sha256 de {}: sin hash no se puede comprobar que sea el fichero \
             publicado, así que no se instala",
            repo, asset.nombre
        ));
    };

    let tmp = dir_tmp(raiz);
    let archivo = tmp.join(&asset.nombre);
    validar_destino(raiz, &archivo)?;
    if archivo.exists() {
        let _ = std::fs::remove_file(&archivo);
    }
    bajar(
        &cli,
        &asset.url,
        &archivo,
        h.nombre,
        &asset.nombre,
        repo,
        cancel,
        publicar,
    )?;

    avisar(
        publicar,
        Progreso::nuevo(
            h.nombre,
            Fase::Verificando,
            &format!("Comprobando el sha256 de {}", asset.nombre),
        ),
    );
    verificar_sha256(&archivo, &esperado)?;

    avisar(
        publicar,
        Progreso::nuevo(h.nombre, Fase::Extrayendo, "Extrayendo el binario"),
    );
    let extra = tmp.join(format!("extraer-{}", h.id.replace(['/', '\\', '.'], "_")));
    let _ = std::fs::remove_dir_all(&extra);
    validar_destino(raiz, &extra)?;
    extraer(&archivo, &extra)?;

    let ruta_final = match colocacion {
        Colocacion::BinDir => {
            let nombre = exe_nombre(h.binarios[0]);
            let origen = buscar_ejecutable(&extra, &nombre).ok_or_else(|| {
                format!("dentro de {} no aparece el ejecutable {nombre}", asset.nombre)
            })?;
            let destino = dir_bin_en(raiz).join(&nombre);
            validar_destino(raiz, &destino)?;
            if let Some(dir) = destino.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {dir:?}: {e}"))?;
            }
            std::fs::copy(&origen, &destino)
                .map_err(|e| format!("no se pudo colocar {destino:?}: {e}"))?;
            marcar_ejecutable(&destino)?;
            destino
        }
        Colocacion::ArbolLlama => {
            // El árbol entero va a `<datos>/machinograph/llama/<tag>`, que es lo que
            // mantiene las bibliotecas al lado de los ejecutables.
            let base = dir_llama(raiz).join(&release.tag);
            validar_destino(raiz, &base)?;
            if base.exists() {
                std::fs::remove_dir_all(&base)
                    .map_err(|e| format!("no se pudo limpiar {base:?}: {e}"))?;
            }
            if let Some(padre) = base.parent() {
                std::fs::create_dir_all(padre)
                    .map_err(|e| format!("no se pudo crear {padre:?}: {e}"))?;
            }
            // `rename` entre la carpeta temporal y la de instalación: están en el
            // mismo sistema de ficheros, así que es atómico y no copia nada.
            std::fs::rename(&extra, &base).map_err(|e| {
                format!("no se pudo mover lo extraído a {base:?}: {e}")
            })?;
            // Los permisos de ejecución vienen del tar, pero se fuerzan por si acaso
            // y porque es barato: un binario sin el bit +x es justo lo que no arranca.
            for nombre in h.binarios {
                if let Some(p) = buscar_ejecutable(&base, &exe_nombre(nombre)) {
                    marcar_ejecutable(&p)?;
                }
            }
            base
        }
    };
    let _ = std::fs::remove_file(&archivo);
    let _ = std::fs::remove_dir_all(&extra);

    // EL PASO QUE DE VERDAD COMPRUEBA: se ejecuta, con la bandera que ESA
    // herramienta entiende. Un fichero que existe pero no arranca está roto, y
    // entonces se borra y se dice, en vez de dejar un binario muerto que la app
    // intentará usar para siempre.
    let (_, bin) = h
        .binarios
        .iter()
        .find_map(|n| encontrar(raiz, n).map(|p| (*n, p)))
        .ok_or_else(|| "lo instalado no aparece en las rutas de búsqueda".to_string())?;
    if !ruta_final.exists() && !bin.exists() {
        return Err("lo instalado no llegó a su sitio".into());
    }
    match ejecutar_bin(&bin, h.comprobar) {
        Ok(salida) => {
            let v = leer_version(h, &bin, &salida)
                .unwrap_or_else(|| "versión sin identificar".into());
            Ok(format!(
                "{} listo ({v}) en {}",
                h.nombre,
                ruta_final.display()
            ))
        }
        Err(e) => {
            limpiar_instalado(h, raiz)?;
            Err(format!(
                "lo instalado no arranca ({e}); se ha descartado y se volverá a intentar"
            ))
        }
    }
}

/// El trabajo de instalación, secuencial y cancelable.
///
/// Se instala de UNA en UNA a propósito: cada descarga emite su progreso y compite
/// por el mismo ancho de banda, así que hacerlas a la vez daría dos números malos y
/// una interfaz confusa. En total son ~25 MB.
fn trabajar(
    raiz: &Path,
    ids: &[String],
    forzar: bool,
    cancel: &AtomicBool,
    publicar: &mut dyn FnMut(&Aviso),
) -> Vec<(String, Result<String, String>)> {
    let mut resultados = Vec::new();
    for h in catalogo() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        if !ids.is_empty() && !ids.contains(&h.id.to_string()) {
            continue;
        }
        let instalable = matches!(h.origen, Origen::Github { .. });
        let est = estado_de(&h, raiz);
        let decision = decidir(est.estado, instalable, forzar);
        if decision == Decision::Saltar {
            continue;
        }
        if decision == Decision::Reparar {
            // Lo que está roto se aparta antes de volver a bajarlo: si no, queda
            // un fichero a medias en el sitio donde la app lo busca.
            let _ = limpiar_instalado(&h, raiz);
        }
        let nombre = h.nombre.to_string();
        match instalar_una(&h, raiz, cancel, publicar) {
            Ok(msg) => {
                publicar(&Aviso {
                    en_curso: None,
                    ultimo: Some(Progreso {
                        herramienta: nombre.clone(),
                        fase: Fase::Terminada,
                        linea: msg.clone(),
                        pct: Some(100.0),
                        bajado_bytes: 0,
                        total_bytes: 0,
                        b_s: None,
                        eta_s: None,
                        error: None,
                    }),
                });
                resultados.push((nombre, Ok(msg)));
            }
            Err(e) => {
                let fase = if cancel.load(Ordering::SeqCst) {
                    Fase::Cancelada
                } else {
                    Fase::Fallida
                };
                publicar(&Aviso {
                    en_curso: None,
                    ultimo: Some(Progreso {
                        herramienta: nombre.clone(),
                        fase,
                        linea: e.clone(),
                        pct: None,
                        bajado_bytes: 0,
                        total_bytes: 0,
                        b_s: None,
                        eta_s: None,
                        error: Some(e.clone()),
                    }),
                });
                resultados.push((nombre, Err(e)));
            }
        }
    }
    resultados
}

/* ── Entradas públicas ────────────────────────────────────────────────────── */

/// Lanza la instalación en segundo plano y vuelve enseguida.
///
/// `ids` vacío = todas las que lo necesiten. `forzar` = reinstalar aunque estén
/// bien. Lo que no haga falta se SALTA: no se baja nada que ya funcione.
pub fn instalar(app: Option<AppHandle>, ids: Vec<String>, forzar: bool) -> Result<String, String> {
    {
        let mut g = GLOBAL.lock();
        if g.ocupado {
            return Err("Ya hay una instalación en curso. Cancélala o espera a que termine.".into());
        }
        g.ocupado = true;
        g.cancelar = Arc::new(AtomicBool::new(false));
    }
    let cancel = GLOBAL.lock().cancelar.clone();
    let raiz = raiz_gestion();
    std::fs::create_dir_all(&raiz).map_err(|e| format!("no se pudo crear {raiz:?}: {e}"))?;

    std::thread::spawn(move || {
        let mut publicar = |a: &Aviso| {
            registrar(a);
            if let Some(app) = &app {
                let _ = app.emit(EVENTO, a);
            }
        };
        let resultados = trabajar(&raiz, &ids, forzar, &cancel, &mut publicar);
        let ok = resultados.iter().filter(|(_, r)| r.is_ok()).count();
        let mal = resultados.len() - ok;
        // Un último aviso con el resumen, aunque no haya nada instalado (así la
        // interfaz vuelve a pedir el estado).
        publicar(&Aviso {
            en_curso: None,
            ultimo: Some(Progreso {
                herramienta: String::new(),
                fase: if mal == 0 { Fase::Terminada } else { Fase::Fallida },
                linea: if resultados.is_empty() {
                    "No faltaba nada por instalar.".into()
                } else {
                    format!("{ok} instalada(s), {mal} con problemas")
                },
                pct: None,
                bajado_bytes: 0,
                total_bytes: 0,
                b_s: None,
                eta_s: None,
                error: None,
            }),
        });
        GLOBAL.lock().ocupado = false;
    });
    Ok("Instalando lo que falta. Se ve el progreso en la tarjeta.".into())
}

/// Igual que `instalar`, pero BLOQUEANTE y con la salida línea a línea: es lo que
/// usa el modo `--cli`, donde no hay ventana ni eventos.
pub fn instalar_bloqueante(
    ids: Vec<String>,
    forzar: bool,
    mut linea: impl FnMut(&str),
) -> Vec<(String, Result<String, String>)> {
    {
        let mut g = GLOBAL.lock();
        if g.ocupado {
            return vec![("(todas)".into(), Err("Ya hay una instalación en curso.".into()))];
        }
        g.ocupado = true;
        g.cancelar = Arc::new(AtomicBool::new(false));
    }
    let cancel = GLOBAL.lock().cancelar.clone();
    let raiz = raiz_gestion();
    if let Err(e) = std::fs::create_dir_all(&raiz) {
        GLOBAL.lock().ocupado = false;
        return vec![("(todas)".into(), Err(format!("no se pudo crear {raiz:?}: {e}")))];
    }
    let mut publicar = |a: &Aviso| {
        registrar(a);
        if let Some(p) = a.en_curso.as_ref().or(a.ultimo.as_ref()) {
            if !p.herramienta.is_empty() || !p.linea.is_empty() {
                linea(&format!("{} · {}", p.herramienta, p.linea));
            }
        }
    };
    let r = trabajar(&raiz, &ids, forzar, &cancel, &mut publicar);
    GLOBAL.lock().ocupado = false;
    r
}

/// Revisa al arrancar: si algo falta o está roto, lo deja instalado.
///
/// Respeta el ajuste `auto_provision` (por defecto activado): con el ajuste
/// apagado NO se baja nada solo, la tarjeta de Ajustes lo ofrece con un botón.
pub fn arranque(app: Option<AppHandle>) {
    if !auto_activo() {
        return;
    }
    let raiz = raiz_gestion();
    let pendientes: Vec<String> = catalogo()
        .iter()
        .filter(|h| {
            let e = estado_de(h, &raiz);
            let instalable = matches!(h.origen, Origen::Github { .. });
            matches!(
                decidir(e.estado, instalable, false),
                Decision::Instalar | Decision::Reparar
            )
        })
        .map(|h| h.id.to_string())
        .collect();
    if pendientes.is_empty() {
        return;
    }
    // El error no se descarta: queda en el aviso de la propia tarjeta, que es donde
    // se puede volver a intentar.
    let _ = instalar(app, pendientes, false);
}

/// Repara solo lo que está roto (y lo que falte), sin tocar lo que funciona. Es la
/// acción manual «Comprobar ahora».
pub fn reparar(app: Option<AppHandle>) -> Result<String, String> {
    let raiz = raiz_gestion();
    let ids: Vec<String> = catalogo()
        .iter()
        .filter(|h| {
            let instalable = matches!(h.origen, Origen::Github { .. });
            matches!(
                decidir(estado_de(h, &raiz).estado, instalable, false),
                Decision::Instalar | Decision::Reparar
            )
        })
        .map(|h| h.id.to_string())
        .collect();
    if ids.is_empty() {
        return Ok("Todo lo que se puede instalar sola está listo y arranca.".into());
    }
    instalar(app, ids, false)
}

/// La ruta gestionada de un binario, si la app lo tiene instalado y arranca.
///
/// La usa `llmfit::binario()`: sin esto, la app se bajaría llmfit y luego diría que
/// no está, que es la peor de las fricciones.
pub fn binario_gestionado(nombre: &str) -> Option<String> {
    let p = dir_bin_en(&raiz_gestion()).join(exe_nombre(nombre));
    p.is_file().then(|| p.to_string_lossy().to_string())
}

#[cfg(test)]
mod pruebas {
    use super::sha256::de_bytes as sha256_bytes;
    use super::*;

    const FIXTURE_LLMFIT: &str = include_str!("../fixtures/llmfit_release.json");
    const FIXTURE_LLAMA: &str = include_str!("../fixtures/llamacpp_release.json");
    const FIXTURE_LLAMA_LISTA: &str = include_str!("../fixtures/llamacpp_releases.json");

    /// Una ruta absoluta VÁLIDA en el sistema que corre.
    ///
    /// `validar_destino` exige que la raíz sea absoluta (`Path::is_absolute()`), y
    /// `/home/alguien/...` NO lo es en Windows (allí lo es `C:\...`). El ayudante
    /// pone la raíz de cada sistema para que la prueba valga en los tres.
    fn abs(p: &str) -> String {
        if cfg!(windows) {
            format!("C:/{}", p.trim_start_matches('/'))
        } else {
            format!("/{}", p.trim_start_matches('/'))
        }
    }

    /* ── SHA-256 ─────────────────────────────────────────────────────────── */

    /// Vectores de la FIPS 180-4, los de manual. Si el algoritmo estuviera mal,
    /// esto no podría pasar.
    #[test]
    fn el_sha256_da_los_vectores_conocidos() {
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_bytes(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// Un fichero, no solo bytes en memoria: es lo que se verifica de verdad.
    #[test]
    fn el_sha256_de_un_fichero_coincide_con_sus_bytes() {
        let p = std::env::temp_dir().join(format!("machinograph-sha-{}", std::process::id()));
        std::fs::write(&p, b"abc").expect("escribir");
        assert_eq!(sha256_archivo(&p).unwrap(), sha256_bytes(b"abc"));
        let _ = std::fs::remove_file(&p);
    }

    /* ── Elección de asset (JSON real como fixture) ──────────────────────── */

    /// LAS SEIS COMBINACIONES de llmfit, con el JSON REAL de v1.1.16.
    #[test]
    fn elige_el_asset_de_llmfit_en_las_seis_combinaciones() {
        let casos = [
            ("linux", "x64", "llmfit-v1.1.16-x86_64-unknown-linux-gnu.tar.gz"),
            ("linux", "arm64", "llmfit-v1.1.16-aarch64-unknown-linux-gnu.tar.gz"),
            ("macos", "x64", "llmfit-v1.1.16-x86_64-apple-darwin.tar.gz"),
            ("macos", "arm64", "llmfit-v1.1.16-aarch64-apple-darwin.tar.gz"),
            ("windows", "x64", "llmfit-v1.1.16-x86_64-pc-windows-msvc.zip"),
            ("windows", "arm64", "llmfit-v1.1.16-aarch64-pc-windows-msvc.zip"),
        ];
        for (so, arq, esperado) in casos {
            let a = elegir_asset("llmfit", FIXTURE_LLMFIT, so, arq)
                .unwrap_or_else(|| panic!("sin asset para {so} {arq}"));
            assert_eq!(a.nombre, esperado, "para {so} {arq}");
            assert!(a.url.starts_with("https://github.com/AlexsJones/llmfit/releases/download/"));
            assert!(a.tamano > 1_000_000, "el tamaño tiene que venir de la API");
            assert_eq!(
                hex_del_digest(a.digest.as_deref().unwrap()).map(|h| h.len()),
                Some(64)
            );
        }
        // Lo que no existe no se inventa.
        assert!(elegir_asset("llmfit", FIXTURE_LLMFIT, "linux", "riscv64").is_none());
        assert!(elegir_asset("llmfit", FIXTURE_LLMFIT, "plan9", "x64").is_none());
    }

    /// En Linux se prefiere glibc y solo si no estuviera se usaría musl.
    #[test]
    fn en_linux_prefiere_glibc_sobre_musl() {
        let elegido = elegir_asset("llmfit", FIXTURE_LLMFIT, "linux", "x64").unwrap();
        assert!(elegido.nombre.contains("gnu"), "{}", elegido.nombre);
        assert!(!elegido.nombre.contains("musl"), "{}", elegido.nombre);
    }

    /// LAS SEIS COMBINACIONES de llama.cpp, con el JSON REAL de b11375.
    #[test]
    fn elige_el_asset_de_llama_cpp_en_las_seis_combinaciones() {
        let casos = [
            ("linux", "x64", "llama-b11375-bin-ubuntu-x64.tar.gz"),
            ("linux", "arm64", "llama-b11375-bin-ubuntu-arm64.tar.gz"),
            ("macos", "x64", "llama-b11375-bin-macos-x64.tar.gz"),
            ("macos", "arm64", "llama-b11375-bin-macos-arm64.tar.gz"),
            ("windows", "x64", "llama-b11375-bin-win-cpu-x64.zip"),
            ("windows", "arm64", "llama-b11375-bin-win-cpu-arm64.zip"),
        ];
        for (so, arq, esperado) in casos {
            let a = elegir_asset("llama.cpp", FIXTURE_LLAMA, so, arq)
                .unwrap_or_else(|| panic!("sin asset para {so} {arq}"));
            assert_eq!(a.nombre, esperado, "para {so} {arq}");
            assert!(!a.nombre.contains("cuda"), "no se elige una build de CUDA a ciegas");
            assert!(!a.nombre.contains("vulkan"), "no se elige una build de Vulkan a ciegas");
        }
    }

    /// De la LISTA de releases (la real, con `b11375`, `b11374`…) se elige la de
    /// mayor número de build, no la primera ni la última por fecha.
    #[test]
    fn la_nocturna_elegida_es_la_de_mayor_numero_de_build() {
        let r = release_de(FIXTURE_LLAMA_LISTA, Canal::Nocturna).expect("tiene que elegir una");
        assert_eq!(r.tag, "b11375");
        // Y una release con tag «de verdad» (v0.5.0) NO cuenta como nocturna.
        assert!(build_nocturno("v0.5.0").is_none());
        assert_eq!(build_nocturno("b11375"), Some(11375));
        assert!(build_nocturno("b").is_none());
        assert!(build_nocturno("b11x").is_none());
    }

    /// El objeto de `/releases/latest` (llmfit) también se lee.
    #[test]
    fn la_release_ultima_se_lee_del_objeto() {
        let r = release_de(FIXTURE_LLMFIT, Canal::Ultima).expect("tiene que leerla");
        assert_eq!(r.tag, "v1.1.16");
        assert_eq!(r.assets.len(), 18);
    }

    /* ── Verificación ────────────────────────────────────────────────────── */

    /// El `.sha256` real de llmfit: `<hex>  <nombre>`.
    #[test]
    fn lee_el_hash_del_fichero_de_sumas() {
        let real = "27fad93d5e579156e4d87609e1bdef51675342cef3da111ef0f58e1ec10be7f1  llmfit-v1.1.16-x86_64-unknown-linux-gnu.tar.gz\n";
        assert_eq!(
            hash_de_sha256(real).as_deref(),
            Some("27fad93d5e579156e4d87609e1bdef51675342cef3da111ef0f58e1ec10be7f1")
        );
        // Un fichero truncado o con otra cosa no vale como verificación.
        assert!(hash_de_sha256("no soy un hash").is_none());
        assert!(hash_de_sha256("27fad93d").is_none());
        assert!(hash_de_sha256("").is_none());
    }

    /// Un sha256 que no coincide CORTA la instalación: no se extrae nada.
    #[test]
    fn un_sha256_que_no_cuadra_se_rechaza() {
        let p = std::env::temp_dir().join(format!("machinograph-verif-{}", std::process::id()));
        std::fs::write(&p, b"contenido de verdad").expect("escribir");

        let bueno = sha256_archivo(&p).unwrap();
        assert!(verificar_sha256(&p, &bueno).is_ok());
        // Con el hash de otro fichero (el de "abc"), tiene que fallar y decir los dos.
        let malo = sha256_bytes(b"abc");
        let e = verificar_sha256(&p, &malo).expect_err("tiene que rechazarlo");
        assert!(e.contains(&bueno), "el motivo dice el real: {e}");
        assert!(e.contains(&malo), "el motivo dice el esperado: {e}");
        let _ = std::fs::remove_file(&p);
    }

    /* ── Decisión y rutas ────────────────────────────────────────────────── */

    /// EL CASO «ROTO -> VOLVER A BAJAR», que es la autorreparación.
    #[test]
    fn lo_roto_se_vuelve_a_bajar_y_lo_listo_no() {
        assert_eq!(decidir(Estado::Roto, true, false), Decision::Reparar);
        assert_eq!(decidir(Estado::Falta, true, false), Decision::Instalar);
        assert_eq!(decidir(Estado::Listo, true, false), Decision::Saltar);
        // Forzar sí reinstala aunque esté bien (lo pide el usuario a mano).
        assert_eq!(decidir(Estado::Listo, true, true), Decision::Instalar);
        // Lo que no se puede instalar sola nunca se toca.
        assert_eq!(decidir(Estado::NoInstalable, false, true), Decision::Saltar);
        assert_eq!(decidir(Estado::Roto, false, false), Decision::Saltar);
        // Y lo que está en curso no se pisa.
        assert_eq!(decidir(Estado::Descargando, true, true), Decision::Saltar);
    }

    /// NADA se instala fuera de la carpeta de datos del usuario.
    #[test]
    fn una_instalacion_fuera_del_directorio_de_datos_se_rechaza() {
        let raiz_s = abs("/home/alguien/.local/share/machinograph");
        let raiz = Path::new(&raiz_s);
        assert!(validar_destino(raiz, &raiz.join("bin").join("llmfit")).is_ok());
        assert!(validar_destino(raiz, &raiz.join("llama").join("b1").join("llama-bench")).is_ok());
        // Fuera de ahí, no.
        assert!(validar_destino(raiz, Path::new(&abs("/usr/local/bin/llmfit"))).is_err());
        assert!(validar_destino(raiz, Path::new(&abs("/home/alguien/.local/bin/llmfit"))).is_err());
        assert!(validar_destino(raiz, Path::new(&abs("/tmp/llmfit"))).is_err());
        // La propia raíz tampoco vale como destino de un fichero.
        assert!(validar_destino(raiz, raiz).is_err());
        // Y una raíz relativa se rechaza siempre: podría acabar en cualquier sitio.
        assert!(validar_destino(Path::new("machinograph"), Path::new("machinograph/bin/llmfit")).is_err());
    }

    /// El orden de búsqueda: lo que ha instalado la app va PRIMERO.
    #[test]
    fn lo_que_instala_la_app_se_busca_antes_que_nada() {
        let raiz = Path::new("/datos/machinograph");
        let c = candidatos_bin(raiz, "llmfit");
        assert_eq!(c[0], raiz.join("bin").join(exe_nombre("llmfit")));
        // Y el primer candidato que existe de verdad es el que se coge.
        let a = std::env::temp_dir().join(format!("machinograph-cand-a-{}", std::process::id()));
        let b = std::env::temp_dir().join(format!("machinograph-cand-b-{}", std::process::id()));
        std::fs::write(&b, b"existe").expect("escribir");
        assert_eq!(primer_existente(&[a.clone(), b.clone()]), Some(b.clone()));
        assert_eq!(primer_existente(&[a, b.clone()]).is_none(), false);
        let _ = std::fs::remove_file(&b);
    }

    /* ── Versiones ───────────────────────────────────────────────────────── */

    /// Los formatos REALES, copiados de los binarios.
    #[test]
    fn lee_las_versiones_con_su_formato_real() {
        assert_eq!(version_de("llmfit", "llmfit 1.1.16").as_deref(), Some("1.1.16"));
        assert_eq!(
            version_de(
                "llama.cpp",
                "load_backend: loaded CPU backend from /x/libggml-cpu.so\nversion: 0.5.0-dev (build 11375, commit 436f6f89e)"
            )
            .as_deref(),
            Some("b11375")
        );
        // Si el formato cambiara, se dice «no lo sé», no un número inventado.
        assert!(version_de("llmfit", "otra cosa").is_none());
        assert!(version_de("llama.cpp", "sin la palabra").is_none());
    }

    /// EL CASO QUE APARECIÓ AL PROBAR EN ESTA MÁQUINA: el `llama-bench` de
    /// `~/.local/bin` rechaza `--version` («error: invalid parameter for argument»)
    /// pero mide perfectamente. Sondeándolo con `--version` se marcaba «roto» y la
    /// app se bajaba 17 MB sin ningún motivo. Por eso la sonda de llama.cpp es
    /// `--help`; aquí se comprueba con un binario de mentira que imita justo eso.
    #[cfg(unix)]
    #[test]
    fn un_binario_que_no_acepta_version_no_esta_roto() {
        use std::os::unix::fs::PermissionsExt;
        let raiz = std::env::temp_dir().join(format!("machinograph-sonda-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let bin_dir = raiz.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("crear");
        let guion = bin_dir.join("llama-bench");
        let escribir_guion = |contenido: &str| {
            std::fs::write(&guion, contenido).expect("escribir");
            let mut perm = std::fs::metadata(&guion).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&guion, perm).unwrap();
        };

        let h = Herramienta {
            id: "llama.cpp",
            nombre: "prueba",
            para_que: "prueba",
            imprescindible: false,
            binarios: &["llama-bench"],
            comprobar: &["--help"],
            version: Some(&["--version"]),
            origen: Origen::Github {
                repo: "ggml-org/llama.cpp",
                canal: Canal::Nocturna,
                colocacion: Colocacion::ArbolLlama,
            },
        };

        // Responde a --help y rechaza --version: está LISTO (sin versión legible).
        escribir_guion(
            "#!/bin/sh\ncase \"$1\" in\n  --help) echo 'uso: llama-bench'; exit 0;;\n  *) echo 'error: invalid parameter' >&2; exit 1;;\nesac\n",
        );
        let e = estado_de(&h, &raiz);
        assert_eq!(e.estado, Estado::Listo, "un binario que mide no está roto: {e:?}");
        assert!(e.version.is_none(), "no se inventa la versión: {e:?}");

        // Si ni siquiera arranca, eso SÍ es estar roto.
        escribir_guion("#!/bin/sh\nexit 3\n");
        let e = estado_de(&h, &raiz);
        assert_eq!(e.estado, Estado::Roto, "{e:?}");
        assert!(e.detalle.is_some());
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /* ── Ajuste ──────────────────────────────────────────────────────────── */

    /// El auto por defecto está encendido, y se puede apagar y volver a leer.
    #[test]
    fn el_ajuste_de_autoinstalacion_se_guarda_y_por_defecto_esta_activo() {
        let raiz = std::env::temp_dir().join(format!("machinograph-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(leer_config_en(&raiz).auto_provision, "por defecto SÍ");
        escribir_config_en(&raiz, &Config { auto_provision: false }).expect("escribir");
        assert!(!leer_config_en(&raiz).auto_provision);
        escribir_config_en(&raiz, &Config { auto_provision: true }).expect("escribir");
        assert!(leer_config_en(&raiz).auto_provision);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// El comando del paquete manual se elige por el gestor, y sin gestor conocido
    /// no se inventa uno. Están los de los tres sistemas: Linux (dnf/apt/pacman/
    /// zypper/rpm-ostree), macOS (brew) y Windows (winget/choco).
    #[test]
    fn el_comando_del_paquete_manual_usa_el_gestor_que_hay() {
        assert_eq!(comando_para("amdsmi", Some("dnf")), "sudo dnf install amdsmi");
        assert_eq!(comando_para("amdsmi", Some("apt")), "sudo apt install amdsmi");
        assert_eq!(comando_para("amdsmi", Some("pacman")), "sudo pacman -S amdsmi");
        assert_eq!(comando_para("amdsmi", Some("zypper")), "sudo zypper install amdsmi");
        assert!(comando_para("amdsmi", Some("rpm-ostree")).starts_with("rpm-ostree install"));
        assert_eq!(comando_para("amdsmi", Some("brew")), "brew install amdsmi");
        assert_eq!(comando_para("amdsmi", Some("winget")), "winget install amdsmi");
        assert_eq!(comando_para("amdsmi", Some("choco")), "choco install amdsmi");
        assert!(comando_para("amdsmi", None).contains("amdsmi"));
        assert!(comando_para("amdsmi", None).contains("gestor"));
    }

    /// El catálogo tiene las tres herramientas del proyecto, y `amd-smi` solo
    /// aparece donde existe.
    #[test]
    fn el_catalogo_es_el_que_la_app_usa() {
        let c = catalogo();
        let ids: Vec<&str> = c.iter().map(|h| h.id).collect();
        assert!(ids.contains(&"llmfit"));
        assert!(ids.contains(&"llama.cpp"));
        if plataforma::so() == "linux" {
            assert!(ids.contains(&"amd-smi"));
        } else {
            assert!(!ids.contains(&"amd-smi"), "en este sistema no existe amd-smi");
        }
        // llmfit es imprescindible; lo demás, no.
        let llmfit = c.iter().find(|h| h.id == "llmfit").unwrap();
        assert!(llmfit.imprescindible);
    }

    /* ── Punta a punta, de verdad (a mano) ───────────────────────────────── */

    /// LA PRUEBA QUE DEMUESTRA QUE EL MECANISMO FUNCIONA: baja el asset REAL de
    /// llmfit para este sistema, comprueba su sha256, lo extrae, lo ejecuta y
    /// comprueba que responde.
    ///
    /// Está marcada `#[ignore]` a propósito: `cargo test` no debe bajarse MB de
    /// internet ni tocar la red. Se lanza a mano:
    ///
    /// ```bash
    /// cd src-tauri && cargo test -- --ignored provision_real
    /// ```
    ///
    /// Instala en un directorio TEMPORAL, no en la carpeta de datos del usuario:
    /// una prueba no escribe en los datos de nadie.
    #[test]
    #[ignore = "baja ~7 MB de internet: se lanza a mano"]
    fn provision_real_instala_llmfit_y_comprueba_que_arranca() {
        let raiz = std::env::temp_dir().join(format!("machinograph-provision-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let h = herramienta("llmfit").expect("llmfit tiene que estar en el catálogo");
        let cancel = AtomicBool::new(false);
        let mut nada = |_: &Aviso| {};
        let r = instalar_una(&h, &raiz, &cancel, &mut nada);
        assert!(r.is_ok(), "{r:?}");

        // Y se COMPRUEBA ejecutándolo, que es el único veredicto que vale.
        let bin = raiz.join("bin").join(exe_nombre("llmfit"));
        assert!(bin.is_file(), "no quedó el binario en {bin:?}");
        let salida = ejecutar_bin(&bin, &["--version"]).expect("tiene que arrancar");
        assert!(salida.contains("llmfit"), "dijo: {salida}");
        println!("llmfit instalado y funcionando: {salida}");

        // Y el estado del catálogo lo ve como listo, con su versión.
        let e = estado_de(&h, &raiz);
        assert_eq!(e.estado, Estado::Listo, "{e:?}");
        assert!(e.version.is_some(), "{e:?}");

        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// La misma prueba para llama.cpp, que recorre el OTRO camino: el árbol entero
    /// se guarda para que los ejecutables tengan sus bibliotecas `.so` al lado, y
    /// `perf::runtimes()` tiene que encontrarlo ahí (por eso se comprueba
    /// `dirs_llama_en`, que es justo lo que consulta `perf`).
    ///
    /// ```bash
    /// cd src-tauri && cargo test -- --ignored provision_real
    /// ```
    #[test]
    #[ignore = "baja ~17 MB de internet: se lanza a mano"]
    fn provision_real_instala_llama_cpp_y_comprueba_que_arranca() {
        let raiz =
            std::env::temp_dir().join(format!("machinograph-provision-llama-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let h = herramienta("llama.cpp").expect("llama.cpp tiene que estar en el catálogo");
        let cancel = AtomicBool::new(false);
        let mut nada = |_: &Aviso| {};
        let r = instalar_una(&h, &raiz, &cancel, &mut nada);
        assert!(r.is_ok(), "{r:?}");

        // `perf` mira aquí: si no se ve, la app no mediría aunque estuviera bajado.
        let dirs = dirs_llama_en(&raiz);
        assert!(!dirs.is_empty(), "no se encontró el árbol de llama.cpp instalado");
        let bench = dirs[0].join(exe_nombre("llama-bench"));
        assert!(bench.is_file(), "{bench:?}");
        let salida = ejecutar_bin(&bench, &["--version"]).expect("llama-bench tiene que arrancar");
        assert!(salida.contains("version:"), "dijo: {salida}");
        println!("llama-bench instalado y funcionando: {}", salida.trim());

        let e = estado_de(&h, &raiz);
        assert_eq!(e.estado, Estado::Listo, "{e:?}");
        assert!(e.version.is_some(), "{e:?}");

        let _ = std::fs::remove_dir_all(&raiz);
    }
}
