//! 独立 JSON 设置文件有界读取、同目录原子替换，不读取参考 App 配置。

use loki_metris_core::{UsageSettings,ScanIntervalMinutes,RetentionDays};
use serde::{Serialize,Deserialize};
use std::{fs,io::{self,Read,Write},path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering}};

/// 缺字段采用产品缺省，出现非法值或损坏内容则拒绝整个快照。
#[derive(Default,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct StoredSettings {scan_interval_minutes:Option<u16>,retention_days:Option<u16>}

/// 父路径必须解析为目录；系统的 /var 等目录别名可以使用，目标文件仍拒绝链接。
fn safe_parent(path:&Path)->io::Result<()> {
    let mut current=PathBuf::new();
    for part in path.parent().ok_or(io::ErrorKind::InvalidInput)?.components() {
        current.push(part.as_os_str());
        match fs::metadata(&current) {
            Ok(metadata) if !metadata.is_dir()=>return Err(io::ErrorKind::InvalidInput.into()),
            Err(error) if error.kind()!=io::ErrorKind::NotFound=>return Err(error),_=>{},
        }
    }Ok(())
}

/// 新安装缺文件返回默认且零写入；其余错误保留供设置页显式重试。
pub(super) fn load(path:&Path)->io::Result<UsageSettings> {
    safe_parent(path)?;
    let metadata=match fs::symlink_metadata(path) {Ok(metadata)=>metadata,Err(error) if error.kind()==io::ErrorKind::NotFound=>return Ok(UsageSettings::default()),Err(error)=>return Err(error)};
    if !metadata.is_file()||metadata.len()>4096 {return Err(io::ErrorKind::InvalidData.into())}
    let mut bytes=Vec::new();fs::File::open(path)?.take(4097).read_to_end(&mut bytes)?;
    if bytes.len()>4096 {return Err(io::ErrorKind::InvalidData.into())}
    let stored:StoredSettings=serde_json::from_slice(&bytes).map_err(|_|io::ErrorKind::InvalidData)?;
    Ok(UsageSettings {
        scan_interval:stored.scan_interval_minutes.map(ScanIntervalMinutes::new).transpose().map_err(|_|io::ErrorKind::InvalidData)?.unwrap_or_default(),
        retention_days:stored.retention_days.map(RetentionDays::new).transpose().map_err(|_|io::ErrorKind::InvalidData)?.unwrap_or_default(),
    })
}

/// 新文件先完整写入并同步，再原子替换；失败临时文件只在本目录回收。
pub(super) fn save(path:&Path,settings:UsageSettings)->io::Result<()> {
    safe_parent(path)?;let parent=path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(parent)?;safe_parent(path)?;
    if fs::symlink_metadata(path).is_ok_and(|metadata|!metadata.is_file()) {return Err(io::ErrorKind::InvalidInput.into())}
    let bytes=serde_json::to_vec_pretty(&StoredSettings{scan_interval_minutes:Some(settings.scan_interval.get()),retention_days:Some(settings.retention_days.get())}).map_err(io::Error::other)?;
    static SEQUENCE:AtomicU64=AtomicU64::new(0);
    let temporary=parent.join(format!(".usage-settings-{}-{}.tmp",std::process::id(),SEQUENCE.fetch_add(1,Ordering::Relaxed)));
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    let result=(||{file.write_all(&bytes)?;file.sync_all()?;drop(file);fs::rename(&temporary,path)})();
    if result.is_err() {let _=fs::remove_file(temporary);}result
}
