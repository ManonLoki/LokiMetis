//! 原生桥回归运行真实共享密码状态机，全部密钥与身份均为隔离合成数据。
use super::*;
const CLIENT:&str="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DEVICE:&str="cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const SESSION:&str="dddddddd-dddd-4ddd-8ddd-dddddddddddd";

/// 模拟JNI/Swift统一UTF-8 JSON边界，不调试或输出真实秘密。
fn call(client:&mut MobileClient,value:Value)->Result<Value,&'static str> {client.call(&serde_json::to_vec(&value).unwrap())}
/// 隔离应用身份包含中文与非BMP字符，保证不用JNI modified-UTF8处理正文。
fn client()->MobileClient {MobileClient::new(&serde_json::to_vec(&json!({"deviceId":DEVICE,"name":"显示终端📱"})).unwrap()).unwrap()}

/// OS保存成功才返回ACK；失败/重复完成/取消及过期均不能生成保存回执。
#[test]
fn pairing_bridge_has_explicit_private_storage_commit_boundary() {
    for scenario in ["stored","failed","cancel","expire"] {
        let mut phone=client();let now=Instant::now();
        let opened=phone.execute(Command::OpenPairing {client_id:CLIENT.into()},now).unwrap();let code=opened["code"].as_str().unwrap();
        let (desktop,hello)=SenderSession::start(CLIENT,DEVICE,code,SESSION.into()).unwrap();
        let reply=phone.execute(Command::PairHello {hello},now).unwrap();
        let (desktop,confirmation)=desktop.accept(serde_json::from_value(reply["reply"].clone()).unwrap()).unwrap();
        let material=phone.execute(Command::PairConfirmation {confirmation},now).unwrap();
        assert!(material.get("ack").is_none());assert_eq!(material["material"]["deviceId"],DEVICE);
        call(&mut phone,json!({"op":"checkCommit","commitId":material["commitId"]})).unwrap();
        if scenario=="cancel" {phone.execute(Command::CancelPairing {},now).unwrap();}
        let at=if scenario=="expire" {now+std::time::Duration::from_secs(60)} else {now};
        if scenario=="expire" {assert!(phone.execute(Command::CheckCommit {commit_id:material["commitId"].as_str().unwrap().into()},at).is_err());}
        let result=phone.execute(Command::PairingStored {commit_id:material["commitId"].as_str().unwrap().into(),stored:scenario!="failed"},at);
        if scenario=="stored" {
            let ack=serde_json::from_value(result.unwrap()["ack"].clone()).unwrap();
            let saved=desktop.verify(ack).unwrap();
            assert_eq!(STANDARD.encode(saved.secret_bytes()),material["material"]["secret"]);
        } else {assert!(result.is_err());}
        assert!(call(&mut phone,json!({"op":"pairingStored","commitId":material["commitId"],"stored":true})).is_err());
    }
}

/// 恢复握手与业务包走真实A/B通道；坏包永久关闭，不能继续复用nonce。
#[test]
fn resumed_native_channel_authenticates_both_directions_and_closes_on_bad_packet() {
    let mut phone=client();let secret=[0xa7;32];
    let value=call(&mut phone,json!({"op":"resume","clientId":CLIENT,"secret":STANDARD.encode(secret)})).unwrap();
    let hello=serde_json::from_value(value["introduction"]["value"].clone()).unwrap();
    let (desktop,reply)=session::AwaitingDeviceProof::accept(hello,CLIENT,DEVICE,&secret,SESSION.into()).unwrap();
    let value=call(&mut phone,json!({"op":"resumeReply","reply":reply})).unwrap();
    let mut desktop=desktop.verify(serde_json::from_value(value["confirmation"].clone()).unwrap()).unwrap();
    let data=json!({"deviceId":DEVICE,"name":"显示终端📱","platform":"ios","appVersion":"1.0"});
    let value=call(&mut phone,json!({"op":"seal","message":data})).unwrap();let packet=serde_json::from_value(value["packet"].clone()).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&desktop.open(&packet).unwrap()).unwrap(),data);
    let packet=desktop.seal(br#"{"type":"snapshot","snapshot":{"version":1}}"#).unwrap();
    assert_eq!(call(&mut phone,json!({"op":"open","packet":packet})).unwrap()["message"]["type"],"snapshot");
    assert!(call(&mut phone,json!({"op":"open","packet":packet})).is_err());
    assert!(call(&mut phone,json!({"op":"seal","message":{"type":"pause"}})).is_err());
}

/// 桥只在完整资源校验后返回complete，乱序/篡改使部分下载失效。
#[test]
fn asset_bridge_only_completes_after_shared_full_content_validation() {
    let mut phone=client();let mut bytes=b"GIF89a".to_vec();bytes.resize(20_001,7);
    let descriptor=AssetDescriptor::from_bytes(&bytes).unwrap();
    call(&mut phone,json!({"op":"assetBegin","descriptor":descriptor})).unwrap();
    let first=descriptor.chunk(&bytes,0).unwrap();
    assert_eq!(call(&mut phone,json!({"op":"assetChunk","chunk":first})).unwrap()["complete"],false);
    assert!(call(&mut phone,json!({"op":"assetChunk","chunk":first})).is_err());
    assert!(call(&mut phone,json!({"op":"assetChunk","chunk":descriptor.chunk(&bytes,16384).unwrap()})).is_err());
    call(&mut phone,json!({"op":"assetBegin","descriptor":descriptor})).unwrap();let mut offset=0;
    loop {let chunk=descriptor.chunk(&bytes,offset).unwrap();let result=call(&mut phone,json!({"op":"assetChunk","chunk":chunk})).unwrap();offset=result["offset"].as_u64().unwrap();if result["complete"]==true {break;}}
    assert_eq!(offset,bytes.len() as u64);
}

/// C入口拒绝无效指针形状与不足输出缓冲，足够容量时才消费状态，旧handle永不复用。
#[test]
fn c_abi_handles_and_buffer_contract_are_fail_closed() {
    use crate::abi::*;
    let identity=serde_json::to_vec(&json!({"deviceId":DEVICE,"name":"终端📱"})).unwrap();
    // SAFETY: 所有切片与输出指针都来自活跃且不重叠的本地Vec/栈变量。
    unsafe {
        assert_eq!(loki_mobile_create(std::ptr::null(),0),0);
        let handle=loki_mobile_create(identity.as_ptr(),identity.len());assert_ne!(handle,0);
        let input=serde_json::to_vec(&json!({"op":"openPairing","clientId":CLIENT})).unwrap();let mut output=vec![0;crate::RESULT_CAPACITY];let mut length=0;
        assert_eq!(loki_mobile_call(handle,input.as_ptr(),input.len(),output.as_mut_ptr(),1,&mut length),-1);
        assert_eq!(loki_mobile_call(handle,input.as_ptr(),input.len(),output.as_mut_ptr(),output.len(),&mut length),0);
        let value:Value=serde_json::from_slice(&output[..length]).unwrap();assert_eq!(value["ok"],true);assert_eq!(value["result"]["code"].as_str().unwrap().len(),4);
        loki_mobile_destroy(handle);loki_mobile_destroy(handle);
        assert_eq!(loki_mobile_call(handle,input.as_ptr(),input.len(),output.as_mut_ptr(),output.len(),&mut length),1);
        assert_eq!(serde_json::from_slice::<Value>(&output[..length]).unwrap()["error"],"mobile_handle_closed");
        let next=loki_mobile_create(identity.as_ptr(),identity.len());assert!(next>handle);loki_mobile_destroy(next);
        let bytes=b"GIF89aisolated-cache";let descriptor=serde_json::to_vec(&AssetDescriptor::from_bytes(bytes).unwrap()).unwrap();
        assert_eq!(loki_mobile_asset_valid(descriptor.as_ptr(),descriptor.len(),bytes.as_ptr(),bytes.len()),1);
        assert_eq!(loki_mobile_asset_valid(descriptor.as_ptr(),descriptor.len(),bytes.as_ptr(),bytes.len()-1),0);
    }
}

/// 旧OS保存回调晚于重新开窗的新pending时，旧commitId不能确认或消费新材料。
#[test]
fn stale_private_storage_callback_cannot_acknowledge_new_pairing_material() {
    let mut phone=client();let mut commits=Vec::new();let mut senders=Vec::new();
    for _ in 0..2 {
        let opened=call(&mut phone,json!({"op":"openPairing","clientId":CLIENT})).unwrap();
        let (desktop,hello)=SenderSession::start(CLIENT,DEVICE,opened["code"].as_str().unwrap(),SESSION.into()).unwrap();
        let value=call(&mut phone,json!({"op":"pairHello","hello":hello})).unwrap();
        let (desktop,confirmation)=desktop.accept(serde_json::from_value(value["reply"].clone()).unwrap()).unwrap();
        commits.push(call(&mut phone,json!({"op":"pairConfirmation","confirmation":confirmation})).unwrap());senders.push(desktop);
    }
    assert!(call(&mut phone,json!({"op":"pairingStored","commitId":commits[0]["commitId"],"stored":true})).is_err());
    let result=call(&mut phone,json!({"op":"pairingStored","commitId":commits[1]["commitId"],"stored":true})).unwrap();
    let material=senders.pop().unwrap().verify(serde_json::from_value(result["ack"].clone()).unwrap()).unwrap();
    assert_eq!(STANDARD.encode(material.secret_bytes()),commits[1]["material"]["secret"]);
    assert_ne!(commits[0]["material"]["secret"],commits[1]["material"]["secret"]);
}
