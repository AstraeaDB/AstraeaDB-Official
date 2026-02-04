//! Distance and similarity functions for vector comparison.
//!
//! All distance functions return values where **lower = more similar**.
//! - Cosine distance: 1.0 - cosine_similarity  (0 = identical)
//! - Euclidean distance: L2 distance
//! - Dot-product distance: negative dot product

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::DistanceMetric;

/// Compute the cosine distance between two vectors.
///
/// Returns `1.0 - cosine_similarity`, so 0.0 means identical direction
/// and 2.0 means opposite direction.
///
/// # Errors
/// Returns `DimensionMismatch` if the vectors have different lengths.
pub fn cosine_distance(a: &[f32], b: &[f32]) -> Result<f32> {
    validate_dimensions(a, b)?;

    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        // If either vector is zero, cosine similarity is undefined.
        // We return 1.0 (maximum distance for non-opposite vectors).
        return Ok(1.0);
    }

    // Clamp to [-1, 1] to handle floating-point rounding.
    let cosine_sim = (dot / denom).clamp(-1.0, 1.0);
    Ok(1.0 - cosine_sim)
}

/// Compute the Euclidean (L2) distance between two vectors.
///
/// # Errors
/// Returns `DimensionMismatch` if the vectors have different lengths.
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> Result<f32> {
    validate_dimensions(a, b)?;

    let mut sum_sq = 0.0_f32;
    for i in 0..a.len() {
        let diff = a[i] - b[i];
        sum_sq += diff * diff;
    }

    Ok(sum_sq.sqrt())
}

/// Compute the negative dot-product distance between two vectors.
///
/// Returns `-dot(a, b)` so that lower values indicate higher similarity.
///
/// # Errors
/// Returns `DimensionMismatch` if the vectors have different lengths.
pub fn dot_product_distance(a: &[f32], b: &[f32]) -> Result<f32> {
    validate_dimensions(a, b)?;

    let mut dot = 0.0_f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
    }

    Ok(-dot)
}

/// Dispatch to the appropriate distance function based on the metric.
///
/// # Errors
/// Returns `DimensionMismatch` if the vectors have different lengths.
pub fn compute_distance(metric: DistanceMetric, a: &[f32], b: &[f32]) -> Result<f32> {
    match metric {
        DistanceMetric::Cosine => cosine_distance(a, b),
        DistanceMetric::Euclidean => euclidean_distance(a, b),
        DistanceMetric::DotProduct => dot_product_distance(a, b),
    }
}

/// Validate that two vectors have the same length.
fn validate_dimensions(a: &[f32], b: &[f32]) -> Result<()> {
    if a.len() != b.len() {
        return Err(AstraeaError::DimensionMismatch {
            expected: a.len(),
            got: b.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_distance_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let d = cosine_distance(&v, &v).unwrap();
        assert!(d.abs() < 1e-6, "identical vectors should have cosine distance ~0, got {d}");
    }

    #[test]
    fn test_cosine_distance_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let d = cosine_distance(&a, &b).unwrap();
        assert!(
            (d - 1.0).abs() < 1e-6,
            "orthogonal vectors should have cosine distance ~1.0, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_opposite_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let d = cosine_distance(&a, &b).unwrap();
        assert!(
            (d - 2.0).abs() < 1e-6,
            "opposite vectors should have cosine distance ~2.0, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let d = cosine_distance(&a, &b).unwrap();
        assert!(
            (d - 1.0).abs() < 1e-6,
            "zero vector should yield cosine distance 1.0, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_known_value() {
        // cos(45 degrees) = sqrt(2)/2 ~ 0.7071
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 1.0];
        let d = cosine_distance(&a, &b).unwrap();
        let expected = 1.0 - (1.0 / 2.0_f32.sqrt());
        assert!(
            (d - expected).abs() < 1e-5,
            "expected cosine distance {expected}, got {d}"
        );
    }

    #[test]
    fn test_euclidean_distance_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let d = euclidean_distance(&v, &v).unwrap();
        assert!(d.abs() < 1e-6, "identical vectors should have L2 distance 0, got {d}");
    }

    #[test]
    fn test_euclidean_distance_known_value() {
        let a = vec![0.0, 0.0];
        let b = vec![3.0, 4.0];
        let d = euclidean_distance(&a, &b).unwrap();
        assert!(
            (d - 5.0).abs() < 1e-6,
            "expected euclidean distance 5.0 (3-4-5 triangle), got {d}"
        );
    }

    #[test]
    fn test_dot_product_distance_known_value() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        // dot = 1*4 + 2*5 + 3*6 = 32
        let d = dot_product_distance(&a, &b).unwrap();
        assert!(
            (d - (-32.0)).abs() < 1e-6,
            "expected dot product distance -32.0, got {d}"
        );
    }

    #[test]
    fn test_dot_product_distance_identical() {
        let v = vec![1.0, 1.0];
        let d = dot_product_distance(&v, &v).unwrap();
        // dot(v,v) = 2, so distance = -2
        assert!(
            (d - (-2.0)).abs() < 1e-6,
            "expected dot product distance -2.0, got {d}"
        );
    }

    #[test]
    fn test_dimension_mismatch() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert!(cosine_distance(&a, &b).is_err());
        assert!(euclidean_distance(&a, &b).is_err());
        assert!(dot_product_distance(&a, &b).is_err());
    }

    #[test]
    fn test_compute_distance_dispatch() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];

        let cos = compute_distance(DistanceMetric::Cosine, &a, &b).unwrap();
        let euc = compute_distance(DistanceMetric::Euclidean, &a, &b).unwrap();
        let dot = compute_distance(DistanceMetric::DotProduct, &a, &b).unwrap();

        assert!((cos - 1.0).abs() < 1e-6);
        assert!((euc - 2.0_f32.sqrt()).abs() < 1e-6);
        assert!(dot.abs() < 1e-6); // dot product of orthogonal unit vectors = 0
    }
}
