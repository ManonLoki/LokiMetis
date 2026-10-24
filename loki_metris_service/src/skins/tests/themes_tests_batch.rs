// 本文件是 themes.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证换皮迁移中的 `batch_prepare_can_be_cancelled_from_the_started_event` 回归场景。；验证换皮迁移中的 `batch_commit_keeps_selected_items_and_expires_the_batch` 回归场景。；验证换皮迁移中的 `batch_commit_preserves_success_when_another_destination_conflicts` 回归场景。

    #[test]
    /// 验证换皮迁移中的 `batch_prepare_can_be_cancelled_from_the_started_event` 回归场景。
    fn batch_prepare_can_be_cancelled_from_the_started_event() {
        crate::skins::test_block_on(async {
            let root = temp_directory("batch-progress-cancel");
            let fixture = root.join("fixture");
            create_fixture(&fixture, "cancel-skin");
            let archive = root.join("cancel.zip");
            create_zip(&archive, &fixture, None);
            let service = Arc::new(create_service(&root));
            let cancel_service = Arc::clone(&service);

            let error = service
                .prepare_import_batch_with_progress(vec![archive.clone()], move |event| {
                    if let SkinImportPreparationEvent::Started { token, .. } = event {
                        cancel_service
                            .cancel_import(&token)
                            .expect("加载期间取消不应等待操作锁");
                    }
                })
                .await
                .expect_err("预检应响应取消");
            assert_eq!(error.code, "skin.import_cancelled");
            assert!(std::fs::read_dir(root.join("user"))
                .expect("应读取用户目录")
                .next()
                .is_none());

            let prepared = service
                .prepare_import_batch(vec![archive])
                .await
                .expect("取消后下一批应立即可用");
            service
                .cancel_import(&prepared.token)
                .expect("应清理后续批次");
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `batch_commit_keeps_selected_items_and_expires_the_batch` 回归场景。
    fn batch_commit_keeps_selected_items_and_expires_the_batch() {
        crate::skins::test_block_on(async {
            let root = temp_directory("batch-selection");
            let first_fixture = root.join("first-fixture");
            let second_fixture = root.join("second-fixture");
            create_fixture(&first_fixture, "first-skin");
            create_fixture(&second_fixture, "second-skin");
            let first = root.join("first.zip");
            let second = root.join("second.zip");
            create_zip(&first, &first_fixture, None);
            create_zip(&second, &second_fixture, None);
            let service = create_service(&root);
            let prepared = service
                .prepare_import_batch(vec![first, second])
                .await
                .expect("批次应通过预检");

            let result = service
                .commit_import_batch(
                    &prepared.token,
                    &[prepared.items[1].item_id.clone()],
                    true,
                )
                .await
                .expect("选中项应成功提交");
            assert_eq!(result.installed.len(), 1);
            assert_eq!(result.installed[0].id, "second-skin");
            assert!(!root.join("user/first-skin").exists());
            assert!(root.join("user/second-skin/theme.json").is_file());
            assert_eq!(
                service
                    .commit_import_batch(
                        &prepared.token,
                        &[prepared.items[0].item_id.clone()],
                        true,
                    )
                    .await
                    .expect_err("完成后的批次令牌必须失效")
                    .code,
                "skin.import_expired"
            );
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `batch_commit_preserves_success_when_another_destination_conflicts` 回归场景。
    fn batch_commit_preserves_success_when_another_destination_conflicts() {
        crate::skins::test_block_on(async {
            let root = temp_directory("batch-partial");
            let first_fixture = root.join("first-fixture");
            let second_fixture = root.join("second-fixture");
            create_fixture(&first_fixture, "first-skin");
            create_fixture(&second_fixture, "second-skin");
            let first = root.join("first.zip");
            let second = root.join("second.zip");
            create_zip(&first, &first_fixture, None);
            create_zip(&second, &second_fixture, None);
            let service = create_service(&root);
            let prepared = service
                .prepare_import_batch(vec![first, second])
                .await
                .expect("批次应通过预检");
            create_fixture(&root.join("user/second-skin"), "second-skin");
            let selected = prepared
                .items
                .iter()
                .map(|item| item.item_id.clone())
                .collect::<Vec<_>>();

            let result = service
                .commit_import_batch(&prepared.token, &selected, true)
                .await
                .expect("单项冲突不应回滚整个批次");
            assert_eq!(result.installed.len(), 1);
            assert_eq!(result.installed[0].id, "first-skin");
            assert_eq!(result.failed.len(), 1);
            assert_eq!(result.failed[0].code, "skin.import_conflict");
            assert!(root.join("user/first-skin/theme.json").is_file());
            assert!(root.join("user/second-skin/theme.json").is_file());
            assert!(!std::fs::read_dir(root.join("user"))
                .expect("应读取用户目录")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(".import-")));
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }
