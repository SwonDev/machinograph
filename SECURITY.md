# Seguridad

## Lo que hace Machinograph con tus datos

**Nada sale de tu equipo.** La aplicación no tiene cuentas, ni nube, ni telemetría, ni analítica. Lo
único que usa la red es:

- Descargar las herramientas que necesita (llmfit y llama.cpp) desde sus **versiones oficiales**, y
  solo cuando hace falta. Cada descarga se comprueba con su **sha256** antes de extraerse.
- Hablar con **tus** servidores de IA locales (`127.0.0.1`) y, si tú lo activas, atender peticiones de
  tu red en el puerto que le digas.

Todo lo demás —hardware, modelos, uso, limpieza, seguridad— se lee y se escribe en tu propio equipo.

## Qué mira, y qué no

- La revisión de **Seguridad** comprueba dónde vive la persistencia (arranque de la sesión, tareas
  programadas, gancho de bibliotecas, llaves SSH, ficheros que se ejecutan al abrir sesión) y lo dice
  **con la prueba de cada hallazgo**. No es un antivirus: no analiza binarios, no usa firmas y no
  descarga reglas.
- **No borra nada sin que se lo pidas.** La limpieza, el borrado y la compactación de bases de datos
  son acciones explícitas, en dos pasos, con su aviso de qué se va a hacer y cuánto se libera. Los
  datos del usuario no se tocan; lo que puede romperse se conserva (una base dañada se **aparta**, con
  fecha y hora, nunca se borra).
- **No lanza `sudo` nunca.** Lo que necesita permisos de administrador se detecta, se dice por qué y se
  enseña el comando exacto para que lo ejecutes tú.
- Las **exclusiones** que configures se respetan en todas las herramientas (analizador, limpieza,
  borrado) y, además, las raíces del sistema están protegidas siempre.

## Cómo reportar un problema de seguridad

Abre un **issue** describiendo el caso (qué esperabas, qué pasó y cómo reproducirlo). Si prefieres no
hacerlo en público, usa el aviso privado de GitHub («Security» → «Report a vulnerability») en este
mismo repositorio.

No hay versiones con soporte a largo plazo: se publica una etiqueta por versión y se corrige en la
siguiente.

## Verificación

- `.github/workflows/ci.yml`: pruebas, empaquetado de los tres instaladores y recorrido del CLI en
  Linux, macOS y Windows.
- Las pruebas del backend no tocan la configuración de nadie: las que necesitan ficheros o modelos
  reales están marcadas como integración (`cargo test -- --ignored`).
