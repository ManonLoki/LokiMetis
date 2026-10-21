//! 完整账本恢复只允许空账本或完全相同材料；公开UUID不能替代恢复码认证。

use super::{DeviceBook, SavedDeviceSummary};
pub use loki_metris_pairing::recovery::MAX_PACKAGE_BYTES;

/// 只返回稳定脱敏类别，错误不包含恢复码、材料或宿主路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryError {
    /// 包、恢复码、版本或私有账本无效。
    #[error("device_recovery_invalid")]
    Invalid,
    /// 当前已有不同信任或预览后发生变更，不能静默覆盖。
    #[error("device_recovery_conflict")]
    Conflict,
}

/// 可展示的原设备身份与数量；不提供配对密钥。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoverySummary {
    /// 包内经恢复码认证的原桌面身份。
    pub client_id: String,
    /// 包内经过完整结构校验的设备列表。
    pub devices: Vec<SavedDeviceSummary>,
}

/// 预览材料只留服务层；提交时必须再次比对预览前的完整账本。
pub struct RecoveryPlan { expected: DeviceBook, recovered: DeviceBook }

impl RecoveryPlan {
    /// 从已认证材料生成无秘密摘要，供用户明确核对恢复对象。
    pub fn summary(&self) -> RecoverySummary {
        RecoverySummary { client_id: self.recovered.client_id().into(), devices: self.recovered.summaries() }
    }
    /// 预览后当前账本变化即拒绝；成功返回待原子落盘的新权威账本。
    pub fn commit(self, current: &DeviceBook) -> Result<DeviceBook, RecoveryError> {
        if *current != self.expected { return Err(RecoveryError::Conflict); }
        Ok(self.recovered)
    }
}

impl DeviceBook {
    /// 原UUID与所有已配对密钥一并加密；恢复码只交调用者单独保存。
    pub fn export_recovery(&self, key: &[u8; 32], nonce: [u8; 12]) -> Result<(Vec<u8>, String), RecoveryError> {
        let bytes = self.encode_private().map_err(|_| RecoveryError::Invalid)?;
        let package = loki_metris_pairing::recovery::seal(&bytes, key, nonce).map_err(|_| RecoveryError::Invalid)?;
        let code = loki_metris_pairing::recovery::encode_code(key).map_err(|_| RecoveryError::Invalid)?;
        Ok((package, code))
    }
    /// 解密后完整验证身份和密钥；非空异样账本失败，不合并或覆盖其现有信任。
    pub fn prepare_recovery(&self, package: &[u8], code: &str) -> Result<RecoveryPlan, RecoveryError> {
        let bytes = loki_metris_pairing::recovery::open(package, code).map_err(|_| RecoveryError::Invalid)?;
        let recovered = Self::decode(&bytes).map_err(|_| RecoveryError::Invalid)?;
        // 空账本的显式恢复可重新授予包内本地信任；UI须提示旧包可能包含后来忘记的配对。
        if self != &recovered && !self.devices.is_empty() {
            return Err(RecoveryError::Conflict);
        }
        Ok(RecoveryPlan { expected: self.clone(), recovered })
    }
}

/// 发现名称明确区分隔离实例，仍由认证握手决定真实信任。
pub fn desktop_instance_name(client_id: &str, isolated: bool) -> String {
    format!("LokiMetris{}-{}", if isolated { " TEST" } else { "" }, client_id.chars().take(8).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    const CLIENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    /// 固定测试账本只供恢复回归，生产仍须四帧证明才能新增配对。
    fn fixture() -> DeviceBook {
        DeviceBook::decode(&serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"clientId":CLIENT,
            "devices":[{"id":DEVICE,"name":"Phone","key":vec![0xa7u8;32]}]})).unwrap()).unwrap()
    }
    /// 新安装恢复保留原UUID与密钥，错误码/异样非空账本/预览后修改均失败。
    #[test]
    fn full_recovery_preserves_credentials_and_rejects_conflicts() {
        let source = fixture(); let fresh = DeviceBook::new(OTHER.into()).unwrap();
        let (package, code) = source.export_recovery(&[7; 32], [8; 12]).unwrap();
        let plan = fresh.prepare_recovery(&package, &code).unwrap();
        assert_eq!(plan.summary().client_id, CLIENT);
        assert_eq!(plan.commit(&fresh).unwrap(), source);
        assert!(fresh.prepare_recovery(&package, "wrong").is_err());
        let emptied = source.without(DEVICE).unwrap();
        assert_eq!(emptied.prepare_recovery(&package, &code).unwrap().commit(&emptied).unwrap(), source);
        let mut different = source.clone(); different.devices[0].key.0 = [9;32];
        assert_eq!(different.prepare_recovery(&package, &code).err(), Some(RecoveryError::Conflict));
        let plan = fresh.prepare_recovery(&package, &code).unwrap();
        assert_eq!(plan.commit(&different).err(), Some(RecoveryError::Conflict));
        assert_eq!(source.prepare_recovery(&package, &code).unwrap().commit(&source).unwrap(), source);
    }
    /// 正式与测试广播名称可见区分，不改变同一UUID与恢复认证合同。
    #[test]
    fn isolated_instance_advertises_test_marker() {
        assert_eq!(desktop_instance_name(CLIENT, false), "LokiMetris-aaaaaaaa");
        assert_eq!(desktop_instance_name(CLIENT, true), "LokiMetris TEST-aaaaaaaa");
    }
    /// 恢复后仍须证明原秘密；仅公开双端UUID相同的旧/错误凭据不能完成握手。
    #[test]
    fn restored_book_does_not_authenticate_public_ids_or_wrong_secret() {
        let source = fixture(); let fresh = DeviceBook::new(OTHER.into()).unwrap();
        let (package, code) = source.export_recovery(&[7; 32], [8; 12]).unwrap();
        let restored = fresh.prepare_recovery(&package, &code).unwrap().commit(&fresh).unwrap();
        let (device, hello) = loki_metris_pairing::session::DeviceSession::start(CLIENT, DEVICE, &[9;32], OTHER.into()).unwrap();
        let (_, reply) = restored.begin_link_session(hello, OTHER.into()).unwrap();
        assert!(device.accept(reply).is_err());
        let empty = restored.without(DEVICE).unwrap();
        let (_, hello) = loki_metris_pairing::session::DeviceSession::start(CLIENT, DEVICE, &[0xa7;32], OTHER.into()).unwrap();
        assert!(empty.begin_link_session(hello, OTHER.into()).is_err());
    }
}
