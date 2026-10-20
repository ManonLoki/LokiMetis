//! 核心仅把共享协议的已认证材料转成设备保存证明，不复制密码算法。

use super::{DeviceError, DiscoveredDevice, PairingAck, PairingConfirmation, PairingHello, PairingKey, PairingReply, VerifiedPairing};

/// 发送端只能为当前明确支持约定配对协议的候选建立一次交换。
pub struct PairingSession(loki_metris_pairing::SenderSession);
/// A确认已产生，但B尚未确认保存，此时不能取得可信设备证明。
pub struct AwaitingPairingAck(loki_metris_pairing::AwaitingAck);

impl PairingSession {
    /// 身份与密码规则由两端共享库验证，广告能力仅作为发起条件。
    pub fn start(client: &str, target: &DiscoveredDevice, code: &str, session: String) -> Result<(Self, PairingHello), DeviceError> {
        if !target.pairing_available { return Err(DeviceError::Invalid); }
        let (state, hello) = loki_metris_pairing::SenderSession::start(client, &target.id, code, session).map_err(|_| DeviceError::Invalid)?;
        Ok((Self(state), hello))
    }

    /// 验证B回应并生成一次A确认，尚不改变保存列表。
    pub fn accept(self, reply: PairingReply) -> Result<(AwaitingPairingAck, PairingConfirmation), DeviceError> {
        let (state, confirmation) = self.0.accept(reply).map_err(|_| DeviceError::PairingFailed)?;
        Ok((AwaitingPairingAck(state), confirmation))
    }
}

impl AwaitingPairingAck {
    /// 验证接收端保存回执后才允许生成领域证明，秘密不进入页面投影。
    pub fn verify(self, ack: PairingAck) -> Result<VerifiedPairing, DeviceError> {
        let material = self.0.verify(ack).map_err(|_| DeviceError::PairingFailed)?;
        Ok(VerifiedPairing { client_id: material.client_id().into(), device_id: material.device_id().into(),
            name: material.device_name().into(), key: PairingKey(*material.secret_bytes()) })
    }
}

/// 网络测试夹具复用真实B状态机；此内存回调仅供测试，不是产品接收端存储实现。
#[cfg(any(test, feature = "test-support"))]
pub fn pairing_test_reply(hello: &PairingHello, code: &str) -> (PairingReply, impl FnOnce(PairingConfirmation) -> Result<PairingAck, DeviceError>) {
    let (receiver, reply) = loki_metris_pairing::ReceiverSession::start(hello.clone(), &hello.device_id, "Test receiver", code,
        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into()).expect("valid test handshake");
    (reply, move |confirmation| receiver.confirm_and_commit(confirmation, |_| Ok(())).map_err(|_| DeviceError::PairingFailed))
}
