//! `.rs` material files: one `subshader` per mesh material, next to each `.sm`.
//!
//! ```text
//! subshader "T-34_Hull_M1_Material0" "StandardMesh/Default"
//! {
//!     lighting true;
//!     materialDiffuse 1 1 1;
//!     texture "texture/T-34Texture";
//!     transparent true; twosided true; alphaTestRef 0.5; depthWrite false;
//! }
//! ```
//! Mesh materials are matched by name ([`crate::sm::MeshMaterial::name`]). Every key is kept as text (in lower
//! case).

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct SubShader {
    pub name: String,
    pub shader: String,
    pub texture: Option<String>,
    pub transparent: bool,
    pub two_sided: bool,
    pub alpha_test: Option<f32>,
    pub diffuse: [f32; 3],
    pub keys: HashMap<String, String>,
}

/// A statement of an `.rs` file, or the brace that closes a subshader.
enum Token<'a> {
    Statement(&'a str),
    Close,
}

/// Statements end at `;`, a line end or a brace (outside quotes): a subshader may close on its last statement's line
/// (`texture "texture/DesertCombat/H-6/H6_Glass";}`, the MD-500's glass) and a line may hold several statements.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' | '\n' | '{' | '}' if !quoted || c == '\n' => {
                quoted = false;
                let s = text[start..i].trim();
                if !s.is_empty() {
                    out.push(Token::Statement(s));
                }
                if c == '}' {
                    out.push(Token::Close);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let s = text[start..].trim();
    if !s.is_empty() {
        out.push(Token::Statement(s));
    }
    out
}

pub fn parse(text: &str) -> Vec<SubShader> {
    let mut out = Vec::new();
    let mut cur: Option<SubShader> = None;
    for token in tokens(text) {
        let line = match token {
            Token::Close => {
                out.extend(cur.take());
                continue;
            }
            Token::Statement(s) => s,
        };
        if let Some(rest) = line.strip_prefix("subshader") {
            // (one left open is kept, not lost)
            out.extend(cur.take());
            let quoted: Vec<&str> = rest.split('"').skip(1).step_by(2).collect();
            cur = Some(SubShader {
                name: quoted.first().unwrap_or(&"").to_string(),
                shader: quoted.get(1).unwrap_or(&"").to_string(),
                diffuse: [1.0; 3],
                ..Default::default()
            });
            continue;
        }
        let Some(s) = cur.as_mut() else { continue };
        let Some((key, value)) = line.split_once(char::is_whitespace) else { continue };
        // A stray quote leaves the statement's `;` inside the string (`texture ""texture/DesertCombat/cp_glass/cp_glass";`, the
        // canopies of DC's Su-25, F-16 and AV-8B): the value is whatever lies between the quotes and semicolons
        // TODO: the trailing trim also cuts `texture "texture/Ilyushin_Low_B "` (the Ilyushin's far hull) to a file that exists;
        // whether the original does the same is not seen: at 30 m the nearer hull is camouflaged in both, the far one never came
        // into frame. How: the Ilyushin at 60-90 m on the Wine side, or a red `Ilyushin_Low_B.dds` in a patch.
        let value = value.trim().trim_matches(|c| c == '"' || c == ';').trim();
        match key.to_ascii_lowercase().as_str() {
            "texture" => s.texture = Some(value.replace('\\', "/")),
            "transparent" => s.transparent = value.eq_ignore_ascii_case("true"),
            "twosided" => s.two_sided = value.eq_ignore_ascii_case("true"),
            "alphatestref" => s.alpha_test = value.parse().ok(),
            "materialdiffuse" => {
                let v: Vec<f32> = value.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    s.diffuse = [v[0], v[1], v[2]];
                }
            }
            _ => {}
        }
        s.keys.insert(key.to_ascii_lowercase(), value.to_string());
    }
    out.extend(cur);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_subshaders() {
        let t = "subshader \"A_Material0\" \"StandardMesh/Default\"\r\n{\r\n\tlighting true;\r\n\ttexture \"texture/T34\";\r\n\ttransparent true;\r\n\talphaTestRef 0.5;\r\n}\r\nsubshader \"A_Material1\" \"StandardMesh/Default\"\r\n{\r\n\tmaterialDiffuse 0.5 0.5 0.5;\r\n}\r\n";
        let s = super::parse(t);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "A_Material0");
        assert_eq!(s[0].texture.as_deref(), Some("texture/T34"));
        assert!(s[0].transparent);
        assert_eq!(s[0].alpha_test, Some(0.5));
        assert_eq!(s[1].diffuse, [0.5; 3]);
    }

    /// The MD-500's fuselage closes its glass on the texture's line; before, that subshader was lost (the glass drawn opaque white)
    #[test]
    fn closes_on_a_statement_line() {
        let t = "subshader \"F_Material0\" \"StandardMesh/Default\"\r\n{\r\n\ttransparent true;\r\n\ttexture \"texture/H-6/H6_Glass\";}\r\n\r\nsubshader \"F_Material1\" \"StandardMesh/Default\"\r\n{\r\n\ttexture \"texture/H-6/MD500_Main\"; twosided true;\r\n}\r\n";
        let s = super::parse(t);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].texture.as_deref(), Some("texture/H-6/H6_Glass"));
        assert!(s[0].transparent);
        assert_eq!(s[1].name, "F_Material1");
        assert_eq!(s[1].texture.as_deref(), Some("texture/H-6/MD500_Main"));
        assert!(s[1].two_sided);
    }

    /// DC's Su-25 cockpit: a doubled opening quote; before, the texture kept `";` and was not found (the canopy drawn opaque grey)
    #[test]
    fn doubled_quote() {
        let t = "subshader \"C_Material1\" \"StandardMesh/Default\"\r\n{\r\n\ttransparent true;\r\n\tenvmap true;\r\n\ttexture \"\"texture/DesertCombat/cp_glass/cp_glass\";\r\n}\r\nsubshader \"C_Material2\" \"StandardMesh/Default\"\r\n{\r\n\ttexture \"texture/su-25_cockpit\";\r\n}\r\n";
        let s = super::parse(t);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].texture.as_deref(), Some("texture/DesertCombat/cp_glass/cp_glass"));
        assert!(s[0].transparent);
        assert_eq!(s[0].keys.get("envmap").map(String::as_str), Some("true"));
        assert_eq!(s[1].texture.as_deref(), Some("texture/su-25_cockpit"));
        // (and the other stray quotes of the six mods: DC_Al_Nas' apartment walls, GC's Lancer HUD)
        for (line, tex) in [("texture texture/iggy-apt2walls2\";", "texture/iggy-apt2walls2"), ("texture \"texture/Lancer/lancerhud;", "texture/Lancer/lancerhud")] {
            let s = super::parse(&format!("subshader \"M\" \"StandardMesh/Default\"\r\n{{\r\n\t{line}\r\n\ttwosided true;\r\n}}\r\n"));
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].texture.as_deref(), Some(tex));
            assert!(s[0].two_sided);
        }
    }
}
