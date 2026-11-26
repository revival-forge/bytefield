//! The texts of the HUD and the menus in the chosen language: the lexicon's
//! (`lexiconAll.dat`) where the original has the key, else ours, below, for what it says
//! otherwise or not at all (English when we lack the language). Languages outside the Latin
//! alphabet are out of scope (decision of the user, 24-oct): their glyphs need other fonts.

use bytefield_formats::lexicon::Lexicon;

/// Ours: a key, then its text in English, Spanish, German, French and Italian. `{0}`, `{1}`… are
/// filled by [`fill`].
const OURS: &[(&str, [&str; 5])] = &[
    ("TICKET_LAST_CRASH", ["The last game hit an error. It is written down in:", "La última partida tuvo un error. Está apuntado en:", "Im letzten Spiel gab es einen Fehler. Er steht in:", "La dernière partie a eu une erreur. C'est noté dans :", "L'ultima partita ha avuto un errore. È annotato in:"]),
    ("TICKET_NOTE", ["Note (Enter saves, Esc none):", "Nota (Intro guarda, Esc sin nota):", "Notiz (Eingabe speichert, Esc keine):", "Note (Entrée enregistre, Échap aucune) :", "Nota (Invio salva, Esc nessuna):"]),
    ("PAUSED", ["PAUSED (P)", "EN PAUSA (P)", "PAUSIERT (P)", "EN PAUSE (P)", "IN PAUSA (P)"]),
    ("LANGUAGE", ["LANGUAGE", "IDIOMA", "SPRACHE", "LANGUE", "LINGUA"]),
    ("FIRST_WHERE", ["Where is your Battlefield 1942 installed?", "¿Dónde está instalado tu Battlefield 1942?", "Wo ist dein Battlefield 1942 installiert?", "Où est installé votre Battlefield 1942 ?", "Dove è installato il tuo Battlefield 1942?"]),
    ("FIRST_NO_DATA", ["Bytefield comes with no game data: it plays from your own copy.", "Bytefield no trae datos del juego: juega con tu propia copia.", "Bytefield enthält keine Spieldaten: Es spielt mit deiner eigenen Kopie.", "Bytefield ne contient aucune donnée de jeu : il utilise votre propre copie.", "Bytefield non include dati di gioco: usa la tua copia."]),
    ("FIRST_FOLDER", ["Folder", "Carpeta", "Ordner", "Dossier", "Cartella"]),
    ("FIRST_NOT_THERE", ["That folder has no Mods/bf1942/init.con.", "Esa carpeta no tiene Mods/bf1942/init.con.", "Dieser Ordner enthält kein Mods/bf1942/init.con.", "Ce dossier ne contient pas Mods/bf1942/init.con.", "Quella cartella non contiene Mods/bf1942/init.con."]),
    ("FIRST_KEYS", ["Up and down to choose, type a folder, Enter to accept, Esc to leave.", "Arriba y abajo para elegir, escribe una carpeta, Intro para aceptar, Esc para salir.", "Mit Auf und Ab wählen, Ordner eingeben, Eingabe zum Bestätigen, Esc zum Beenden.", "Haut et bas pour choisir, saisissez un dossier, Entrée pour valider, Échap pour quitter.", "Su e giù per scegliere, scrivi una cartella, Invio per accettare, Esc per uscire."]),
    ("ROUND_OVER", ["ROUND OVER", "FIN DE LA RONDA", "RUNDE BEENDET", "FIN DE LA MANCHE", "FINE DEL ROUND"]),
    ("AXIS_WINS", ["The Axis wins", "Gana el Eje", "Die Achse gewinnt", "L'Axe gagne", "Vince l'Asse"]),
    ("ALLIES_WIN", ["The Allies win", "Ganan los Aliados", "Die Alliierten gewinnen", "Les Alliés gagnent", "Vincono gli Alleati"]),
    ("DRAW", ["Draw", "Empate", "Unentschieden", "Match nul", "Pareggio"]),
    ("YOUR_SIDE", ["your side", "tu bando", "deine Seite", "votre camp", "la tua parte"]),
    ("OTHER_SIDE", ["the other side", "el otro bando", "die andere Seite", "l'autre camp", "l'altra parte"]),
    ("NEXT_ROUND", ["next round in {0} s", "siguiente ronda en {0} s", "nächste Runde in {0} s", "manche suivante dans {0} s", "prossimo round tra {0} s"]),
    ("TICKETS", ["tickets", "tickets", "Tickets", "tickets", "ticket"]),
    ("YOU", ["you", "tú", "du", "vous", "tu"]),
    ("BOT", ["bot", "bot", "Bot", "bot", "bot"]),
    ("PLAYER", ["player", "jugador", "Spieler", "joueur", "giocatore"]),
    ("SUICIDE", ["{0} killed himself", "{0} se mató", "{0} hat sich getötet", "{0} s'est tué", "{0} si è ucciso"]),
    ("DIED", ["{0} died", "{0} murió", "{0} ist gestorben", "{0} est mort", "{0} è morto"]),
    ("LOST_FLAG", ["{0} lost {1}", "{1}: la pierde {0}", "{0} verliert {1}", "{0} perd {1}", "{0} perde {1}"]),
    ("THE_AXIS", ["the Axis", "el Eje", "die Achse", "l'Axe", "l'Asse"]),
    ("THE_ALLIES", ["the Allies", "los Aliados", "die Alliierten", "les Alliés", "gli Alleati"]),
    ("YOU_DIED", ["You are down", "Has caído", "Du bist gefallen", "Vous êtes tombé", "Sei caduto"]),
    ("FLAG_HOME", ["{0}: at home", "{0}: en casa", "{0}: zu Hause", "{0} : au camp", "{0}: alla base"]),
    ("FLAG_CARRIED", ["{0}: carried by {1}", "{0}: la lleva {1}", "{0}: getragen von {1}", "{0} : porté par {1}", "{0}: portata da {1}"]),
    ("FLAG_DOWN", ["{0}: on the ground ({1} s)", "{0}: en el suelo ({1} s)", "{0}: am Boden ({1} s)", "{0} : au sol ({1} s)", "{0}: a terra ({1} s)"]),
    ("TIME_LEFT", ["{0} left", "quedan {0}", "noch {0}", "reste {0}", "restano {0}"]),
    ("BASE", ["base", "base", "Basis", "base", "base"]),
    ("GRAPHICS", ["GRAPHICS", "GRÁFICOS", "GRAFIK", "GRAPHISMES", "GRAFICA"]),
    ("GRAPHICS_ORIGINAL", ["Original", "Original", "Original", "Original", "Originale"]),
    ("GRAPHICS_ENHANCED", ["Enhanced", "Mejorado", "Verbessert", "Amélioré", "Migliorata"]),
    ("HD_PACKS", ["HD PACKS", "PACKS HD", "HD-PAKETE", "PACKS HD", "PACCHETTI HD"]),
    // a pack's or a mod's folder typed on the options' page (`menu::add_folder`)
    ("ADD_FOLDER", ["ADD FOLDER", "AÑADIR CARPETA", "ORDNER HINZUFÜGEN", "AJOUTER DOSSIER", "AGGIUNGI CARTELLA"]),
    ("FOLDER_NOT_ADDED", ["Not added", "No añadida", "Nicht hinzugefügt", "Non ajouté", "Non aggiunta"]),
    // the graphics pages (`graphics.rs`); the rest of their names are the original's `VIDEO_OPTIONS_*`
    // (the original's `MENU_GENERAL_OPTIONS` is «PERSONALIZAR» in Spanish: next to «Personalizada», confusing)
    ("GFX_GENERAL", ["GENERAL", "GENERAL", "ALLGEMEIN", "GÉNÉRAL", "GENERALE"]),
    ("GFX_EXPERIMENTAL", ["EXPERIMENTAL", "EXPERIMENTAL", "EXPERIMENTELL", "EXPÉRIMENTAL", "SPERIMENTALE"]),
    ("GFX_SHARPEN", ["SHARPENING", "NITIDEZ", "SCHÄRFE", "NETTETÉ", "NITIDEZZA"]),
    ("GFX_ANISO", ["ANISOTROPIC FILTER", "FILTRO ANISÓTROPO", "ANISOTROPER FILTER", "FILTRE ANISOTROPE", "FILTRO ANISOTROPICO"]),
    ("GFX_MIP_BIAS", ["TEXTURE DETAIL", "DETALLE TEXTURAS", "TEXTURDETAIL", "DÉTAIL TEXTURES", "DETTAGLIO TEXTURE"]),
    ("GFX_REFLECTIONS", ["WATER REFLECTIONS", "REFLEJOS EN AGUA", "WASSERSPIEGELUNG", "REFLETS DE L'EAU", "RIFLESSI ACQUA"]),
    ("GFX_COVERAGE", ["SMOOTH FOLIAGE", "FOLLAJE SUAVE", "WEICHES LAUB", "FEUILLAGE LISSE", "FOGLIAME MORBIDO"]),
    ("GFX_BLOOM", ["BLOOM", "RESPLANDOR", "BLOOM", "LUEUR", "BAGLIORE"]),
    ("GFX_LOOK", ["COLOUR", "COLOR", "FARBE", "COULEUR", "COLORE"]),
    ("GFX_NEUTRAL", ["Neutral", "Neutro", "Neutral", "Neutre", "Neutro"]),
    ("GFX_VIVID", ["Vivid", "Vivo", "Lebhaft", "Vif", "Vivace"]),
    ("GFX_WARM", ["Warm", "Cálido", "Warm", "Chaud", "Caldo"]),
    ("GFX_VIGNETTE", ["VIGNETTE", "VIÑETA", "VIGNETTE", "VIGNETTAGE", "VIGNETTATURA"]),
    ("GFX_SSAO", ["AMBIENT OCCLUSION", "OCLUSIÓN AMBIENTAL", "UMGEBUNGSVERDECKUNG", "OCCLUSION AMBIANTE", "OCCLUSIONE AMBIENTALE"]),
    ("GFX_CONTACT_SHADOWS", ["CONTACT SHADOWS", "SOMBRAS DE CONTACTO", "KONTAKTSCHATTEN", "OMBRES DE CONTACT", "OMBRE DI CONTATTO"]),
    ("GFX_WATER_FX", ["WATER FOAM AND REFRACTION", "ESPUMA Y REFRACCIÓN", "GISCHT UND BRECHUNG", "ÉCUME ET RÉFRACTION", "SCHIUMA E RIFRAZIONE"]),
    ("GFX_GRASS", ["GRASS", "HIERBA", "GRAS", "HERBE", "ERBA"]),
    ("GFX_HEAT", ["HEAT HAZE", "DISTORSIÓN POR CALOR", "HITZEFLIMMERN", "BRUME DE CHALEUR", "FOSCHIA DI CALORE"]),
    ("GFX_RELIEF", ["TERRAIN RELIEF", "RELIEVE DEL TERRENO", "GELÄNDERELIEF", "RELIEF DU TERRAIN", "RILIEVO DEL TERRENO"]),
    ("GFX_AERIAL", ["AERIAL PERSPECTIVE", "PERSPECTIVA AÉREA", "LUFTPERSPEKTIVE", "PERSPECTIVE AÉRIENNE", "PROSPETTIVA AEREA"]),
    ("GFX_DECALS", ["IMPACT MARKS", "MARCAS DE IMPACTOS", "EINSCHLAGSPUREN", "TRACES D'IMPACT", "SEGNI DEGLI IMPATTI"]),
    ("GFX_CLOUDS", ["CLOUD SHADOWS", "SOMBRAS DE NUBES", "WOLKENSCHATTEN", "OMBRES DES NUAGES", "OMBRE DELLE NUVOLE"]),
    ("GFX_DYN_LIGHTS", ["DYNAMIC LIGHTS", "LUCES DINÁMICAS", "DYNAMISCHE LICHTER", "LUMIÈRES DYNAMIQUES", "LUCI DINAMICHE"]),
    ("GFX_OBJECT_SHADOWS", ["OBJECT SHADOWS", "SOMBRAS DE OBJETOS", "OBJEKTSCHATTEN", "OMBRES DES OBJETS", "OMBRE DEGLI OGGETTI"]),
    ("GFX_MESH_SHADE", ["ORIGINAL BAKED SHADOW ON OBJECTS", "SOMBRA COCIDA EN OBJETOS", "GEBACKENER SCHATTEN AUF OBJEKTEN", "OMBRE PRÉCALCULÉE SUR LES OBJETS", "OMBRA PRECALCOLATA SUGLI OGGETTI"]),
    ("GFX_WATER_GAMMA", ["ORIGINAL BLENDING", "MEZCLA ORIGINAL", "ORIGINALE MISCHUNG", "MÉLANGE D'ORIGINE", "MISCELA ORIGINALE"]),
    ("GFX_FIXED_LIGHT", ["ORIGINAL LIGHTING", "LUZ DEL ORIGINAL", "ORIGINALBELEUCHTUNG", "ÉCLAIRAGE D'ORIGINE", "ILLUMINAZIONE ORIGINALE"]),
    ("GFX_ENVMAP", ["VEHICLE REFLECTIONS", "REFLEJOS DE VEHÍCULOS", "FAHRZEUGSPIEGELUNG", "REFLETS DES VÉHICULES", "RIFLESSI DEI VEICOLI"]),
    ("GFX_LIGHTMAPS", ["LIGHTMAPS", "MAPAS DE LUZ", "LIGHTMAPS", "LIGHTMAPS", "LIGHTMAP"]),
    ("GFX_OIT", ["EXACT TRANSPARENCY", "TRANSPARENCIA EXACTA", "EXAKTE TRANSPARENZ", "TRANSPARENCE EXACTE", "TRASPARENZA ESATTA"]),
    ("GFX_SOFT_PARTICLES", ["SOFT PARTICLES", "PARTÍCULAS SUAVES", "WEICHE PARTIKEL", "PARTICULES DOUCES", "PARTICELLE MORBIDE"]),
    ("GFX_DOF", ["AIM DEPTH OF FIELD", "DESENF. AL APUNTAR", "TIEFENSCHÄRFE", "FLOU DE VISÉE", "SFOCATURA MIRA"]),
    ("GFX_RENDER_SCALE", ["RENDER SCALE", "ESCALA DE RENDER", "RENDERSKALIERUNG", "ÉCHELLE DE RENDU", "SCALA DI RENDER"]),
    ("GFX_MOTION_BLUR", ["MOTION BLUR", "DESENF. MOVIMIENTO", "BEWEGUNGSUNSCHÄRFE", "FLOU DE MOUVEMENT", "SFOCATURA MOVIMENTO"]),
    ("GFX_FOV", ["FIELD OF VIEW", "CAMPO DE VISIÓN", "SICHTFELD", "CHAMP DE VISION", "CAMPO VISIVO"]),
    ("GFX_VSYNC", ["VSYNC", "SINCR. VERTICAL", "VSYNC", "SYNCHRO VERTICALE", "VSYNC"]),
    ("GFX_FULLSCREEN", ["FULL SCREEN", "PANTALLA COMPLETA", "VOLLBILD", "PLEIN ÉCRAN", "SCHERMO INTERO"]),
    // (DC_Final's lexicon, over bf1942's, lacks it)
    ("RESPAWN_SCOREBOARD", ["SCORE BOARD", "RESULTADOS", "SCORE-BOARD", "SCORE", "TABELLA PUNTEGGI"]),
    (
        "ABOUT",
        [
            "{} {} · GPL-3.0-or-later\nNot affiliated with EA or DICE.\nBattlefield 1942 is a trademark of Electronic Arts.",
            "{} {} · GPL-3.0-or-later\nNo afiliado a EA ni a DICE.\nBattlefield 1942 es una marca de Electronic Arts.",
            "{} {} · GPL-3.0-or-later\nNicht mit EA oder DICE verbunden.\nBattlefield 1942 ist eine Marke von Electronic Arts.",
            "{} {} · GPL-3.0-or-later\nNon affilié à EA ni à DICE.\nBattlefield 1942 est une marque d'Electronic Arts.",
            "{} {} · GPL-3.0-or-later\nNon affiliato a EA né a DICE.\nBattlefield 1942 è un marchio di Electronic Arts.",
        ],
    ),
    (
        "GFX_NOTE",
        [
            "* From the next round. Smooth foliage needs MSAA (SMAA does not do it).",
            "* Desde la siguiente ronda. El follaje suave necesita MSAA (con SMAA no sale).",
            "* Ab der nächsten Runde. Weiches Laub braucht MSAA (SMAA reicht nicht).",
            "* À partir de la manche suivante. Le feuillage lisse demande le MSAA (pas le SMAA).",
            "* Dal prossimo round. Il fogliame morbido richiede MSAA (non SMAA).",
        ],
    ),
];

/// The project's name as the player sees it (window title, «About»): one place, for the rename before publishing.
pub const PROJECT: &str = "Bytefield";

/// «About» under the general options: version, licence and the trademark notice ({} is the project, then the commit).
pub fn about() -> String {
    let t = match ours("ABOUT") {
        Some(t) => t,
        None => return String::new(),
    };
    t.replacen("{}", PROJECT, 1).replacen("{}", crate::tickets::COMMIT, 1)
}

/// The lexicon's languages we show (its names, as its first entry spells them).
const COLUMNS: [&str; 5] = ["English", "Spanish", "German", "French", "Italian"];

/// The lexicon of a mod's files (the mod's own, which the vfs puts over bf1942's).
pub fn load(vfs: &bytefield_vfs::Vfs) -> Lexicon {
    let mut lex = vfs.read("lexiconAll.dat").ok().and_then(|b| Lexicon::parse(&b)).unwrap_or_default();
    // an expansion's or a mod's lexicon only has what it adds: the rest is bf1942's (as the menu's `text`)
    let own = vfs.origin("lexiconAll.dat").map(std::path::PathBuf::from);
    let beside = own.as_deref().and_then(|p| p.parent()?.parent()).map(std::path::Path::to_path_buf);
    let base = beside.into_iter().chain(bytefield_con::session::mods_dirs()).find_map(|d| bytefield_con::session::resolve_ci(&d, "bf1942/lexiconAll.dat"));
    if let Some(base) = base.filter(|b| Some(b) != own.as_ref()) {
        lex.base = std::fs::read(base).ok().and_then(|b| Lexicon::parse(&b)).map(Box::new);
    }
    lex
}

/// Ours, in the chosen language, if we have the key.
pub fn ours(key: &str) -> Option<&'static str> {
    let l = crate::settings::language();
    let col = COLUMNS.iter().position(|c| c.eq_ignore_ascii_case(&l)).unwrap_or(0);
    OURS.iter().find(|(k, _)| *k == key).map(|(_, t)| t[col])
}

/// The language the system asks for (`LC_ALL`, `LC_MESSAGES`, `LANG`; on Windows, which sets none of them, the user's
/// `LocaleName`; on the first screen nobody has chosen one yet), else English.
pub fn system_language() -> &'static str {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).or_else(windows_locale).unwrap_or_default();
    crate::settings::language_name(locale.split(['_', '-', '.', '@']).next().unwrap_or("")).unwrap_or("English")
}

/// The value in what `reg query "HKCU\\Control Panel\\International" /v LocaleName` prints (`    LocaleName    REG_SZ    es-ES`).
fn locale_in_reg_query(out: &str) -> Option<String> {
    let line = out.lines().find(|l| l.trim_start().to_ascii_lowercase().starts_with("localename"))?;
    Some(line.split_once("REG_SZ")?.1.trim().to_string()).filter(|v| !v.is_empty())
}

// TODO: not tried on a real Windows yet
fn windows_locale() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("reg").args(["query", "HKCU\\Control Panel\\International", "/v", "LocaleName"]).output().ok()?;
    locale_in_reg_query(&String::from_utf8_lossy(&out.stdout))
}

/// `key`: the lexicon's text, else ours, else the key itself.
pub fn tr(lex: &Lexicon, key: &str) -> String {
    lex.get(key, &crate::settings::language()).or_else(|| ours(key)).unwrap_or(key).trim().to_string()
}

/// A level's name when its lexicon has no text for it: the level's folder name in capitals and with spaces (`GC_Mini_Caves`: GC MINI CAVES).
/// The briefing's title; the loading panel writes the name as it is (`LOADING DC_Al_Nas` in the original).
pub fn level_fallback(level: &str) -> String {
    level.replace('_', " ").to_uppercase()
}

/// A column's heading: [`tr`] on one line and without the colon some languages give it.
pub fn heading(lex: &Lexicon, key: &str) -> String {
    tr(lex, key).replace(['\n', '\r'], " ").trim().trim_end_matches(':').trim().to_string()
}

/// `{0}`, `{1}`… of `text` filled with `args`.
pub fn fill(text: &str, args: &[&str]) -> String {
    args.iter().enumerate().fold(text.to_string(), |t, (i, a)| t.replace(&format!("{{{i}}}"), a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_its_placeholders_in_each_language() {
        for (k, t) in OURS {
            let n = |s: &str| (0..3).filter(|i| s.contains(&format!("{{{i}}}"))).count();
            assert!(t.iter().all(|s| n(s) == n(t[0])), "{k}");
        }
        assert_eq!(fill("{1}: la pierde {0}", &["el Eje", "Tobruk"]), "Tobruk: la pierde el Eje");
        assert_eq!(locale_in_reg_query("\r\nHKEY_CURRENT_USER\\Control Panel\\International\r\n    LocaleName    REG_SZ    es-ES\r\n").as_deref(), Some("es-ES"));
    }
}
