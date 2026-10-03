/// <reference types="vite/client" />

// Declara los módulos que Vite resuelve pero TypeScript no conoce por sí solo:
// el `import "./styles.css"` de main.tsx, los `?raw`/`?url` y las variables
// `import.meta.env`. Sin este fichero, `tsc` se queja del CSS (TS2882) aunque el
// build funcione, porque Vite sí sabe resolverlo y TypeScript no.
