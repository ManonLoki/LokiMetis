//! 只连接用户选中的已发现地址，不解析任意URL、不使用代理或HTTP重定向。

use loki_metris_core::devices::{DiscoveredDevice, PairingAck, PairingConfirmation, PairingHello, PairingReply, PairingSession, VerifiedPairing};
use serde::{Serialize, de::DeserializeOwned};
use tokio::net::TcpStream;

/// 有界JSON帧读取先检查网络长度，再分配至多4096字节。
pub(super) async fn read_frame<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T, &'static str> {
    super::framing::FrameReader::new(4096).read(stream).await
}

/// 每帧采用网络字节序长度和UTF-8 JSON，不输出密码或持久共享密钥。
pub(super) async fn write_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<(), &'static str> {
    super::framing::write_frame(stream, value, 4096).await
}

/// 每个用户动作仅尝试一次密码交换；连接失败可以选下一个固定端点，发送Hello后不重试。
pub(super) async fn pair(target: &DiscoveredDevice, client: &str, code: &str) -> Result<VerifiedPairing, &'static str> {
    let mut connected = None;
    for endpoint in &target.endpoints {
        if let Ok(Ok(stream)) = tokio::time::timeout(std::time::Duration::from_secs(2), TcpStream::connect(endpoint)).await {
            connected = Some(stream); break;
        }
    }
    let mut stream = connected.ok_or("device_connection_failed")?;
    pair_stream(&mut stream, target, client, code).await
}

/// 同一四帧状态机兼容传统桌面连接与终端主动连接，不复制密码交换逻辑。
pub(super) async fn pair_stream(stream: &mut TcpStream, target: &DiscoveredDevice, client: &str, code: &str) -> Result<VerifiedPairing, &'static str> {
    let (session, hello): (PairingSession, PairingHello) = PairingSession::start(client, target, code, uuid::Uuid::new_v4().to_string()).map_err(|_| "device_protocol_invalid")?;
    write_frame(stream, &hello).await?;
    let reply: PairingReply = read_frame(stream).await?;
    let (waiting, confirmation): (_, PairingConfirmation) = session.accept(reply).map_err(|_| "device_pairing_failed")?;
    write_frame(stream, &confirmation).await?;
    let ack: PairingAck = read_frame(stream).await?;
    waiting.verify(ack).map_err(|_| "device_pairing_failed")
}
