//! Core types and utilities shared across all InfEngine crates.
//!
//! Currently exposes:
//! - `math`: re-export of `glam` for shared 2D/3D math.
//! - `scene`: serializable 2D scene model with entities and components.

#![forbid(unsafe_code)]

pub mod project;
pub mod scene;

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
