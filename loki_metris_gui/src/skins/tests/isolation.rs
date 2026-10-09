// 真实生产载荷交给Node VM执行，验证归属切换和迟到回调，不连接用户宿主。

#[test]
/// 双应用接管、回调重放与旧generation清理都必须保护当前归属。
fn production_scripts_preserve_foreign_owner_and_quiesce_own_callbacks() {
    use std::process::{Command, Stdio};
    let temporary = tempfile::tempdir().unwrap();
    let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let mut payloads = Vec::new();
    for entry in std::fs::read_dir(resources.join("builtin-skins")).unwrap() {
        let directory = entry.unwrap().path();
        if !directory.is_dir() {
            continue;
        }
        let id = directory.file_name().unwrap().to_str().unwrap();
        let descriptor = load_descriptor(&directory, id, SkinSource::Builtin).unwrap();
        let payload = super::append_runtime_skin_marker(
            build_payload(&directory, true).unwrap(),
            &descriptor,
        )
        .unwrap();
        payloads.push(json!({"name":id,"script":payload,"probe":current_skin_expression(&SkinReference{source:SkinSource::Builtin,id:id.into()})}));
    }
    let pure = temporary.path().join("pure");
    create_theme_css_fixture(&pure, "pure");
    let descriptor = load_descriptor(&pure, "pure", SkinSource::User).unwrap();
    let payload =
        super::append_runtime_skin_marker(build_payload(&pure, false).unwrap(), &descriptor)
            .unwrap();
    payloads.push(json!({"name":"pure","script":payload,"probe":current_skin_expression(&SkinReference{source:SkinSource::User,id:"pure".into()})}));
    let fixture = json!({
        "payloads":payloads,
        "compat":[
            {"host":"codex","script":super::guarded_skin_script(&format!("return {HOST_COMPATIBILITY_SCRIPT}"))},
            {"host":"workBuddy","script":super::guarded_skin_script(&format!("return {WORKBUDDY_HOST_COMPATIBILITY_SCRIPT}"))}
        ],
        "marker":super::guarded_skin_script("window.__LOKI_METRIS_SKIN_TRANSACTION__ = 'fixture'; return true;"),
        "remove":super::REMOVE_SCRIPT,
        "active":super::ACTIVE_SKIN_PROBE_SCRIPT
    });
    let mut child = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/skin_runtime_owner.mjs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("项目声明的Node测试运行时必须可用");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(fixture.to_string().as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "JS ownership fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
/// CSS执行适配不修改用户文字、外部资源或同前缀的其它选择器。
fn runtime_css_namespace_preserves_non_selector_content() {
    let css = r##"/* #codex-dream-skin-style */
      #codex-dream-skin-style, #codex-dream-skin-chrome:hover { content: "#codex-dream-skin-style"; background:url(#codex-dream-skin-chrome); }
      #codex-dream-skin-style-custom { color:red; }"##;
    let result = super::runtime_css_namespace(css);
    assert!(result.contains("#loki-metris-dream-skin-style, #loki-metris-dream-skin-chrome:hover"));
    assert!(result.contains("/* #codex-dream-skin-style */"));
    assert!(result.contains("content: \"#codex-dream-skin-style\""));
    assert!(result.contains("url(#codex-dream-skin-chrome)"));
    assert!(result.contains("#codex-dream-skin-style-custom"));
    for untouched in [r".\#codex-dream-skin-style {color:red;}", "#codex-dream-skin-style💫 {color:red;}"] {
        assert_eq!(super::runtime_css_namespace(untouched), untouched);
    }
}

#[test]
/// 归属冲突是明确失败，不能折叠为兼容规则降级。
fn foreign_owner_evaluation_is_a_stable_error() {
    assert_eq!(
        super::skin_owner_result(json!({"kind":"foreignOwner"}))
            .unwrap_err()
            .code,
        "skin.foreign_owner"
    );
    assert_eq!(
        super::skin_owner_result(json!({"kind":"applied","value":true})).unwrap(),
        Some(json!(true))
    );
    assert!(super::skin_owner_result(json!(true)).is_err());
}
