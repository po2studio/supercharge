//! How vectors are stored: int8 with one scale per vector, plus a 1-bit code for the first pass.

use crate::embed::DIM;

pub const CODE_BYTES: usize = DIM / 8;

/// A fixed centre, so signs split the vectors instead of all falling on one side
/// (e5 embeddings share a large common component).
static CENTER: &[u8] = include_bytes!("center.f32");

fn center() -> impl Iterator<Item = f32> {
    CENTER
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
}

/// `(scale, int8 values)`; `v[i] ≈ scale * q[i]`.
pub fn quantize(v: &[f32]) -> (f32, Vec<i8>) {
    let max = v.iter().fold(0f32, |m, x| m.max(x.abs()));
    if max == 0.0 {
        return (0.0, vec![0; v.len()]);
    }
    let scale = max / 127.0;
    (scale, v.iter().map(|x| (x / scale).round() as i8).collect())
}

/// One bit per dimension: is it above the centre.
pub fn code(v: &[f32]) -> [u8; CODE_BYTES] {
    let mut out = [0u8; CODE_BYTES];
    for (i, c) in center().enumerate().take(v.len()) {
        if v[i] > c {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

pub fn hamming(a: &[u8], b: &[u8]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

/// Dot product of a float query with a stored int8 vector.
pub fn dot(query: &[f32], scale: f32, q: &[i8]) -> f32 {
    scale
        * query
            .iter()
            .zip(q)
            .map(|(a, &b)| a * f32::from(b))
            .sum::<f32>()
}

pub fn i8_bytes(q: &[i8]) -> Vec<u8> {
    q.iter().map(|&x| x as u8).collect()
}

pub fn bytes_i8(b: &[u8]) -> Vec<i8> {
    b.iter().map(|&x| x as i8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::normalize;

    fn wave(seed: usize) -> Vec<f32> {
        let mut v: Vec<f32> = (0..DIM)
            .map(|i| (((i * 37 + seed * 101) % 97) as f32 - 48.0) / 48.0)
            .collect();
        normalize(&mut v);
        v
    }

    #[test]
    fn int8_keeps_the_dot_product_close() {
        let (a, b) = (wave(1), wave(2));
        let exact: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        let (s, q) = quantize(&b);
        assert!((dot(&a, s, &q) - exact).abs() < 0.01);
    }

    #[test]
    fn a_zero_vector_quantizes_to_zeros() {
        assert_eq!(quantize(&[0.0; 4]), (0.0, vec![0; 4]));
    }

    #[test]
    fn identical_vectors_have_identical_codes_and_distinct_ones_differ() {
        assert_eq!(hamming(&code(&wave(1)), &code(&wave(1))), 0);
        assert!(hamming(&code(&wave(1)), &code(&wave(2))) > 0);
    }

    #[test]
    fn bytes_round_trip() {
        let q = vec![-128i8, -1, 0, 1, 127];
        assert_eq!(bytes_i8(&i8_bytes(&q)), q);
    }

    #[test]
    fn the_centre_has_one_value_per_dimension() {
        assert_eq!(CENTER.len(), DIM * 4);
    }
}
