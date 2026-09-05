//! 本模块定义 GUI 层可直接调用的本机扫描器句柄。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

/// Codex 本机扫描器句柄。
pub(crate) struct CodexLocalUsageScanner {
    /// 当前应用的本产品 app-data 根目录。
    app_data_dir: PathBuf,
    /// 与官方读取串行化共享的账号上下文门禁。
    account_context_gate: Arc<Mutex<()>>,
}

/// Claude Code 扫描器句柄。
pub(crate) struct ClaudeLocalUsageScanner {
    /// 本机索引所在的客户端私有目录。
    app_data_dir: PathBuf,
    /// 产品 app-data 根，用于读取全局设置。
    product_app_data_dir: PathBuf,
}

/// Grok Build CLI 扫描器句柄。
pub(crate) struct GrokLocalUsageScanner {
    /// 本机索引所在的客户端私有目录。
    app_data_dir: PathBuf,
    /// 产品 app-data 根，用于读取全局设置。
    product_app_data_dir: PathBuf,
}

impl CodexLocalUsageScanner {
    /// 绑定共享账号上下文门禁。
    pub(crate) fn with_account_context_gate(
        app_data_dir: PathBuf,
        account_context_gate: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            app_data_dir,
            account_context_gate,
        }
    }

    /// 供同步扫描边界读取产品根目录。
    pub(crate) fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }

    /// 克隆共享门禁句柄，供 async 扫描任务跨 await 持有。
    pub(crate) fn account_context_gate_arc(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.account_context_gate)
    }
}

impl ClaudeLocalUsageScanner {
    /// 创建 Claude Code 扫描句柄。
    pub(crate) fn new(app_data_dir: PathBuf, product_app_data_dir: PathBuf) -> Self {
        Self {
            app_data_dir,
            product_app_data_dir,
        }
    }

    /// 供同步扫描边界读取客户端私有目录。
    pub(crate) fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }

    /// 返回产品 app-data 根，供扫描读取已保存保留天数。
    pub(crate) fn product_app_data_dir(&self) -> &Path {
        &self.product_app_data_dir
    }
}

impl GrokLocalUsageScanner {
    /// 创建 Grok Build CLI 扫描句柄。
    pub(crate) fn new(app_data_dir: PathBuf, product_app_data_dir: PathBuf) -> Self {
        Self {
            app_data_dir,
            product_app_data_dir,
        }
    }

    /// 供同步扫描边界读取客户端私有目录。
    pub(crate) fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }

    /// 返回产品 app-data 根，供扫描读取已保存保留天数。
    pub(crate) fn product_app_data_dir(&self) -> &Path {
        &self.product_app_data_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 验证 Codex 扫描器构造后能正确暴露账户上下文门与根目录。
    #[test]
    fn codex_scanner_construction_exposes_gate_and_root() {
        let app_data = tempdir().expect("isolated app-data");
        let scanner = CodexLocalUsageScanner::with_account_context_gate(
            app_data.path().to_path_buf(),
            Arc::new(Mutex::new(())),
        );

        assert_eq!(scanner.app_data_dir(), app_data.path());
    }

    /// 验证 Claude 扫描器构造后能正确暴露根目录。
    #[test]
    fn claude_scanner_exposes_root() {
        let app_data = tempdir().expect("isolated app-data");
        let scanner = ClaudeLocalUsageScanner::new(
            app_data.path().join("clients").join("claude-code"),
            app_data.path().to_path_buf(),
        );
        assert_eq!(
            scanner.app_data_dir(),
            app_data.path().join("clients").join("claude-code")
        );
        assert_eq!(scanner.product_app_data_dir(), app_data.path());
    }
}
