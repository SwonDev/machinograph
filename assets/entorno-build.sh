#!/usr/bin/env bash
#
# Prepara el entorno para COMPILAR Machinograph en Bazzite (Fedora atómico).
#
# El problema: Bazzite trae las bibliotecas de EJECUCIÓN de GTK y WebKit
# (`libgtk-3.so.0`, `libwebkit2gtk-4.1.so.0`…) pero no los paquetes `-devel`, y
# Tauri necesita `pkg-config` y los enlaces `libX.so` sin número de versión para
# poder enlazar. Sin esto, la compilación falla al final con
# `unable to find library -lgtk-3 / -lwebkit2gtk-4.1 / -lgdk-3 / …`.
#
# La solución que se usa aquí es un «overlay»: los `.pc` y los encabezados de los
# RPM de desarrollo, extraídos FUERA del sistema (~/.local/share/machinograph-build),
# con los enlaces apuntando a las bibliotecas que sí están en /usr/lib64. No se
# toca el sistema de ficheros de la imagen, así que no hace falta reiniciar.
#
# Uso:
#     source assets/entorno-build.sh
#     pnpm exec tauri build --no-bundle
#
# Se usa con `source` a propósito: tiene que exportar PKG_CONFIG_PATH al shell
# desde el que se compila, y eso un script ejecutado no puede hacerlo.
#
# Alternativa sin overlay (requiere reiniciar la máquina):
#     rpm-ostree install gtk3-devel webkit2gtk4.1-devel javascriptcoregtk4.1-devel libsoup3-devel
#
# Una nota sobre las versiones, para quien lo revise en el futuro: el overlay
# trae los `.pc` de WebKit 2.54.0 (los RPM de desarrollo más recientes de fc44),
# pero los enlaces apuntan a la biblioteca que hay instalada, que en esta máquina
# es la 2.52.5. Es seguro porque `webkit2gtk-sys` solo activa funciones por
# versión hasta la 2.40, así que las dos cumplen; pero si alguna vez se activara
# una función de 2.42 en adelante, el enlace pasaría y el programa fallaría al
# arrancar. Si eso ocurre, hay que igualar los `.pc` a la versión instalada.
set -u

OVERLAY="${MACHINOGRAPH_OVERLAY_DIR:-$HOME/.local/share/machinograph-build/overlay}"

# Cuando se ejecuta en vez de hacerse `source`, las variables se quedan aquí y no
# llegan al shell del usuario: merece la pena decirlo en vez de dejarlo pasar.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  echo "AVISO: hay que hacer 'source', no ejecutarlo:" >&2
  echo "    source assets/entorno-build.sh" >&2
  echo "(un script ejecutado no puede exportar variables a tu shell)" >&2
  echo >&2
fi

if [[ ! -d "$OVERLAY/usr/lib64/pkgconfig" ]]; then
  cat >&2 <<FIN
entorno-build: no encuentro el overlay en $OVERLAY

  Opciones:
   a) instalarlo de verdad en el sistema (requiere reiniciar):
        rpm-ostree install gtk3-devel webkit2gtk4.1-devel javascriptcoregtk4.1-devel libsoup3-devel
   b) indicar dónde está el overlay:
        MACHINOGRAPH_OVERLAY_DIR=/ruta/al/overlay source assets/entorno-build.sh
FIN
  return 1 2>/dev/null || exit 1
fi

# Los `.pc` viven en dos sitios: los normales en lib64/pkgconfig y los de Xorg
# (xproto y compañía, que necesita GTK) en share/pkgconfig. Si falta el segundo,
# pkg-config no resuelve `gtk+-3.0` y el enlace vuelve a fallar.
export PKG_CONFIG_PATH="$OVERLAY/usr/lib64/pkgconfig:$OVERLAY/usr/share/pkgconfig"

# No basta con exportar la variable: se comprueba que pkg-config resuelva de
# verdad lo que Tauri va a pedir, porque si falta un `.pc` el error que sale
# después (al enlazar) no dice cuál era el problema.
if command -v pkg-config >/dev/null 2>&1; then
  faltan=0
  for p in gtk+-3.0 webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0; do
    if v="$(pkg-config --modversion "$p" 2>/dev/null)"; then
      printf '  %-26s %s\n' "$p" "$v"
    else
      printf '  %-26s FALTA\n' "$p" >&2
      faltan=1
    fi
  done
  if (( faltan )); then
    echo "entorno-build: falta algún paquete en el overlay; el enlace fallará." >&2
    return 1 2>/dev/null || exit 1
  fi
else
  echo "entorno-build: no hay pkg-config en el PATH." >&2
  return 1 2>/dev/null || exit 1
fi

echo "entorno-build: listo (PKG_CONFIG_PATH -> $OVERLAY)"
