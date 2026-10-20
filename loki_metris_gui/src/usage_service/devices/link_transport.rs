//! 配对后业务握手与加密帧驱动；领域持有密钥，适配器只负责有界网络字节。

use super::{framing::{FrameReader, write_frame}, transport};
use loki_metris_core::devices::{DeviceBook, link::{AuthenticatedLinkSession, DeviceInfo, EncryptedPacket,
    MAX_ENCRYPTED_FRAME_BYTES, SessionConfirmation, SessionHello}};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{io::{AsyncRead, AsyncWrite}, net::TcpStream};

/// 业务握手总预算包含密钥确认和第一条认证设备信息，不按字节到达重置期限。
pub(super) const AUTHENTICATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// 已认证连接仍须由应用owner与当前账本原子建立presence，不能把此快照直接当在线。
pub(super) struct AuthenticatedConnection { pub(super) session: AuthenticatedLinkSession, pub(super) info: DeviceInfo }

/// 完成三帧恢复握手及首条加密元信息；调用方在接受TCP后先限制并发并检查实际peer。
pub(super) async fn authenticate(stream: &mut TcpStream, book: &DeviceBook, hello: SessionHello) -> Result<AuthenticatedConnection, &'static str> {
    tokio::time::timeout(AUTHENTICATION_TIMEOUT, async {
        let (waiting, reply) = book.begin_link_session(hello, uuid::Uuid::new_v4().to_string()).map_err(|_| "device_link_authentication_failed")?;
        transport::write_frame(stream, &reply).await?;
        let confirmation: SessionConfirmation = transport::read_frame(stream).await?;
        let mut session = waiting.verify(confirmation).map_err(|_| "device_link_authentication_failed")?;
        let mut framing = FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES);
        let info: DeviceInfo = receive(&mut framing, stream, &mut session).await?;
        info.validate_for(session.device_id()).map_err(|_| "device_protocol_invalid")?;
        Ok(AuthenticatedConnection { session, info })
    }).await.map_err(|_| "device_timeout")?
}

/// 对业务JSON认证加密后限界发送；明文和凭据不进入日志。
pub(super) async fn send(writer: &mut (impl AsyncWrite + Unpin), session: &mut AuthenticatedLinkSession, value: &impl Serialize) -> Result<(), &'static str> {
    let plaintext = serde_json::to_vec(value).map_err(|_| "device_protocol_invalid")?;
    let packet = session.seal(&plaintext).map_err(|_| "device_link_authentication_failed")?;
    write_frame(writer, &packet, MAX_ENCRYPTED_FRAME_BYTES).await
}

/// 同一framer可跨select取消保留半帧；认证先于JSON解析，坏包由owner关闭连接。
pub(super) async fn receive<T: DeserializeOwned>(framing: &mut FrameReader, reader: &mut (impl AsyncRead + Unpin), session: &mut AuthenticatedLinkSession) -> Result<T, &'static str> {
    let packet: EncryptedPacket = framing.read(reader).await?;
    let plaintext = session.open(&packet).map_err(|_| "device_link_authentication_failed")?;
    serde_json::from_slice(&plaintext).map_err(|_| "device_protocol_invalid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{CLIENT, DEVICE, link_book};
    use loki_metris_core::devices::link::{DevicePlatform, DeviceSession, SessionReply};
    use tokio::net::TcpListener;


    /// 真实环回TCP完成握手、元信息验证及双向业务包，不冒充真实局域网设备验收。
    #[tokio::test]
    async fn tcp_session_authenticates_metadata_and_exchanges_encrypted_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
        let desktop = async {
            let (mut stream, _) = listener.accept().await.unwrap(); let hello: SessionHello = transport::read_frame(&mut stream).await.unwrap();
            let mut connection = authenticate(&mut stream, &link_book(), hello).await.unwrap();
            assert_eq!(connection.info.name, "Test iPhone");
            send(&mut stream, &mut connection.session, &serde_json::json!({"tokens":"1000000001"})).await.unwrap();
            let response: serde_json::Value = receive(&mut FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES), &mut stream, &mut connection.session).await.unwrap();
            assert_eq!(response["ack"], true);
        };
        let device = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let (waiting, hello) = DeviceSession::start(CLIENT, DEVICE, &[0xa7;32], uuid::Uuid::new_v4().to_string()).unwrap();
            transport::write_frame(&mut stream, &hello).await.unwrap(); let reply: SessionReply = transport::read_frame(&mut stream).await.unwrap();
            let (mut channel, confirmation) = waiting.accept(reply).unwrap(); transport::write_frame(&mut stream, &confirmation).await.unwrap();
            let info = DeviceInfo { device_id:DEVICE.into(),name:"Test iPhone".into(),platform:DevicePlatform::Ios,app_version:"1.0".into() };
            let packet = channel.seal(&serde_json::to_vec(&info).unwrap()).unwrap(); write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
            let packet: EncryptedPacket = FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES).read(&mut stream).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&channel.open(&packet).unwrap()).unwrap(); assert_eq!(value["tokens"], "1000000001");
            let packet = channel.seal(br#"{"ack":true}"#).unwrap(); write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
        };
        tokio::join!(desktop, device);
    }
}
