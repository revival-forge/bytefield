//! The 3-vector helpers several systems wrote out the same, gathered once.
//!
//! Only the variants that were the same to the bit live here. A port that needs another order of the sums or wider
//! intermediates keeps its own helper next to it, with a comment saying why: the order of a float sum changes the
//! last bit, and the engine's figures are compared with the original's to the bit (`contact`, `damage::normalize`,
//! `ref2`, the AI's planes). Each name here says its order when it matters.

/// A position, velocity or direction: x, y (up), z.
pub type V3 = [f32; 3];

/// a − b.
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// a · b, summed x, y, z.
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// a · b, summed z, y, x: the order the rigid bodies' and contacts' ports need.
pub fn dot_zyx(a: V3, b: V3) -> f32 {
    a[2] * b[2] + a[1] * b[1] + a[0] * b[0]
}

/// a × b in single precision.
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// |v| in single precision, summed x, y, z.
pub fn len(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// |v| with the squares summed in double precision (x, y, z), as on the x87.
pub fn len_f64(v: V3) -> f64 {
    let s = |x: f32| x as f64 * x as f64;
    (s(v[0]) + s(v[1]) + s(v[2])).sqrt()
}

/// v / |v| computed in double precision (`v` itself when it is 0).
pub fn normalize_f64(v: V3) -> V3 {
    let l = ((v[0] as f64).powi(2) + (v[1] as f64).powi(2) + (v[2] as f64).powi(2)).sqrt();
    if l == 0.0 {
        return v;
    }
    let k = 1.0 / l;
    [(v[0] as f64 * k) as f32, (v[1] as f64 * k) as f32, (v[2] as f64 * k) as f32]
}
