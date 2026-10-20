//! C ABI只复制调用方提供的有界缓冲；句柄是注册表ID，绝不解引用外部handle指针。

use std::{collections::BTreeMap, sync::{Mutex,OnceLock}, panic::{AssertUnwindSafe,catch_unwind}};
use crate::{MobileClient,MAX_COMMAND_BYTES,RESULT_CAPACITY};
use serde_json::json;

/// 全进程上限防止错误宿主持续创建不可回收密码状态。
const MAX_HANDLES:usize=64;
/// 递增句柄永不复用，释放后的旧句柄不能访问后来的会话。
#[derive(Default)]
struct Registry { next:u64, clients:BTreeMap<u64,MobileClient> }
static REGISTRY:OnceLock<Mutex<Registry>>=OnceLock::new();
/// poison恢复只用于有界状态注册表，不输出私有数据。
fn registry()->std::sync::MutexGuard<'static,Registry> {REGISTRY.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)}

/// 创建独占移动协议状态；返回0表示非法参数/容量/内部失败。
/// # Safety
/// identity须为有效可读的identity_len字节，调用期间不得修改；最大1024字节。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loki_mobile_create(identity:*const u8,identity_len:usize)->u64 {
    if identity.is_null()||identity_len==0||identity_len>1024 {return 0;}
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: 非空与有界已检查，可读范围由C调用合同保证。
        let bytes=unsafe {std::slice::from_raw_parts(identity,identity_len)};
        let Ok(client)=MobileClient::new(bytes) else {return 0;};
        let mut state=registry();if state.clients.len()>=MAX_HANDLES {return 0;}
        let Some(id)=state.next.checked_add(1) else {return 0;};state.next=id;state.clients.insert(id,client);id
    })).unwrap_or(0)
}

/// 固定输出缓冲足够才消费操作；返回0成功、1业务错误、-1 ABI参数/内部失败。
/// # Safety
/// input须可读input_len字节；output须可写output_capacity字节且不与input/length重叠；
/// length须为有效对齐的可写size_t指针。缓冲在调用期间均由调用方独占。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loki_mobile_call(handle:u64,input:*const u8,input_len:usize,output:*mut u8,output_capacity:usize,length:*mut usize)->i32 {
    if input.is_null()||output.is_null()||length.is_null()||input_len==0||input_len>MAX_COMMAND_BYTES||output_capacity<RESULT_CAPACITY {return -1;}
    // SAFETY: length的对齐/有效性由C合同保证；无容量不足的隐式重试。
    unsafe {length.write(0);}
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: 非空/上限已经检查；可读性与不变性由C合同保证。
        let bytes=unsafe {std::slice::from_raw_parts(input,input_len)};
        let mut state=registry();
        let result=state.clients.get_mut(&handle).ok_or("mobile_handle_closed").and_then(|client|client.call(bytes));
        let (status,value)=match result {Ok(result)=>(0,json!({"ok":true,"result":result})),Err(code)=>(1,json!({"ok":false,"error":code}))};
        let Ok(encoded)=serde_json::to_vec(&value) else {return -1;};
        if encoded.len()>output_capacity {state.clients.remove(&handle);return -1;}
        // SAFETY: 容量已检查；合同要求两个缓冲不重叠且output/length有效。
        unsafe {std::ptr::copy_nonoverlapping(encoded.as_ptr(),output,encoded.len());length.write(encoded.len());}
        status
    })).unwrap_or_else(|_|{registry().clients.remove(&handle);-1})
}

/// 释放原生owner的全部密码及未完成资源状态；未知/重复释放幂等，无外部指针。
#[unsafe(no_mangle)]
pub extern "C" fn loki_mobile_destroy(handle:u64) {let _=catch_unwind(AssertUnwindSafe(||{registry().clients.remove(&handle);}));}

/// 持久缓存复用前对真实正文核对长度、摘要、魔数；返回1匹配、0无效、-1参数失败。
/// # Safety
/// descriptor与bytes分别须为对应长度的可读独占稳定范围；不把路径交Rust桥。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loki_mobile_asset_valid(descriptor:*const u8,descriptor_len:usize,bytes:*const u8,bytes_len:usize)->i32 {
    if descriptor.is_null()||bytes.is_null()||descriptor_len==0||descriptor_len>512||bytes_len>loki_metris_pairing::assets::MAX_ASSET_BYTES {return -1;}
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: 非空与长度上限已检查；真实可读范围由C调用合同保证。
        let raw=unsafe {std::slice::from_raw_parts(descriptor,descriptor_len)};
        // SAFETY: 正文地址与长度由同一C合同保证，函数只读不保存指针。
        let body=unsafe {std::slice::from_raw_parts(bytes,bytes_len)};
        let Ok(descriptor)=serde_json::from_slice::<loki_metris_pairing::assets::AssetDescriptor>(raw) else {return 0;};
        i32::from(descriptor.matches(body))
    })).unwrap_or(-1)
}
