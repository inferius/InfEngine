//! Core types and utilities shared across all InfEngine crates.
//!
//! At this stage only a version constant and a `math` re-export of `glam`
//! are provided. More substantial functionality lands in Phase 1.

#![forbid(unsafe_code)]

/// Crate version, sourced from Cargo.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Math primitives re-exported from `glam`.
pub mod math {
    pub use glam::*;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn math_reexport_works() {
        let v = math::Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.length_squared(), 14.0);
    }
}
