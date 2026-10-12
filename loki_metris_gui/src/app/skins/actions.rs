//! 资源操作与宿主应用使用固定身份，所有实际I/O交给应用后台owner。

use super::host_action::{ApplyStep, apply_host_action};
use super::state::{PendingApply, RestartMode};
use super::{ModalKind, Outcome, Shell, SkinError, skin_reference};
use gpui_kit::{Context, Window};
use loki_metris_core::SkinReference;
use std::{io::Write, path::PathBuf, sync::atomic::Ordering};

impl Shell {
    /// 菜单中的目录打开也在后台等待launcher终态。
    pub(super) fn open_skin_directory(&mut self, reference: SkinReference, cx: &mut Context<Self>) {
        self.skin_mutation(
            move |service| async move {
                service.open_directory(&reference).await?;
                Ok(Outcome::Opened)
            },
            cx,
        );
    }
    /// 导出先选择保存位置，取消不生成文件或成功通知。
    pub(super) fn export_skin(&mut self, reference: SkinReference, cx: &mut Context<Self>) {
        if !self.skin_writable() || self.skins.modal.is_some() {
            return;
        }
        self.skins.picking = true;
        let authority = self.skins.authority.clone();
        let epoch = authority.load(Ordering::Acquire);
        let picker =
            cx.prompt_for_new_path(&PathBuf::from("."), Some(&format!("{}.zip", reference.id)));
        self.skins.mutation = Some(cx.spawn(async move |entity, cx| {
            let result = picker.await;
            let _ = entity.update(cx, |this, cx| {
                this.skins.picking = false;
                if this.page != super::Page::Skins || authority.load(Ordering::Acquire) != epoch {
                    cx.notify();
                    return;
                }
                this.restore_skin_picker_focus(cx);
                let path = match result {
                    Ok(Ok(Some(path))) => path,
                    Ok(Ok(None)) => {
                        cx.notify();
                        return;
                    }
                    _ => {
                        this.skins.feedback = Some(Err(SkinError::new(
                            "skin.export_failed",
                            "无法打开皮肤保存窗口。",
                        )));
                        cx.notify();
                        return;
                    }
                };
                this.skin_mutation(
                    move |service| async move {
                        let bytes = service.build_export_archive(&reference)?;
                        let mut path = path;
                        let saved = match path.extension() {
                            None => {
                                path.set_extension("zip");
                                // 面板未确认补名后的目标，原子创建避免覆盖已有ZIP或竞态文件。
                                std::fs::OpenOptions::new().write(true).create_new(true)
                                    .open(&path).and_then(|mut output| output.write_all(&bytes))
                            }
                            Some(ext) if ext.eq_ignore_ascii_case("zip") => std::fs::write(path, bytes),
                            _ => {
                                return Err(SkinError::new(
                                    "skin.export_invalid",
                                    "请选择 ZIP 文件名。",
                                ));
                            }
                        };
                        saved.map_err(|_| {
                            SkinError::new("skin.export_failed", "无法保存皮肤压缩包。")
                        })?;
                        Ok(Outcome::Exported)
                    },
                    cx,
                )
            });
        }));
        cx.notify();
    }
    /// 用户输入仅用于标准脚手架；trim及长度的最终校验仍由core拥有。
    pub(super) fn create_skin(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = &self.skins.modal else {
            return;
        };
        let ModalKind::Create(inputs) = &modal.kind else {
            return;
        };
        let name = inputs[0].read(cx).value().to_string();
        let author = inputs[1].read(cx).value().to_string();
        if loki_metris_core::normalize_skin_creator_text(&name).is_err()
            || loki_metris_core::normalize_skin_creator_text(&author).is_err()
        {
            return;
        }
        self.skin_mutation(
            move |service| async move {
                service
                    .create_user_theme(&name, &author)
                    .await
                    .map(Outcome::Created)
            },
            cx,
        );
    }
    /// 生成提示词使用当前安装包与当前用户库的真实绝对路径。
    pub(super) fn skin_prompt(&mut self, cx: &mut Context<Self>) {
        self.skin_mutation(
            move |service| async move {
                let root = crate::usage_service::skin_resource_root()?.join("codex-skin-generator");
                service
                    .skin_creation_prompt(&root)
                    .map(|value| Outcome::Prompt(value.prompt))
            },
            cx,
        );
    }
    /// 删除和转换必须先经过对应原生确认窗，服务再次验证活动引用。
    pub(super) fn confirm_skin_resource(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.skin_writable() {
            return;
        }
        let Some(modal) = self.skins.modal.as_ref() else {
            return;
        };
        let kind = modal.kind.clone();
        match kind {
            ModalKind::Delete(references) => self.skin_mutation(
                move |service| async move {
                    service.delete_many(&references).await.map(Outcome::Deleted)
                },
                cx,
            ),
            ModalKind::Convert(skin) => self.skin_mutation(
                move |service| async move {
                    service
                        .convert_to_theme(&skin_reference(&skin))
                        .await
                        .map(Outcome::Converted)
                },
                cx,
            ),
            ModalKind::Appearance(_, mut intent) => {
                self.close_skin_dialog_now(window, cx);
                intent.allow_appearance = true;
                self.apply_skin_intent(intent, ApplyStep::InstallDirect, cx);
            }
            ModalKind::Restart(mut intent, mode) => {
                self.close_skin_dialog_now(window, cx);
                let step = if mode == RestartMode::RecoverWindowsWorkBuddy {
                    intent.allow_workbuddy_recovery = true;
                    intent.instance = None;
                    ApplyStep::InstallDirect
                } else {
                    ApplyStep::RestartSelected
                };
                self.apply_skin_intent(intent, step, cx);
            }
            ModalKind::Create(_) | ModalKind::Import => return,
        }
        cx.notify();
    }
    /// 兼容包直接应用，与历史产品一样仅在导入预检要求代码信任勾选。
    pub(super) fn apply_skin(
        &mut self,
        skin: &crate::skins::SkinDescriptor,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.skins.host else { return };
        self.apply_skin_intent(
            PendingApply {
                host,
                skin: skin_reference(skin),
                instance: None,
                instance_label: String::new(),
                allow_appearance: false,
                allow_workbuddy_recovery: false,
                allow_third_party_code: skin.package_type
                    == loki_metris_core::SkinPackageType::LegacySkin,
            },
            ApplyStep::Resolve,
            cx,
        );
    }
    /// 每次异步读取后和新副作用之前核对页面授权代数，离页不能晚到启动宿主。
    fn apply_skin_intent(&mut self, intent: PendingApply, step: ApplyStep, cx: &mut Context<Self>) {
        if self.skins.host != Some(intent.host) || !self.skin_hosts().contains(&intent.host) {
            return;
        }
        let authority = self.skins.authority.clone();
        let epoch = authority.load(Ordering::Acquire);
        self.skin_host_mutation(
            move |service| async move {
                let valid = || {
                    if authority.load(Ordering::Acquire) == epoch {
                        Ok(())
                    } else {
                        Err(SkinError::new(
                            "skin.operation_cancelled",
                            "操作上下文已改变。",
                        ))
                    }
                };
                apply_host_action(
                    service.as_ref(),
                    intent,
                    step,
                    cfg!(target_os = "windows"),
                    valid,
                )
                .await
            },
            cx,
        );
    }
    /// 停用只绑定当前唯一实例，后台服务只清理本应用运行状态。
    pub(super) fn stop_skin(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.skins.host else { return };
        let instance = if self.skins.instances.len() == 1 {
            Some(self.skins.instances[0].id.clone())
        } else {
            None
        };
        self.skin_host_mutation(
            move |service| async move {
                service.uninstall(host, instance.as_deref()).await?;
                Ok(Outcome::Stopped)
            },
            cx,
        );
    }
}
