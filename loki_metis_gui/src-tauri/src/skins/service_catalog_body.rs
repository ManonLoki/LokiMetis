impl SkinService {
    /// 执行换皮宿主内部的 `new` 步骤。
    pub fn new(builtin_root: PathBuf, user_root: PathBuf) -> Self {
        Self {
            builtin_root,
            user_root,
            catalog: StdRwLock::new(None),
            pending_import: StdMutex::new(None),
            import_preparation: StdMutex::new(None),
            next_codex_operation_id: AtomicU64::new(1),
            codex_operation: StdMutex::new(None),
            codex_runtime_generation: AtomicU64::new(0),
            workbuddy_runtime_generation: AtomicU64::new(0),
            account_profile_probes: StdMutex::new(HashMap::new()),
            verified_endpoint_hints: StdMutex::new(HashMap::new()),
            watch_task_reaper: Arc::new(StdMutex::new(WatchTaskReaper::default())),
            operation: Mutex::new(()),
            runtime: Mutex::new(RuntimeState::default()),
        }
    }

    /// 执行换皮宿主内部的 `initialize` 步骤。
    pub fn initialize(&self) -> Result<(), AppError> {
        let metadata = std::fs::symlink_metadata(&self.builtin_root)
            .map_err(|_| AppError::new("skin.builtin_unavailable", "无法读取应用内置皮肤资源。"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AppError::new(
                "skin.builtin_unavailable",
                "应用内置皮肤资源目录无效。",
            ));
        }
        std::fs::create_dir_all(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;
        let user_metadata = std::fs::symlink_metadata(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法检查用户皮肤资源库。"))?;
        if !user_metadata.is_dir() || user_metadata.file_type().is_symlink() {
            return Err(AppError::new(
                "skin.library_unavailable",
                "用户皮肤资源库目录无效。",
            ));
        }
        self.cleanup_transient_directories()?;
        self.refresh_catalog().map(|_| ())
    }

    /// 执行换皮宿主内部的 `list_skins` 步骤。
    pub fn list_skins(&self) -> Result<Vec<SkinDescriptor>, AppError> {
        std::fs::create_dir_all(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;
        let fingerprint = catalog_fingerprint(&self.builtin_root, &self.user_root)?;
        let cached = self
            .catalog
            .read()
            .map_err(|_| catalog_cache_error())?
            .clone();
        if let Some(cached) = cached {
            if cached.fingerprint == fingerprint {
                return Ok(cached.skins);
            }
        }
        self.refresh_catalog()
    }

    /// 执行换皮宿主内部的 `catalog_changed` 步骤。
    pub fn catalog_changed(&self) -> Result<bool, AppError> {
        std::fs::create_dir_all(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;
        let fingerprint = catalog_fingerprint(&self.builtin_root, &self.user_root)?;
        let catalog = self.catalog.read().map_err(|_| catalog_cache_error())?;
        Ok(match catalog.as_ref() {
            Some(cached) => cached.fingerprint != fingerprint,
            None => true,
        })
    }

    /// 执行换皮宿主内部的 `skin_creation_prompt` 步骤。
    pub fn skin_creation_prompt(
        &self,
        bundled_skill_root: &Path,
    ) -> Result<SkinCreationPrompt, AppError> {
        std::fs::create_dir_all(&self.user_root).map_err(|_| {
            AppError::new(
                "skin.prompt_unavailable",
                "暂时无法准备当前用户的皮肤资源库。",
            )
        })?;
        validate_prompt_directory(&self.user_root, "当前用户的皮肤资源库不可用。")?;
        let skill_entry = validate_bundled_skill_file(bundled_skill_root, Path::new("SKILL.md"))?;
        let validator = validate_bundled_skill_file(
            bundled_skill_root,
            Path::new("scripts").join("validate_skin.mjs").as_path(),
        )?;
        let user_root = prompt_json_path(&self.user_root)?;
        let skill_entry = prompt_json_path(&skill_entry)?;
        let validator = prompt_json_path(&validator)?;

        Ok(SkinCreationPrompt {
            prompt: format!(
                r#"请使用 LokiMetis 随安装包提供的 Codex 皮肤生成 Skill 完成本次任务。

以下三个值都是 JSON 字符串形式的绝对路径；使用时请解析字符串值，不要把反斜杠转义当作实际目录名：
- Skill 入口：{skill_entry}
- 正式校验器：{validator}
- 当前用户皮肤资源库：{user_root}

工作要求：
1. 先完整读取 Skill 入口，并严格遵循其中的工作流。Skill 引用的 references、scripts 和 assets 都必须相对 Skill 入口所在目录解析；不要假设 Skill 已安装到 Codex，也不要复制或修改安装包内的 Skill。
2. 开始时只收集尚缺的四项信息，并使用下面的格式，不要在这一轮询问主色、图片路径或颜色模式：

请提供以下信息：

1. 主题名称：
2. 主题说明：
3. 作者：
4. 期望风格：

3. 收齐四项后再单独追问：“使用现有图片背景还是 AI 生成背景？”选择现有图片时再要求提供绝对路径；选择 AI 生成时，直接使用可用的图片生成能力根据期望风格生成适合作为 Codex 大面积背景的图片。图片生成能力不可用时必须如实说明并改为索要现有图片，不得用占位图冒充成品。
4. 默认创建 schemaVersion 3 浅色与深色双模式纯主题，不再询问是否保持双模式；只有我主动明确要求单模式时才改为对应模式。双模式先根据同一视觉意图自动生成两套可用配色，不能简单反色。模式 CSS 支持分别引用不同背景图片，但默认共用一张且不要主动询问；只有我明确要求浅色与深色使用不同图片时才分别使用现有图片或生成图片。
5. 不要要求我提供主色；从期望风格和背景图片推导主色与完整颜色角色。不要询问输出目录，不要制作 ZIP，也不要把设计预览放进用户皮肤资源库。
6. 在用户皮肤资源库内先创建点号开头的隐藏草稿目录，所有文件只写入草稿。根据主题名称生成合法 ID；如果正式目录已存在，依次在 ID 后追加 _1、_2，并让 theme.json.id 与最终目录名一致，显示名称同步追加 (1)、(2)。不得覆盖或合并已有目录。
7. 使用上面给出的正式校验器校验草稿目录；必须真实执行并取得退出码 0，才能继续。修复时保留并处理校验器报告的全部错误，不得以人工目测或自制宽松脚本替代。
8. 草稿通过后，再确认最终目录不存在，并在同一用户皮肤资源库中一次重命名为最终 ID 目录。随后对最终目录再次运行同一正式校验器；如果失败，立即把它重命名回点号开头的隐藏草稿目录，避免 LokiMetis 发现无效半成品。
9. 最终校验退出码为 0 后，提醒我回到 LokiMetis 的“换皮”页面刷新资源库、选择目标 Codex 实例并启用主题。不得自行连接 CDP、调用 Codex 私有设置写入动作，也不得通过命令行启动 LokiMetis。
10. 完成后报告最终目录、文件清单、两次实际校验的命令与退出码、校验器原始结论、预览范围、未验证项和已知限制。无法执行正式校验时必须明确写“静态校验未通过”，不得声称已经通过。

现在请先读取 Skill，再向我收集尚缺的主题输入。"#
            ),
        })
    }

    /// 执行换皮宿主内部的 `create_user_theme` 步骤。
    pub async fn create_user_theme(
        &self,
        name: &str,
        author: &str,
    ) -> Result<SkinDescriptor, AppError> {
        let name = validate_theme_creator_text(name, "主题名称")?;
        let author = validate_theme_creator_text(author, "作者")?;
        let _operation = self.operation.lock().await;
        std::fs::create_dir_all(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;

        let id = available_theme_id(&self.user_root)?;
        let suffix = IMPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let staging = self
            .user_root
            .join(format!(".create-{}-{suffix}", std::process::id()));
        let destination = self.user_root.join(&id);
        let result = (|| {
            std::fs::create_dir(&staging)
                .map_err(|_| AppError::new("skin.create_failed", "无法创建主题暂存目录。"))?;
            write_theme_scaffold(&staging, &id, &name, &author)?;
            let descriptor = load_descriptor(&staging, &id, SkinSource::User)?;
            std::fs::rename(&staging, &destination)
                .map_err(|_| AppError::new("skin.create_failed", "无法保存新建的用户主题。"))?;
            Ok(descriptor)
        })();
        if result.is_err() && staging.exists() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        let descriptor = result?;
        self.invalidate_catalog()?;
        Ok(descriptor)
    }

    /// 把旧六文件兼容皮肤转换成用户资源库中的一份 v3 纯主题副本。
    ///
    /// 转换是有损的：只迁移清单中的配色角色与背景图片，自由 CSS、注入脚本和布局
    /// 不会被转换。原皮肤目录保持不变，内置皮肤同样只读。
    pub async fn convert_to_theme(
        &self,
        skin: &SkinReference,
    ) -> Result<ThemeConversionResult, AppError> {
        let directory = self.skin_directory(skin)?;
        let manifest = read_manifest(&directory)?;
        validate_manifest(&directory, &manifest)?;
        let SkinManifest::Legacy(legacy) = &manifest else {
            return Err(AppError::new(
                "skin.convert_not_legacy",
                "这个资源已经是新版纯主题，不需要转换。",
            ));
        };
        if legacy.id != skin.id {
            return Err(AppError::new(
                "skin.assets_invalid",
                "皮肤目录标识与 theme.json 不一致。",
            ));
        }

        let _operation = self.operation.lock().await;
        std::fs::create_dir_all(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;
        let (id, name) = available_import_identity(
            &self.user_root,
            &converted_theme_id(&legacy.id),
            &converted_theme_name(&legacy.name),
        )?;
        let suffix = IMPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let staging = self
            .user_root
            .join(format!(".convert-{}-{suffix}", std::process::id()));
        let destination = self.user_root.join(&id);
        let result = (|| {
            std::fs::create_dir(&staging)
                .map_err(|_| AppError::new("skin.convert_failed", "无法创建主题暂存目录。"))?;
            let fallback_roles = write_converted_theme(&staging, &directory, legacy, &id, &name)?;
            let descriptor = load_descriptor(&staging, &id, SkinSource::User)?;
            std::fs::rename(&staging, &destination)
                .map_err(|_| AppError::new("skin.convert_failed", "无法保存转换后的主题。"))?;
            Ok((descriptor, fallback_roles))
        })();
        if result.is_err() && staging.exists() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        let (theme, fallback_roles) = result?;
        self.invalidate_catalog()?;
        Ok(ThemeConversionResult {
            theme,
            fallback_roles,
        })
    }

    /// 执行换皮宿主内部的 `build_export_archive` 步骤。
    pub fn build_export_archive(&self, skin: &SkinReference) -> Result<Vec<u8>, AppError> {
        validate_skin_reference(skin)?;
        if skin.source != SkinSource::User {
            return Err(AppError::new(
                "skin.export_builtin",
                "内置主题或皮肤不能导出。",
            ));
        }
        let directory = self.skin_directory(skin)?;
        let manifest = read_manifest(&directory)?;
        validate_manifest(&directory, &manifest)?;
        if manifest.id() != skin.id {
            return Err(AppError::new(
                "skin.assets_invalid",
                "皮肤目录标识与 theme.json 不一致。",
            ));
        }
        build_skin_archive(&directory, &export_file_names(&directory, &manifest)?)
    }

    /// 执行换皮宿主内部的 `refresh_catalog` 步骤。
    pub fn refresh_catalog(&self) -> Result<Vec<SkinDescriptor>, AppError> {
        std::fs::create_dir_all(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;
        let fingerprint_before = catalog_fingerprint(&self.builtin_root, &self.user_root)?;
        let mut skins = Vec::new();
        scan_catalog_root(&self.builtin_root, SkinSource::Builtin, &mut skins)?;
        scan_catalog_root(&self.user_root, SkinSource::User, &mut skins)?;
        skins.sort_by(
            |left, right| match (left.descriptor.source, right.descriptor.source) {
                (SkinSource::User, SkinSource::Builtin) => std::cmp::Ordering::Less,
                (SkinSource::Builtin, SkinSource::User) => std::cmp::Ordering::Greater,
                (SkinSource::User, SkinSource::User) => right
                    .modified
                    .cmp(&left.modified)
                    .then_with(|| left.descriptor.name.cmp(&right.descriptor.name))
                    .then_with(|| left.descriptor.id.cmp(&right.descriptor.id)),
                (SkinSource::Builtin, SkinSource::Builtin) => left
                    .descriptor
                    .name
                    .cmp(&right.descriptor.name)
                    .then_with(|| left.descriptor.id.cmp(&right.descriptor.id)),
            },
        );
        let skins = skins
            .into_iter()
            .map(|skin| skin.descriptor)
            .collect::<Vec<_>>();
        let fingerprint_after = catalog_fingerprint(&self.builtin_root, &self.user_root)?;
        let mut catalog = self.catalog.write().map_err(|_| catalog_cache_error())?;
        *catalog = (fingerprint_before == fingerprint_after).then(|| CachedCatalog {
            fingerprint: fingerprint_after,
            skins: skins.clone(),
        });
        Ok(skins)
    }

    /// 执行换皮宿主内部的 `invalidate_catalog` 步骤。
    fn invalidate_catalog(&self) -> Result<(), AppError> {
        *self.catalog.write().map_err(|_| catalog_cache_error())? = None;
        Ok(())
    }
}
