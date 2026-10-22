//! 每个原生owner独占握手/通道；平台只拥有网络、OS私有保存与原子缓存文件。

use base64::{Engine as _, engine::general_purpose::STANDARD};
use loki_metris_pairing::{*, session::{DeviceSession, SecureChannel, SessionReply, EncryptedPacket}, window::{PairingWindow, PairingAttempt}, assets::{AssetDescriptor, AssetReceiver, AssetChunk}};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Instant;

/// JSON身份只包含应用UUID和展示名，不承载真实硬件标识。
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Identity { device_id:String, name:String }
/// ABI语法在此解码，所有值域与状态转换继续交共享核心。
#[derive(Deserialize)]
#[serde(tag="op",rename_all="camelCase",deny_unknown_fields)]
enum Command {
    OpenPairing { #[serde(rename="clientId")] client_id:String },
    PairHello { hello:PairingHello },
    PairConfirmation { confirmation:PairingConfirmation },
    CheckCommit { #[serde(rename="commitId")] commit_id:String },
    PairingStored { #[serde(rename="commitId")] commit_id:String, stored:bool },
    Resume { #[serde(rename="clientId")] client_id:String, secret:String },
    ResumeReply { reply:SessionReply },
    Seal { message:Value },
    Open { packet:EncryptedPacket },
    AssetBegin { descriptor:AssetDescriptor },
    AssetChunk { chunk:AssetChunk },
    Disconnect {},
    CancelPairing {},
}
/// 本轮密码状态不能Clone或序列化，任何已消费操作都不能重试。
enum Mode {
    Disconnected,
    Pairing(PairingAttempt,ReceiverSession),
    Pending(PairingAttempt,PendingReceiverCommit,String),
    Resuming(DeviceSession),
    Connected(SecureChannel),
}
/// 安全Rust入口供JNI/C封装和测试共用；不依赖数据库或网络运行时。
pub struct MobileClient { identity:Identity, expected_client:Option<String>, window:PairingWindow, mode:Mode, asset:Option<AssetReceiver> }

impl MobileClient {
    /// 私有设备UUID由宿主持久保存后传入，构造不发包、不写文件。
    pub fn new(identity_json:&[u8]) -> Result<Self,&'static str> {
        let identity:Identity=serde_json::from_slice(identity_json).map_err(|_|"mobile_invalid")?;
        if !valid_device_id(&identity.device_id)||!valid_device_name(&identity.name) {return Err("mobile_invalid");}
        Ok(Self {identity,expected_client:None,window:PairingWindow::default(),mode:Mode::Disconnected,asset:None})
    }
    /// 调用结果可能含待保存密钥，只交OS存储，不允许写日志、崩溃详情或UI快照。
    pub fn call(&mut self, command_json:&[u8]) -> Result<Value,&'static str> {
        if command_json.len()>crate::MAX_COMMAND_BYTES {return Err("mobile_limit");}
        let command:Command=serde_json::from_slice(command_json).map_err(|_|"mobile_invalid")?;
        self.execute(command,Instant::now())
    }
    /// 操作消费独占状态；密码验证失败或显式断开终止本轮，尝试预算不返还。
    /// 旧 commitId 被拒绝且不消费当前 pending，避免迟到回调破坏新尝试。
    fn execute(&mut self, command:Command, now:Instant) -> Result<Value,&'static str> {
        match command {
            Command::OpenPairing { client_id } => {
                if !valid_device_id(&client_id)||client_id==self.identity.device_id {return Err("mobile_invalid");}
                self.disconnect();self.window.close();
                let code=format!("{:04}",uuid::Uuid::new_v4().as_u128()%10_000);
                self.window.open(code.clone(),now).map_err(|_|"mobile_pairing_closed")?;
                self.expected_client=Some(client_id.clone());
                Ok(json!({"code":code,"offer":{"kind":"pairing","value":{"version":1,"clientId":client_id,"deviceId":self.identity.device_id,"name":self.identity.name}}}))
            },
            Command::PairHello { hello } => {
                if self.expected_client.as_deref()!=Some(hello.client_id.as_str()) {return Err("mobile_untrusted");}
                if !matches!(self.mode,Mode::Disconnected) {return Err("mobile_busy");}
                let (attempt,receiver,reply)=self.window.begin(hello,&self.identity.device_id,&self.identity.name,uuid::Uuid::new_v4().to_string(),now).map_err(|_|"mobile_pairing_closed")?;
                self.mode=Mode::Pairing(attempt,receiver);Ok(json!({"reply":reply}))
            },
            Command::PairConfirmation { confirmation } => {
                let Mode::Pairing(attempt,receiver)=std::mem::replace(&mut self.mode,Mode::Disconnected) else {return Err("mobile_state_invalid");};
                if self.window.check(&attempt,now).is_err() {self.window.failed(attempt);return Err("mobile_pairing_closed");}
                let pending=match receiver.verify_confirmation(confirmation) {Ok(p)=>p,Err(_)=>{self.window.failed(attempt);return Err("mobile_authentication_failed");}};
                let commit_id=uuid::Uuid::new_v4().to_string();
                let material=pending.material();
                let value=json!({"commitId":commit_id,"material":{"clientId":material.client_id(),"deviceId":material.device_id(),"name":material.device_name(),"secret":STANDARD.encode(material.secret_bytes())}});
                self.mode=Mode::Pending(attempt,pending,commit_id);Ok(value)
            },
            Command::CheckCommit { commit_id } => {
                let Mode::Pending(attempt,_,expected)=&self.mode else {return Err("mobile_state_invalid");};
                if expected!=&commit_id {return Err("mobile_state_invalid");}
                self.window.check(attempt,now).map_err(|_|"mobile_pairing_closed")?;Ok(json!({}))
            },
            Command::PairingStored { commit_id,stored } => {
                if !matches!(&self.mode,Mode::Pending(_,_,expected) if expected==&commit_id) {return Err("mobile_state_invalid");}
                let Mode::Pending(attempt,pending,_)=std::mem::replace(&mut self.mode,Mode::Disconnected) else {return Err("mobile_state_invalid");};
                let ack=self.window.saved(attempt,pending,stored,now).map_err(|_|"mobile_pairing_not_saved")?;
                self.expected_client=None;Ok(json!({"ack":ack}))
            },
            Command::Resume { client_id,secret } => {
                self.disconnect();self.window.close();self.expected_client=None;
                let raw=STANDARD.decode(&secret).map_err(|_|"mobile_invalid")?;
                if raw.len()!=32||STANDARD.encode(&raw)!=secret {return Err("mobile_invalid");}
                let key:[u8;32]=raw.try_into().map_err(|_|"mobile_invalid")?;
                let (waiting,hello)=DeviceSession::start(&client_id,&self.identity.device_id,&key,uuid::Uuid::new_v4().to_string()).map_err(|_|"mobile_invalid")?;
                self.mode=Mode::Resuming(waiting);Ok(json!({"introduction":{"kind":"resume","value":hello}}))
            },
            Command::ResumeReply { reply } => {
                let Mode::Resuming(waiting)=std::mem::replace(&mut self.mode,Mode::Disconnected) else {return Err("mobile_state_invalid");};
                let (channel,confirmation)=waiting.accept(reply).map_err(|_|"mobile_authentication_failed")?;
                self.mode=Mode::Connected(channel);Ok(json!({"confirmation":confirmation}))
            },
            Command::Seal { message } => {
                let Mode::Connected(channel)=&mut self.mode else {return Err("mobile_state_invalid");};
                let plaintext=serde_json::to_vec(&message).map_err(|_|"mobile_invalid")?;
                match channel.seal(&plaintext) {Ok(packet)=>Ok(json!({"packet":packet})),Err(_)=>{self.disconnect();Err("mobile_authentication_failed")}}
            },
            Command::Open { packet } => {
                let Mode::Connected(channel)=&mut self.mode else {return Err("mobile_state_invalid");};
                let plain=match channel.open(&packet) {Ok(plain)=>plain,Err(_)=>{self.disconnect();return Err("mobile_authentication_failed");}};
                match serde_json::from_slice::<Value>(&plain) {Ok(message)=>Ok(json!({"message":message})),Err(_)=>{self.disconnect();Err("mobile_invalid")}}
            },
            Command::AssetBegin { descriptor } => {
                self.asset=None;
                self.asset=Some(AssetReceiver::new(descriptor).map_err(|_|"mobile_asset_invalid")?);Ok(json!({"offset":0}))
            },
            Command::AssetChunk { chunk } => {
                let receiver=self.asset.as_mut().ok_or("mobile_state_invalid")?;
                let complete=match receiver.accept(&chunk) {Ok(c)=>c,Err(_)=>{self.asset=None;return Err("mobile_asset_invalid");}};
                let offset=receiver.offset();
                if complete {let receiver=self.asset.take().ok_or("mobile_state_invalid")?;let _=receiver.finish().map_err(|_|"mobile_asset_invalid")?;}
                Ok(json!({"complete":complete,"offset":offset}))
            },
            Command::Disconnect {} => {self.disconnect();Ok(json!({}))},
            Command::CancelPairing {} => {self.disconnect();self.window.close();self.expected_client=None;Ok(json!({}))},
        }
    }
    /// 传输终态丢弃密码通道和部分资源；只保留用户当前窗口剩余预算。
    fn disconnect(&mut self) {
        match std::mem::replace(&mut self.mode,Mode::Disconnected) {Mode::Pairing(attempt,_)|Mode::Pending(attempt,_,_)=>self.window.failed(attempt),_=>{}}
        self.asset=None;
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
