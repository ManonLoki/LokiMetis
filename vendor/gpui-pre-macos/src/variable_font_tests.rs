// 自有真实 macOS 回归：使用不可变字体字节、CoreText shaping 及 GPUI CPU glyph raster。
// 此文件没有 mock/skip/ignore；测试未执行时不得称为 native 通过。
use super::*;
use core_foundation::{array::CFArray, date::CFDate, dictionary::CFDictionary};
use core_graphics::geometry::CGAffineTransform;
use core_text::font_descriptor::{
    TraitAccessors, kCTFontCascadeListAttribute, kCTFontFamilyNameAttribute,
    kCTFontFeatureSettingsAttribute, kCTFontTraitsAttribute,
};
use variable_font::{effective_weight, native_equivalent};

/// 夹具由固定摘要/OFL 清单登记，不做系统字体安装或全局注册。
const GEIST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/test-fixtures/Geist-Latin-variable.woff2"
));
/// 等宽字体独立证明不将其它可变字体折回默认字重。
const MONO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/test-fixtures/GeistMono-Latin-variable.woff2"
));
/// 静态字体用独立系统，避免相同 PostScript 名的可变/静态资源并存。
const STATIC_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/test-fixtures/Geist-static-Regular.ttf"
));
/// 静态 Bold 保持原候选匹配，不能伪造 wght 轴。
const STATIC_BOLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/test-fixtures/Geist-static-Bold.ttf"
));

/// 完全沿用生产 Borrowed add_fonts 路径：CGFont 数据加载并保留 CoreText 本地对象。
fn raw_font(bytes: &'static [u8]) -> CTFont {
    let provider = unsafe { core_graphics::data_provider::CGDataProvider::from_slice(bytes) };
    let cg = core_graphics::font::CGFont::from_data_provider(provider)
        .expect("fixture must be a real CoreGraphics font");
    FontKitFont::from_core_graphics_font(cg).native_font()
}

/// 内存字体源走真实 add_fonts，family 从夹具读取，不依赖全局已安装同名字体。
fn system(bytes: &'static [u8]) -> (MacTextSystem, String) {
    let native = raw_font(bytes);
    let family = unsafe { FontKitFont::from_native_font(&native) }.family_name();
    let system = MacTextSystem::new();
    system
        .add_fonts(vec![Cow::Borrowed(bytes)])
        .expect("real fixture registration");
    (system, family)
}

/// 测试请求只设置原生公开 Font 字段。
fn request(family: &str, weight: f32) -> Font {
    let mut font = gpui::font(family.to_owned());
    font.weight = FontWeight(weight);
    font
}

/// 注册结果必须从保存的实际 CTFont 读取，不用请求字重冒充实例。
fn native(system: &MacTextSystem, id: FontId) -> CTFont {
    system.0.read().fonts[id.0].native_font()
}

/// 真实 raster 包含尺寸和字节；不同几何同样属于可观察输出差异。
fn raster(system: &MacTextSystem, id: FontId) -> (Size<DevicePixels>, Vec<u8>) {
    let params = RenderGlyphParams {
        font_id: id,
        glyph_id: system.glyph_for_char(id, 'M').expect("M glyph"),
        font_size: px(32.),
        subpixel_variant: point(0, 0),
        scale_factor: 1.,
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    };
    let bounds = system
        .glyph_raster_bounds(&params)
        .expect("real raster bounds");
    let pixels = system
        .rasterize_glyph(&params, bounds)
        .expect("real raster bytes");
    assert!(
        pixels.1.iter().any(|byte| *byte != 0),
        "glyph must draw nonempty coverage"
    );
    pixels
}

/// 编译期检验完整 MacTextSystem 的 Send/Sync，无法由纯 Rust key 的单独检查替代。
#[test]
fn mac_text_system_remains_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<MacTextSystem>();
    check::<NativeFontIdentity>();
}

/// 两个真实可变夹具的 650/700/900 FontId 互异、重复选择稳定、CTRun 仍保留实际字重。
#[test]
fn variable_weights_have_distinct_stable_font_ids_and_shaped_runs() {
    for fixture in [GEIST, MONO] {
        let (system, family) = system(fixture);
        let mut ids = Vec::new();
        for weight in [650., 700., 900.] {
            let font = request(&family, weight);
            let id = system.font_id(&font).unwrap();
            assert_eq!(effective_weight(&native(&system, id)), Some(weight as f64));
            assert_eq!(system.font_id(&font).unwrap(), id);
            let layout = system.layout_line(
                "Mmffi",
                px(32.),
                &[FontRun {
                    len: 5,
                    font_id: id,
                }],
            );
            assert!(!layout.runs.is_empty());
            for run in layout.runs {
                assert_eq!(
                    effective_weight(&native(&system, run.font_id)),
                    Some(weight as f64)
                );
            }
            ids.push(id);
        }
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[2]);
    }
}

/// 32pt 复制与布局内部 next_up 字号不能按帧无限追加同一实例。
#[test]
fn sized_native_copies_and_repeated_shaping_do_not_grow_font_cache() {
    let (system, family) = system(GEIST);
    let id = system.font_id(&request(&family, 650.)).unwrap();
    let original = native(&system, id);
    let copied = original.clone_with_font_size(32.);
    assert_eq!(native_identity(&original), native_identity(&copied));
    assert!(
        native_identity(&original).is_some(),
        "normal fixture should use semantic key"
    );
    for _ in 0..2 {
        system.layout_line(
            "ffiMm",
            px(32.),
            &[FontRun {
                len: 5,
                font_id: id,
            }],
        );
    }
    let settled = system.0.read().fonts.len();
    for _ in 0..128 {
        assert_eq!(system.0.write().id_for_native_font(copied.clone()), id);
        system.layout_line(
            "ffiMm",
            px(32.),
            &[FontRun {
                len: 5,
                font_id: id,
            }],
        );
    }
    assert_eq!(system.0.read().fonts.len(), settled);
}

/// 字重变化确实改变 GPUI 的最终 raster，不能只比较 key、PS 名或请求值。
#[test]
fn variable_weight_raster_output_differs() {
    for fixture in [GEIST, MONO] {
        let (system, family) = system(fixture);
        let pixels = [650., 700., 900.]
            .map(|w| raster(&system, system.font_id(&request(&family, w)).unwrap()));
        assert_ne!(pixels[0], pixels[1]);
        assert_ne!(pixels[1], pixels[2]);
        assert_ne!(pixels[0], pixels[2]);
    }
}

/// 静态字体、默认字重及合法轴范围裁切均读取真实 CoreText 结果。
#[test]
fn static_default_weight_and_axis_clamping_keep_original_semantics() {
    let variable = raw_font(GEIST);
    assert_eq!(effective_weight(&variable), Some(400.));
    assert!(weight_instance(&variable, 400.).unwrap().is_none());
    assert!(weight_instance(&variable, f32::NAN).unwrap().is_none());
    assert!(weight_instance(&variable, f32::INFINITY).unwrap().is_none());
    assert_eq!(
        effective_weight(
            &weight_instance(&variable, 99.)
                .unwrap()
                .unwrap()
                .native_font()
        ),
        Some(100.)
    );
    assert_eq!(
        effective_weight(
            &weight_instance(&variable, 1000.)
                .unwrap()
                .unwrap()
                .native_font()
        ),
        Some(900.)
    );
    for bytes in [STATIC_REGULAR, STATIC_BOLD] {
        assert!(effective_weight(&raw_font(bytes)).is_none());
        assert!(weight_instance(&raw_font(bytes), 650.).unwrap().is_none());
    }
    let (system, family) = system(STATIC_REGULAR);
    system.add_fonts(vec![Cow::Borrowed(STATIC_BOLD)]).unwrap();
    let regular = system.font_id(&request(&family, 400.)).unwrap();
    let bold = system.font_id(&request(&family, 700.)).unwrap();
    assert_ne!(regular, bold);
    assert_ne!(raster(&system, regular), raster(&system, bold));
    assert!(native(&system, bold).postscript_name().contains("Bold"));
}

/// 原始 feature/cascade 属性用于与 CTRun 字体进行逐值比较。
fn context(font: &CTFont, key: core_foundation::string::CFStringRef) -> Option<CFType> {
    font.copy_descriptor()
        .attributes()
        .find(key)
        .map(|v| (*v).clone())
}

/// 复用原上游 feature 构造，不改 tag 顺序、AAT 兼容或系统 fallback 拼接逻辑。
#[test]
fn features_survive_weight_size_shaping_and_remain_distinct() {
    let (system, family) = system(GEIST);
    let ids = [0, 1].map(|enabled| {
        let mut font = request(&family, 650.);
        font.features = FontFeatures(Arc::new(vec![
            ("liga".into(), enabled),
            ("calt".into(), enabled),
        ]));
        system.font_id(&font).unwrap()
    });
    assert_ne!(ids[0], ids[1]);
    // CoreText 会省略中性的 feature 设置；实际 canonical Option 是复制/CTRun oracle。
    let contexts = ids.map(|id| unsafe {
        context(&native(&system, id), kCTFontFeatureSettingsAttribute)
    });
    eprintln!("real CoreText feature contexts: {:?}", contexts);
    assert_ne!(contexts[0], contexts[1]);
    for (id, expected) in ids.into_iter().zip(contexts) {
        let font = native(&system, id);
        let copy = font.clone_with_font_size(32.);
        assert_eq!(
            unsafe { context(&copy, kCTFontFeatureSettingsAttribute) },
            expected.clone()
        );
        let layout = system.layout_line(
            "office ffi",
            px(32.),
            &[FontRun {
                len: 10,
                font_id: id,
            }],
        );
        for run in layout.runs {
            let shaped = native(&system, run.font_id);
            assert_eq!(effective_weight(&shaped), Some(650.));
            assert_eq!(
                unsafe { context(&shaped, kCTFontFeatureSettingsAttribute) },
                expected.clone()
            );
        }
    }
    // Geist 的实际 GSUB 是否包含 liga/calt 不预设；上面严格验证 descriptor 和真实 CTRun。
    // 缺失 feature 的字体没有义务产生不同 glyph；已有 variable raster 差异测试独立覆盖绘制。
}

/// 显式 fallback 的顺序和加权 trait 必须在 wght 修改之后由原算法生成。
#[test]
fn fallback_order_weight_traits_and_sized_copies_are_preserved() {
    let (system, family) = system(GEIST);
    let mut request_a = request(&family, 650.);
    request_a.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Times New Roman".into(),
        "Courier New".into(),
    ]));
    let a = system.font_id(&request_a).unwrap();
    let mut request_b = request_a.clone();
    request_b.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Courier New".into(),
        "Times New Roman".into(),
    ]));
    let b = system.font_id(&request_b).unwrap();
    assert_ne!(a, b);
    let font = native(&system, a);
    let cascade_value = unsafe { context(&font, kCTFontCascadeListAttribute) }.unwrap();
    assert_eq!(
        cascade_value.type_of(),
        CFArray::<CTFontDescriptor>::type_id()
    );
    let cascade = unsafe {
        CFArray::<CTFontDescriptor>::wrap_under_get_rule(cascade_value.as_CFTypeRef() as _)
    };
    let first = cascade.get(0).unwrap().attributes();
    let second = cascade.get(1).unwrap().attributes();
    assert_eq!(
        unsafe { first.find(kCTFontFamilyNameAttribute) }
            .unwrap()
            .downcast::<CFString>()
            .unwrap()
            .to_string(),
        "Times New Roman"
    );
    assert_eq!(
        unsafe { second.find(kCTFontFamilyNameAttribute) }
            .unwrap()
            .downcast::<CFString>()
            .unwrap()
            .to_string(),
        "Courier New"
    );
    let traits_value = unsafe { first.find(kCTFontTraitsAttribute) }.unwrap();
    assert_eq!(
        traits_value.type_of(),
        CFDictionary::<CFString, CFType>::type_id()
    );
    let traits = unsafe {
        CFDictionary::<CFString, CFType>::wrap_under_get_rule(traits_value.as_CFTypeRef() as _)
    };
    assert_eq!(
        traits.normalized_weight(),
        font.all_traits().normalized_weight()
    );
    assert_eq!(
        native_identity(&font),
        native_identity(&font.clone_with_font_size(32.))
    );
    let expected = unsafe { context(&font, kCTFontCascadeListAttribute) };
    for run in system
        .layout_line("Mm", px(32.), &[FontRun { len: 2, font_id: a }])
        .runs
    {
        assert_eq!(
            unsafe { context(&native(&system, run.font_id), kCTFontCascadeListAttribute) },
            expected
        );
    }
}

/// 修改字体 descriptor 的真实 CoreText 入口；用于矩阵与未知类型回归，不伪造缓存内容。
fn font_with_attributes(
    font: &CTFont,
    attributes: &CFDictionary<CFString, CFType>,
    matrix: Option<&CGAffineTransform>,
) -> CTFont {
    let descriptor = core_text::font_descriptor::new_from_attributes(attributes);
    let pointer = unsafe {
        variable_font::CTFontCreateCopyWithAttributes(
            font.as_concrete_TypeRef(),
            0.,
            matrix.map_or(std::ptr::null(), |m| m as *const _),
            descriptor.as_concrete_TypeRef(),
        )
    };
    assert!(!pointer.is_null());
    unsafe { CTFont::wrap_under_create_rule(pointer) }
}

/// 非单位矩阵必须保留在 native clone、字重设置、反查、字号复制和 raster 路径中。
#[test]
fn matrix_is_preserved_and_changes_native_identity_and_raster() {
    let (system, family) = system(GEIST);
    let id = system.font_id(&request(&family, 650.)).unwrap();
    let original = native(&system, id);
    let transform = CGAffineTransform::new(1., 0., 0.25, 1., 2., 0.);
    let changed = font_with_attributes(
        &original,
        &original.copy_descriptor().attributes(),
        Some(&transform),
    );
    assert_ne!(native_identity(&original), native_identity(&changed));
    assert_eq!(
        native_identity(&changed),
        native_identity(&changed.clone_with_font_size(32.))
    );
    let id_changed = system.0.write().id_for_native_font(changed.clone());
    assert_ne!(id, id_changed);
    assert!(native_equivalent(&changed, &native(&system, id_changed)));
    let heavier = weight_instance(&changed, 900.)
        .unwrap()
        .unwrap()
        .native_font();
    let a = heavier.get_matrix();
    let b = changed.get_matrix();
    assert_eq!(
        [a.a, a.b, a.c, a.d, a.tx, a.ty],
        [b.a, b.b, b.c, b.d, b.tx, b.ty]
    );
    assert_ne!(raster(&system, id), raster(&system, id_changed));
}

/// CFDate 故意不在支持模型内；feature settings 会丢弃它，因此使用真实保留的 cascade descriptor。
fn font_with_unknown_cascade_value(font: &CTFont, date: f64) -> CTFont {
    let marker = CFDate::new(date);
    let marker_key = CFString::new("LokiMetrisUnknownNativeProbe");
    let fallback_attributes = CFDictionary::from_CFType_pairs(&[
        (unsafe {CFString::wrap_under_get_rule(kCTFontFamilyNameAttribute)},CFString::new("Times New Roman").as_CFType()),
        (marker_key.clone(), marker.as_CFType()),
    ]);
    let fallback = core_text::font_descriptor::new_from_attributes(&fallback_attributes);
    let value = CFArray::from_CFTypes(&[fallback]);
    let attributes = CFDictionary::from_CFType_pairs(&[(
        unsafe { CFString::wrap_under_get_rule(kCTFontCascadeListAttribute) },value.as_CFType(),
    )]);
    let result = font_with_attributes(font, &attributes, None);
    // 先读取实际 CTFont descriptor，证明未知 CFDate 没有被 CoreText 丢弃。
    let settings = unsafe { context(&result, kCTFontCascadeListAttribute) }.expect("native cascade retained");
    assert_eq!(settings.type_of(), CFArray::<CTFontDescriptor>::type_id());
    let settings = unsafe {CFArray::<CTFontDescriptor>::wrap_under_get_rule(settings.as_CFTypeRef() as _)};
    let retained = settings.get(0).unwrap().attributes().find(&marker_key).unwrap().downcast::<CFDate>().unwrap().abs_time();
    assert_eq!(retained, date,"CoreText must actually retain this unknown value");
    eprintln!("retained unknown cascade CFDate = {retained}");
    assert!(native_identity(&result).is_none(),"retained unknown cascade value must use native CFEqual fallback");
    result
}

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

