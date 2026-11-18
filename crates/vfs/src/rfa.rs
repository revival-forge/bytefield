//! `.rfa` (Refractor Flat Archive) reader.
//!
//! Layout, as documented by librfa (OpenField42, GPL-3) and checked against the retail discs:
//! ```text
//! u32 table offset, u32 compressed (0/1), 148 bytes of unknown purpose (a signature?)
//! … file data …
//! at table offset: u32 count, then per file:
//!     u32 name length, name (no terminator), u32 csize, u32 ucsize, u32 offset, 3 x u32 unknown
//! ```
//! In a compressed archive each file starts with its segments: u32 count, then per segment
//! u32 csize, u32 ucsize, u32 data offset (relative to the end of this header block). Each
//! segment is an independent LZO1X stream. Everything is little-endian.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::lzo;

#[derive(Debug, Clone)]
pub struct Entry {
    /// Path inside the archive, as stored (mixed case, `/` separators).
    pub name: String,
    pub csize: u32,
    pub ucsize: u32,
    pub offset: u32,
}

pub struct Archive {
    pub path: PathBuf,
    pub compressed: bool,
    pub entries: Vec<Entry>,
    file: Mutex<File>,
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn u32_at(buf: &[u8], pos: usize) -> io::Result<u32> {
    buf.get(pos..pos + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| invalid("truncated"))
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Archive> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let len = file.metadata()?.len();
        let mut head = [0u8; 8];
        file.read_exact(&mut head)?;
        let table = u32_at(&head, 0)? as u64;
        let compressed = match u32_at(&head, 4)? {
            0 => false,
            1 => true,
            n => return Err(invalid(format!("{}: compression flag {n}", path.display()))),
        };
        if table >= len {
            return Err(invalid(format!("{}: table offset {table} past the end", path.display())));
        }
        let mut buf = Vec::with_capacity((len - table) as usize);
        file.seek(SeekFrom::Start(table))?;
        file.read_to_end(&mut buf)?;

        let count = u32_at(&buf, 0)? as usize;
        let mut pos = 4;
        let mut entries = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            let n = u32_at(&buf, pos)? as usize;
            pos += 4;
            let name = buf.get(pos..pos + n).ok_or_else(|| invalid("truncated name"))?;
            // names are ASCII in practice; latin-1 keeps any odd byte instead of failing
            let name: String = name.iter().map(|&b| b as char).collect();
            pos += n;
            entries.push(Entry {
                name: name.replace('\\', "/"),
                csize: u32_at(&buf, pos)?,
                ucsize: u32_at(&buf, pos + 4)?,
                offset: u32_at(&buf, pos + 8)?,
            });
            pos += 24; // 3 known + 3 unknown u32
        }
        Ok(Archive { path, compressed, entries, file: Mutex::new(file) })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name))
    }

    pub fn read(&self, entry: &Entry) -> io::Result<Vec<u8>> {
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(entry.offset as u64))?;
        if !self.compressed {
            let mut out = vec![0; entry.ucsize as usize];
            file.read_exact(&mut out)?;
            return Ok(out);
        }
        let mut raw = vec![0; entry.csize as usize];
        file.read_exact(&mut raw)?;
        drop(file);
        decode_segments(&raw, entry.ucsize as usize)
            .map_err(|e| invalid(format!("{}: {}: {e}", self.path.display(), entry.name)))
    }
}

/// Decode a compressed file's block: segment headers followed by LZO1X streams.
pub fn decode_segments(raw: &[u8], ucsize: usize) -> io::Result<Vec<u8>> {
    let count = u32_at(raw, 0)? as usize;
    let base = 4 + count * 12;
    let mut out = Vec::with_capacity(ucsize.min(1 << 28));
    for i in 0..count {
        let h = 4 + i * 12;
        let (cs, us, ofs) = (u32_at(raw, h)? as usize, u32_at(raw, h + 4)? as usize, u32_at(raw, h + 8)? as usize);
        let data = raw.get(base + ofs..base + ofs + cs).ok_or_else(|| invalid(format!("segment {i} out of bounds")))?;
        out.extend(lzo::decompress(data, us).map_err(|e| invalid(format!("segment {i}: {e}")))?);
    }
    if out.len() != ucsize {
        return Err(invalid(format!("decoded {} bytes, expected {ucsize}", out.len())));
    }
    Ok(out)
}
