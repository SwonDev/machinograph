# Créditos

Machinograph es software libre con licencia **MIT** (ver [LICENSE](LICENSE)). Parte de su trabajo
deriva de proyectos de terceros, y esta es su atribución:

- **[Kudu](https://github.com/adventdevinc/kudu)** (MIT): el catálogo de limpieza —cachés, temporales
  y registros que se regeneran— está portado de sus reglas, con las rutas tal como las documenta el
  proyecto. De ahí vienen también las ideas de las exclusiones globales, la limpieza de fichas
  huérfanas de la papelera y los modos de coincidencia de reglas (`fileMatch`, `recursiveMatch`,
  `cacheReset`, la revalidación antes de borrar).
- **[llmfit](https://github.com/AlexsJones/llmfit)** (MIT): la aplicación lo usa —y lo instala por su
  cuenta— para descubrir modelos, estimar el plan de hardware y medir sirviendo.
- **[llama.cpp](https://github.com/ggml-org/llama.cpp)** (MIT): sus binarios (`llama-bench`,
  `llama-fit-params`) son los que dan las medidas reales y el encaje; la aplicación también los
  instala por su cuenta si faltan.
- **[Tauri](https://tauri.app)** (MIT/Apache-2.0): el armazón de la aplicación, con Rust y WebKitGTK /
  WebView2 / WKWebView.
- **[Tabler Icons](https://tabler.io/icons)** (MIT): los iconos de la interfaz.
- **[React](https://react.dev)**, **[Vite](https://vite.dev)**, **[Tailwind CSS](https://tailwindcss.com)**
  y **[zustand](https://zustand.docs.pmnd.rs)**: la interfaz y su construcción.

Las imágenes de marca (`assets/marca/`) se generaron con una herramienta de imagen por IA a partir de
la paleta del propio proyecto.
