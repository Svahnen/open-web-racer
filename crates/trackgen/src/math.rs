//! Deterministic maths: a small vector type, trigonometry built only from IEEE
//! basic operations (so results are bit-identical on every platform), a
//! SplitMix64 generator and seeded value noise.

use std::f64::consts::{PI, TAU};
use std::ops::{Add, Mul, Neg, Sub};

/// Double-precision 3-vector (x east, y up, z south in RCP coordinates).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const UP: V3 = v3(0.0, 1.0, 0.0);
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l > 1e-12 { self * (1.0 / l) } else { V3::UP }
    }
    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self + (o - self) * t
    }
    pub fn arr(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
    /// Horizontal (xz) length.
    pub fn hlen(self) -> f64 {
        (self.x * self.x + self.z * self.z).sqrt()
    }
}
impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// Sine from basic operations only (Taylor series after range reduction).
pub fn sin(x: f64) -> f64 {
    let mut r = x - (x / TAU).floor() * TAU; // [0, 2pi)
    if r > PI {
        r -= TAU;
    }
    // fold into [-pi/2, pi/2] for faster convergence
    if r > PI / 2.0 {
        r = PI - r;
    } else if r < -PI / 2.0 {
        r = -PI - r;
    }
    let r2 = r * r;
    let mut term = r;
    let mut sum = r;
    let mut k = 1.0;
    for _ in 0..11 {
        term *= -r2 / ((k + 1.0) * (k + 2.0));
        sum += term;
        k += 2.0;
    }
    sum
}

pub fn cos(x: f64) -> f64 {
    sin(x + PI / 2.0)
}

/// Arc tangent from basic operations only.
pub fn atan(x: f64) -> f64 {
    let neg = x < 0.0;
    let mut a = x.abs();
    let inv = a > 1.0;
    if inv {
        a = 1.0 / a;
    }
    // two half-angle reductions bring a below ~0.2
    a /= 1.0 + (1.0 + a * a).sqrt();
    a /= 1.0 + (1.0 + a * a).sqrt();
    let a2 = a * a;
    let mut term = a;
    let mut sum = a;
    for k in 1..12 {
        term *= -a2;
        sum += term / (2.0 * f64::from(k) + 1.0);
    }
    let mut r = 4.0 * sum;
    if inv {
        r = PI / 2.0 - r;
    }
    if neg { -r } else { r }
}

pub fn atan2(y: f64, x: f64) -> f64 {
    if x > 0.0 {
        atan(y / x)
    } else if x < 0.0 {
        if y >= 0.0 {
            atan(y / x) + PI
        } else {
            atan(y / x) - PI
        }
    } else if y > 0.0 {
        PI / 2.0
    } else if y < 0.0 {
        -PI / 2.0
    } else {
        0.0
    }
}

pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// SplitMix64 pseudo random generator.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f()
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn lattice(seed: u64, x: i64, y: i64) -> f64 {
    let h = mix(seed
        ^ (x as u64).wrapping_mul(0x8CB9_2BA7_2F3D_8DD7)
        ^ (y as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise in [0, 1]. `px`/`py` > 0 make the lattice periodic with that
/// period (in lattice cells), giving seamlessly tiling output.
pub fn value2(seed: u64, x: f64, y: f64, px: i64, py: i64) -> f64 {
    let xf = x.floor();
    let yf = y.floor();
    let (xi, yi) = (xf as i64, yf as i64);
    let (tx, ty) = (fade(x - xf), fade(y - yf));
    let wrap = |v: i64, p: i64| if p > 0 { v.rem_euclid(p) } else { v };
    let c = |dx: i64, dy: i64| lattice(seed, wrap(xi + dx, px), wrap(yi + dy, py));
    let a = lerp(c(0, 0), c(1, 0), tx);
    let b = lerp(c(0, 1), c(1, 1), tx);
    lerp(a, b, ty)
}

/// Fractal noise in [0, 1]; the base lattice has `px` x `py` cells when periodic.
pub fn fbm2(seed: u64, x: f64, y: f64, octaves: u32, px: i64, py: i64) -> f64 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let (mut fx, mut fy) = (x, y);
    let (mut ppx, mut ppy) = (px, py);
    for o in 0..octaves {
        sum += amp * value2(seed.wrapping_add(u64::from(o) * 7919), fx, fy, ppx, ppy);
        norm += amp;
        amp *= 0.5;
        fx *= 2.0;
        fy *= 2.0;
        ppx *= 2;
        ppy *= 2;
    }
    sum / norm
}

/// Deterministic 64-bit FNV-1a hash of a string (used for per-name seeds).
pub fn hash_str(s: &str) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trig_matches_reference() {
        for i in -200..200 {
            let x = f64::from(i) * 0.173;
            assert!((sin(x) - x.sin()).abs() < 1e-9, "sin {x}");
            assert!((cos(x) - x.cos()).abs() < 1e-9, "cos {x}");
            let y = f64::from(i) * 0.05 - 1.0;
            assert!((atan2(y, 0.7) - y.atan2(0.7)).abs() < 1e-9);
            assert!((atan2(y, -0.7) - y.atan2(-0.7)).abs() < 1e-9);
        }
    }

    #[test]
    fn periodic_noise_tiles() {
        let a = value2(3, 0.25, 0.5, 4, 4);
        let b = value2(3, 4.25, 4.5, 4, 4);
        assert!((a - b).abs() < 1e-12);
    }

    #[test]
    fn rng_is_reproducible() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        assert_eq!(a.next_u64(), b.next_u64());
        assert!((0.0..1.0).contains(&a.f()));
    }
}
