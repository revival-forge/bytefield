//! `sf6 <installer.exe> <out>`: unpack a Setup Factory 5/6 installer without running it.
//!
//! Layout after the PE stub: the signature E0..E7, u32 count of setup files, then per file
//! name[260], u32 size, u32 crc, and a DCL-imploded body (irsetup.exe, irsetup.dat = the script,
//! images, language). The payload follows: the files of the script's list, in order, each one
//! either a DCL stream or stored raw (already-compressed archives are stored).
//!
//! The script (irsetup.dat, an MFC archive) lists per file a run of length-prefixed strings:
//! full source path, base name, source dir, suffix, one byte, then u32 size; the destination
//! (`%AppDir%\...`) comes later in the record. Only those fields are read.

use std::path::{Path, PathBuf};

use bytefield_extract::blast::explode;

struct Entry {
    name: String,
    dest: String,
    size: usize,
}

fn lp_string(d: &[u8], pos: usize) -> Option<(String, usize)> {
    let n = *d.get(pos)? as usize;
    let s = d.get(pos + 1..pos + 1 + n)?;
    Some((s.iter().map(|&b| b as char).collect(), pos + 1 + n))
}

/// The file list of the script, in payload order.
fn manifest(script: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 4 < script.len() {
        // a full source path: length byte, then "X:\"
        let is_path = script[pos + 2] == b':' && script[pos + 3] == b'\\' && script[pos + 1].is_ascii_alphabetic();
        let parsed = is_path.then(|| -> Option<(Entry, usize)> {
            let (full, p) = lp_string(script, pos)?;
            let (base, p) = lp_string(script, p)?;
            if !full.ends_with(&format!("\\{base}")) {
                return None;
            }
            let (_srcdir, p) = lp_string(script, p)?;
            let (_suffix, p) = lp_string(script, p)?;
            let p = p + 1;
            let size = u32::from_le_bytes(script.get(p..p + 4)?.try_into().ok()?) as usize;
            // the destination is the next "%AppDir%..." string
            let mark = script[p..].windows(8).position(|w| w == b"%AppDir%")? + p;
            let (dest, _) = lp_string(script, mark - 1)?;
            Some((Entry { name: base, dest, size }, p + 4))
        });
        match parsed.flatten() {
            Some((e, next)) => {
                out.push(e);
                pos = next;
            }
            None => pos += 1,
        }
    }
    out
}

fn safe_join(root: &Path, dest: &str, name: &str) -> Result<PathBuf, String> {
    let rel = dest.trim_start_matches("%AppDir%").replace('\\', "/");
    let mut p = root.to_path_buf();
    for part in rel.split('/').chain(std::iter::once(name)) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return Err(format!("refusing path {dest}\\{name}"));
        }
        p.push(part);
    }
    Ok(p)
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [exe, out] = args.as_slice() else { return Err("usage: sf6 <installer.exe> <out>".into()) };
    let data = std::fs::read(exe).map_err(|e| e.to_string())?;
    let out = PathBuf::from(out);
    let setup_dir = out.join("_setup");
    std::fs::create_dir_all(&setup_dir).map_err(|e| e.to_string())?;

    let sig = [0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7];
    let mut pos = data.windows(8).position(|w| w == sig).ok_or("no Setup Factory signature")? + 8;
    let u32_at = |p: usize| u32::from_le_bytes(data[p..p + 4].try_into().unwrap()) as usize;
    let count = u32_at(pos);
    pos += 4;
    let mut script = Vec::new();
    for _ in 0..count {
        let name: String = data[pos..pos + 260].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
        let size = u32_at(pos + 260);
        let (plain, _) = explode(&data[pos + 268..pos + 268 + size]).map_err(|e| format!("{name}: {e:?}"))?;
        if name.contains(['/', '\\']) || name.contains("..") {
            return Err(format!("refusing name {name}"));
        }
        std::fs::write(setup_dir.join(&name), &plain).map_err(|e| e.to_string())?;
        if name.eq_ignore_ascii_case("irsetup.dat") {
            script = plain;
        }
        pos += 268 + size;
    }

    let files = manifest(&script);
    println!("{} files in the script", files.len());
    let (mut packed, mut stored) = (0, 0);
    for e in &files {
        let target = safe_join(&out, &e.dest, &e.name)?;
        // a DCL stream starts with 0/1 (literal coding) and 4..6 (dictionary)
        let looks_dcl = data.get(pos).is_some_and(|&b| b <= 1) && data.get(pos + 1).is_some_and(|b| (4..=6).contains(b));
        let body = match looks_dcl.then(|| explode(&data[pos..])) {
            Some(Ok((plain, used))) if plain.len() == e.size => {
                pos += used;
                packed += 1;
                plain
            }
            _ => {
                let raw = data.get(pos..pos + e.size).ok_or_else(|| format!("{}: truncated installer", e.name))?;
                pos += e.size;
                stored += 1;
                raw.to_vec()
            }
        };
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&target, body).map_err(|e| e.to_string())?;
    }
    println!("{packed} imploded + {stored} stored files; {} of {} bytes used", pos, data.len());
    Ok(())
}
