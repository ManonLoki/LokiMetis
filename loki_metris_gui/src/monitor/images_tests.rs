//! 图库事务使用隔离目录和真实 GIF，验证并发引用与提交失败后的磁盘事实。

use super::*;
use loki_metris_core::{AiProfileDraft, AiTool, HookBehavior};

/// 两帧 GIF89a，可由实际渲染器完整解码而非只满足魔数。
const GIF: &[u8] = include_bytes!("../../tests/fixtures/monitor-loop.gif");

/// 导入、引用阻止删除、清空引用并删除的完整存储闭环。
#[test]
fn import_bind_reload_and_delete_preserves_reference_integrity() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let imported = save_monitor_image(root.path(), "original.gif", GIF).unwrap();
    assert_eq!(imported.snapshot.gallery.counts.gif, 1);
    assert_eq!(
        &*read_monitor_image(root.path(), &imported.id).unwrap(),
        GIF
    );
    let mut draft = AiProfileDraft::default_for(AiTool::Codex);
    draft.hooks[0].image = imported.id.clone();
    draft.hooks[0].content = " ready ".into();
    draft.slot = 8;
    super::super::profiles::save_profile_draft(&config, root.path(), draft.clone()).unwrap();
    let reloaded = super::super::profiles::load_profile_drafts(&config).unwrap();
    assert_eq!(reloaded.drafts[0].slot, 8);
    assert_eq!(reloaded.drafts[0].hooks[0].content, "ready");
    let error = delete_monitor_image(&config, root.path(), &imported.id).unwrap_err();
    assert_eq!(error.code, "error.monitor.imageInUse");
    assert_eq!(
        list_monitor_images(root.path())
            .unwrap()
            .gallery
            .images
            .len(),
        1
    );
    draft.hooks[0].image.clear();
    super::super::profiles::save_profile_draft(&config, root.path(), draft).unwrap();
    assert!(
        delete_monitor_image(&config, root.path(), &imported.id)
            .unwrap()
            .gallery
            .images
            .is_empty()
    );
    assert_eq!(
        std::fs::read_dir(images_dir(root.path())).unwrap().count(),
        1
    );
}

/// 内部隐藏协议的已保存引用也必须阻止删除，公开页面过滤不能丢失它。
#[test]
fn hidden_profile_reference_blocks_image_deletion() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let image = save_monitor_image(root.path(), "hidden.gif", GIF).unwrap();
    let mut hidden = AiProfileDraft::default_for(AiTool::OpenCode);
    hidden.hooks[0].image = image.id.clone();
    super::super::profiles::save_profile_draft(&config, root.path(), hidden).unwrap();
    assert_eq!(
        delete_monitor_image(&config, root.path(), &image.id)
            .unwrap_err()
            .code,
        "error.monitor.imageInUse"
    );
}

/// 索引提交故障清理新文件并逐字保留原索引，不能向读者返回虚假新增。
#[test]
fn failed_index_commit_rolls_back_only_the_new_image() {
    let root = tempfile::tempdir().unwrap();
    save_monitor_image(root.path(), "old.gif", GIF).unwrap();
    let before = std::fs::read(images_dir(root.path()).join("index.json")).unwrap();
    let failure = save_monitor_image_with_commit(root.path(), "new.gif", GIF, |_, _| {
        Err(HookError::new("error.monitor.imagesWriteFailed"))
    });
    assert!(failure.is_err());
    assert_eq!(
        std::fs::read(images_dir(root.path()).join("index.json")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read_dir(images_dir(root.path())).unwrap().count(),
        2
    );
}

/// 不同线程并发导入不能覆盖索引，原名重复也必须取得不同的稳定 ID。
#[test]
fn concurrent_imports_keep_every_distinct_id() {
    let root = tempfile::tempdir().unwrap();
    let mut threads = Vec::new();
    for _ in 0..8 {
        let path = root.path().to_owned();
        threads.push(std::thread::spawn(move || {
            save_monitor_image(&path, "same.gif", GIF).unwrap().id
        }));
    }
    let ids = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(ids.len(), 8);
    let gallery = list_monitor_images(root.path()).unwrap();
    assert_eq!(gallery.gallery.images.len(), 8);
    assert_eq!(gallery.gallery.counts.gif, 8);
}

/// 同时绑定与删除只能有一项成功，不能形成成功保存后的悬空引用。
#[test]
fn concurrent_bind_and_delete_cannot_both_commit() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let image = save_monitor_image(root.path(), "race.gif", GIF).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let one_root = root.path().to_owned();
    let one_config = config.clone();
    let one_id = image.id.clone();
    let one_gate = barrier.clone();
    let save = std::thread::spawn(move || {
        let mut draft = AiProfileDraft::default_for(AiTool::Codex);
        draft.hooks[0].image = one_id;
        one_gate.wait();
        super::super::profiles::save_profile_draft(&one_config, &one_root, draft)
    });
    let two_root = root.path().to_owned();
    let two_id = image.id.clone();
    let two_gate = barrier.clone();
    let delete = std::thread::spawn(move || {
        two_gate.wait();
        delete_monitor_image(&config, &two_root, &two_id)
    });
    barrier.wait();
    assert_ne!(save.join().unwrap().is_ok(), delete.join().unwrap().is_ok());
}

/// 篡改存储名为父目录、Windows 分隔符或绝对路径时不能读取或删除外部对象。
#[test]
fn index_paths_are_confined_to_regular_gallery_images() {
    for name in [
        "../outside.gif",
        "/tmp/outside.gif",
        "..\\outside.gif",
        "index.json",
        "nested/pic.gif",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(images_dir(root.path())).unwrap();
        let record = MonitorImageRecord {
            id: "one".into(),
            filename: "name.gif".into(),
            stored_name: name.into(),
        };
        std::fs::write(
            images_dir(root.path()).join("index.json"),
            serde_json::to_vec(&vec![record]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            list_monitor_images(root.path()).unwrap_err().code,
            "error.monitor.imagesInvalid"
        );
    }
}

/// 非普通图片对象即使索引合法也不读取，图库目录链接不能重定向存储。
#[cfg(unix)]
#[test]
fn symlinked_image_and_directory_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("outside.gif");
    std::fs::write(&source, GIF).unwrap();
    let imported = save_monitor_image(root.path(), "inside.gif", GIF).unwrap();
    let records = load_index(root.path()).unwrap();
    let path = images_dir(root.path()).join(&records[0].stored_name);
    std::fs::remove_file(&path).unwrap();
    symlink(&source, &path).unwrap();
    assert!(read_monitor_image(root.path(), &imported.id).is_err());
    assert!(delete_monitor_image(&root.path().join("monitor"), root.path(), &imported.id).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), GIF);
    let redirected = tempfile::tempdir().unwrap();
    symlink(outside.path(), images_dir(redirected.path())).unwrap();
    assert!(save_monitor_image(redirected.path(), "one.gif", GIF).is_err());
}

/// 超量档案保存失败不能覆盖先前可读快照，写后仍满足读取上限。
#[test]
fn oversized_profile_keeps_last_committed_draft() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let mut draft = AiProfileDraft::default_for(AiTool::Codex);
    draft.slot = 4;
    super::super::profiles::save_profile_draft(&config, root.path(), draft.clone()).unwrap();
    draft
        .hooks
        .iter_mut()
        .find(|hook| hook.behavior == HookBehavior::Idle)
        .unwrap()
        .content = "x".repeat(128 * 1024);
    assert!(super::super::profiles::save_profile_draft(&config, root.path(), draft).is_err());
    assert_eq!(
        super::super::profiles::load_profile_drafts(&config)
            .unwrap()
            .drafts[0]
            .slot,
        4
    );
}

/// 多个坏资源保留可删除记录，不能让整库失去读取与修复入口。
#[test]
fn missing_and_corrupt_images_remain_individually_repairable() {
    let root = tempfile::tempdir().unwrap();
    let missing = save_monitor_image(root.path(), "missing.gif", GIF).unwrap();
    let corrupt = save_monitor_image(root.path(), "corrupt.gif", GIF).unwrap();
    let valid = save_monitor_image(root.path(), "valid.gif", GIF).unwrap();
    let records = load_index(root.path()).unwrap();
    let directory = images_dir(root.path());
    std::fs::remove_file(directory.join(&records[0].stored_name)).unwrap();
    std::fs::write(directory.join(&records[1].stored_name), b"corrupt").unwrap();
    let snapshot = list_monitor_images(root.path()).unwrap();
    assert_eq!(snapshot.gallery.images.len(), 1);
    assert_eq!(snapshot.gallery.images[0].id, valid.id);
    assert_eq!(snapshot.unavailable.len(), 2);
    assert_eq!(snapshot.bytes.len(), 1);
    let config = root.path().join("monitor");
    let snapshot = delete_monitor_image(&config, root.path(), &missing.id).unwrap();
    assert_eq!(snapshot.unavailable.len(), 1);
    let snapshot = delete_monitor_image(&config, root.path(), &corrupt.id).unwrap();
    assert!(snapshot.unavailable.is_empty());
    assert_eq!(snapshot.gallery.images.len(), 1);
}

/// 档案目录链接不能重定向读取、写入或删除前的引用检查。
#[cfg(unix)]
#[test]
fn profile_directory_symlink_is_rejected_without_touching_target() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let original = b"[]";
    std::fs::write(outside.path().join("monitor-profiles.json"), original).unwrap();
    std::os::unix::fs::symlink(outside.path(), &config).unwrap();
    assert!(super::super::profiles::load_profile_drafts(&config).is_err());
    assert!(
        super::super::profiles::save_profile_draft(
            &config,
            root.path(),
            AiProfileDraft::default_for(AiTool::Codex)
        )
        .is_err()
    );
    let image = save_monitor_image(root.path(), "valid.gif", GIF).unwrap();
    assert!(delete_monitor_image(&config, root.path(), &image.id).is_err());
    assert_eq!(
        std::fs::read(outside.path().join("monitor-profiles.json")).unwrap(),
        original
    );
    assert_eq!(
        list_monitor_images(root.path())
            .unwrap()
            .gallery
            .images
            .len(),
        1
    );
}

/// 合法 JSON 也必须满足四行为结构，不能把多余卡片送进固定原生输入数组。
#[test]
fn malformed_profile_behavior_structure_is_rejected_at_read_boundary() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    std::fs::create_dir_all(&config).unwrap();
    for extra in [false, true] {
        let mut draft = AiProfileDraft::default_for(AiTool::Codex);
        if extra {
            draft.hooks.push(draft.hooks[0].clone());
        } else {
            draft.hooks[3] = draft.hooks[0].clone();
        }
        std::fs::write(
            config.join("monitor-profiles.json"),
            serde_json::to_vec(&vec![draft]).unwrap(),
        )
        .unwrap();
        assert!(super::super::profiles::load_profile_drafts(&config).is_err());
    }
}


/// 原生图库快照只返回识别元数据与原始资源，不能为每轮读取重新构造data URL。
#[test]
fn native_gallery_snapshot_has_metadata_and_raw_bytes_without_encoded_payload() {
    let root = tempfile::tempdir().unwrap();
    let imported = save_monitor_image(root.path(), "native.gif", GIF).unwrap();
    let snapshot = list_monitor_images(root.path()).unwrap();
    assert_eq!(&*snapshot.bytes[&imported.id], GIF);
    assert_eq!(snapshot.gallery.counts.gif, 1);
    let json = serde_json::to_value(&snapshot.gallery).unwrap();
    let entry = &json["images"][0];
    assert_eq!(entry["id"], imported.id);
    assert_eq!(entry["filename"], "native.gif");
    assert!(entry.get("image").is_none(), "native metadata cannot contain full Base64 payload");
    assert!(serde_json::to_string(&json).unwrap().len() < 512);
}

/// 桌宠单图读取通过已校验索引，不接受外部路径。
pub(crate) fn read_monitor_image(data: &Path, id: &str) -> Result<Arc<[u8]>, HookError> {
    let _guard = crate::monitor::catalog::read_lock()?;
    let records = load_index(data)?;
    let record = records
        .iter()
        .find(|record| record.id == id)
        .ok_or_else(|| HookError::new("error.monitor.imageNotFound"))?;
    crate::monitor::catalog::read_regular(
        &images_dir(data).join(&record.stored_name),
        MAX_IMAGE_BYTES,
        "error.monitor.imagesReadFailed",
    )
    .map(Arc::from)
}
