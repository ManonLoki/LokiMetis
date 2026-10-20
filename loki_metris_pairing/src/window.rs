//! 终端配对窗口的全局60秒/5次/单在途规则，重连与地址改变不重置预算。

use std::time::{Duration, Instant};
use crate::*;

/// 全局窗口单调期限，不能按每次收到字节延长。
pub const PAIRING_WINDOW_LIFETIME: Duration = Duration::from_secs(60);
/// 一次用户开窗最多允许的密码验证次数。
pub const PAIRING_WINDOW_ATTEMPTS: u8 = 5;
/// 线性尝试凭据；宿主必须持原窗口归还，不按IP归还预算。
pub struct PairingAttempt { generation: u64, ticket: u64, binding: [u8;32] }
/// 宿主只映射稳定类别，错误不包含四位码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowError {
    /// 码/身份/消息非法。
    Invalid,
    /// 已取消、成功关闭、过期或使用旧尝试。
    Closed,
    /// 同一窗口已有另一个握手或存储在途。
    Busy,
    /// 五次预算或代数已用尽。
    Limit,
}
/// 单个终端应用拥有一个窗口，绝不把密码/预算暴露在发现广告中。
#[derive(Default)]
pub struct PairingWindow { generation: u64, ticket: u64, opened: Option<Instant>, code: Option<String>, attempts:u8, active:Option<u64> }
impl PairingWindow {
    /// 只有用户明确重新开窗才调用；新码由宿主安全随机源提供。
    pub fn open(&mut self, code: String, now: Instant) -> Result<(), WindowError> {
        if !valid_pairing_code(&code) { return Err(WindowError::Invalid); }
        let generation = self.generation.checked_add(1).ok_or(WindowError::Limit)?;
        self.close(); self.generation = generation; self.opened = Some(now); self.code = Some(code); self.attempts = 0; Ok(())
    }
    /// 取消使所有在途尝试失效；重连不能自动调用open。
    pub fn close(&mut self) { self.opened = None; self.code = None; self.active = None; }
    /// 精确60秒边界和单调时间回退均视为失效。
    fn live(&self, now: Instant) -> bool { self.opened.and_then(|opened| now.checked_duration_since(opened)).is_some_and(|age| age < PAIRING_WINDOW_LIFETIME) && self.code.is_some() }
    /// 先消费全局预算再验证Hello，失败不返还；并发握手不重复预约。
    pub fn begin(&mut self, hello: PairingHello, own_id:&str, name:&str, nonce:String, now:Instant) -> Result<(PairingAttempt,ReceiverSession,PairingReply), WindowError> {
        if !self.live(now) { self.close(); return Err(WindowError::Closed); }
        if self.active.is_some() { return Err(WindowError::Busy); }
        if self.attempts >= PAIRING_WINDOW_ATTEMPTS { return Err(WindowError::Limit); }
        self.attempts += 1;
        self.ticket = self.ticket.checked_add(1).ok_or(WindowError::Limit)?;
        let (receiver,reply) = ReceiverSession::start(hello,own_id,name,self.code.as_deref().ok_or(WindowError::Closed)?,nonce).map_err(|_| WindowError::Invalid)?;
        self.active = Some(self.ticket);
        let binding = receiver.binding();
        Ok((PairingAttempt { generation:self.generation,ticket:self.ticket,binding },receiver,reply))
    }
    /// 交OS存储前与结果返回时均验证原窗口及尝试；不能延长截止或复用旧代次。
    pub fn check(&mut self, attempt:&PairingAttempt, now:Instant) -> Result<(),WindowError> {
        if !self.live(now) { self.close(); return Err(WindowError::Closed); }
        if attempt.generation != self.generation || self.active != Some(attempt.ticket) { return Err(WindowError::Closed); }
        Ok(())
    }
    /// 网络失败/错码结束本次尝试，但不返还预算，也不关闭仍有效的用户窗口。
    pub fn failed(&mut self, attempt:PairingAttempt) { if attempt.generation == self.generation && self.active == Some(attempt.ticket) { self.active = None; } }
    /// 只有原尝试在有效期内实际存储成功才能结束窗口并生成一次ACK。
    pub fn saved(&mut self, attempt:PairingAttempt, pending:PendingReceiverCommit, stored:bool, now:Instant) -> Result<PairingAck,WindowError> {
        self.check(&attempt,now)?;
        if attempt.binding != pending.binding() { self.failed(attempt); return Err(WindowError::Closed); }
        self.active = None;
        if !stored { return Err(WindowError::Invalid); }
        self.close(); pending.finish_commit(Ok(())).map_err(|_| WindowError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const CLIENT:&str="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const DEVICE:&str="cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    const SESSION:&str="dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    const NONCE:&str="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    /// 五次预算跨失败连接保留，单在途不会被第二次请求挤掉，第五次仍允许成功。
    #[test]
    fn window_owns_budget_and_single_attempt_until_real_commit() {
        let now=Instant::now(); let mut window=PairingWindow::default();window.open("0037".into(),now).unwrap();
        for index in 0..5 {
            let (sender,hello)=SenderSession::start(CLIENT,DEVICE,"0037",SESSION.into()).unwrap();
            let (attempt,receiver,reply)=window.begin(hello.clone(),DEVICE,"Phone",NONCE.into(),now).unwrap();
            assert!(matches!(window.begin(hello,DEVICE,"Phone",NONCE.into(),now),Err(WindowError::Busy)));
            if index < 4 { window.failed(attempt); continue; }
            let (sender,confirmation)=sender.accept(reply).unwrap();let pending=receiver.verify_confirmation(confirmation).unwrap();
            let secret=*pending.material().secret_bytes();window.check(&attempt,now).unwrap();
            let ack=window.saved(attempt,pending,true,now).unwrap();assert_eq!(sender.verify(ack).unwrap().secret_bytes(),&secret);
        }
        assert!(window.opened.is_none());
        window.open("0037".into(),now).unwrap();
        for _ in 0..5 { let (_,hello)=SenderSession::start(CLIENT,DEVICE,"0037",SESSION.into()).unwrap();let (attempt,_,_)=window.begin(hello,DEVICE,"Phone",NONCE.into(),now).unwrap();window.failed(attempt); }
        let (_,hello)=SenderSession::start(CLIENT,DEVICE,"0037",SESSION.into()).unwrap();assert!(matches!(window.begin(hello,DEVICE,"Phone",NONCE.into(),now),Err(WindowError::Limit)));
    }
    /// 存储期间过期、重新开窗或取消不再发成功ACK；旧attempt不能影响新窗口。
    #[test]
    fn expired_or_cancelled_pending_commit_never_acknowledges() {
        for action in ["expire","reopen","cancel","store-error"] {
            let now=Instant::now();let mut window=PairingWindow::default();window.open("0037".into(),now).unwrap();
            let (sender,hello)=SenderSession::start(CLIENT,DEVICE,"0037",SESSION.into()).unwrap();
            let (attempt,receiver,reply)=window.begin(hello,DEVICE,"Phone",NONCE.into(),now).unwrap();
            let (_,confirmation)=sender.accept(reply).unwrap();let pending=receiver.verify_confirmation(confirmation).unwrap();
            if action=="reopen" {window.open("1234".into(),now).unwrap();} if action=="cancel" {window.close();}
            let at=if action=="expire" {now+PAIRING_WINDOW_LIFETIME} else {now};
            assert!(window.saved(attempt,pending,action!="store-error",at).is_err());
        }
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    /// 即使宿主误复用session与nonce，旧随机交换的pending也不能借新尝试生成ACK。
    #[test]
    fn pending_commit_from_other_random_exchange_is_rejected() {
        let client="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";let device="cccccccc-cccc-4ccc-8ccc-cccccccccccc";
        let session="dddddddd-dddd-4ddd-8ddd-dddddddddddd";let nonce="bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let now=Instant::now();let mut window=PairingWindow::default();window.open("0037".into(),now).unwrap();
        let (sender,hello)=SenderSession::start(client,device,"0037",session.into()).unwrap();
        let (attempt,receiver,reply)=window.begin(hello,device,"Phone",nonce.into(),now).unwrap();
        let (_,confirmation)=sender.accept(reply).unwrap();let old=receiver.verify_confirmation(confirmation).unwrap();window.failed(attempt);
        let (_,hello)=SenderSession::start(client,device,"0037",session.into()).unwrap();
        let (attempt,_,_)=window.begin(hello,device,"Phone",nonce.into(),now).unwrap();
        assert!(window.saved(attempt,old,true,now).is_err());
    }
}
