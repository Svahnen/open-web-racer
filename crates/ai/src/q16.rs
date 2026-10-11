//! Q16 fixed-point helpers (16 fraction bits; `ONE` = 1.0).
//!
//! `q(x, y)` multiplies with a 64-bit product, floors toward negative
//! infinity and adds the bit just below the binary point (round half up),
//! wrapping the result to 32 bits.

/// 1.0 in Q16.
pub const ONE: i32 = 65_536;

/// Low 32 bits of `x`, read as signed.
pub fn wrap32(x: i64) -> i32 {
    x as i32
}

/// Low 16 bits of `x`, read as signed.
pub fn wrap16(x: i32) -> i32 {
    i32::from(x as i16)
}

/// Rounded Q16 product.
pub fn q(x: i32, y: i32) -> i32 {
    let product = i64::from(x) * i64::from(y);
    wrap32((product >> 16) + ((product >> 15) & 1))
}

/// Reciprocal of a Q16 value: `floor(2^32 / x)` wrapped to 32 bits.
pub fn recip(x: i32) -> i32 {
    if x == 0 {
        return 0;
    }
    wrap32(floor_div(1i64 << 32, i64::from(x)))
}

fn floor_div(a: i64, b: i64) -> i64 {
    let d = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        d - 1
    } else {
        d
    }
}

/// Q16 from a float, saturating.
pub fn from_f64(x: f64) -> i32 {
    (x * f64::from(ONE))
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// Float from Q16.
pub fn to_f64(x: i32) -> f64 {
    f64::from(x) / f64::from(ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_rounds_half_up_and_floors_negatives() {
        assert_eq!(q(ONE, ONE), ONE);
        assert_eq!(q(3, 32768), 2); // 1.5 rounds up
        assert_eq!(q(-3, 32768), -1); // floor(-1.5) = -2, plus the bit-15 round = -1
        assert_eq!(q(ONE * 70, ONE), ONE * 70);
    }

    #[test]
    fn reciprocal_and_wraps() {
        assert_eq!(recip(ONE * 2), ONE / 2);
        assert_eq!(wrap16(0x1_8000), -32768);
        assert_eq!(wrap32(1 << 32), 0);
    }
}
