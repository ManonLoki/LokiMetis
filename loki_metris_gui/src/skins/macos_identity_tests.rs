// 本文件是 macos_identity.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use tempfile::tempdir;

    use super::*;

    /// 创建最小伪造 bundle，允许测试证明自报 plist 不构成官方身份。
    fn fake_bundle(root: &Path, name: &str, bundle_identifier: &str) -> PathBuf {
        let bundle = root.join(format!("{name}.app"));
        let executable = bundle.join("Contents/MacOS/Fake");
        std::fs::create_dir_all(executable.parent().expect("应有主程序目录"))
            .expect("应创建伪造 bundle");
        std::fs::write(&executable, b"#!/bin/sh\nexit 0\n").expect("应写入伪造主程序");
        let mut permissions = std::fs::metadata(&executable)
            .expect("应读取伪造主程序")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("应设置可执行位");
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\"?><plist><dict><key>CFBundleIdentifier</key><string>{bundle_identifier}</string><key>CFBundleExecutable</key><string>Fake</string></dict></plist>"
            ),
        )
        .expect("应写入伪造 plist");
        bundle
    }

    /// 创建并使用系统 codesign 临时签署最小 Mach-O bundle，仅用于资源封装回归。
    fn ad_hoc_signed_bundle(root: &Path) -> PathBuf {
        let bundle = root.join("ResourceProbe.app");
        let executable = bundle.join("Contents/MacOS/ResourceProbe");
        let resource = bundle.join("Contents/Resources/probe.txt");
        std::fs::create_dir_all(executable.parent().expect("应有主程序目录"))
            .expect("应创建主程序目录");
        std::fs::create_dir_all(resource.parent().expect("应有资源目录")).expect("应创建资源目录");
        std::fs::copy("/usr/bin/true", &executable).expect("应复制系统 Mach-O 样本");
        std::fs::write(&resource, b"sealed\n").expect("应写入待封装资源");
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
              <plist version=\"1.0\"><dict>\
              <key>CFBundleIdentifier</key><string>local.lokimetis.resource-probe</string>\
              <key>CFBundleExecutable</key><string>ResourceProbe</string>\
              <key>CFBundlePackageType</key><string>APPL</string>\
              </dict></plist>",
        )
        .expect("应写入测试 bundle 元数据");
        crate::skins::test_block_on(async {
            let mut command = tokio::process::Command::new("/usr/bin/codesign");
            command
                .args(["--force", "--sign", "-", "--timestamp=none"])
                .arg(&bundle);
            assert!(
                super::super::macos_process::run_bounded_test_command(command).await,
                "系统 codesign 应能创建临时测试签名"
            );
        });
        bundle
    }

    /// 即使 plist 自报完全匹配，未签名 bundle 仍必须被 Security.framework 拒绝。
    #[test]
    fn unsigned_bundle_with_forged_plist_is_rejected() {
        let temporary = tempdir().expect("应创建临时目录");
        let applications = temporary
            .path()
            .canonicalize()
            .expect("临时根应可规范化")
            .join("Applications");
        std::fs::create_dir(&applications).expect("应创建批准根");
        let bundle = fake_bundle(&applications, "ForgedCodex", "com.openai.codex");
        assert!(validated_bundle_location(&bundle, std::slice::from_ref(&applications)).is_some());
        assert!(!verify_code_signature(
            &bundle,
            MacHostIdentity::codex(),
            SECURITY_MUTATION_FLAGS,
        ));
        assert!(
            verify_bundle_in_roots(
                MacHostIdentity::codex(),
                &bundle,
                &[applications],
                SECURITY_MUTATION_FLAGS,
            )
            .is_none()
        );
    }

    /// 符号链接 bundle 与批准根之外的真实目录都不能进入签名验证候选。
    #[test]
    fn symlink_and_outside_bundle_paths_are_rejected() {
        let temporary = tempdir().expect("应创建临时目录");
        let temporary_root = temporary.path().canonicalize().expect("临时根应可规范化");
        let applications = temporary_root.join("Applications");
        let outside = temporary_root.join("Outside");
        std::fs::create_dir(&applications).expect("应创建批准根");
        std::fs::create_dir(&outside).expect("应创建根外目录");
        let outside_bundle = fake_bundle(&outside, "OutsideCodex", "com.openai.codex");
        let linked_bundle = applications.join("LinkedCodex.app");
        symlink(&outside_bundle, &linked_bundle).expect("应创建 bundle 链接");

        assert!(validated_bundle_location(&linked_bundle, std::slice::from_ref(&applications)).is_none());
        assert!(validated_bundle_location(&outside_bundle, &[applications]).is_none());
    }

    /// 轻量枚举可跳过资源封装，但资源篡改后不得进入启动、终止、CDP 或注入的 mutation trust。
    #[test]
    fn resource_tampering_is_rejected_by_mutation_trust() {
        let temporary = tempdir().expect("应创建临时目录");
        let bundle = ad_hoc_signed_bundle(temporary.path());
        assert!(verify_code_signature_with_requirement(
            &bundle,
            SECURITY_DISCOVERY_FLAGS,
            None,
        ));
        assert!(verify_code_signature_with_requirement(
            &bundle,
            SECURITY_MUTATION_FLAGS,
            None,
        ));

        std::fs::write(bundle.join("Contents/Resources/probe.txt"), b"tampered\n")
            .expect("应篡改已封装资源");

        assert!(verify_code_signature_with_requirement(
            &bundle,
            SECURITY_DISCOVERY_FLAGS,
            None,
        ));
        assert!(!verify_code_signature_with_requirement(
            &bundle,
            SECURITY_MUTATION_FLAGS,
            None,
        ));
        assert_eq!(
            SECURITY_MUTATION_FLAGS & SECURITY_DO_NOT_VALIDATE_RESOURCES,
            0
        );
    }

    /// 官方本机样本存在时必须同时通过固定 bundle ID、Apple anchor 与 Team ID requirement。
    #[test]
    fn installed_official_samples_match_fixed_requirements_when_present() {
        for (path, identity) in [
            (
                Path::new("/Applications/ChatGPT.app"),
                MacHostIdentity::codex(),
            ),
            (
                Path::new("/Applications/WorkBuddy.app"),
                MacHostIdentity::workbuddy(),
            ),
        ] {
            if path.is_dir() {
                assert!(verify_code_signature(
                    path,
                    identity,
                    SECURITY_MUTATION_FLAGS,
                ));
                let discovered = discover_host_app(identity.host).expect("官方样本应可安全发现");
                assert_eq!(discovered.bundle, path);
                assert!(
                    discovered
                        .executable
                        .starts_with(path.join("Contents/MacOS"))
                );
            }
        }
    }

    /// 固定签名 requirement 必须明确绑定产品标识、Apple 锚和官方 Team ID。
    #[test]
    fn designated_requirements_bind_all_identity_fields() {
        let codex = MacHostIdentity::codex().requirement();
        assert!(codex.contains("identifier \"com.openai.codex\""));
        assert!(codex.contains("anchor apple generic"));
        assert!(codex.contains("2DC432GLL2"));
        let workbuddy = MacHostIdentity::workbuddy().requirement();
        assert!(workbuddy.contains("identifier \"com.tencent.workbuddy.mac\""));
        assert!(workbuddy.contains("FN2V63AD2J"));
    }
}
