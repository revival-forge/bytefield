# Bytefield

*Un motor libre (GPL-3.0-or-later) que juega a Battlefield 1942 y a sus mods **con los datos de tu propia copia**, al
estilo de OpenMW con Morrowind. No trae ningún dato del juego ni de los mods.* · [English](README.md)

Proyecto no afiliado a Electronic Arts ni a DICE. «Battlefield 1942» es una marca de sus dueños y aquí solo sirve para
decir con qué datos funciona.

## La idea

Primero, **fidelidad**: jugar al original y a sus mods clásicos como eran, en sistemas de hoy y con los ficheros
originales: los mismos mapas, vehículos, armas, reglas y aspecto. El original es la referencia, y lo que se aparta de él
es un fallo.

Luego, sobre esa base, lo que el motor de 2002 no podía:

- **mejores bots**, también en mods y mapas que nunca los tuvieron;
- **más jugadores** por servidor;
- **vehículos, modos de juego y mods nuevos**, sobre los mismos formatos que la comunidad ya conoce;
- **mejores gráficos** como opción (perfil *Mejorado*, packs de texturas HD), con el aspecto original siempre a mano.

## Lo que hay hoy

- **Contenido:**
  - Battlefield 1942 1.6;
  - sus expansiones, The Road to Rome y Secret Weapons of WWII;
  - Desert Combat Final 0.8 (el mod más probado), y Desert Combat 0.7, que carga;
  - Galactic Conquest 8.5;
  - los packs de texturas del HD Remaster.

  Otros mods también salen en el menú; lo bien que se jueguen depende de lo que usen. Detalle en
  [docs/user/features.md](docs/user/features.md) (inglés).
- **Modos:** conquista, CTF, objetivos, cooperativo y todos contra todos por equipos, con vehículos, aviones, barcos y
  armas fijas.
- **Bots:** aquí Bytefield se aparta del original a propósito, para bien o para mal. Son propios, no una copia de los
  del original: toman banderas, conducen, vuelan, navegan, usan armas fijas, curan y reparan, aunque sin mucha maña
  todavía. Mejorarlos está en la hoja de ruta.
- **Juego en red** entre clientes de Bytefield, con su propio protocolo (`bytefield-server` y «Unirse» en el menú). No
  se conecta a servidores originales.
- **Clave de CD:** Bytefield no la comprueba. Desde que cerró el servicio en línea original, los servidores de la
  comunidad dejaron de comprobar las claves, y Bytefield hace lo mismo. Se juega igualmente con los ficheros de tu propia
  copia del juego.
- **Perfiles gráficos:** *Original* (el aspecto del juego) y *Mejorado*, más ajustes sueltos y una página *Experimental*.

## Limitaciones conocidas (0.1)

Es la primera versión pública. Habrá diferencias con el original: cada una es un fallo, y avisar de ellas es lo que más ayuda.

- **El juego en red** se ha probado con varios clientes en una misma máquina; entre máquinas distintas es nuevo.
- **Galactic Conquest** carga y se juega, pero lo han jugado personas mucho menos que Desert Combat.
- **Los bots** juegan todos los modos, pero aún no bien: en algunos mapas se atascan o no salen de su base.
- **El sonido** está incompleto en algunos sitios, y en algunos mods pueden faltar sonidos o sonar bajos.
- Todavía no hay lanzador: el contenido se activa en el menú o con `bytefield-content`.

## Qué necesitas

Tu propio Battlefield 1942, instalado (la versión 1.6). Las expansiones y los mods son opcionales; cuáles conoce
Bytefield y dónde conseguirlos, en [docs/user/features.md](docs/user/features.md) (inglés).

Paso a paso: **[guía para Windows](docs/user/windows.es.md)**.

## Compilar y jugar

Hay paquetes listos para Windows y Linux en la página de [versiones](https://github.com/revival-forge/bytefield/releases).
Para compilar Bytefield tú mismo (está escrito en Rust) hace falta:

1. **Rust**, instalado con [rustup](https://rustup.rs) (estable). Trae `cargo`, la herramienta que compila el proyecto.
2. **En Linux**, un compilador de C, `pkg-config` y las cabeceras de ALSA para el sonido: en Debian y Ubuntu,
   `sudo apt install build-essential pkg-config libasound2-dev`; en Fedora, `sudo dnf install gcc pkg-config alsa-lib-devel`.
   **En Windows**, las Visual Studio Build Tools, que rustup se ofrece a instalar.

Después, en la carpeta del código:

```
cargo build --release -p bytefield-client
target/release/bytefield
```

La primera compilación tarda unos minutos. Al arrancar, Bytefield busca el juego: lo guardado en `content.cfg`, y si
no, las instalaciones habituales y los prefijos de Wine; si hay dudas, pregunta por la carpeta en una primera pantalla.
También se le puede decir: `target/release/bytefield <carpeta> [mod] [nivel] [--bots 16]`.

Más: compilar para Windows o para distribuciones de Linux antiguas, en [docs/user/building.md](docs/user/building.md);
todas las órdenes, en [docs/commands.md](docs/commands.md); cómo está hecho el código, en [docs/dev/](docs/dev/README.md)
(arquitectura, formatos, el dialecto `.con`, el banco sin ventana). Todo en inglés.

## Tests

```
cargo test
cd python && uv run pytest
```

Los tests que usan datos reales se saltan si no los encuentran; los demás usan datos sintéticos.

## Hoja de ruta

1. Cerrar lo que falta frente al original en los mapas más jugados, vanilla y expansiones; luego Desert Combat y Galactic Conquest.
2. Un lanzador para el contenido y los packs (hoy, `bytefield-content` en la línea de órdenes y el menú del juego).
3. Mejores bots en todas partes, también con los vehículos de Galactic Conquest.
4. Servidores más grandes.
5. Más mods: dinos cuál juegas tú ([CONTRIBUTING.md](CONTRIBUTING.md)).

## Contribuir y avisar de fallos

[CONTRIBUTING.md](CONTRIBUTING.md) (inglés): cómo avisar de un fallo (Ctrl+F11 en partida deja una marca con captura),
cómo pedir un mod y las normas del código; entre ellas, que **no se acepta nada derivado de descompilar los ejecutables de EA**.

## Licencia y contacto

GPL-3.0-or-later. Atribuciones de lo ajeno en [CREDITS.md](CREDITS.md).
Titulares de derechos y asuntos privados: **revival-forge@proton.me**; contestamos en unos días.
