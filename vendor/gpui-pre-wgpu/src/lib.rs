// 上游入口通过生成目录展开，内部模块及相对 shader 路径保持原有解析规则。
include!(concat!(env!("OUT_DIR"), "/gpui-patched/src/gpui_wgpu.rs"));
