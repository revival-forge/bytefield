//! Where the game is, without being told, and where what the
//! client writes goes. No game data is needed to ask.

use std::path::{Path, PathBuf};

use bytefield_con::session::find_ci;

use crate::config::Config;

/// Whether a folder holds the game: `Mods/bf1942/init.con`, in any case.
pub fn looks_installed(dir: &Path) -> bool {
    find_ci(dir, "Mods").and_then(|m| find_ci(&m, "bf1942")).and_then(|b| find_ci(&b, "init.con")).is_some()
}

const GAME_FOLDERS: [&str; 4] = [
    "Program Files (x86)/EA GAMES/Battlefield 1942",
    "Program Files/EA GAMES/Battlefield 1942",
    "Program Files (x86)/Origin Games/Battlefield 1942",
    "Program Files/Origin Games/Battlefield 1942",
];

/// The folder in a `GAMEDIR` value of a Wine prefix's `system.reg`, `C:\\…` as under `drive_c`.
fn gamedir_in_reg(prefix: &Path, reg: &str) -> Option<PathBuf> {
    let at = reg.lines().position(|l| l.to_ascii_lowercase().contains("ea games\\\\battlefield 1942]"))?;
    let value = reg.lines().skip(at + 1).take_while(|l| !l.starts_with('[')).find_map(|l| l.strip_prefix("\"GAMEDIR\"=\"")?.strip_suffix('"'))?;
    let rest = value.split_once(":\\\\").map_or(value, |x| x.1).replace("\\\\", "/");
    Some(prefix.join("drive_c").join(rest))
}

/// The `GAMEDIR` value in what `reg query <key> /v GAMEDIR` prints (`    GAMEDIR    REG_SZ    D:\\Juegos\\BF1942`).
fn gamedir_in_reg_query(out: &str) -> Option<PathBuf> {
    let line = out.lines().find(|l| l.trim_start().to_ascii_uppercase().starts_with("GAMEDIR"))?;
    let (_, value) = line.split_once("REG_SZ")?;
    Some(PathBuf::from(value.trim())).filter(|p| !p.as_os_str().is_empty())
}

// TODO: not tried on a real Windows yet (the registry query has only been run). How: run the first-run test on Windows.
fn windows_gamedir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    ["HKLM\\Software\\Wow6432Node", "HKLM\\Software"].iter().find_map(|root| {
        let out = std::process::Command::new("reg").args(["query", &format!("{root}\\EA GAMES\\Battlefield 1942"), "/v", "GAMEDIR"]).output().ok()?;
        gamedir_in_reg_query(&String::from_utf8_lossy(&out.stdout))
    })
}

/// The folders worth looking in, for a home and the Wine prefixes given.
fn places(home: Option<&Path>, wine_prefix: Option<&Path>) -> Vec<PathBuf> {
    let mut prefixes: Vec<PathBuf> = wine_prefix.map(Path::to_path_buf).into_iter().collect();
    if let Some(h) = home {
        prefixes.push(h.join(".wine"));
        // (one prefix per game, as Lutris and others leave them)
        if let Ok(games) = std::fs::read_dir(h.join("Games")) {
            let mut more: Vec<PathBuf> = games.flatten().map(|e| e.path()).collect();
            more.sort();
            prefixes.extend(more);
        }
    }
    let mut v = Vec::new();
    for p in &prefixes {
        if let Ok(reg) = std::fs::read(p.join("system.reg")) {
            v.extend(gamedir_in_reg(p, &String::from_utf8_lossy(&reg)));
        }
        v.extend(GAME_FOLDERS.iter().map(|f| p.join("drive_c").join(f)));
    }
    if cfg!(windows) {
        // the installer's GAMEDIR, then the usual folders on every drive there is, C: first
        v.extend(windows_gamedir());
        let drives = (b'C'..=b'Z').map(|d| PathBuf::from(format!("{}:/", d as char))).filter(|d| d.exists());
        v.extend(drives.flat_map(|d| GAME_FOLDERS.iter().map(move |f| d.join(f))));
    }
    v.push("data/game".into());
    v
}

/// Where the game may be, of the folders that have it, first the likeliest and each once.
pub fn candidates() -> Vec<PathBuf> {
    // (Wine prefixes only from Linux: Wine hands a Windows build HOME and WINEPREFIX too, and the same install then shows
    // twice, through the prefix and through C:, so that none is picked on its own)
    let home = std::env::var_os("HOME").filter(|_| !cfg!(windows)).map(PathBuf::from);
    let prefix = std::env::var_os("WINEPREFIX").filter(|_| !cfg!(windows)).map(PathBuf::from);
    let mut seen: Vec<PathBuf> = Vec::new();
    for p in places(home.as_deref(), prefix.as_deref()) {
        if looks_installed(&p) && !seen.contains(&p) {
            seen.push(p);
        }
    }
    seen
}

/// The install to play with, if nobody has to be asked: `content.cfg`'s, or the only one there is.
pub fn resolve(cfg: Option<&Config>, found: &[PathBuf]) -> Option<PathBuf> {
    if let Some(i) = cfg.and_then(|c| c.install.as_ref()).filter(|i| looks_installed(i)) {
        return Some(i.clone());
    }
    (found.len() == 1).then(|| found[0].clone())
}

/// Saves the install chosen in `content.cfg`, keeping the rest of it.
pub fn remember(install: &Path) -> Result<PathBuf, String> {
    let mut c = Config::load().unwrap_or_default();
    c.install = Some(install.to_path_buf());
    c.save()
}

/// Where the client writes `what` (`marks`, `cache/bink`…): `data/out`, in a working tree that has one, or the user's cache folder.
pub fn out_dir(what: &str) -> PathBuf {
    let dev = Path::new("data/out");
    if dev.is_dir() {
        return dev.join(what);
    }
    let local = || std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let home = || std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"));
    let xdg = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
    let base = if cfg!(windows) { xdg.or_else(local).or_else(home) } else { xdg.or_else(home).or_else(local) };
    base.unwrap_or_else(std::env::temp_dir).join("bytefield").join(what)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bytefield-first-run-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn game(at: &Path) {
        std::fs::create_dir_all(at.join("mods/BF1942")).unwrap();
        std::fs::write(at.join("mods/BF1942/Init.con"), "").unwrap();
    }

    #[test]
    fn a_game_is_told_by_its_files_in_any_case() {
        let d = tmp("told");
        assert!(!looks_installed(&d));
        game(&d);
        assert!(looks_installed(&d));
    }

    #[test]
    fn reg_query_gives_the_gamedir() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\EA GAMES\\Battlefield 1942\r\n    GAMEDIR    REG_SZ    D:\\Juegos\\Battlefield 1942\r\n\r\n";
        assert_eq!(gamedir_in_reg_query(out), Some(PathBuf::from("D:\\Juegos\\Battlefield 1942")));
        assert_eq!(gamedir_in_reg_query("ERROR: The system was unable to find the specified registry key or value.\r\n"), None);
    }

    #[test]
    fn the_registry_of_a_prefix_says_where() {
        let home = tmp("reg");
        let prefix = home.join(".wine");
        let at = prefix.join("drive_c/Juegos/BF");
        game(&at);
        std::fs::write(prefix.join("system.reg"), "[Software\\\\Wow6432Node\\\\EA GAMES\\\\Battlefield 1942] 1\n\"GAMEDIR\"=\"C:\\\\Juegos\\\\BF\"\n").unwrap();
        let p = places(Some(&home), None);
        assert_eq!(p[0], at);
        assert!(p.iter().any(|x| x.ends_with("drive_c/Program Files (x86)/EA GAMES/Battlefield 1942")));
    }

    #[test]
    fn nobody_is_asked_when_there_is_one_or_it_is_saved() {
        let a = tmp("a");
        game(&a);
        assert_eq!(resolve(None, std::slice::from_ref(&a)), Some(a.clone()));
        assert_eq!(resolve(None, &[]), None);
        assert_eq!(resolve(None, &[a.clone(), "other".into()]), None);
        let c = Config { install: Some(a.clone()), ..Default::default() };
        assert_eq!(resolve(Some(&c), &[a.clone(), "other".into()]), Some(a.clone()));
        // a saved one that has gone: as if it were not saved
        let gone = Config { install: Some("/no/such/place".into()), ..Default::default() };
        assert_eq!(resolve(Some(&gone), &[]), None);
    }
}
