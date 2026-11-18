//! The first screen: where the game is, when nobody has said and it cannot be found. It needs no game
//! data: only Bevy's own font. A folder chosen goes to `content.cfg`, and the client starts again with it.
// TODO: say here which version the folder holds (`fingerprint::check_install`, seconds of reading: `bytefield-content verify` for now).

use std::path::{Path, PathBuf};

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use bytefield_content::first_run;

/// What it holds, apart from Bevy, so that the tests can use it.
#[derive(Default, Debug, PartialEq)]
pub struct Choice {
    /// the folders found
    pub found: Vec<PathBuf>,
    /// the one marked, then `typed`
    pub at: usize,
    pub typed: String,
    pub error: bool,
}

/// What one key does.
#[derive(Debug, PartialEq)]
pub enum Press {
    Char(char),
    Back,
    Up,
    Down,
    Enter,
}

impl Choice {
    /// A key: `Some` if it chose a folder that holds the game.
    pub fn press(&mut self, key: Press, holds_game: impl Fn(&Path) -> bool) -> Option<PathBuf> {
        self.error = false;
        match key {
            Press::Up => self.at = self.at.saturating_sub(1),
            Press::Down => self.at = (self.at + 1).min(self.found.len()),
            Press::Char(c) => {
                self.typed.push(c);
                self.at = self.found.len();
            }
            Press::Back => {
                self.typed.pop();
            }
            Press::Enter => {
                let p = self.found.get(self.at).cloned().unwrap_or_else(|| PathBuf::from(self.typed.trim()));
                if holds_game(&p) {
                    return Some(p);
                }
                self.error = true;
            }
        }
        None
    }

    /// The lines on screen.
    pub fn lines(&self) -> String {
        let t = |key| crate::lang::ours(key).unwrap_or(key);
        let mut s = format!("Bytefield\n\n{}\n{}\n\n", t("FIRST_WHERE"), t("FIRST_NO_DATA"));
        for (i, p) in self.found.iter().enumerate() {
            s += &format!("{} {}\n", if i == self.at { ">" } else { " " }, p.display());
        }
        s += &format!("{} {}: {}_\n\n", if self.at == self.found.len() { ">" } else { " " }, t("FIRST_FOLDER"), self.typed);
        if self.error {
            s += t("FIRST_NOT_THERE");
        }
        s += "\n\n";
        s += t("FIRST_KEYS");
        s
    }
}

#[derive(Resource)]
struct Screen(Choice);

#[derive(Component)]
struct Text2;

/// The screen, until a folder is chosen; then the same client again, with it. `args` are those it was started with.
pub fn run(args: Vec<String>, shot: Option<PathBuf>) {
    let found = first_run::candidates();
    crate::userlog::open(shot.is_some());
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(Window { title: crate::lang::PROJECT.into(), ..default() }), ..default() }).set(crate::userlog::plugin()));
    app.insert_resource(Screen(Choice { found, ..default() })).insert_resource(Restart(args)).insert_resource(Shot(shot, 0));
    app.add_systems(Startup, setup).add_systems(Update, (keys, shoot)).add_systems(PreUpdate, crate::graphics::ui_in_gamma);
    app.run();
}

#[derive(Resource)]
struct Restart(Vec<String>);

#[derive(Resource)]
struct Shot(Option<PathBuf>, u32);

fn setup(mut commands: Commands, screen: Res<Screen>, mut fonts: ResMut<Assets<Font>>) {
    crate::hud::default_font(&mut fonts);
    commands.spawn((Camera2d, bevy::camera::CompositingSpace::Srgb));
    commands.spawn((Text::new(screen.0.lines()), TextFont { font_size: FontSize::Px(20.0), ..default() }, Node { margin: UiRect::all(Val::Px(48.0)), ..default() }, Text2));
}

fn keys(mut screen: ResMut<Screen>, mut typed: MessageReader<KeyboardInput>, mut text: Query<&mut Text, With<Text2>>, restart: Res<Restart>, mut exit: MessageWriter<AppExit>) {
    for k in typed.read().filter(|k| k.state.is_pressed()) {
        let press = match &k.logical_key {
            Key::ArrowUp => Press::Up,
            Key::ArrowDown => Press::Down,
            Key::Backspace => Press::Back,
            Key::Enter => Press::Enter,
            Key::Escape => {
                exit.write(AppExit::Success);
                return;
            }
            Key::Space => Press::Char(' '),
            Key::Character(c) => match c.chars().next() {
                Some(c) => Press::Char(c),
                None => continue,
            },
            _ => continue,
        };
        if let Some(p) = screen.0.press(press, first_run::looks_installed) {
            if let Err(e) = first_run::remember(&p) {
                warn!("content.cfg: {e}");
            }
            start_again(&p, &restart.0);
            exit.write(AppExit::Success);
            return;
        }
    }
    if screen.is_changed() {
        for mut t in &mut text {
            t.0 = screen.0.lines();
        }
    }
}

/// The client again, with the folder.
fn start_again(install: &Path, args: &[String]) {
    let Ok(me) = std::env::current_exe() else { return };
    if let Err(e) = std::process::Command::new(me).arg(install).args(args).spawn() {
        warn!("could not start again: {e}");
    }
}

/// `BYTEFIELD_FIRST_SCREEN_SHOT=<png>`: a picture of it and out, to check it without anyone at the keyboard.
fn shoot(mut commands: Commands, mut shot: ResMut<Shot>, mut exit: MessageWriter<AppExit>) {
    let Some(path) = shot.0.clone() else { return };
    shot.1 += 1;
    if shot.1 == 30 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if shot.1 == 60 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_chosen_from_the_list_or_typed() {
        let mut c = Choice { found: vec!["/a".into(), "/b".into()], ..Default::default() };
        let yes = |_: &Path| true;
        assert_eq!(c.press(Press::Down, yes), None);
        assert_eq!(c.press(Press::Enter, yes), Some("/b".into()));
        assert_eq!(c.press(Press::Char('/'), yes), None);
        assert_eq!(c.press(Press::Char('x'), yes), None);
        assert_eq!(c.press(Press::Enter, yes), Some("/x".into()));
    }

    #[test]
    fn a_folder_without_the_game_is_said_so() {
        let mut c = Choice { typed: "/nothing".into(), ..Default::default() };
        assert_eq!(c.press(Press::Enter, |_| false), None);
        assert!(c.error && c.lines().contains("Mods/bf1942/init.con"));
    }
}
