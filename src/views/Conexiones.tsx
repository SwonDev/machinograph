/**
 * Conexiones: qué cliente de IA usa este equipo y con qué configuración.
 *
 * POR QUÉ ES SECCIÓN PROPIA: antes vivía al final de Servidores, y no es lo
 * mismo: Servidores es "qué motor está en marcha", esto es "quién habla con él".
 * La historia que motiva la app es justo esta —una herramienta escribe en la
 * ruta que *espera* y en un home aislado falla en silencio—, así que merece su
 * sitio y no ser un apéndice debajo de otras tarjetas.
 *
 * La frontera (dónde se escribe y dónde no) la dibuja `ClientesConexion`: solo se
 * escribe donde el formato está comprobado leyendo el fichero real.
 */
import ClientesConexion from "../components/ClientesConexion";

export default function Conexiones() {
  return (
    <div className="flex flex-col gap-4">
      <ClientesConexion />
    </div>
  );
}
