//! GPU renderer for InfEngine.
//!
//! The renderer wraps `wgpu` to provide a cross-platform graphics backend
//! supporting Vulkan, DirectX 12, Metal, and WebGPU. Phase 1 will introduce
//! the initial device/surface/swapchain plumbing.

#![forbid(unsafe_code)]

/// Marker function to verify the crate compiles and links.
pub fn placeholder() -> &'static str {
    "infengine_render placeholder"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_returns_expected_string() {
        assert_eq!(placeholder(), "infengine_render placeholder");
    }
}
