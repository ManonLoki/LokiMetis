//! 原生元数据目录零编码合同。

use super::*;

/// 大图元数据只识别魔数；既不调用Base64也不把原图内容编码进可序列化目录。
#[test]
fn native_metadata_catalog_never_invokes_base64_and_keeps_counts() {
    let mut bytes = vec![0; 4 * 1024 * 1024];
    bytes[..6].copy_from_slice(b"GIF89a");
    let before = ENCODE_CALLS.with(std::cell::Cell::get);
    let metadata = metadata_from_bytes("g1", "large.gif", &bytes).unwrap();
    let catalog = assemble_image_catalog(vec![metadata]);
    assert_eq!(ENCODE_CALLS.with(std::cell::Cell::get), before);
    assert_eq!(catalog.counts.gif, 1);
    assert_eq!(catalog.images[0].format, ImageFormat::Gif);
    let json = serde_json::to_string(&catalog).unwrap();
    assert!(!json.contains("data:"));
    assert!(!json.contains("base64"));
    assert!(json.len() < 256);
}
