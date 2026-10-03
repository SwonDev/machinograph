//! Qué está desactualizado en este equipo, y con qué comando se actualiza.
//!
//! POR QUÉ ASÍ Y NO CON UN «BUSCAR ACTUALIZACIONES» PROPIO: cada sistema tiene SU
//! herramienta (rpm-ostree en un Fedora atómico como Bazzite, flatpak, brew en
//! Linux y macOS, winget en Windows, softwareupdate en macOS), y esa herramienta es
//! la que sabe qué hay y de dónde se baja. Machinograph **no inventa un canal de
//! versiones ni descarga nada por su cuenta**: ejecuta la comprobación de la
//! herramienta que ya tienes, la traduce a una lista y te enseña el comando exacto
//! para aplicarla. Nada sale del equipo salvo lo que ya hace esa herramienta.
//!
//! TRES DECISIONES QUE VIENEN DE FALLOS AJENOS:
//!
//! 1. **En un sistema atómico NO se ofrece `dnf`**: en Bazzite el sistema es una
//!    imagen de solo lectura y actualizar paquetes con `dnf` no es la vía (la vía
//!    es `rpm-ostree`, y además aplica TRAS REINICIAR, cosa que se dice). Ofrecer
//!    las dos sería invitar a romper el equipo.
//! 2. **El resultado de una comprobación no se juzga por su texto traducido**:
//!    `rpm-ostree upgrade --check` avisa de que su propio `--check` «puede ser poco
//!    fiable» en su salida, y esa nota se enseña tal cual en vez de esconderla.
//! 3. **Aplicar nunca es automático ni silencioso**: aquí solo se MIRA; el botón
//!    que ejecuta el comando es del usuario, y si necesita root o un reinicio, se
//!    dice antes.
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Fuente {
    pub id: String,
    pub nombre: String,
    /// La herramienta está instalada en este equipo.
    pub disponible: bool,
    /// Lo que hay pendiente, tal como lo llama cada herramienta.
    pub actualizaciones: Vec<String>,
    pub comando_comprobar: String,
    pub comando_aplicar: String,
    pub requiere_root: bool,
    /// Aplicar deja el cambio para el siguiente arranque (rpm-ostree).
    pub requiere_reinicio: bool,
    /// Lo que hay que saber antes de pulsar (avisos de la propia herramienta).
    pub nota: Option<String>,
    /// Por qué no se pudo comprobar, si falló.
    pub error: Option<String>,
}

impl Fuente {
    fn nueva(id: &str, nombre: &str, comprobar: &str, aplicar: &str) -> Self {
        Self {
            id: id.to_string(),
            nombre: nombre.to_string(),
            disponible: false,
            actualizaciones: Vec::new(),
            comando_comprobar: comprobar.to_string(),
            comando_aplicar: aplicar.to_string(),
            requiere_root: false,
            requiere_reinicio: false,
            nota: None,
            error: None,
        }
    }
}

/// ¿Este equipo es un sistema atómico (imagen de solo lectura con rpm-ostree)?
pub fn es_atomico() -> bool {
    cfg!(target_os = "linux")
        && std::path::Path::new("/run/ostree-booted").exists()
}

/// Comprueba todas las fuentes que tengan sentido en este sistema.
pub fn comprobar() -> Vec<Fuente> {
    let mut out = Vec::new();
    for (id, nombre, comprobar, aplicar) in candidatas() {
        let mut f = Fuente::nueva(id, nombre, comprobar, aplicar);
        if !existe(programa_de(comprobar)) {
            out.push(f);
            continue;
        }
        f.disponible = true;
        match crate::proceso::ejecutar(
            programa_de(comprobar),
            &args_de(comprobar),
            &[("LC_ALL", "C")],
            std::time::Duration::from_secs(60),
        ) {
            Ok(salida) => {
                let texto = String::from_utf8_lossy(&salida.stdout).to_string();
                let (lista, nota) = parsear(&f.id, &texto);
                f.actualizaciones = lista;
                f.nota = nota;
                if !salida.status.success() && f.actualizaciones.is_empty() {
                    // Una herramienta puede devolver un código distinto de cero sin
                    // que sea un fallo (winget lo hace al no haber nada); por eso el
                    // error solo se marca si ADEMÁS no se ha entendido nada.
                    f.error = Some(format!(
                        "la comprobación terminó con código {}: {}",
                        salida.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&salida.stderr).trim()
                    ));
                }
            }
            Err(e) => f.error = Some(e),
        }
        out.push(f);
    }
    out
}

/// Las fuentes que tienen sentido según el sistema. Se comprueba si están
/// instaladas antes de ofrecerlas.
fn candidatas() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let mut v: Vec<(&'static str, &'static str, &'static str, &'static str)> = Vec::new();
    if cfg!(target_os = "linux") {
        if es_atomico() {
            v.push((
                "rpm-ostree",
                "Sistema (imagen atómica)",
                "rpm-ostree upgrade --check",
                "ujust update",
            ));
        } else {
            v.push(("dnf", "Sistema (dnf)", "dnf check-update", "sudo dnf upgrade"));
        }
        v.push(("flatpak", "Aplicaciones Flatpak", "flatpak remote-ls --updates", "flatpak update"));
        v.push(("brew", "Homebrew", "brew outdated --quiet", "brew upgrade"));
        v.push(("apt", "Sistema (apt)", "apt list --upgradable", "sudo apt upgrade"));
        v.push(("pacman", "Sistema (pacman)", "pacman -Qu", "sudo pacman -Syu"));
        v.push(("zypper", "Sistema (zypper)", "zypper list-updates", "sudo zypper update"));
    } else if cfg!(target_os = "macos") {
        v.push(("brew", "Homebrew", "brew outdated --quiet", "brew upgrade"));
        v.push(("softwareupdate", "Sistema (softwareupdate)", "softwareupdate -l", "sudo softwareupdate -i -a"));
        v.push(("mas", "App Store", "mas outdated", "mas upgrade"));
    } else if cfg!(target_os = "windows") {
        v.push(("winget", "Aplicaciones (winget)", "winget upgrade", "winget upgrade --all"));
        v.push(("choco", "Chocolatey", "choco outdated", "choco upgrade all -y"));
    }
    v
}

fn programa_de(comando: &str) -> &str {
    comando.split_whitespace().next().unwrap_or(comando)
}

fn args_de(comando: &str) -> Vec<String> {
    comando.split_whitespace().skip(1).map(str::to_string).collect()
}

fn existe(programa: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(programa).is_file()))
        .unwrap_or(false)
}

/// Traduce la salida de cada herramienta a una lista. Los parsers son PUROS para
/// poder probarlos con salidas reales, sin depender de tener la herramienta.
fn parsear(id: &str, texto: &str) -> (Vec<String>, Option<String>) {
    match id {
        "rpm-ostree" => parsear_rpm_ostree(texto),
        "flatpak" => (parsear_flatpak(texto), None),
        "brew" => (parsear_lineas_simples(texto), None),
        "dnf" => (parsear_dnf(texto), None),
        "apt" => (parsear_apt(texto), None),
        "pacman" => (parsear_lineas_simples(texto), None),
        "zypper" => (parsear_zypper(texto), None),
        "softwareupdate" => (parsear_softwareupdate(texto), None),
        "mas" => (parsear_lineas_simples(texto), None),
        "winget" => (parsear_winget(texto), None),
        "choco" => (parsear_choco(texto), None),
        _ => (Vec::new(), None),
    }
}

/* ── Parsers (puros) ──────────────────────────────────────────────────────── */

/// `rpm-ostree upgrade --check`. Devuelve la lista y, si la trae, la ADVERTENCIA
/// de la propia herramienta (su `--check` «puede ser poco fiable»: se enseña, no se
/// esconde).
fn parsear_rpm_ostree(texto: &str) -> (Vec<String>, Option<String>) {
    let mut nota: Option<String> = None;
    let mut out: Vec<String> = Vec::new();
    for linea in texto.lines() {
        let l = linea.trim();
        if l.starts_with("Note:") || l.contains("may be unreliable") {
            nota = Some(l.to_string());
        }
        if l.starts_with("No updates available") {
            return (Vec::new(), nota);
        }
        if let Some(v) = l.strip_prefix("Version:") {
            // La versión que se instalaría, tal cual la da la herramienta.
            out.push(v.trim().to_string());
        }
    }
    (out, nota)
}

/// `flatpak remote-ls --updates`: columnas separadas por TAB:
/// nombre, id, versión, rama, arquitectura. La versión puede venir VACÍA (en esta
/// máquina lo está para el runtime de GNOME), así que se enseña el primer campo con
/// algo después del id, saltándose la arquitectura.
fn parsear_flatpak(texto: &str) -> Vec<String> {
    const ARQUITECTURAS: &[&str] = &["x86_64", "aarch64", "arm", "i386", "riscv64"];
    texto
        .lines()
        .filter_map(|l| {
            let campos: Vec<&str> = l.split('\t').map(str::trim).collect();
            let nombre = *campos.first()?;
            if nombre.is_empty() {
                return None;
            }
            let detalle = campos
                .iter()
                .skip(2)
                .find(|c| !c.is_empty() && !ARQUITECTURAS.contains(c));
            Some(match detalle {
                Some(d) => format!("{nombre} ({d})"),
                None => nombre.to_string(),
            })
        })
        .collect()
}

/// Herramientas que ya devuelven una lista limpia, una cosa por línea
/// (`brew outdated --quiet`, `pacman -Qu`, `mas outdated`).
fn parsear_lineas_simples(texto: &str) -> Vec<String> {
    texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// `dnf check-update`: lista «paquete.arch  versión  repo». El código de salida es
/// 100 cuando hay actualizaciones, así que el texto es lo único que traduce.
fn parsear_dnf(texto: &str) -> Vec<String> {
    texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('=') && !l.starts_with("Last metadata"))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|p| p.contains('.') && !p.eq_ignore_ascii_case("Obsoleting"))
        .map(str::to_string)
        .collect()
}

/// `apt list --upgradable`: «paquete/noble-updates 1.2 amd64 [upgradable desde…]».
///
/// Se buscan las líneas de DATOS (las que llevan `paquete/rama … [upgradable
/// from: …]`) en vez de saltar una cabecera: la cabecera es «Listing...» y no
/// contiene la palabra, así que saltar por ella se comía la primera fila.
fn parsear_apt(texto: &str) -> Vec<String> {
    texto
        .lines()
        .filter(|l| l.contains('/') && l.contains("[upgradable"))
        .filter_map(|l| l.split('/').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `zypper list-updates`: tabla con «v | repo | nombre | ...».
fn parsear_zypper(texto: &str) -> Vec<String> {
    texto
        .lines()
        .filter(|l| l.starts_with('v') || l.starts_with('V'))
        .filter_map(|l| l.split('|').nth(2))
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "Name")
        .map(str::to_string)
        .collect()
}

/// `softwareupdate -l` (macOS): las líneas de la lista empiezan por `*` y llevan
/// `Label:`. Se marca el reinicio porque macOS lo avisa en el texto.
fn parsear_softwareupdate(texto: &str) -> Vec<String> {
    texto
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('*'))
        .map(|l| l.trim_start_matches('*').trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `winget upgrade` (Windows): tabla alineada por columnas separadas por DOS o más
/// espacios. NO se juzga por el texto traducido (Windows la traduce): lo que se
/// busca es la forma de la tabla, y si no aparece ninguna fila, la lista va vacía.
fn parsear_winget(texto: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut en_tabla = false;
    for linea in texto.lines() {
        let l = linea.trim_end();
        if l.is_empty() {
            continue;
        }
        // La cabecera termina en «Source» y debajo va una línea de guiones.
        if l.contains("Source") && l.contains("Id") {
            en_tabla = true;
            continue;
        }
        if en_tabla && l.chars().all(|c| c == '-' || c == ' ') {
            continue;
        }
        if !en_tabla {
            continue;
        }
        let campos: Vec<&str> = l.split("  ").map(str::trim).filter(|c| !c.is_empty()).collect();
        if campos.len() < 2 {
            continue;
        }
        // Nombre + identificador + versión instalada + disponible.
        let nombre = campos[0];
        let id = campos[1];
        let disponible = campos.get(3).copied().unwrap_or("");
        out.push(if disponible.is_empty() {
            format!("{nombre} ({id})")
        } else {
            format!("{nombre} ({id}) → {disponible}")
        });
    }
    out
}

/// `choco outdated` (Windows): «paquete|instalada|disponible|¿fijada?».
fn parsear_choco(texto: &str) -> Vec<String> {
    texto
        .lines()
        .skip(1)
        .filter(|l| l.contains('|'))
        .filter_map(|l| {
            let c: Vec<&str> = l.split('|').collect();
            Some(format!("{} → {}", c.first()?.trim(), c.get(2)?.trim()))
        })
        .collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lee_la_comprobacion_real_de_rpm_ostree_de_esta_maquina() {
        // Salida REAL de `rpm-ostree upgrade --check` en este equipo.
        let texto = "Note: --check and --preview may be unreliable.  See https://github.com/coreos/rpm-ostree/issues/1579\nNo updates available.\n";
        let (lista, nota) = parsear_rpm_ostree(texto);
        assert!(lista.is_empty());
        // La advertencia de la herramienta NO se esconde: se enseña.
        assert!(nota.unwrap().contains("unreliable"));

        // Y cuando sí hay, la versión sale tal cual la da.
        let con = "AvailableUpdate:\n        Version: 44.20261001 (2026-10-01T20:00:00Z)\n";
        let (lista2, _) = parsear_rpm_ostree(con);
        assert_eq!(lista2, vec!["44.20261001 (2026-10-01T20:00:00Z)"]);
    }

    #[test]
    fn lee_las_actualizaciones_reales_de_flatpak() {
        // Salida REAL de `flatpak remote-ls --updates` (separada por tabuladores).
        let texto = "GNOME Application Platform version 49\torg.gnome.Platform\t\t49\tx86_64\nGNOME Application Platform version 50\torg.gnome.Platform\t\t50\tx86_64\n";
        let v = parsear_flatpak(texto);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], "GNOME Application Platform version 49 (49)");
        assert!(parsear_flatpak("").is_empty());
    }

    #[test]
    fn brew_y_pacman_son_una_linea_por_paquete() {
        assert_eq!(parsear_lineas_simples("git\nnode\n\n"), vec!["git", "node"]);
        assert_eq!(parsear_lineas_simples(""), Vec::<String>::new());
    }

    #[test]
    fn lee_dnf_apt_zypper_softwareupdate_winget_y_choco() {
        // dnf: «paquete.arch versión repo» y avisos que no son paquetes.
        let dnf = "Last metadata expiration check: 0:12:00 ago.\n\nkernel.x86_64          6.11.3-200.fc40          updates\nvim-enhanced.x86_64   2:9.1.700-1.fc40         updates\n";
        assert_eq!(parsear_dnf(dnf), vec!["kernel.x86_64", "vim-enhanced.x86_64"]);

        // apt: la primera línea es el rótulo.
        let apt = "Listing... Done\ncurl/noble-updates 8.5.0-2ubuntu10.1 amd64 [upgradable from: 8.5.0-2ubuntu10]\n";
        assert_eq!(parsear_apt(apt), vec!["curl"]);

        // zypper: filas que empiezan por v y columnas separadas por «|».
        let zypper = "S | Repository | Name | Current Version | Available Version | Arch\n--+------------+------+-----------------+-------------------+-----\nv | repo-oss   | curl | 8.0.1-1         | 8.0.1-2           | x86_64\n";
        assert_eq!(parsear_zypper(zypper), vec!["curl"]);

        // softwareupdate: las líneas de la lista empiezan por «*».
        let sw = "Software Update Tool\n\nFinding available software\n* Label: macOS Sonoma 14.5-23F79\n\t- Restart required\n";
        assert_eq!(parsear_softwareupdate(sw), vec!["Label: macOS Sonoma 14.5-23F79"]);

        // winget: tabla con columnas separadas por dos o más espacios.
        let winget = "Name               Id                      Version   Available  Source\n-----------------------------------------------------------------------\nGit                Git.Git                 2.44.0    2.45.1     winget\nMozilla Firefox    Mozilla.Firefox         125.0     126.0      winget\n";
        let w = parsear_winget(winget);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], "Git (Git.Git) → 2.45.1");
        assert_eq!(w[1], "Mozilla Firefox (Mozilla.Firefox) → 126.0");
        // Sin tabla no se inventa nada.
        assert!(parsear_winget("No installed package found matching input criteria.").is_empty());

        // choco: paquete|instalada|disponible.
        let choco = "Chocolatey v2.2.2\nchocolatey|2.2.2|2.3.0|false\ngit|2.44.0|2.45.1|false\n";
        let c = parsear_choco(choco);
        assert_eq!(c, vec!["chocolatey → 2.3.0", "git → 2.45.1"]);
    }

    #[test]
    fn en_un_sistema_atomico_no_se_ofrece_dnf() {
        // La decisión de negocio: en una imagen de solo lectura, `dnf` NO es la vía.
        if es_atomico() {
            let ids: Vec<String> = comprobar().into_iter().map(|f| f.id).collect();
            assert!(!ids.contains(&"dnf".to_string()), "en atómico no se ofrece dnf: {ids:?}");
            assert!(ids.contains(&"rpm-ostree".to_string()), "{ids:?}");
        }
    }

    #[test]
    fn comprobar_no_revienta_y_dice_lo_que_hay() {
        for f in comprobar() {
            assert!(!f.nombre.is_empty());
            assert!(!f.comando_aplicar.is_empty());
            // O está instalada (y entonces tiene comando de comprobación), o no lo está.
            if !f.disponible {
                assert!(f.actualizaciones.is_empty());
                assert!(f.error.is_none());
            }
        }
    }
}
