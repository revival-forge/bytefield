# Jugar con Bytefield en Windows, paso a paso

Esta guía lleva de cero a una partida de Battlefield 1942 con Bytefield, y si quieres, de Desert Combat, Galactic Conquest
y las texturas del HD Remaster. No hace falta la línea de órdenes.

*Estado: escrita y comprobada contra el código y en Wine; lo marcado **(por confirmar)** falta probarlo en un Windows de
verdad. Si algo no te cuadra, [avísanos](../../CONTRIBUTING.md#reporting-a-bug).*

## 1. Qué necesitas

- Windows 10 u 11 de 64 bits, con una tarjeta gráfica con Vulkan o DirectX 12 (cualquiera de los últimos diez años).
- Tu copia de **Battlefield 1942**: los CD originales (con o sin The Road to Rome y Secret Weapons of WWII) o un
  recopilatorio que los traiga. Hacen falta los ficheros del juego, no su ejecutable: Bytefield nunca arranca el `BF1942.exe`
  original ni pide la clave de CD.
- Unos 2 GB para el juego y 1–2 GB por mod.

## 2. Instalar Battlefield 1942

1. Mete el disco 1 (o monta su imagen: doble clic en un `.iso`) y ejecuta `Setup.exe`. El instalador encadena el juego,
   The Road to Rome y Secret Weapons si están en los discos y pide una clave por producto (la pide el instalador, no
   Bytefield). PunkBuster se puede desmarcar: Bytefield no lo usa.
2. Deja la carpeta por defecto, `C:\Program Files (x86)\EA GAMES\Battlefield 1942`: ahí Bytefield la encuentra solo. Otra
   carpeta también vale; la elegirás en la primera pantalla de bytefield.
3. Instala el **parche 1.6** si tus discos son anteriores (la 1.61 también vale).

**(por confirmar)** si el instalador de 2002 funciona en Windows 11 con todas las ediciones. Si no, dinos cuál es la tuya:
los ficheros se pueden sacar de los discos por otros medios.

No te preocupes si el juego original no arranca en un Windows moderno (su protección de copia ya no funciona ahí):
Bytefield solo lee sus ficheros.

## 3. Conseguir Bytefield

1. Descarga el zip para Windows de la última versión desde la página del proyecto.
2. Descomprímelo en una carpeta tuya, por ejemplo `C:\Juegos\Bytefield`. Trae `bytefield.exe` (el juego), `bytefield-content.exe`
   (la herramienta de contenido) y `bytefield-server.exe` (un servidor dedicado). Para jugar solo hace falta el primero;
   los otros dos son de terminal (con doble clic explican qué son y esperan a Intro).
3. SmartScreen puede avisar de que el programa no es conocido: **Más información → Ejecutar de todas formas**. No está firmado.

## 4. El primer arranque

1. Doble clic en `bytefield.exe`.
2. Si el juego está en la carpeta por defecto, va directo al menú. Si no, una primera pantalla enseña las carpetas que
   ha encontrado o deja escribir una: escribe la carpeta donde está `Mods\bf1942` y pulsa Intro. La recuerda en
   `%APPDATA%\Bytefield\content.cfg` y no vuelve a preguntar.
3. En el menú: **MULTIJUGADOR → CREAR PARTIDA**, elige mapa y número de bots e **INICIAR**. Antes de aparecer, elige punto
   de aparición con ← → y kit con 1–5, y **LISTO**.

Los vídeos del principio solo salen si tienes `ffmpeg` instalado y en el PATH; si no, se saltan.

## 5. Mods y HD Remaster (opcional)

### Elegir el mod

Como en el original: **JUEGO MODIFICADO** en la barra del menú lista los mods instalados con su versión y su
información; elige uno y pulsa **ACTIVAR**. El menú pasa a ser el de ese mod (fondo, música, textos y mapas) y Bytefield
lo recuerda para la próxima vez. También se puede cambiar solo para una partida, en **CREAR PARTIDA → MOD**.

¿Un icono por mod, como los que dejaban los instaladores? Clic derecho en `bytefield.exe` → **Crear acceso directo**;
en las **Propiedades** del acceso directo, añade el nombre de la carpeta del mod al final de **Destino**, tras un espacio:
`...ytefield.exe DC_Final` (o `GCMOD`, `XPack1`…). Ese icono abre el menú de ese mod.

### Desert Combat Final

1. Primero **Desert Combat 0.7** (DC Final se monta sobre él): baja el instalador completo de
   [archive.org](https://archive.org/details/desert-combat-0.7-full-install_202310) y ejecútalo apuntando a la carpeta de
   Battlefield 1942. Crea `Mods\DesertCombat`.
2. Luego **Desert Combat Final**, de [archive.org](https://archive.org/details/dc_final_client). Crea `Mods\DC_Final`.
3. En Bytefield: **JUEGO MODIFICADO**, `DC_Final` y **ACTIVAR**.

### Galactic Conquest

Instala Galactic Conquest 8.5 desde la página del mod en la carpeta de Battlefield 1942 (crea `Mods\GCMOD`) y activa
`GCMOD` en **JUEGO MODIFICADO**.

### Cualquier otro mod

Un mod que se instale en `Mods\<nombre>` con su `init.con` sale en **JUEGO MODIFICADO**. Que se juegue bien depende de lo que
use; cuéntanos qué tal.

### Texturas del HD Remaster

El [HD Remaster](https://www.moddb.com/mods/high-definition-remaster) está pensado para copiarlo encima del juego. Con
Bytefield **no** se copia: se deja en su carpeta y Bytefield lo pone por encima, así que se puede encender y apagar.

1. Baja de ModDB `BF1942graphicsmod2023.zip`, `SkyboxMultiplayer.zip` y, si tienes las expansiones,
   `XpackModded2023.zip` (y si quieres `BF1942customgraphicsmod2023.zip`).
2. Crea una carpeta `content` al lado de `bytefield.exe` y descomprime cada uno en su carpeta dentro de ella:
   `content\base`, `content\skybox`, `content\xpack` (y `content\custom`). Los `.exe`, dgVoodoo y ReShade que traen
   no hacen falta: Bytefield no los usa.
3. Abre Bytefield: los encuentra solo y los enciende. **OPCIONES → GENERAL** tiene una línea por pack (`HD-BASE`,
   `HD-SKYBOX`, `HD-XPACK`) para encender o apagar cada uno, desde la siguiente ronda; el que apagues se queda apagado.

¿Los tienes en otro sitio? `bytefield-content add <carpeta>` (en una terminal en la carpeta de Bytefield) añade un pack o un
mod desde cualquier carpeta; `bytefield-content list` enseña lo que hay.

## 6. Gráficos

**OPCIONES → VÍDEO → calidad**: *Original* se ve como el juego de 2002; *Mejorado* añade mejor filtrado, antialiasing,
sombras y reflejos. Cada ajuste se puede cambiar suelto. **EXPERIMENTAL** guarda efectos que aún se están probando.
Qué hace cada cosa: [features.md](features.md#graphics-what-is-drawn-and-how) (inglés).

## 7. Jugar con amigos

Uno elige **CREAR PARTIDA**; los demás, **MULTIJUGADOR → UNIRSE** con la IP de quien hospeda. Por Internet, quien
hospeda tiene que abrir el **puerto UDP 14567** en su router (y dejar pasar a Bytefield en el cortafuegos de Windows cuando
pregunte). Bytefield solo juega con Bytefield, no con los servidores originales.

## 8. Si algo va mal

- En partida, pulsa **Ctrl+F11** justo cuando pase, escribe una nota corta e Intro. Guarda una captura y el estado en
  `%LOCALAPPDATA%\Bytefield\tickets`. Una caída también deja allí su carpeta.
- El registro de la última partida queda en `%LOCALAPPDATA%\Bytefield\logs\latest.log` (la anterior, en `previous.log`); con una caída, al final.
- Comprime esa carpeta y `latest.log` y adjúntalos a un [aviso de fallo](../../CONTRIBUTING.md#reporting-a-bug).
- `bytefield-content verify` (en la terminal del paso 5) dice si tus ficheros coinciden con una versión que conocemos.

## Dónde está cada cosa

| Qué | Dónde |
|---|---|
| Contenido y gráficos | `%APPDATA%\Bytefield\content.cfg` |
| Opciones del menú, controles | `%APPDATA%\Bytefield\settings.cfg`, `controls.cfg` |
| Marcas, caídas | `%LOCALAPPDATA%\Bytefield\tickets` |
| Capturas (Impr Pant) | `%LOCALAPPDATA%\Bytefield\shots` |
| Registro (última partida y la anterior) | `%LOCALAPPDATA%\Bytefield\logs` |

Para empezar de cero, borra `%APPDATA%\Bytefield`.
