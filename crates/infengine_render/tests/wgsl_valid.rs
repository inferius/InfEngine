//! W199: validate every bundled WGSL shader with naga at `cargo test`
//! time, so a syntax / type error surfaces here instead of crashing the
//! app at `device.create_shader_module()` on launch. naga is the same
//! front-end wgpu uses, so a pass here means the runtime accepts it too.

use naga::valid::{Capabilities, ValidationFlags, Validator};

fn check(name: &str, src: &str) {
    let module = match naga::front::wgsl::parse_str(src) {
        Ok(m) => m,
        Err(e) => panic!("WGSL parse failed for {name}:\n{}", e.emit_to_string(src)),
    };
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    if let Err(e) = validator.validate(&module) {
        panic!("WGSL validation failed for {name}: {e:?}");
    }
}

#[test]
fn all_bundled_shaders_validate() {
    check("mesh3d.wgsl", include_str!("../src/mesh3d.wgsl"));
    check("debug.wgsl", include_str!("../src/debug.wgsl"));
    check("debug3d.wgsl", include_str!("../src/debug3d.wgsl"));
    check("grid.wgsl", include_str!("../src/grid.wgsl"));
    check("particle.wgsl", include_str!("../src/particle.wgsl"));
    check("shadow.wgsl", include_str!("../src/shadow.wgsl"));
    check("sprite.wgsl", include_str!("../src/sprite.wgsl"));
}
