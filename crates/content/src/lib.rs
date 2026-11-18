//! What the player has and turns on, the OpenMW way: the install, the mods (the
//! install's and those kept apart), the graphics packs laid over them and the graphics profile.
//! Two separate axes: the packs change what is drawn, the profile how. Vanilla is no packs and
//! the `Original` profile. No Bevy: the client, the `bytefield-content` tool and, later, a
//! launcher share it.

pub mod catalog;
pub mod config;
pub mod detect;
pub mod discover;
pub mod fingerprint;
pub mod first_run;
pub mod graphics;

pub use config::{Config, Pack};
pub use graphics::{Graphics, Overrides, Profile};
