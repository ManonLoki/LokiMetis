//! 使用 macOS 系统签名与进程 API 验证可换肤宿主的官方身份。
//!
//! 本模块不读取应用自报的 Info.plist，也不接受 Spotlight 或 PATH 返回的候选；
//! 只有位于系统/当前用户 Applications 根目录、路径链无符号链接且满足固定签名要求的
//! bundle 才能成为后续启动、终止或重启目标。

use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::ptr;

use loki_metis_core::SkinHostKind;

use super::AppError;

/// Core Foundation 不透明对象引用。
type CoreFoundationRef = *const c_void;
/// Security.framework 静态代码对象引用。
type SecurityCodeRef = *const c_void;
/// Security.framework designated requirement 对象引用。
type SecurityRequirementRef = *const c_void;

const UTF8_ENCODING: u32 = 0x0800_0100;
const SECURITY_CHECK_ALL_ARCHITECTURES: u32 = 1 << 0;
const SECURITY_DO_NOT_VALIDATE_RESOURCES: u32 = 1 << 2;
const SECURITY_CHECK_NESTED_CODE: u32 = 1 << 3;
const SECURITY_STRICT_VALIDATE: u32 = 1 << 4;
const SECURITY_DISCOVERY_FLAGS: u32 =
    SECURITY_CHECK_ALL_ARCHITECTURES | SECURITY_DO_NOT_VALIDATE_RESOURCES;
const SECURITY_MUTATION_FLAGS: u32 =
    SECURITY_CHECK_ALL_ARCHITECTURES | SECURITY_CHECK_NESTED_CODE | SECURITY_STRICT_VALIDATE;
const PROCESS_PATH_BUFFER_BYTES: usize = 4096;
const SIGTERM: c_int = 15;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    /// 由绝对文件系统路径创建 CFURL。
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: CoreFoundationRef,
        buffer: *const u8,
        buffer_length: isize,
        is_directory: u8,
    ) -> CoreFoundationRef;
    /// 由 UTF-8 C 字符串创建 CFString。
    fn CFStringCreateWithCString(
        allocator: CoreFoundationRef,
        value: *const c_char,
        encoding: u32,
    ) -> CoreFoundationRef;
    /// 释放 Core Foundation 对象。
    fn CFRelease(value: CoreFoundationRef);
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    /// 从 bundle URL 创建静态代码对象。
    fn SecStaticCodeCreateWithPath(
        path: CoreFoundationRef,
        flags: u32,
        code: *mut SecurityCodeRef,
    ) -> i32;
    /// 从固定文本创建代码签名 requirement。
    fn SecRequirementCreateWithString(
        text: CoreFoundationRef,
        flags: u32,
        requirement: *mut SecurityRequirementRef,
    ) -> i32;
    /// 以固定 requirement 严格校验 bundle 及其嵌套代码。
    fn SecStaticCodeCheckValidity(
        code: SecurityCodeRef,
        flags: u32,
        requirement: SecurityRequirementRef,
    ) -> i32;
}

#[link(name = "proc")]
unsafe extern "C" {
    /// 读取指定 PID 当前映射的可执行文件绝对路径。
    fn proc_pidpath(pid: c_int, buffer: *mut c_void, buffer_size: u32) -> c_int;
}

unsafe extern "C" {
    /// 向已经再次绑定可执行路径的进程发送信号。
    fn kill(pid: c_int, signal: c_int) -> c_int;
}

/// 描述一个受支持宿主不可由运行时覆盖的签名身份。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MacHostIdentity {
    host: SkinHostKind,
    bundle_identifier: &'static str,
    team_identifier: &'static str,
}

impl MacHostIdentity {
    /// 返回 Codex 的固定签名身份。
    const fn codex() -> Self {
        Self {
            host: SkinHostKind::Codex,
            bundle_identifier: "com.openai.codex",
            team_identifier: "2DC432GLL2",
        }
    }

    /// 返回 WorkBuddy 的固定签名身份。
    const fn workbuddy() -> Self {
        Self {
            host: SkinHostKind::WorkBuddy,
            bundle_identifier: "com.tencent.workbuddy.mac",
            team_identifier: "FN2V63AD2J",
        }
    }

    /// 为宿主类型选择不可变的签名身份。
    pub(crate) const fn for_host(host: SkinHostKind) -> Self {
        match host {
            SkinHostKind::Codex => Self::codex(),
            SkinHostKind::WorkBuddy => Self::workbuddy(),
        }
    }

    /// 生成 Security.framework 使用的 designated requirement。
    fn requirement(self) -> String {
        format!(
            "identifier \"{}\" and anchor apple generic and certificate leaf[subject.OU] = \"{}\"",
            self.bundle_identifier, self.team_identifier
        )
    }

    /// 返回发行方使用的稳定物理 bundle 名，避免扫描并验证 Applications 中任意应用。
    fn bundle_names(self) -> &'static [&'static str] {
        match self.host {
            SkinHostKind::Codex => &["ChatGPT.app", "Codex.app"],
            SkinHostKind::WorkBuddy => &["WorkBuddy.app"],
        }
    }

    /// 返回发现失败时不暴露候选路径的稳定错误。
    fn not_found_error(self) -> AppError {
        match self.host {
            SkinHostKind::Codex => {
                AppError::new("skin.codex_not_found", "未找到签名有效的官方 Codex 应用。")
            }
            SkinHostKind::WorkBuddy => AppError::new(
                "skin.workbuddy_not_found",
                "未找到签名有效的官方 WorkBuddy 应用。",
            ),
        }
    }

    /// 返回操作前身份复核失败的稳定错误。
    fn identity_error(self) -> AppError {
        match self.host {
            SkinHostKind::Codex => AppError::new(
                "skin.codex_identity_invalid",
                "Codex 应用身份已变化，请重新安装官方版本后再试。",
            ),
            SkinHostKind::WorkBuddy => AppError::new(
                "skin.workbuddy_identity_invalid",
                "WorkBuddy 应用身份已变化，请重新安装官方版本后再试。",
            ),
        }
    }
}

/// 保存经过路径与签名主体验证的宿主 bundle 和主程序路径。
///
/// 此值可来自跳过资源封装的轻量枚举；任何启动、终止、CDP 或注入路径
/// 都必须再调用 `verify_host_executable` 建立 mutation trust。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedMacApp {
    pub(crate) bundle: PathBuf,
    pub(crate) executable: PathBuf,
}

/// 在限定 Applications 根中轻量发现签名主体有效的宿主，不授予 mutation trust。
pub(crate) fn discover_host_app(host: SkinHostKind) -> Result<VerifiedMacApp, AppError> {
    let identity = MacHostIdentity::for_host(host);
    discover_host_app_in_roots(identity, &approved_application_roots())
        .ok_or_else(|| identity.not_found_error())
}

/// 在启动、终止或重启前重新验证既有可执行路径的 bundle 与签名身份。
pub(crate) fn verify_host_executable(
    host: SkinHostKind,
    executable: &Path,
) -> Result<VerifiedMacApp, AppError> {
    let identity = MacHostIdentity::for_host(host);
    verify_host_executable_in_roots(
        identity,
        executable,
        &approved_application_roots(),
        SECURITY_MUTATION_FLAGS,
    )
    .ok_or_else(|| identity.identity_error())
}

/// 为频繁的只读进程发现复核签名主体，不跳过主可执行文件校验。
pub(crate) fn verify_discovered_executable(
    host: SkinHostKind,
    executable: &Path,
) -> Result<VerifiedMacApp, AppError> {
    let identity = MacHostIdentity::for_host(host);
    verify_host_executable_in_roots(
        identity,
        executable,
        &approved_application_roots(),
        SECURITY_DISCOVERY_FLAGS,
    )
    .ok_or_else(|| identity.identity_error())
}

/// 确认 PID 仍映射到预期官方主程序，避免 PID 复用后操作其它进程。
pub(crate) fn process_matches_executable(pid: u32, executable: &Path) -> bool {
    let Ok(pid) = c_int::try_from(pid) else {
        return false;
    };
    let mut buffer = [0_u8; PROCESS_PATH_BUFFER_BYTES];
    // SAFETY: buffer 在调用期间保持有效且长度精确传入；proc_pidpath 只写入该缓冲区。
    let length = unsafe {
        proc_pidpath(
            pid,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len() as u32,
        )
    };
    if length <= 0 {
        return false;
    }
    let Ok(length) = usize::try_from(length) else {
        return false;
    };
    let process_path = Path::new(std::ffi::OsStr::from_bytes(&buffer[..length]));
    process_path == executable
}

/// 仅在 PID 仍绑定预期官方主程序时发送 SIGTERM。
pub(crate) fn terminate_verified_process(pid: u32, executable: &Path) -> bool {
    if !process_matches_executable(pid, executable) {
        return false;
    }
    let Ok(pid) = c_int::try_from(pid) else {
        return false;
    };
    // SAFETY: PID 已通过 proc_pidpath 与预期主程序再次绑定，信号值为 POSIX SIGTERM。
    unsafe { kill(pid, SIGTERM) == 0 }
}

/// 返回不受 HOME/PATH 环境变量影响的系统与当前用户 Applications 根。
fn approved_application_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications"),
        PathBuf::from(objc2_foundation::NSHomeDirectory().to_string()).join("Applications"),
    ]
}

/// 在给定根目录中检查固定物理 bundle 名，并只采用完整通过路径与签名验证的应用。
fn discover_host_app_in_roots(
    identity: MacHostIdentity,
    roots: &[PathBuf],
) -> Option<VerifiedMacApp> {
    for root in roots {
        for name in identity.bundle_names() {
            let bundle = root.join(name);
            let Some(app) =
                verify_bundle_in_roots(identity, &bundle, roots, SECURITY_DISCOVERY_FLAGS)
            else {
                continue;
            };
            return Some(app);
        }
    }
    None
}

/// 从既有主程序路径回溯 bundle，并重新执行与发现阶段相同的验证。
fn verify_host_executable_in_roots(
    identity: MacHostIdentity,
    executable: &Path,
    roots: &[PathBuf],
    signature_flags: u32,
) -> Option<VerifiedMacApp> {
    let bundle = executable
        .parent()
        .filter(|path| path.file_name().is_some_and(|value| value == "MacOS"))?
        .parent()
        .filter(|path| path.file_name().is_some_and(|value| value == "Contents"))?
        .parent()?;
    let app = verify_bundle_in_roots(identity, bundle, roots, signature_flags)?;
    (app.executable == executable).then_some(app)
}

/// 组合路径边界、主程序与系统签名验证，任何一步不可判定都失败关闭。
fn verify_bundle_in_roots(
    identity: MacHostIdentity,
    bundle: &Path,
    roots: &[PathBuf],
    signature_flags: u32,
) -> Option<VerifiedMacApp> {
    let bundle = validated_bundle_location(bundle, roots)?;
    let executable = validated_main_executable(&bundle)?;
    verify_code_signature(&bundle, identity, signature_flags)
        .then_some(VerifiedMacApp { bundle, executable })
}

/// 要求 bundle 是已规范化、无符号链接且位于批准根直接子级的真实目录。
fn validated_bundle_location(bundle: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let metadata = std::fs::symlink_metadata(bundle).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return None;
    }
    let canonical_bundle = std::fs::canonicalize(bundle).ok()?;
    if canonical_bundle != bundle || canonical_bundle.extension()? != "app" {
        return None;
    }
    let approved = roots.iter().any(|root| {
        let Ok(metadata) = std::fs::symlink_metadata(root) else {
            return false;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return false;
        }
        let Ok(canonical_root) = std::fs::canonicalize(root) else {
            return false;
        };
        canonical_root == *root && canonical_bundle.parent() == Some(canonical_root.as_path())
    });
    approved.then_some(canonical_bundle)
}

/// 从 Contents/MacOS 中选出唯一、规范化、无链接且可执行的普通文件。
fn validated_main_executable(bundle: &Path) -> Option<PathBuf> {
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    for path in [&contents, &macos] {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return None;
        }
    }
    let mut executables = std::fs::read_dir(&macos)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let Ok(metadata) = std::fs::symlink_metadata(path) else {
                return false;
            };
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.permissions().mode() & 0o111 != 0
                && std::fs::canonicalize(path).is_ok_and(|canonical| canonical == *path)
        })
        .collect::<Vec<_>>();
    executables.sort();
    (executables.len() == 1).then(|| executables.remove(0))
}

/// 使用 Security.framework 校验固定 bundle ID、Apple 信任锚与 Team ID。
fn verify_code_signature(bundle: &Path, identity: MacHostIdentity, flags: u32) -> bool {
    let Ok(requirement) = std::ffi::CString::new(identity.requirement()) else {
        return false;
    };
    verify_code_signature_with_requirement(bundle, flags, Some(requirement.as_c_str()))
}

/// 执行底层静态代码校验；生产路径始终传入固定 designated requirement。
fn verify_code_signature_with_requirement(
    bundle: &Path,
    flags: u32,
    requirement: Option<&CStr>,
) -> bool {
    let path = bundle.as_os_str().as_bytes();
    let Ok(path_length) = isize::try_from(path.len()) else {
        return false;
    };
    // SAFETY: URL/String 输入缓冲在创建调用期间有效；返回对象均由本函数成对 CFRelease。
    unsafe {
        let url =
            CFURLCreateFromFileSystemRepresentation(ptr::null(), path.as_ptr(), path_length, 1);
        if url.is_null() {
            return false;
        }
        let requirement_text = requirement.map_or(ptr::null(), |requirement| {
            CFStringCreateWithCString(ptr::null(), requirement.as_ptr(), UTF8_ENCODING)
        });
        if requirement.is_some() && requirement_text.is_null() {
            CFRelease(url);
            return false;
        }

        let mut code = ptr::null();
        let mut parsed_requirement = ptr::null();
        let code_status = SecStaticCodeCreateWithPath(url, 0, &mut code);
        let requirement_status = if requirement_text.is_null() {
            0
        } else {
            SecRequirementCreateWithString(requirement_text, 0, &mut parsed_requirement)
        };
        let requirement_valid = requirement_text.is_null()
            || (requirement_status == 0 && !parsed_requirement.is_null());
        let valid = code_status == 0
            && !code.is_null()
            && requirement_valid
            && SecStaticCodeCheckValidity(code, flags, parsed_requirement) == 0;

        if !parsed_requirement.is_null() {
            CFRelease(parsed_requirement);
        }
        if !code.is_null() {
            CFRelease(code);
        }
        if !requirement_text.is_null() {
            CFRelease(requirement_text);
        }
        CFRelease(url);
        valid
    }
}

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
        tauri::async_runtime::block_on(async {
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
        assert!(validated_bundle_location(&bundle, &[applications.clone()]).is_some());
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

        assert!(validated_bundle_location(&linked_bundle, &[applications.clone()]).is_none());
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
