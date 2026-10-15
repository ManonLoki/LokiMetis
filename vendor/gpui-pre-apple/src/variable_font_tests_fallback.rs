// 本文件是 variable_font_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：未知值不进入哈希键，相等的实际属性尺寸复制稳定复用，不同未知值不能错误合并。；缺字由原 CoreText cascade 解析；实际 fallback FontId 反查和重复 shaping 必须稳定。；不同 feature FontKey 再次 load_family 不得因全局已有 PS 名而产生空候选集。

/// 未知值不进入哈希键，相等的实际属性尺寸复制稳定复用，不同未知值不能错误合并。
#[test]
fn unknown_context_uses_exact_native_scan_without_unbounded_growth() {
    let (system, family) = system(GEIST);
    let id = system.font_id(&request(&family, 650.)).unwrap();
    let a = font_with_unknown_cascade_value(&native(&system, id), 123.);
    let same = font_with_unknown_cascade_value(&native(&system, id), 123.);
    let b = font_with_unknown_cascade_value(&native(&system, id), 456.);
    assert!(native_equivalent(&a, &same));
    assert!(!native_equivalent(&a, &b));
    let a_id = system.0.write().id_for_native_font(a.clone());
    let b_id = system.0.write().id_for_native_font(b.clone());
    assert_ne!(a_id, b_id);
    let count = system.0.read().fonts.len();
    for _ in 0..128 {
        assert_eq!(
            system
                .0
                .write()
                .id_for_native_font(same.clone_with_font_size(32.)),
            a_id
        );
        assert_eq!(
            system
                .0
                .write()
                .id_for_native_font(b.clone_with_font_size(32.)),
            b_id
        );
    }
    assert_eq!(system.0.read().fonts.len(), count);
    assert!(raster(&system, a_id).1.iter().any(|p| *p > 0));
}

/// 缺字由原 CoreText cascade 解析；实际 fallback FontId 反查和重复 shaping 必须稳定。
#[test]
fn actual_system_fallback_runs_reuse_native_fonts() {
    let (system, family) = system(GEIST);
    let id = system.font_id(&request(&family, 650.)).unwrap();
    let text = "M中😀";
    let layout = system.layout_line(
        text,
        px(32.),
        &[FontRun {
            len: text.len(),
            font_id: id,
        }],
    );
    assert!(!layout.runs.is_empty());
    assert!(
        layout.runs.iter().any(|run| run.font_id != id),
        "fixture must exercise real system fallback"
    );
    assert!(
        layout
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .all(|glyph| glyph.id.0 != 0),
        "system must contain real fallback glyphs"
    );
    let signature = layout
        .runs
        .iter()
        .map(|run| run.font_id)
        .collect::<Vec<_>>();
    let count = system.0.read().fonts.len();
    for _ in 0..32 {
        let again = system.layout_line(
            text,
            px(32.),
            &[FontRun {
                len: text.len(),
                font_id: id,
            }],
        );
        assert_eq!(
            again.runs.iter().map(|run| run.font_id).collect::<Vec<_>>(),
            signature
        );
    }
    assert_eq!(system.0.read().fonts.len(), count);
}

/// 不同 feature FontKey 再次 load_family 不得因全局已有 PS 名而产生空候选集。
#[test]
fn repeated_family_loading_with_another_font_key_keeps_candidates() {
    let (system, family) = system(GEIST);
    for enabled in [0, 1, 0, 1] {
        let mut request = request(&family, 650.);
        request.features = FontFeatures(Arc::new(vec![("tnum".into(), enabled)]));
        let id = system.font_id(&request).unwrap();
        assert_eq!(effective_weight(&native(&system, id)), Some(650.));
        assert!(system.glyph_for_char(id, 'm').is_some());
    }
}

/// 静态 Geist 夹具含真实 GSUB liga；特性必须改变实际 glyph 序列，而不仅改变缓存键。
#[test]
fn real_liga_feature_changes_shaped_glyphs() {
    let (system, family) = system(STATIC_REGULAR);
    let ids = [0, 1].map(|enabled| {
        let mut font = request(&family, 400.);
        font.features = FontFeatures(Arc::new(vec![("liga".into(), enabled)]));
        system.font_id(&font).unwrap()
    });
    let text = "office ffi fi fl";
    let layouts = ids.map(|id| {
        system.layout_line(
            text,
            px(32.),
            &[FontRun {
                len: text.len(),
                font_id: id,
            }],
        )
    });
    let glyphs = layouts.each_ref().map(|layout| {
        layout
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|g| g.id)
            .collect::<Vec<_>>()
    });
    assert_ne!(
        glyphs[0], glyphs[1],
        "real GSUB liga must affect GPUI shaped glyphs"
    );
    assert_eq!(layouts[0].len, text.len());
    assert_eq!(layouts[1].len, text.len());
}
