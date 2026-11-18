//! The engine's float matrices as it builds them (the engine's `ref2`), where the bench needs them to the
//! bit: `setRotation` (what `Object.rotation` gives an object placed in the level).
//!
//! `setRotation(m, ypr)`: from the identity, `yaw` about the matrix's own y axis, then `pitch`
//! about its x axis, then `roll` about its z axis; each `rotateAboutLine(m, axis, deg)` (nothing
//! at exactly 0) is m ← m × (Bᵀ · Rz(deg) · B), B the orthonormal basis of the axis
//! (`setRotateAboutLine`: `makeOrthonormalBasis` against z, or y when the axis is within 0.1 of
//! it), Rz from `fsincos` of the angle in radians (a float, ±1000 at most). Every element is
//! summed in the FPU, the first column's product first, and stored as a float.

pub type M3 = [[f32; 3]; 3];

/// a × b over the three rows (the matrix mult, `rotateZDeg`, `rotateAboutLine`)
fn mul(a: &M3, b: &M3) -> M3 {
    std::array::from_fn(|r| std::array::from_fn(|c| ((b[0][c] as f64 * a[r][0] as f64 + b[1][c] as f64 * a[r][1] as f64) + b[2][c] as f64 * a[r][2] as f64) as f32))
}

/// The vector normalize: unchanged within 2⁻²³ of unit length; zero when (nearly) zero.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l2 = (v[0] as f64 * v[0] as f64 + v[1] as f64 * v[1] as f64) + v[2] as f64 * v[2] as f64;
    let eps = f32::EPSILON as f64;
    if (l2 - 1.0).abs() < eps {
        return v;
    }
    if l2.abs() < eps {
        return [0.0; 3];
    }
    let k = 1.0 / l2.sqrt();
    v.map(|c| (c as f64 * k) as f32)
}

/// a × b, each component's difference in the FPU
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let p = |x: f32, y: f32| x as f64 * y as f64;
    [(p(a[1], b[2]) - p(a[2], b[1])) as f32, (p(a[2], b[0]) - p(a[0], b[2])) as f32, (p(a[0], b[1]) - p(a[1], b[0])) as f32]
}

/// `setRotateZDeg`: the angle in radians as a float (clamped to ±1000), its cosine and sine
/// stored as floats; rows (c, −s, 0), (s, c, 0), (0, 0, 1).
fn rot_z(deg: f32) -> M3 {
    let a = (f32::from_bits(0x3c8e_fa35) * deg).clamp(-1000.0, 1000.0) as f64;
    let (s, c) = (a.sin() as f32, a.cos() as f32);
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

/// `rotateAboutLine(m, axis, deg)`
pub(crate) fn rotate_about_line(m: &M3, axis: [f32; 3], deg: f32) -> M3 {
    if deg == 0.0 {
        return *m;
    }
    // setRotateAboutLine: makeOrthonormalBasis(w = axis, up): u = w × up, up = u × w, w = up × u
    let up = if axis[0].abs() < 0.1 && axis[1].abs() < 0.1 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
    let w = normalize(axis);
    let up = normalize(up);
    let u = normalize(cross(w, up));
    let up = normalize(cross(u, w));
    let w = normalize(cross(up, u));
    let b = [u, up, w];
    let bt: M3 = std::array::from_fn(|r| std::array::from_fn(|c| b[c][r]));
    let r = mul(&mul(&bt, &rot_z(deg)), &b);
    mul(m, &r)
}

/// `setRotation(m, (yaw, pitch, roll))` in degrees: the matrix's rows (its x, y and z axes).
pub fn set_rotation(ypr: [f32; 3]) -> M3 {
    let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    m = rotate_about_line(&m, m[1], ypr[0]);
    m = rotate_about_line(&m, m[0], ypr[1]);
    rotate_about_line(&m, m[2], ypr[2])
}

/// (a0·b0 + a1·b1) + a2·b2 summed in the FPU and stored as a float, rounded once: the products of
/// floats are exact in f64 and the FPU's 64-bit sums of them are too (in f64 the sum may round,
/// and then the float again: a double rounding the FPU does not make).
pub fn fpu_dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    // two-sum: s + e exactly
    let two_sum = |x: f64, y: f64| {
        let s = x + y;
        let bb = s - x;
        (s, (x - (s - bb)) + (y - bb))
    };
    let p = [0, 1, 2].map(|k| a[k] as f64 * b[k] as f64);
    let (s1, e1) = two_sum(p[0], p[1]);
    let (s2, e2) = two_sum(s1, p[2]);
    round_f32(s2, e1 + e2)
}

/// `ref2::interpolate(a, b, t)`: a·(1 − t) + b·t per coordinate on the FPU, stored as floats.
/// 1 − t needs up to ~29 bits, so a product can miss f64's 53: each is split exactly (FMA) and
/// the sum rounded once, as the FPU's 64 bits do.
pub fn fpu_lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    let (u, t) = (1.0 - t as f64, t as f64);
    let prod = |x: f64, y: f64| {
        let p = x * y;
        (p, x.mul_add(y, -p))
    };
    [0, 1].map(|k| {
        let (p0, e0) = prod(a[k] as f64, u);
        let (p1, e1) = prod(b[k] as f64, t);
        let s = p0 + p1;
        let bb = s - p0;
        let e = (p0 - (s - bb)) + (p1 - bb);
        round_f32(s, e + e0 + e1)
    })
}

/// hi + lo (|lo| far below hi's ulp) rounded once to a float, to nearest, ties to even.
pub fn round_f32(hi: f64, lo: f64) -> f32 {
    let r = hi as f32;
    if !r.is_finite() || r == 0.0 {
        return (hi + lo) as f32;
    }
    let d = (hi - r as f64) + lo;
    let up = f32::from_bits(if r > 0.0 { r.to_bits() + 1 } else { r.to_bits() - 1 });
    let down = f32::from_bits(if r > 0.0 { r.to_bits() - 1 } else { r.to_bits() + 1 });
    let (hu, hd) = ((up as f64 - r as f64) / 2.0, (r as f64 - down as f64) / 2.0);
    if d > hu || (d == hu && up.to_bits() & 1 == 0) {
        up
    } else if -d > hd || (-d == hd && down.to_bits() & 1 == 0) {
        down
    } else {
        r
    }
}

/// An object placed with `Object.absolutePosition` and `Object.rotation`: its matrix (row-major,
/// the position in the last row).
pub fn placed(pos: [f64; 3], ypr: [f64; 3]) -> [f32; 16] {
    let r = set_rotation(ypr.map(|a| a as f32));
    let mut m = [0.0f32; 16];
    for (k, row) in r.iter().enumerate() {
        m[4 * k..4 * k + 3].copy_from_slice(row);
    }
    m[12..15].copy_from_slice(&pos.map(|c| c as f32));
    m[15] = 1.0;
    m
}

#[cfg(test)]
mod tests {
    /// A Stalingrad building's (`Object.rotation`), as the server built it (its reference run's cover
    /// matrix, `ai_stalingrad_coverplan`).
    #[test]
    fn a_rotation_as_the_server_builds_it() {
        let m = super::set_rotation([-90.1944, 0.0740308, -1.52588e-5]);
        let theirs = [[-0.003_392_852_1, -2.663_157_7e-7, 0.999_994_1], [-0.001_292_074_2, 0.999_999_05, -4.117_525e-6], [-0.999_993_26, -0.001_292_080_7, -0.003_392_849_6]];
        assert_eq!(m, theirs);
    }
}
