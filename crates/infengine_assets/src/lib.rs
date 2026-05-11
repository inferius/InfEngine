//! Asset pipeline for InfEngine.
//!
//! Will provide typed asset handles, a loader registry, and filesystem
//! hot-reload via `notify`. Phase 3 introduces the initial implementation.

#![forbid(unsafe_code)]

/// Marker function to verify the crate compiles and links.
pub fn placeholder() -> &'static str {
    "infengine_assets placeholder"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_returns_expected_string() {
        assert_eq!(placeholder(), "infengine_assets placeholder");
    }
}
