//! 位置解析只读取当前产品的完整档案，缺失补齐与默认值由 core 决定。

use std::{io::Read,path::Path};
use loki_metris_core::{AiProfileDraft,AiProfileDraftSet,HookError,merge_profile_drafts};

/// 档案读取上限阻止损坏文件在事件工作线程无界分配。
const MAX_PROFILE_BYTES:u64=128*1024;

/// 只读零写入入口；拒绝符号链接、非常规文件及超量输入。
pub(super) fn load_profile_drafts(directory:&Path)->Result<AiProfileDraftSet,HookError> {
 let path=directory.join("monitor-profiles.json");
 let metadata=match std::fs::symlink_metadata(&path) {Ok(metadata)=>metadata,Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(AiProfileDraftSet{drafts:merge_profile_drafts(Vec::new())}),Err(_)=>return Err(HookError::new("error.monitor.profilesReadFailed"))};
 if !metadata.is_file()||metadata.len()>MAX_PROFILE_BYTES {return Err(HookError::new("error.monitor.profilesInvalid"))}
 let mut raw=Vec::new();std::fs::File::open(&path).and_then(|file|file.take(MAX_PROFILE_BYTES+1).read_to_end(&mut raw)).map_err(|_|HookError::new("error.monitor.profilesReadFailed"))?;
 if raw.len() as u64>MAX_PROFILE_BYTES {return Err(HookError::new("error.monitor.profilesInvalid"))}
 let drafts=serde_json::from_slice::<AiProfileDraftSet>(&raw).map(|set|set.drafts).or_else(|_|serde_json::from_slice::<Vec<AiProfileDraft>>(&raw)).map_err(|_|HookError::new("error.monitor.profilesInvalid"))?;
 Ok(AiProfileDraftSet{drafts:merge_profile_drafts(drafts)})
}
