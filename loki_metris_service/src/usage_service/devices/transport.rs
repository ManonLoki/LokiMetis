//! 在客户端已建立的连接中执行有界四帧认证，不新建连接或重试密码交换。

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

/// 用户确认精确入站请求后，在原连接上完成双向密码与最终回执验证。
pub(super) async fn pair_stream(stream: &mut TcpStream, target: &DiscoveredDevice, client: &str, code: &str) -> Result<VerifiedPairing, &'static str> {
    let (session, hello): (PairingSession, PairingHello) = PairingSession::start(client, target, code, uuid::Uuid::new_v4().to_string()).map_err(|_| "device_protocol_invalid")?;
    write_frame(stream, &hello).await?;
    let reply: PairingReply = read_frame(stream).await?;
    let (waiting, confirmation): (_, PairingConfirmation) = session.accept(reply).map_err(|_| "device_pairing_failed")?;
    write_frame(stream, &confirmation).await?;
    let ack: PairingAck = read_frame(stream).await?;
    waiting.verify(ack).map_err(|_| "device_pairing_failed")
}
