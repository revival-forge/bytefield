# Playing with Bytefield on Windows, step by step

This guide takes you from nothing to a game of Battlefield 1942 (and, if you want, Desert Combat, Galactic Conquest
and the HD Remaster textures) with bytefield. No command line is needed.

*Status: written and checked against the code and in Wine; parts marked **(to be confirmed)** still need a run on a
real Windows. If something here is wrong for you, please [tell us](../../CONTRIBUTING.md#reporting-a-bug).*

## 1. What you need

- Windows 10 or 11, 64-bit, with a graphics card that supports Vulkan or DirectX 12 (any card from the last ten years).
- Your copy of **Battlefield 1942**: the original CDs (with or without The Road to Rome and Secret Weapons of WWII), or
  a compilation that contains them. You need the game's files, not its executable: Bytefield never runs the original
  `BF1942.exe` and never asks for the CD key.
- About 2 GB for the game, plus 1–2 GB per mod.

## 2. Install Battlefield 1942

1. Insert disc 1 (or mount its image: double-click a `.iso`) and run `Setup.exe`. The installer chains the base game,
   The Road to Rome and Secret Weapons if they are on the discs; it asks for one CD key per product (the installer
   wants them; Bytefield does not). You can untick PunkBuster: Bytefield does not use it.
2. Keep the default folder, `C:\Program Files (x86)\EA GAMES\Battlefield 1942`: Bytefield finds it on its own there.
   Another folder works too; you will just choose it on Bytefield's first screen.
3. Install the **1.6 patch** if your discs are older (1.61 also works).

**(to be confirmed)** whether the 2002 installer runs on Windows 11 for every edition. If it does not, tell us your
edition: the files can also be taken from the discs by other means.

Do not worry if the original game itself does not start on modern Windows (its copy protection no longer works there):
Bytefield only reads its files.

## 3. Get Bytefield

1. Download the Windows zip of the latest release from the project's page.
2. Unzip it into a folder of your own, for example `C:\Games\Bytefield`. It contains `bytefield.exe` (the game),
   `bytefield-content.exe` (the content tool) and `bytefield-server.exe` (a dedicated server). Only the first is needed to
   play; the other two are command-line tools (opened with a double click, they say what they are and wait for Enter).
3. Windows SmartScreen may warn that the program is not known: **More info → Run anyway**. The program is not signed.

## 4. First start

1. Double-click `bytefield.exe`.
2. If the game is in the default folder, it goes straight to the menu. If not, a first screen lists the folders it
   found or lets you type one: type the folder where `Mods\bf1942` lives and press Enter. It remembers it in
   `%APPDATA%\Bytefield\content.cfg`, so it will not ask again.
3. In the menu: **MULTIPLAYER → CREATE GAME**, pick a map and the number of bots, **START**. In game, before you
   spawn, choose a spawn point with ← → and a kit with 1–5, then **READY**.

The intro movies only play if `ffmpeg` is installed and on the PATH; without it they are skipped.

## 5. Mods and the HD Remaster (optional)

### Choosing the mod

As in the original: **CUSTOM GAME** on the menu's bar lists the installed mods with their version and info; pick one
and press **ACTIVATE**. The menu becomes that mod's (background, music, texts and maps), and Bytefield remembers it for
next time. For a single game, **CREATE GAME → MOD** changes it too.

An icon per mod, like the ones the installers used to leave? Right-click `bytefield.exe` → **Create shortcut**; in the
shortcut's **Properties**, add the mod's folder name at the end of **Target**, after a space:
`...ytefield.exe DC_Final` (or `GCMOD`, `XPack1`…). That icon opens that mod's menu.

### Desert Combat Final

1. Install **Desert Combat 0.7** first (Desert Combat Final is built on it): download the full installer from
   [archive.org](https://archive.org/details/desert-combat-0.7-full-install_202310) and run it; point it at your
   Battlefield 1942 folder. It creates `Mods\DesertCombat`.
2. Then **Desert Combat Final** from [archive.org](https://archive.org/details/dc_final_client). It creates
   `Mods\DC_Final`.
3. In Bytefield: **CUSTOM GAME**, `DC_Final` and **ACTIVATE**.

### Galactic Conquest

Install Galactic Conquest 8.5 from the mod's own page into the Battlefield 1942 folder (it creates `Mods\GCMOD`),
then activate `GCMOD` in **CUSTOM GAME**.

### Any other mod

A mod that installs into `Mods\<name>` with its own `init.con` shows up in **CUSTOM GAME**. Whether it plays well depends
on what it uses; tell us how it goes.

### HD Remaster textures

The [HD Remaster](https://www.moddb.com/mods/high-definition-remaster) is meant to be copied over the game. With Bytefield
you do **not** copy it: you keep it in its own folder and Bytefield lays it over the game, so you can turn it on and off.

1. Download from ModDB `BF1942graphicsmod2023.zip`, `SkyboxMultiplayer.zip` and, if you have the expansions,
   `XpackModded2023.zip` (optionally `BF1942customgraphicsmod2023.zip`).
2. Make a folder called `content` next to `bytefield.exe` and unzip each one into its own folder inside it:
   `content\base`, `content\skybox`, `content\xpack` (and `content\custom`). Ignore the `.exe`, dgVoodoo and ReShade
   that come with them: Bytefield does not use them.
3. Start Bytefield: it finds them on its own and turns them on. **OPTIONS → GENERAL** has one line per pack (`HD-BASE`,
   `HD-SKYBOX`, `HD-XPACK`) to turn each on or off, from the next round; one you turn off stays off.

Kept somewhere else? `bytefield-content add <folder>` (in a terminal in the Bytefield folder) adds a pack or a mod from any
folder; `bytefield-content list` shows what there is.

## 6. Graphics

**OPTIONS → VIDEO → quality**: *Original* looks like the 2002 game; *Enhanced* adds better filtering, antialiasing,
shadows, reflections, shadows of clouds, heat shimmer and some grass. Each setting can be changed on its own. **EXPERIMENTAL** holds effects that are still being
tried. What each does: [features.md](features.md#graphics-what-is-drawn-and-how).

## 7. Playing with friends

One player chooses **CREATE GAME**; the others use **MULTIPLAYER → JOIN** with the host's IP address. Over the Internet
the host must open **UDP port 14567** in their router (and allow Bytefield in the Windows firewall when it asks).
Bytefield only plays with Bytefield, not with the original servers.

## 8. When something goes wrong

- In game, press **Ctrl+F11** right when it happens, type a short note, press Enter. It saves a screenshot and the game
  state in `%LOCALAPPDATA%\Bytefield\tickets`. A crash also leaves a folder there.
- The log of the last run is in `%LOCALAPPDATA%\Bytefield\logs\latest.log` (the one before, `previous.log`); a crash, at its end.
- Zip that folder and `latest.log` and attach them to a [bug report](../../CONTRIBUTING.md#reporting-a-bug).
- `bytefield-content verify` (in the terminal of step 5) says whether your game files match a version we know.

## Where things are

| What | Where |
|---|---|
| Content and graphics configuration | `%APPDATA%\Bytefield\content.cfg` |
| Menu options, controls | `%APPDATA%\Bytefield\settings.cfg`, `controls.cfg` |
| Marks, crashes | `%LOCALAPPDATA%\Bytefield\tickets` |
| Screenshots (Print Screen) | `%LOCALAPPDATA%\Bytefield\shots` |
| Log (last run and the one before) | `%LOCALAPPDATA%\Bytefield\logs` |

To start over from scratch, delete `%APPDATA%\Bytefield`.
