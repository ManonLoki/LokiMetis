use super::*;

/// 验证查询收到原 HWND/uID、正确结构大小和完整 GUID，而非改写托盘身份。
fn assert_identifier(identifier: &NOTIFYICONIDENTIFIER, guid: Option<GUID>) {
    assert_eq!(identifier.hWnd, 0x1234usize as HWND);
    assert_eq!(identifier.uID, 42);
    assert_eq!(identifier.cbSize as usize, size_of::<NOTIFYICONIDENTIFIER>());
    // 安全性：GUID 的全部整数域都允许零，用于匹配无 GUID 的原始合同。
    let expected = guid.unwrap_or_else(|| unsafe { std::mem::zeroed() });
    assert_eq!(identifier.guidItem.data1, expected.data1);
    assert_eq!(identifier.guidItem.data2, expected.data2);
    assert_eq!(identifier.guidItem.data3, expected.data3);
    assert_eq!(identifier.guidItem.data4, expected.data4);
}

/// 注入实际观察到的 S_FALSE 及其它成功码，原查询结果必须可用于恢复入口和鼠标事件。
#[test]
fn successful_hresult_preserves_rect_and_identity() {
    let guid = GUID { data1: 0x13579bdf, data2: 0x2468, data3: 0xace0, data4: [1, 2, 3, 4, 5, 6, 7, 8] };
    for identity in [None, Some(guid)] {
        for hresult in [0, 1, i32::MAX] {
            let result = get_tray_rect_with_query(42, identity, 0x1234usize as HWND, |identifier, rect| {
                assert_identifier(identifier, identity);
                *rect = RECT { left: -160, top: 20, right: -130, bottom: 50 };
                hresult
            }).unwrap_or_else(|| panic!("successful HRESULT {hresult} lost the tray rectangle"));
            assert_eq!((result.left, result.top, result.right, result.bottom), (-160, 20, -130, 50));
        }
    }
}

/// 错误码即使写入矩形也不能被接受，防止失败查询制造不存在的恢复入口。
#[test]
fn failed_hresult_rejects_written_rectangle() {
    for hresult in [-1, i32::MIN, 0x80004005u32 as i32] {
        let result = get_tray_rect_with_query(42, None, 0x1234usize as HWND, |identifier, rect| {
            assert_identifier(identifier, None);
            *rect = RECT { left: 10, top: 20, right: 30, bottom: 40 };
            hresult
        });
        assert!(result.is_none(), "failed HRESULT {hresult} exposed a tray rectangle");
    }
}
