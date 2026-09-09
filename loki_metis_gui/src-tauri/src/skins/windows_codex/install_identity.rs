//! 解析 Windows 官方安装根并验证宿主可执行文件的规范化身份。

use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, HWND, WIN32_ERROR,
};
use windows::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_NAME_ATTR_TYPE, CertGetNameStringW, szOID_ORGANIZATION_NAME,
};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTD_UICONTEXT_EXECUTE, WTHelperGetProvCertFromChain,
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows::Win32::Storage::Packaging::Appx::{GetPackageFamilyName, GetPackageFullName};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX64,
    FOLDERID_ProgramFilesX86, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::{PCWSTR, PWSTR};

pub(super) const STORE_PACKAGE_FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";
const STORE_PACKAGE_PREFIX: &str = "openai.codex_";
const STORE_PACKAGE_PUBLISHER_SUFFIX: &str = "__2p2nqsd0c76g0";
const MAX_PACKAGE_IDENTITY_CHARS: u32 = 1_024;
const MAX_PUBLISHER_NAME_CHARS: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 传统桌面宿主必须匹配的 Authenticode 发行方集合。
pub(super) enum TrustedPublisher {
    OpenAi,
    Tencent,
}

/// 保存由 Windows Known Folder API 返回且已规范化的安装根。
#[derive(Debug)]
pub(super) struct KnownInstallRoots {
    pub(super) local_app_data: Option<PathBuf>,
    pub(super) program_files: Vec<PathBuf>,
}

impl KnownInstallRoots {
    /// 只从 Windows Known Folder API 收集安装根，不读取进程环境或 PATH。
    pub(super) fn discover() -> Self {
        let local_app_data =
            known_folder_path(&FOLDERID_LocalAppData).and_then(canonical_directory);
        let mut program_files: Vec<PathBuf> = Vec::new();
        for folder_id in [
            &FOLDERID_ProgramFiles,
            &FOLDERID_ProgramFilesX64,
            &FOLDERID_ProgramFilesX86,
        ] {
            let Some(root) = known_folder_path(folder_id).and_then(canonical_directory) else {
                continue;
            };
            if !program_files
                .iter()
                .any(|known| paths_equal_ignore_ascii_case(known, &root))
            {
                program_files.push(root);
            }
        }
        Self {
            local_app_data,
            program_files,
        }
    }
}

/// 从固定官方相对位置发现传统桌面安装，并拒绝链接或重解析到其它位置的候选。
pub(super) fn discover_traditional_executables(roots: &KnownInstallRoots) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(root) = roots.local_app_data.as_ref() {
        candidates.extend([
            fixed_existing_executable(root, "Programs/ChatGPT/ChatGPT.exe"),
            fixed_existing_executable(root, "Programs/Codex/Codex.exe"),
            fixed_existing_executable(root, "OpenAI/ChatGPT/ChatGPT.exe"),
        ]);
    }
    for root in &roots.program_files {
        candidates.extend([
            fixed_existing_executable(root, "ChatGPT/ChatGPT.exe"),
            fixed_existing_executable(root, "OpenAI/ChatGPT/ChatGPT.exe"),
            fixed_existing_executable(root, "Codex/Codex.exe"),
        ]);
    }
    let mut discovered: Vec<PathBuf> = Vec::new();
    for candidate in candidates
        .into_iter()
        .flatten()
        .filter(|candidate| is_trusted_traditional_executable(candidate, TrustedPublisher::OpenAi))
    {
        if !discovered
            .iter()
            .any(|known| paths_equal_ignore_ascii_case(known, &candidate))
        {
            discovered.push(candidate);
        }
    }
    discovered
}

/// 把固定相对位置解析为真实文件，并要求规范路径仍等于该固定位置。
pub(super) fn fixed_existing_executable(root: &Path, relative: &str) -> Option<PathBuf> {
    let expected = root.join(relative);
    let canonical = std::fs::canonicalize(&expected).ok()?;
    if canonical.is_file() && paths_equal_ignore_ascii_case(&canonical, &expected) {
        Some(canonical)
    } else {
        None
    }
}

/// 验证 Store ChatGPT 可执行文件位于 WindowsApps 的精确 Codex 包身份内。
pub(super) fn is_store_gui_path(path: &Path, roots: &KnownInstallRoots) -> bool {
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return false;
    };
    roots
        .program_files
        .iter()
        .any(|root| store_path_matches_root(&canonical, root))
}

/// 将已验证路径与进程自身的 AppModel 身份绑定；路径目录名不能替代 Package API。
pub(super) fn is_verified_store_process(
    process: HANDLE,
    path: &Path,
    roots: &KnownInstallRoots,
) -> bool {
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return false;
    };
    let Some((family, full_name)) = query_process_package_identity(process) else {
        return false;
    };
    roots
        .program_files
        .iter()
        .any(|root| package_identity_matches_store_path(&canonical, root, &family, &full_name))
}

/// 对纯路径执行 Store 包目录结构与 publisher ID 校验，供回归测试复用。
pub(super) fn store_path_matches_root(path: &Path, program_files_root: &Path) -> bool {
    let Some(relative) = strip_prefix_ignore_ascii_case(path, program_files_root) else {
        return false;
    };
    let components = relative
        .components()
        .map(|component| component.as_os_str())
        .collect::<Vec<_>>();
    if components.len() != 4
        || !os_str_eq_ignore_ascii_case(components[0], "WindowsApps")
        || !os_str_eq_ignore_ascii_case(components[2], "app")
        || !os_str_eq_ignore_ascii_case(components[3], "ChatGPT.exe")
    {
        return false;
    }
    components[1]
        .to_str()
        .is_some_and(is_supported_store_package_directory)
}

/// 同时验证路径、精确 family、含 publisher ID 的 full name 及二者目录绑定。
pub(super) fn package_identity_matches_store_path(
    path: &Path,
    program_files_root: &Path,
    family: &str,
    full_name: &str,
) -> bool {
    if !family.eq_ignore_ascii_case(STORE_PACKAGE_FAMILY)
        || !is_supported_store_package_directory(full_name)
        || !store_path_matches_root(path, program_files_root)
    {
        return false;
    }
    strip_prefix_ignore_ascii_case(path, program_files_root)
        .and_then(|relative| {
            relative
                .components()
                .nth(1)
                .map(|component| component.as_os_str().to_owned())
        })
        .is_some_and(|directory| os_str_eq_ignore_ascii_case(&directory, full_name))
}

/// 验证 WindowsApps 包目录只接受精确包名、四段数字版本、架构与 publisher ID。
fn is_supported_store_package_directory(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let Some(body) = lower
        .strip_prefix(STORE_PACKAGE_PREFIX)
        .and_then(|body| body.strip_suffix(STORE_PACKAGE_PUBLISHER_SUFFIX))
    else {
        return false;
    };
    let Some((version, architecture)) = body.rsplit_once('_') else {
        return false;
    };
    matches!(architecture, "x64" | "x86" | "arm64" | "neutral")
        && version.split('.').count() == 4
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// 检查路径是否是固定传统候选或严格 Store 包路径。
pub(super) fn is_verified_gui_process(
    process: HANDLE,
    path: &Path,
    traditional_candidates: &[PathBuf],
    roots: &KnownInstallRoots,
) -> bool {
    if is_store_gui_path(path, roots) {
        return is_verified_store_process(process, path, roots);
    }
    traditional_candidates
        .iter()
        .any(|candidate| paths_equal_ignore_ascii_case(path, candidate))
        && is_trusted_traditional_executable(path, TrustedPublisher::OpenAi)
}

/// 在启动传统宿主前重新解析候选，防止发现后路径被链接或替换到其它位置。
pub(super) fn revalidate_traditional_launch_path(
    path: &Path,
    roots: &KnownInstallRoots,
) -> Option<PathBuf> {
    discover_traditional_executables(roots)
        .into_iter()
        .find(|candidate| paths_equal_ignore_ascii_case(candidate, path))
}

/// 检查当前用户的 Store 包数据是否存在，不读取可伪造的 LOCALAPPDATA 环境变量。
pub(super) fn store_package_data_exists(roots: &KnownInstallRoots) -> bool {
    roots
        .local_app_data
        .as_ref()
        .is_some_and(|root| root.join("Packages").join(STORE_PACKAGE_FAMILY).is_dir())
}

/// 传统桌面候选必须通过 Windows 信任策略且主签名证书发行方在产品白名单内。
pub(super) fn is_trusted_traditional_executable(path: &Path, publisher: TrustedPublisher) -> bool {
    verified_authenticode_publisher(path)
        .is_some_and(|subject| publisher_subject_matches(publisher, &subject))
}

/// 对纯字符串执行精确发行方匹配，避免 contains/前缀匹配接纳相似名称。
pub(super) fn publisher_subject_matches(publisher: TrustedPublisher, subject: &str) -> bool {
    let normalized = subject.trim();
    let expected: &[&str] = match publisher {
        TrustedPublisher::OpenAi => &["OpenAI, L.L.C.", "OpenAI OpCo, LLC"],
        TrustedPublisher::Tencent => &["Tencent Technology (Shenzhen) Company Limited"],
    };
    expected
        .iter()
        .any(|candidate| normalized.eq_ignore_ascii_case(candidate))
}

/// 使用进程句柄读取 AppModel family/full name；未打包进程与任一查询失败都拒绝。
fn query_process_package_identity(process: HANDLE) -> Option<(String, String)> {
    let family = query_package_string(|length, output| {
        // SAFETY: process 是调用方仍持有的查询句柄，缓冲区协议由 query_package_string 保证。
        unsafe { GetPackageFamilyName(process, length, output) }
    })?;
    let full_name = query_package_string(|length, output| {
        // SAFETY: 同上；GetPackageFullName 只写入声明长度的 UTF-16 缓冲区。
        unsafe { GetPackageFullName(process, length, output) }
    })?;
    Some((family, full_name))
}

/// 按 Win32 两阶段缓冲区协议读取并验证单个包身份字符串。
fn query_package_string(
    mut query: impl FnMut(*mut u32, Option<PWSTR>) -> WIN32_ERROR,
) -> Option<String> {
    let mut length = 0u32;
    if query(&mut length, None) != ERROR_INSUFFICIENT_BUFFER
        || !(2..=MAX_PACKAGE_IDENTITY_CHARS).contains(&length)
    {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    if query(&mut length, Some(PWSTR(buffer.as_mut_ptr()))) != ERROR_SUCCESS
        || length == 0
        || length as usize > buffer.len()
    {
        return None;
    }
    buffer.truncate(length as usize);
    if buffer.last() == Some(&0) {
        buffer.pop();
    }
    String::from_utf16(&buffer).ok()
}

/// WinVerifyTrust 成功后从同一 provider state 读取主签名证书显示名。
fn verified_authenticode_publisher(path: &Path) -> Option<String> {
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide_path.as_ptr()),
        ..Default::default()
    };
    let mut trust = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        // 禁止 UI 与联网取回；信任链和签名完整性仍由系统策略验证。
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        dwUIContext: WTD_UICONTEXT_EXECUTE,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: file/trust 及其路径缓冲在 VERIFY 和对应 CLOSE 完成前保持有效。
    let status = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    let publisher = (status == 0)
        .then(|| publisher_from_trust_state(trust.hWVTStateData))
        .flatten();
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: 使用同一个 action 与 state 对称释放 VERIFY 分配的 provider state。
    let _ = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    publisher
}

/// 从尚未关闭的 WinVerifyTrust provider state 提取主签名证书发布者。
fn publisher_from_trust_state(state: HANDLE) -> Option<String> {
    // SAFETY: state 来自仍未 CLOSE 的成功 WinVerifyTrust 调用。
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return None;
    }
    // SAFETY: provider 指向当前 trust state；索引 0 是主签名，不读取时间戳副签名。
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, false, 0) };
    if signer.is_null() {
        return None;
    }
    // SAFETY: signer 属于同一 provider state；索引 0 是叶子签名证书。
    let certificate = unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if certificate.is_null() {
        return None;
    }
    // SAFETY: certificate 在 trust state CLOSE 前有效。
    let context: *const CERT_CONTEXT = unsafe { (*certificate).pCert };
    if context.is_null() {
        return None;
    }
    let organization_oid = szOID_ORGANIZATION_NAME.0.cast();
    // SAFETY: context 有效，OID 是静态 NUL 结尾字符串；空输出先查询包含 NUL 的所需字符数。
    let length = unsafe {
        CertGetNameStringW(
            context,
            CERT_NAME_ATTR_TYPE,
            0,
            Some(organization_oid),
            None,
        )
    };
    if !(2..=MAX_PUBLISHER_NAME_CHARS).contains(&length) {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: buffer 长度来自上一次查询，API 最多写入该切片长度。
    let written = unsafe {
        CertGetNameStringW(
            context,
            CERT_NAME_ATTR_TYPE,
            0,
            Some(organization_oid),
            Some(&mut buffer),
        )
    };
    if written != length {
        return None;
    }
    buffer.truncate(written.saturating_sub(1) as usize);
    String::from_utf16(&buffer).ok()
}

/// 读取 Windows Known Folder 路径并释放系统分配的字符串缓冲区。
pub(super) fn known_folder_path(folder_id: &windows::core::GUID) -> Option<PathBuf> {
    let raw = unsafe { SHGetKnownFolderPath(folder_id, KF_FLAG_DEFAULT, None) }.ok()?;
    let path = unsafe { raw.to_string() }.ok().map(PathBuf::from);
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    path
}

/// 规范化真实目录并拒绝缺失或非目录对象。
fn canonical_directory(path: PathBuf) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(path).ok()?;
    canonical.is_dir().then_some(canonical)
}

/// 按 Windows 路径语义比较规范或 Win32 形式，兼容扩展长度前缀与分隔符。
pub(super) fn paths_equal_ignore_ascii_case(left: &Path, right: &Path) -> bool {
    match (windows_path_key(left), windows_path_key(right)) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(&right),
        _ => false,
    }
}

/// 不区分 ASCII 大小写地移除可信根前缀，并保证边界落在完整路径组件上。
fn strip_prefix_ignore_ascii_case(path: &Path, root: &Path) -> Option<PathBuf> {
    let path_components = path.components().collect::<Vec<_>>();
    let root_components = root.components().collect::<Vec<_>>();
    if root_components.len() > path_components.len()
        || !root_components
            .iter()
            .zip(&path_components)
            .all(|(left, right)| {
                left.as_os_str()
                    .to_str()
                    .zip(right.as_os_str().to_str())
                    .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
            })
    {
        return None;
    }
    Some(
        path_components[root_components.len()..]
            .iter()
            .map(|component| component.as_os_str())
            .collect(),
    )
}

/// 将 Windows 路径转成只用于身份比较的稳定文本键。
fn windows_path_key(path: &Path) -> Option<String> {
    let mut value = path.to_str()?.replace('/', "\\");
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        value = format!(r"\\{rest}");
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        value = rest.to_owned();
    }
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    Some(value)
}

/// 比较受控 ASCII 路径组件，无法无损读取时失败关闭。
fn os_str_eq_ignore_ascii_case(value: &OsStr, expected: &str) -> bool {
    value
        .to_str()
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}
