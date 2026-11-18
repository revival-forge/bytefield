//! `lexiconAll.dat` (the mod's texts): a count of entries and of strings in each (u32 each), then
//! UTF-16 strings, each ended by a 0: the first entry is `LANGUAGE` and the languages' names,
//! the others a key and its text in each language. Keys match whatever their case: the original finds `Aberdeen`'s
//! name under `ABERDEEN`.
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Lexicon {
    pub languages: Vec<String>,
    /// by the key in capitals
    pub texts: HashMap<String, Vec<String>>,
    /// bf1942's, under a mod's: an expansion's lexicon only has what it adds
    pub base: Option<Box<Lexicon>>,
}

impl Lexicon {
    pub fn parse(b: &[u8]) -> Option<Lexicon> {
        let n = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
        let units: Vec<u16> = b[8..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let strings: Vec<String> = units.split(|&u| u == 0).map(String::from_utf16_lossy).collect();
        let mut rows = strings.chunks_exact(n.max(1));
        let languages = rows.next()?[1..].to_vec();
        let texts = rows.map(|r| (r[0].to_uppercase(), r[1..].to_vec())).collect();
        Some(Lexicon { languages, texts, base: None })
    }

    /// `key` in `language` (by name, as the first entry spells it), else in the base's lexicon,
    /// else in the first language that has it.
    pub fn get(&self, key: &str, language: &str) -> Option<&str> {
        self.cell(key, language).or_else(|| self.base.as_ref()?.cell(key, language)).or_else(|| self.first(key)).or_else(|| self.base.as_ref()?.first(key))
    }

    fn cell(&self, key: &str, language: &str) -> Option<&str> {
        let i = self.languages.iter().position(|l| l.eq_ignore_ascii_case(language))?;
        self.texts.get(&key.to_uppercase())?.get(i).filter(|s| !s.is_empty()).map(String::as_str)
    }

    fn first(&self, key: &str) -> Option<&str> {
        self.texts.get(&key.to_uppercase())?.first().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lexicon_reads() {
        let mut b = vec![0u8; 4];
        b.extend(3u32.to_le_bytes());
        for s in ["LANGUAGE", "English", "Spanish", "RADIO_ROGER", "Roger that", "Roger"] {
            b.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
            b.extend([0, 0]);
        }
        let l = Lexicon::parse(&b).unwrap();
        assert_eq!(l.get("RADIO_ROGER", "Spanish"), Some("Roger"));
        assert_eq!(l.get("RADIO_ROGER", "Klingon"), Some("Roger that"));
        assert_eq!(l.get("Radio_Roger", "Spanish"), Some("Roger"));
    }

    #[test]
    fn the_base_fills_what_a_mod_lacks() {
        let lex = |rows: &[&str]| {
            let mut b = vec![0u8; 4];
            b.extend(3u32.to_le_bytes());
            for s in ["LANGUAGE", "English", "Spanish"].iter().chain(rows) {
                b.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
                b.extend([0, 0]);
            }
            Lexicon::parse(&b).unwrap()
        };
        let mut m = lex(&["ONLY_MOD", "Mine", "Mío", "NO_SPANISH", "Close", ""]);
        m.base = Some(Box::new(lex(&["ONLY_BASE", "Base", "Base es", "NO_SPANISH", "Close", "Cerrar"])));
        assert_eq!(m.get("ONLY_MOD", "Spanish"), Some("Mío"));
        assert_eq!(m.get("ONLY_BASE", "Spanish"), Some("Base es"));
        assert_eq!(m.get("NO_SPANISH", "Spanish"), Some("Cerrar"));
        assert_eq!(m.get("MISSING", "Spanish"), None);
    }
}

#[cfg(test)]
mod data_tests {
    #[test]
    fn the_games_lexicon() {
        let game = bytefield_testdata::require!(Base);
        let b = std::fs::read(game.join("Mods/bf1942/lexiconAll.dat")).unwrap();
        let l = super::Lexicon::parse(&b).expect("parse");
        assert_eq!(l.languages.len(), 8);
        assert_eq!(l.get("RADIO_LOCAL_MEDIC", "Spanish"), Some("¡Un médico!"));
    }
}
