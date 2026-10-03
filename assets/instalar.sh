#!/usr/bin/env bash
#
# Instala Machinograph para el usuario actual: lanzador en el PATH e entrada en el menú
# de aplicaciones.
#
# Por qué un script y no copiar ficheros a mano: los tres ficheros que genera
# llevan rutas absolutas (el binario vive en el árbol de compilación del
# repositorio), así que si se copian a mano se quedan apuntando a la ruta de
# otra persona o de otra máquina. Aquí se calculan desde la ubicación real del
# script.
#
# Uso:
#     bash assets/instalar.sh
#
# Requiere haber compilado antes:
#     pnpm exec tauri build --no-bundle
set -euo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LANZADOR="$HOME/.local/bin/machinograph"
MENU="$HOME/.local/share/applications"
ICONOS="$HOME/.local/share/icons/hicolor/256x256/apps"

mkdir -p "$(dirname "$LANZADOR")" "$MENU" "$ICONOS"

# ── 1. Icono ──────────────────────────────────────────────────────────────────
# 128x128@2x son 256x256 reales, que es justo lo que espera hicolor.
if [[ -f "$RAIZ/src-tauri/icons/128x128@2x.png" ]]; then
  cp -f "$RAIZ/src-tauri/icons/128x128@2x.png" "$ICONOS/machinograph.png"
  echo "icono   -> $ICONOS/machinograph.png"
else
  echo "AVISO: no hay iconos generados. Ejecuta antes:" >&2
  echo "       python3 assets/generar-icono.py && pnpm exec tauri icon assets/icono-fuente.png" >&2
fi

# ── 2. Lanzador ───────────────────────────────────────────────────────────────
cat > "$LANZADOR" <<FIN
#!/usr/bin/env bash
#
# Lanzador de Machinograph. Generado por assets/instalar.sh; no lo edites a mano.
#
# Existe para que el menú de aplicaciones y la terminal tengan UNA sola ruta
# estable (~/.local/bin/machinograph) aunque el binario viva en el árbol de compilación
# del repositorio. Prefiere la compilación release y, si no existe, usa la de
# depuración.
set -euo pipefail
REPO="$RAIZ"
for candidato in "\$REPO/src-tauri/target/release/machinograph" "\$REPO/src-tauri/target/debug/machinograph"; do
  if [[ -x "\$candidato" ]]; then
    exec "\$candidato" "\$@"
  fi
done
printf 'Machinograph no está compilado todavía.\n' >&2
printf '  cd %s && pnpm exec tauri build --no-bundle\n' "\$REPO" >&2
exit 1
FIN
chmod +x "$LANZADOR"
echo "lanzador-> $LANZADOR"

# ── 3. Entrada del menú ───────────────────────────────────────────────────────
# Ojo con dos cosas de este heredoc, que están puestas a propósito:
#   * va SIN comillas porque tiene que expandir $LANZADOR;
#   * por eso mismo no puede llevar backticks ni $ sueltos en el texto (se
#     ejecutarían como comandos), y el fichero .desktop tampoco admite
#     comentarios: el formato no los tiene, así que aquí no van.
# Solo va UNA categoría principal: `Utility` y `System` lo son las dos, y poner
# las dos hace que la aplicación salga duplicada en el menú (`Monitor` es
# adicional y no cuenta).
cat > "$MENU/machinograph.desktop" <<FIN
[Desktop Entry]
Type=Application
Version=1.0
Name=Machinograph
GenericName=Panel de IA local
Comment=Estado del hardware y de los servidores de IA locales
Exec=$LANZADOR
Icon=machinograph
Terminal=false
Categories=System;Monitor;
Keywords=IA;AI;GPU;llama;ollama;modelos;monitor;vram;
StartupNotify=true
StartupWMClass=machinograph
FIN
echo "menú    -> $MENU/machinograph.desktop"

# Refresca la base de datos de entradas de menú si la herramienta existe.
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$MENU" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -qtf "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo
echo "Listo. Machinograph debería aparecer en el menú de aplicaciones y en el PATH."
