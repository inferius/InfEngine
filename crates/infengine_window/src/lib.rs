//! Window creation and input handling for InfEngine.
//!
//! This crate will wrap `winit` once Phase 1 begins. For now it only exposes
//! a placeholder so the workspace builds.

#![forbid(unsafe_code)]

/// Marker function to verify the crate compiles and links.
pub fn placeholder() -> &'static str {
    "infengine_window placeholder"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_returns_expected_string() {
        assert_eq!(placeholder(), "infengine_window placeholder");
    }
}
