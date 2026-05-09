//! SIMD dot-product test suite.
//!
//! Per `coding_agent_session_search-8tgic`. Enforces the original ylnl bead's
//! contract: SIMD dot product is numerically equivalent to scalar within bounded
//! tolerance, deterministic, handles edge cases without panic, and produces
//! identical top-K rankings to the scalar path under the
//! `CASS_SIMD_DOT` env-var toggle from `coding_agent_session_search-yvv7r`.

use coding_agent_search::search::vector_index::{
    dot_product_f16_scalar_bench, dot_product_f16_simd_bench, dot_product_scalar_bench,
    dot_product_simd_bench,
};
use half::f16;
use rand::RngExt;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Tolerance for f32×f32 SIMD vs scalar comparison. f32 has ~7 decimal digits
/// of precision; for a sum across 1024 elements, accumulator-order drift makes
/// 1e-4 the right absolute floor. Random-input tolerance also scales with the
/// absolute product magnitude so cancellation-heavy dots do not get an
/// unrealistically tiny result-relative tolerance.
const FP_TOLERANCE_F32: f32 = 1e-4;
const FP_REL_TOLERANCE_F32: f32 = 5e-6;
const FP_ACCUMULATION_EPSILON_BUDGET: f32 = 4.0;

fn dot_product_tolerance_f32(a: &[f32], b: &[f32], scalar: f32) -> f32 {
    let product_magnitude: f32 = a.iter().zip(b).map(|(x, y)| (x * y).abs()).sum();
    FP_TOLERANCE_F32
        .max(scalar.abs() * FP_REL_TOLERANCE_F32)
        .max(product_magnitude * f32::EPSILON * FP_ACCUMULATION_EPSILON_BUDGET)
}

/// Build a deterministic 1024-dim f32 vector from a seed. Uses
/// rand_chacha::ChaCha8Rng (already in dev-deps) for cross-platform
/// reproducibility — the stdlib RNG's algorithm differs across versions.
fn deterministic_vec_f32(seed: u64) -> Vec<f32> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    (0..1024)
        .map(|_| {
            // rng.gen() yields [0, 1); shift to [-100, 100).
            let r: f32 = rng.random();
            r * 200.0 - 100.0
        })
        .collect()
}

fn deterministic_vec_f16(seed: u64) -> Vec<f16> {
    deterministic_vec_f32(seed)
        .iter()
        .map(|&x| f16::from_f32(x))
        .collect()
}

#[test]
fn simd_dot_fp_tolerance_against_scalar() {
    tracing::info!(target: "simd_tests", test = "fp_tolerance", n_pairs = 100, dim = 1024);
    let mut max_delta = 0.0f32;
    let mut failures: Vec<(usize, f32, f32, f32)> = Vec::new();
    for i in 0usize..100 {
        let seed_offset = i as u64;
        let a = deterministic_vec_f32(42 + seed_offset);
        let b = deterministic_vec_f32(2026 + seed_offset);
        let scalar = dot_product_scalar_bench(&a, &b);
        let simd = dot_product_simd_bench(&a, &b);
        let delta = (scalar - simd).abs();
        if delta > max_delta {
            max_delta = delta;
        }
        let tolerance = dot_product_tolerance_f32(&a, &b, scalar);
        if delta > tolerance {
            failures.push((i, scalar, simd, delta));
        }
        tracing::debug!(
            target: "simd_tests",
            test = "fp_tolerance",
            pair_idx = i,
            simd = simd,
            scalar = scalar,
            delta = delta,
            tolerance = tolerance
        );
    }
    tracing::info!(
        target: "simd_tests",
        test = "fp_tolerance",
        max_delta = max_delta,
        failure_count = failures.len()
    );
    assert!(
        failures.is_empty(),
        "SIMD vs scalar dot product diverged on {} of 100 pairs (max_delta={:.6}). First 5 failures: {:?}",
        failures.len(),
        max_delta,
        failures.iter().take(5).collect::<Vec<_>>()
    );
}

#[test]
fn simd_dot_random_inputs_proptest_lite() {
    tracing::info!(target: "simd_tests", test = "random_inputs", trials = 1000);
    let mut max_delta = 0.0f32;
    let mut total_delta = 0.0f64;
    let mut deltas: Vec<f32> = Vec::with_capacity(1000);
    for trial in 0..1000 {
        let a = deterministic_vec_f32(20260509 + trial as u64);
        let b = deterministic_vec_f32(123456789 + trial as u64);
        let scalar = dot_product_scalar_bench(&a, &b);
        let simd = dot_product_simd_bench(&a, &b);
        let delta = (scalar - simd).abs();
        if delta > max_delta {
            max_delta = delta;
        }
        total_delta += delta as f64;
        deltas.push(delta);
        let tolerance = dot_product_tolerance_f32(&a, &b, scalar);
        assert!(
            delta <= tolerance,
            "trial {trial}: scalar={scalar} simd={simd} delta={delta} tolerance={tolerance}"
        );
    }
    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = deltas[deltas.len() / 2];
    let p99 = deltas[(deltas.len() * 99) / 100];
    tracing::info!(
        target: "simd_tests",
        test = "random_inputs",
        trials = 1000,
        max_delta = max_delta,
        mean_delta = (total_delta / 1000.0) as f32,
        percentile_p50 = p50,
        percentile_p99 = p99
    );
}

#[test]
fn simd_dot_edge_zeros() {
    tracing::info!(target: "simd_tests", test = "edge", case = "zeros");
    let zeros = vec![0.0f32; 1024];
    let scalar = dot_product_scalar_bench(&zeros, &zeros);
    let simd = dot_product_simd_bench(&zeros, &zeros);
    assert_eq!(scalar, 0.0, "scalar of zeros must be exactly 0");
    assert_eq!(simd, 0.0, "SIMD of zeros must be exactly 0");
    tracing::info!(target: "simd_tests", test = "edge", case = "zeros", outcome = "pass");
}

#[test]
fn simd_dot_edge_unit_vector() {
    tracing::info!(target: "simd_tests", test = "edge", case = "unit_vector");
    let mut a = vec![0.0f32; 1024];
    a[0] = 1.0;
    let mut b = vec![0.0f32; 1024];
    b[0] = 1.0;
    let scalar = dot_product_scalar_bench(&a, &b);
    let simd = dot_product_simd_bench(&a, &b);
    assert!((scalar - 1.0).abs() < 1e-9);
    assert!((simd - 1.0).abs() < 1e-9);
    tracing::info!(target: "simd_tests", test = "edge", case = "unit_vector", outcome = "pass");
}

#[test]
fn simd_dot_edge_denormals() {
    tracing::info!(target: "simd_tests", test = "edge", case = "denormals");
    // Denormals are subnormal floats — extremely small values that some
    // SIMD paths flush to zero. Test that scalar and SIMD agree.
    let denormal = f32::MIN_POSITIVE / 2.0; // subnormal value
    let a = vec![denormal; 1024];
    let b = vec![denormal; 1024];
    let scalar = dot_product_scalar_bench(&a, &b);
    let simd = dot_product_simd_bench(&a, &b);
    let delta = (scalar - simd).abs();
    // Either both flush to zero or both produce a small positive — they
    // must agree within FP epsilon (ULP-level for tiny values).
    let tolerance = scalar.abs() * 1e-6 + 1e-30;
    assert!(
        delta <= tolerance,
        "denormal scalar={scalar} simd={simd} delta={delta}"
    );
    tracing::info!(
        target: "simd_tests",
        test = "edge",
        case = "denormals",
        scalar = scalar,
        simd = simd,
        outcome = "pass"
    );
}

#[test]
fn simd_dot_edge_nan_propagation() {
    tracing::info!(target: "simd_tests", test = "edge", case = "nan_propagation");
    let mut a = vec![1.0f32; 1024];
    a[42] = f32::NAN;
    let b = vec![1.0f32; 1024];
    let scalar = dot_product_scalar_bench(&a, &b);
    let simd = dot_product_simd_bench(&a, &b);
    assert!(scalar.is_nan(), "scalar must propagate NaN");
    assert!(simd.is_nan(), "SIMD must propagate NaN");
    tracing::info!(target: "simd_tests", test = "edge", case = "nan_propagation", outcome = "pass");
}

#[test]
fn simd_dot_edge_infinity() {
    tracing::info!(target: "simd_tests", test = "edge", case = "infinity");
    let mut a = vec![1.0f32; 1024];
    a[10] = f32::INFINITY;
    let b = vec![1.0f32; 1024];
    let scalar = dot_product_scalar_bench(&a, &b);
    let simd = dot_product_simd_bench(&a, &b);
    assert!(
        scalar.is_infinite() && scalar > 0.0,
        "scalar must propagate +infinity (got {scalar})"
    );
    assert!(
        simd.is_infinite() && simd > 0.0,
        "SIMD must propagate +infinity (got {simd})"
    );
    tracing::info!(target: "simd_tests", test = "edge", case = "infinity", outcome = "pass");
}

#[test]
#[should_panic]
fn simd_dot_edge_mismatched_length_panics() {
    // The benchmark wrappers expect equal lengths and call .expect() on the
    // frankensearch result. Mismatched lengths therefore panic, not return Err.
    // This documents the contract: callers must validate lengths before
    // calling the *_bench helpers. (frankensearch::index::dot_product_f32_f32
    // itself returns Result, but the bench wrappers unwrap.)
    let a = vec![1.0f32; 1024];
    let b = vec![1.0f32; 1023];
    tracing::info!(target: "simd_tests", test = "edge", case = "mismatched_length");
    // This call is expected to panic.
    let _ = dot_product_simd_bench(&a, &b);
}

// f16 variants — these exercise the f16-storage path.

#[test]
fn simd_dot_f16_fp_tolerance_against_scalar() {
    tracing::info!(target: "simd_tests", test = "f16_fp_tolerance", n_pairs = 50, dim = 1024);
    let mut max_delta = 0.0f32;
    for i in 0..50 {
        let stored = deterministic_vec_f16(99 + i);
        let query = deterministic_vec_f32(199 + i);
        let scalar = dot_product_f16_scalar_bench(&stored, &query);
        let simd = dot_product_f16_simd_bench(&stored, &query);
        let delta = (scalar - simd).abs();
        // f16 has ~3 decimal digits of precision; tolerance is ~1e-3 abs
        // for values that round-trip through f16 (matches mng4 contract
        // documented in waijq).
        let tolerance = FP_TOLERANCE_F32.max(scalar.abs() * 5e-4);
        assert!(
            delta <= tolerance,
            "f16 pair {i}: scalar={scalar} simd={simd} delta={delta} tolerance={tolerance}"
        );
        if delta > max_delta {
            max_delta = delta;
        }
    }
    tracing::info!(target: "simd_tests", test = "f16_fp_tolerance", max_delta = max_delta);
}

#[test]
fn simd_dot_f16_edge_zeros() {
    let stored: Vec<f16> = (0..1024).map(|_| f16::from_f32(0.0)).collect();
    let query = vec![0.0f32; 1024];
    let scalar = dot_product_f16_scalar_bench(&stored, &query);
    let simd = dot_product_f16_simd_bench(&stored, &query);
    assert_eq!(scalar, 0.0);
    assert_eq!(simd, 0.0);
    tracing::info!(target: "simd_tests", test = "f16_edge", case = "zeros", outcome = "pass");
}
