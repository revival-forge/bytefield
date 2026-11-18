//! Without the optional bots the rest of the simulation can defer to: nothing to ask, and nothing to tell.
impl crate::Sim<'_> {
    /// This build has only the simple bots.
    pub fn enable_better_bots(&mut self) {}
    pub(crate) fn better_bots_on(&self) -> bool {
        false
    }
    pub(crate) fn restart_better_bots(&mut self) -> bool {
        false
    }
    pub(crate) fn better_bots_spawn(&self) -> bool {
        false
    }
    pub(crate) fn keep_points_on_map(&mut self, _: Vec<(usize, [f32; 3], bool)>, _: &mut Vec<usize>) {}
    pub(crate) fn tell_bot_seat_wanted(&mut self, _: usize, _: usize, _: usize, _: f32) -> bool {
        false
    }
    pub(crate) fn think_better_bots(&mut self) {}
}

impl crate::World {
    /// A pathfinding map of the level, if it ships it.
    pub fn pathfinding_file(&self, name: &str) -> Option<std::sync::Arc<Vec<u8>>> {
        let dir = self.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0);
        self.vfs.read(&format!("{dir}/Pathfinding/{name}")).ok().map(std::sync::Arc::new)
    }
}
