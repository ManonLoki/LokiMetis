//! 为本产品私有 SQLite 提供统一的路径、连接和 Unix 权限边界。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sea_orm::{ConnectOptions, Database, DatabaseConnection};

/// 标识私有 SQLite 打开失败的稳定类别，不携带路径或 SQL。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateSqliteError {
    /// app-data 或数据库文件不符合本地普通文件边界。
    InvalidPath,
    /// 当前用户无权创建、读取或收紧数据库权限。
    PermissionDenied,
    /// 文件系统操作失败。
    StorageUnavailable,
    /// SQLite 连接无法建立。
    Database,
}

/// 在显式 app-data 目录中安全打开指定私有 SQLite，并返回真实文件路径。
pub(crate) async fn open_private_sqlite(
    app_data_dir: &Path,
    file_name: &str,
) -> Result<(DatabaseConnection, PathBuf), PrivateSqliteError> {
    if app_data_dir.as_os_str().is_empty()
        || file_name.is_empty()
        || Path::new(file_name).components().count() != 1
    {
        return Err(PrivateSqliteError::InvalidPath);
    }
    if let Ok(metadata) = tokio::fs::symlink_metadata(app_data_dir).await
        && metadata.file_type().is_symlink()
    {
        return Err(PrivateSqliteError::InvalidPath);
    }
    tokio::fs::create_dir_all(app_data_dir)
        .await
        .map_err(map_io_error)?;
    let database_path = app_data_dir.join(file_name);
    if let Ok(metadata) = tokio::fs::symlink_metadata(&database_path).await
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(PrivateSqliteError::InvalidPath);
    }

    let mut options = ConnectOptions::new("sqlite://placeholder.sqlite3");
    options.sqlx_logging(false);
    {
        let database_path = database_path.clone();
        options.map_sqlx_sqlite_opts(move |sqlite_options| {
            sqlite_options
                .filename(&database_path)
                .create_if_missing(true)
                .foreign_keys(true)
                .busy_timeout(Duration::from_millis(5_000))
                .pragma("trusted_schema", "OFF")
        });
    }
    let connection = Database::connect(options)
        .await
        .map_err(|_| PrivateSqliteError::Database)?;
    set_private_database_permissions(&database_path).await?;
    Ok((connection, database_path))
}

/// 在 Unix 上把数据库限制为当前用户读写；其他平台沿用 app-data ACL。
async fn set_private_database_permissions(_database_path: &Path) -> Result<(), PrivateSqliteError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = tokio::fs::metadata(_database_path)
            .await
            .map_err(map_io_error)?
            .permissions();
        permissions.set_mode(0o600);
        tokio::fs::set_permissions(_database_path, permissions)
            .await
            .map_err(map_io_error)?;
    }
    Ok(())
}

/// 把文件系统错误收敛为稳定类别。
fn map_io_error(error: std::io::Error) -> PrivateSqliteError {
    if error.kind() == ErrorKind::PermissionDenied {
        PrivateSqliteError::PermissionDenied
    } else {
        PrivateSqliteError::StorageUnavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 验证共用打开器创建普通私有 SQLite 文件。
    #[tokio::test]
    async fn creates_private_sqlite_file() {
        let temp = tempdir().expect("isolated app-data exists");
        let (_connection, path) = open_private_sqlite(temp.path(), "test.sqlite3")
            .await
            .expect("database opens");
        assert!(path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path)
                    .expect("database metadata exists")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    /// 验证数据库文件符号链接在连接前拒绝。
    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_database_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("isolated app-data exists");
        let target = temp.path().join("target.sqlite3");
        std::fs::write(&target, []).expect("target exists");
        symlink(&target, temp.path().join("linked.sqlite3")).expect("symlink exists");
        assert_eq!(
            open_private_sqlite(temp.path(), "linked.sqlite3")
                .await
                .expect_err("symlink is rejected"),
            PrivateSqliteError::InvalidPath
        );
    }
}
