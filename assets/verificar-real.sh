#!/usr/bin/env bash
#
# Verificación de nivel 2 de Machinograph: recorre las 15 secciones del BINARIO REAL
# instalado y deja una captura de cada una.
#
# Por qué existe además del arnés del navegador (`verificar-ui.mjs`): aquel prueba
# la interfaz con el puente Tauri SIMULADO, y este prueba el programa de verdad —
# el binario, el backend Rust, la webview y los datos de esta máquina. Los dos
# niveles se han encontrado cosas que el otro no ve.
#
# LO QUE COSTÓ APRENDERLO (por si hay que depurarlo algún día):
#
#   * Los CLICS no llegan a esta ventana. `xdotool mousemove --window ... click`
#     devuelve éxito y la ventana no recibe NADA (las capturas salen idénticas).
#     El TECLADO sí llega.
#   * En Wayland nativo KWin no deja inyectar, así que se lanza con
#     GDK_BACKEND=x11.
#   * Tabulando "hacia atrás" desde el contenido el recorrido NO es fiable: al
#     activar una sección la app lleva el foco a `#contenido`, y desde ahí
#     Shift+Tab cae en la última entrada de la barra lateral, pero cuántos pasos
#     hacen falta depende de cuántos elementos enfocables tenga esa vista, y eso
#     cambia con los DATOS (una tarjeta de aviso más, un botón más). Así se
#     desfasó un recorrido entero.
#   * Lo estable: desde el arranque el foco está en el documento y el orden es
#     fijo —[1] "Saltar al contenido", [2..16] las 15 entradas—. Por eso se
#     arranca la app UNA VEZ POR SECCIÓN y se tabula siempre hacia adelante.
#
# Uso: bash assets/verificar-real.sh [/directorio/de/capturas]
set -uo pipefail

SALIDA="${1:-/tmp/machinograph-verificacion-real}"
mkdir -p "$SALIDA"

SECCIONES=(Inicio Descubrir 'En disco' Rendimiento Servidores Uso Conexiones Hardware Pantalla Almacenamiento Optimizacion Seguridad Diagnostico Mantenimiento Ajustes)

for j in "${!SECCIONES[@]}"; do
  nombre="${SECCIONES[$j]}"
  pkill -x machinograph 2>/dev/null
  sleep 1

  GDK_BACKEND=x11 nohup "$HOME/.local/bin/machinograph" > "$SALIDA/$nombre.log" 2>&1 &
  sleep 8

  WID="$(xdotool search --name '^Machinograph$' | head -1)"
  if [[ -z "$WID" ]]; then
    echo "ERROR: no aparece la ventana en la sección $nombre. Log:" >&2
    cat "$SALIDA/$nombre.log" >&2
    exit 1
  fi
  xdotool windowactivate --sync "$WID" 2>/dev/null || true
  sleep 0.6

  # [1] enlace de salto, [2] Inicio … [j+2] la sección que toca.
  for ((n = 0; n <= j + 1; n++)); do
    xdotool key --clearmodifiers Tab
    sleep 0.12
  done
  if (( j > 0 )); then
    xdotool key --clearmodifiers Return
    # Las dos secciones de disco hacen trabajo de verdad al abrirse (el análisis
    # del home son ~14 s y el escaneo de cachés ~3 s). Con la espera corta la
    # captura salía en "analizando" y no se veía la tabla, que es justo lo que se
    # quiere mirar aquí: WebKit reparte las tablas distinto que Chromium.
    case "$nombre" in
      Almacenamiento) sleep 18 ;;
      Optimizacion) sleep 6 ;;
      # Seguridad arranca su revisión al abrirse (crontab, permisos, huellas): con
      # la espera corta la captura saldría en «Comprobando…».
      Seguridad) sleep 5 ;;
      *) sleep 1.8 ;;
    esac
  fi

  fichero="$SALIDA/$(printf '%02d' "$j")-${nombre}.png"
  import -window "$WID" "$fichero" 2>/dev/null
  printf '  %-16s -> %s\n' "$nombre" "$(basename "$fichero")"
done

sleep 1
echo "--- ¿algún log con errores? ---"
grep -il "error\|panic\|warning" "$SALIDA"/*.log 2>/dev/null || echo "  ninguno"
echo "--- ¿son distintas las capturas? ---"
DISTINTAS=$(md5sum "$SALIDA"/*.png | awk '{print $1}' | sort -u | wc -l)
TOTAL=$(ls "$SALIDA"/*.png | wc -l)
echo "  $DISTINTAS distintas de $TOTAL"
pkill -x machinograph 2>/dev/null
