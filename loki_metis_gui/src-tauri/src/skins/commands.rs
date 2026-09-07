//! 把换皮页面的类型化 IPC 映射到唯一 `SkinService` 宿主实例。

use std::collections::HashSet;
use std::path::PathBuf;

use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_dialog::DialogExt;

use crate::bundled_resources::resolve_bundled_resource;

use super::{
    AppError, BatchDeleteResult, BatchImportResult, CodexInstance, CodexRuntimeStatus,
    InstallSkinResult, MAX_IMPORT_BATCH_FILES, PreparedSkinImportBatch, SkinCreationPrompt,
    SkinDescriptor, SkinImportPreparationEvent, SkinReference, SkinService, SkinStatus,
    ThemeConversionResult, save_export_archive,
};
use loki_metis_core::SkinHostKind;

/// 读取当前皮肤注入状态。
#[tauri::command]
pub async fn skin_status(
    host: SkinHostKind,
    service: State<'_, SkinService>,
) -> Result<SkinStatus, AppError> {
    Ok(service.status(host).await)
}

/// 列出经过完整校验的内置与用户皮肤。
#[tauri::command]
pub fn list_skins(service: State<'_, SkinService>) -> Result<Vec<SkinDescriptor>, AppError> {
    service.list_skins()
}

/// 判断用户资源目录是否发生需要重新扫描的变化。
#[tauri::command]
pub fn skin_catalog_changed(service: State<'_, SkinService>) -> Result<bool, AppError> {
    service.catalog_changed()
}

/// 返回使用打包 Skill 创建纯主题的动态提示词。
#[tauri::command]
pub fn skin_creation_prompt(
    app: AppHandle,
    service: State<'_, SkinService>,
) -> Result<SkinCreationPrompt, AppError> {
    let skill_root = resolve_bundled_resource(&app, "codex-skin-generator").map_err(|_| {
        AppError::new(
            "skin.prompt_unavailable",
            "暂时无法定位 LokiMetis 安装资源，请重新启动应用后再试。",
        )
    })?;
    service.skin_creation_prompt(&skill_root)
}

/// 打开原生 ZIP 多选框并流式执行导入预检。
#[tauri::command]
pub async fn prepare_skin_import(
    app: AppHandle,
    on_progress: Channel<SkinImportPreparationEvent>,
    service: State<'_, SkinService>,
) -> Result<Option<PreparedSkinImportBatch>, AppError> {
    let selected = app
        .dialog()
        .file()
        .add_filter("主题/皮肤压缩包", &["zip"])
        .blocking_pick_files();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let paths = selected
        .into_iter()
        .map(|selected| {
            selected
                .into_path()
                .map_err(|_| AppError::new("skin.import_invalid", "无法读取所选 ZIP 路径。"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let paths = validate_import_paths(paths)?;
    service
        .prepare_import_batch_with_progress(paths, move |event| {
            if on_progress.send(event).is_err() {
                tracing::warn!("批量导入预检进度发送失败");
            }
        })
        .await
        .map(Some)
}

/// 在 LokiMetis 用户资源库中创建标准纯主题。
#[tauri::command]
pub async fn create_user_theme(
    name: String,
    author: String,
    service: State<'_, SkinService>,
) -> Result<SkinDescriptor, AppError> {
    service.create_user_theme(&name, &author).await
}

/// 把兼容皮肤有损转换为新的用户纯主题。
#[tauri::command]
pub async fn convert_skin_to_theme(
    skin: SkinReference,
    service: State<'_, SkinService>,
) -> Result<ThemeConversionResult, AppError> {
    service.convert_to_theme(&skin).await
}

/// 生成白名单 ZIP，并在用户确认保存位置后写入。
#[tauri::command]
pub async fn export_skin_package(
    skin: SkinReference,
    app: AppHandle,
    service: State<'_, SkinService>,
) -> Result<bool, AppError> {
    let archive = service.build_export_archive(&skin)?;
    let selected = app
        .dialog()
        .file()
        .set_file_name(format!("{}.zip", skin.id))
        .add_filter("主题/皮肤压缩包", &["zip"])
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(false);
    };
    let mut path = selected
        .into_path()
        .map_err(|_| AppError::new("skin.export_failed", "无法读取皮肤导出保存路径。"))?;
    match path.extension().and_then(|value| value.to_str()) {
        None => {
            path.set_extension("zip");
        }
        Some(value) if value.eq_ignore_ascii_case("zip") => {}
        Some(_) => {
            return Err(AppError::new(
                "skin.export_failed",
                "主题或皮肤必须导出为 ZIP 文件。",
            ));
        }
    }
    save_export_archive(&path, &archive)?;
    Ok(true)
}

/// 校验前端拖入的本机绝对路径，并拒绝批次内重复项。
fn validate_import_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, AppError> {
    loki_metis_core::validate_skin_import_batch_size(paths.len()).map_err(|_| {
        AppError::new(
            "skin.import_batch_limit",
            format!("每次最多选择 {MAX_IMPORT_BATCH_FILES} 个文件。"),
        )
    })?;
    let mut unique = HashSet::new();
    paths
        .into_iter()
        .map(|path| {
            if !path.is_absolute() || !path.is_file() {
                return Err(AppError::new(
                    "skin.import_invalid",
                    "拖入内容必须是可读取的本机文件。",
                ));
            }
            let canonical = path
                .canonicalize()
                .map_err(|_| AppError::new("skin.import_invalid", "无法读取拖入的文件。"))?;
            if !unique.insert(canonical.clone()) {
                return Err(AppError::new(
                    "skin.import_duplicate_path",
                    "同一批次不能包含重复文件。",
                ));
            }
            Ok(canonical)
        })
        .collect()
}

/// 预检来自 Tauri 原生拖放事件的一批路径。
#[tauri::command]
pub async fn prepare_skin_zip_paths(
    paths: Vec<String>,
    on_progress: Channel<SkinImportPreparationEvent>,
    service: State<'_, SkinService>,
) -> Result<PreparedSkinImportBatch, AppError> {
    let paths = validate_import_paths(paths.into_iter().map(PathBuf::from).collect())?;
    service
        .prepare_import_batch_with_progress(paths, move |event| {
            if on_progress.send(event).is_err() {
                tracing::warn!("拖拽批量导入预检进度发送失败");
            }
        })
        .await
}

/// 提交预检批次中由用户确认的条目。
#[tauri::command]
pub async fn commit_skin_import(
    token: String,
    selected_item_ids: Vec<String>,
    service: State<'_, SkinService>,
) -> Result<BatchImportResult, AppError> {
    service
        .commit_import_batch(&token, &selected_item_ids)
        .await
}

/// 取消并清理由指定令牌拥有的导入批次。
#[tauri::command]
pub fn cancel_skin_import(token: String, service: State<'_, SkinService>) -> Result<(), AppError> {
    service.cancel_import(&token)
}

/// 用系统文件管理器打开用户皮肤目录。
#[tauri::command]
pub fn open_skin_directory(
    skin: SkinReference,
    service: State<'_, SkinService>,
) -> Result<(), AppError> {
    service.open_directory(&skin)
}

/// 删除一个经过验证且非内置的用户皮肤。
#[tauri::command]
pub async fn delete_skin(
    skin: SkinReference,
    service: State<'_, SkinService>,
) -> Result<(), AppError> {
    service.delete(&skin).await
}

/// 尝试删除一批用户皮肤并保留逐项失败结果。
#[tauri::command]
pub async fn delete_skins(
    skins: Vec<SkinReference>,
    service: State<'_, SkinService>,
) -> Result<BatchDeleteResult, AppError> {
    service.delete_many(&skins).await
}

/// 读取 Codex 运行与调试连接状态。
#[tauri::command]
pub async fn skin_host_runtime_status(
    host: SkinHostKind,
    service: State<'_, SkinService>,
) -> Result<CodexRuntimeStatus, AppError> {
    service.host_runtime_status(host).await
}

/// 快速列出已验证的本机 Codex GUI 实例。
#[tauri::command]
pub async fn list_skin_host_instances(
    host: SkinHostKind,
    service: State<'_, SkinService>,
) -> Result<Vec<CodexInstance>, AppError> {
    service.scanned_host_instances(host).await
}

/// 对一个已列出的实例补充有界账户资料和活动皮肤。
#[tauri::command]
pub async fn probe_skin_host_instance(
    host: SkinHostKind,
    instance_id: String,
    service: State<'_, SkinService>,
) -> Result<CodexInstance, AppError> {
    service.probe_host_instance(host, &instance_id).await
}

/// 在用户确认后只重启所选且身份未变化的 Codex GUI。
#[tauri::command]
pub async fn restart_skin_host_instance(
    host: SkinHostKind,
    instance_id: String,
    service: State<'_, SkinService>,
) -> Result<CodexInstance, AppError> {
    service.restart_host_instance(host, &instance_id).await
}

/// 启动 Codex 并等待受限 CDP 端点就绪。
#[tauri::command]
pub async fn launch_skin_host(
    host: SkinHostKind,
    service: State<'_, SkinService>,
) -> Result<CodexRuntimeStatus, AppError> {
    service.launch_host(host).await
}

/// 在用户确认后关闭受支持的 Codex GUI，再以调试端口启动。
#[tauri::command]
pub async fn force_launch_skin_host(
    host: SkinHostKind,
    service: State<'_, SkinService>,
) -> Result<CodexRuntimeStatus, AppError> {
    service.force_launch_host(host).await
}

/// 取消当前有 owner 的 Codex 启动或换皮操作。
#[tauri::command]
pub fn cancel_codex_operation(service: State<'_, SkinService>) -> bool {
    service.cancel_codex_operation()
}

/// 把所选皮肤应用到唯一或显式指定的 Codex 实例。
#[tauri::command]
pub async fn install_skin(
    host: SkinHostKind,
    skin: SkinReference,
    allow_appearance_mismatch: bool,
    instance_id: Option<String>,
    service: State<'_, SkinService>,
) -> Result<InstallSkinResult, AppError> {
    service
        .install(
            host,
            &skin,
            allow_appearance_mismatch,
            instance_id.as_deref(),
        )
        .await
}

/// 从唯一或显式指定的 Codex 实例清理换皮注入。
#[tauri::command]
pub async fn uninstall_skin(
    host: SkinHostKind,
    instance_id: Option<String>,
    service: State<'_, SkinService>,
) -> Result<SkinStatus, AppError> {
    service.uninstall(host, instance_id.as_deref()).await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::validate_import_paths;

    /// 验证拖放路径必须绝对、存在且批次内不重复。
    #[test]
    fn dropped_paths_require_absolute_existing_unique_files() {
        let directory = tempfile::tempdir().expect("应创建临时目录");
        let archive = directory.path().join("skin.ZIP");
        fs::write(&archive, []).expect("应创建测试 ZIP 占位文件");

        assert_eq!(
            validate_import_paths(vec![archive.clone()]).expect("绝对且存在的路径应通过"),
            vec![archive.canonicalize().expect("应规范化测试路径")]
        );
        assert_eq!(
            validate_import_paths(vec![PathBuf::from("skin.zip")])
                .expect_err("相对路径必须被拒绝")
                .code,
            "skin.import_invalid"
        );
        assert_eq!(
            validate_import_paths(vec![archive.clone(), archive])
                .expect_err("重复路径必须被拒绝")
                .code,
            "skin.import_duplicate_path"
        );
    }
}
