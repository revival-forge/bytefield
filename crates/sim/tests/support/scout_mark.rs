//! What the `scout_mark` tests of several layers share.

pub fn fire_binoculars(s: &mut bytefield_sim::Sim, sol: usize, frames: usize, pitch: f32) {
    use bytefield_sim::soldier::axis;
    for f in 0..frames {
        // the head turns for the first frames (down: positive), the fire key at the end
        let look = if f < 20 { pitch } else { 0.0 };
        let fire = if f + 3 >= frames && f + 1 < frames { 1.0 } else { 0.0 };
        s.vehicles[sol].movement.as_mut().unwrap().set_input(&[(axis::MOUSE_Y, look), (axis::FIRE, fire)]);
        s.step();
    }
}
