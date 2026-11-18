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

pub fn parse(text: &str) -> Vec<SubShader> {
    let mut out = Vec::new();
    let mut cur: Option<SubShader> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("subshader") {
            let quoted: Vec<&str> = rest.split('"').skip(1).step_by(2).collect();
            cur = Some(SubShader {
                name: quoted.first().unwrap_or(&"").to_string(),
                shader: quoted.get(1).unwrap_or(&"").to_string(),
                diffuse: [1.0; 3],
                ..Default::default()
            });
            continue;
        }
        if line == "}" {
            if let Some(s) = cur.take() {
                out.push(s);
            }
            continue;
        }
        let Some(s) = cur.as_mut() else { continue };
        let line = line.trim_end_matches(';');
        let Some((key, value)) = line.split_once(char::is_whitespace) else { continue };
        let value = value.trim().trim_matches('"');
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
}
