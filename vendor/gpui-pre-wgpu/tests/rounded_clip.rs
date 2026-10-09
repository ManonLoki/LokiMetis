//! 校验圆角视口新增字段在 Rust、存储缓冲与 WebGL shader 中使用相同 ABI。

use gpui::PolychromeSprite;
use wgpu::naga;

/// 解析两种真实 shader，核对所有实例字段偏移与总步幅，防止末尾视口错位。
#[test]
fn rounded_clip_abi_matches_storage_and_webgl_shaders() {
    let common = include_str!(concat!(env!("OUT_DIR"), "/gpui-patched/src/shaders.wgsl"));
    let transports = [
        include_str!(concat!(env!("OUT_DIR"), "/gpui-patched/src/shaders_storage.wgsl")),
        include_str!(concat!(env!("OUT_DIR"), "/gpui-patched/src/shaders_webgl.wgsl")),
    ];
    let expected = [
        ("order", std::mem::offset_of!(PolychromeSprite, order)),
        ("pad", std::mem::offset_of!(PolychromeSprite, pad)),
        ("grayscale", std::mem::offset_of!(PolychromeSprite, grayscale)),
        ("opacity", std::mem::offset_of!(PolychromeSprite, opacity)),
        ("bounds", std::mem::offset_of!(PolychromeSprite, bounds)),
        ("content_mask", std::mem::offset_of!(PolychromeSprite, content_mask)),
        ("corner_radii", std::mem::offset_of!(PolychromeSprite, corner_radii)),
        ("tile", std::mem::offset_of!(PolychromeSprite, tile)),
        ("rounded_clip_bounds", std::mem::offset_of!(PolychromeSprite, rounded_clip_bounds)),
    ];

    for transport in transports {
        let source = format!("{common}{transport}");
        let module = naga::front::wgsl::parse_str(&source).expect("generated shader parses");
        // 使用上游同样的无扩展能力限制，验证实际绑定与所有入口函数。
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("generated shader validates without optional capabilities");
        let (_, sprite) = module.types.iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("PolychromeSprite"))
            .expect("polychrome instance type exists");
        let naga::TypeInner::Struct { members, span } = &sprite.inner else {
            panic!("polychrome instance must be a struct");
        };
        assert_eq!(*span as usize, std::mem::size_of::<PolychromeSprite>());
        assert_eq!(members.len(), expected.len());
        for (member, (name, offset)) in members.iter().zip(expected) {
            assert_eq!(member.name.as_deref(), Some(name));
            assert_eq!(member.offset as usize, offset, "field {name}");
        }
    }
}
