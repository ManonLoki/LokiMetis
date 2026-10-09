//! 原生图片以 App 为缓存边界；位置拥有需求，最后一个位置才退役 CPU 与所有窗口图集。

use super::image_decode::{DecodedMonitorImage, MonitorImageDecoder, MonitorImageSource};
use gpui_kit::{App, Context, EntityId, Global, Subscription, Window, WindowId};
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    rc::{Rc, Weak as RcWeak},
    sync::{Arc, Weak},
};

/// 播放器、桌宠聚合与图库 entry 属于不同位置；entry 身份对应真实 Rc allocation。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Placement {
    Playback,
    Overlay,
    Catalog(usize),
}

/// Entity 和窗口均参与位置身份，关闭旧窗或迟到旧 entry 不能退役新位置。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct OwnerKey {
    window: WindowId,
    entity: EntityId,
    placement: Placement,
}

/// 弱 allocation anchor 防止旧 owner 仍登记期间 Rc 地址被新 entry 复用，不强持像素或 entry。
#[derive(Clone)]
pub(crate) struct ImageOwner {
    key: OwnerKey,
    allocation: Option<RcWeak<dyn Any>>,
}
impl ImageOwner {
    /// 每个实际播放器实体在它所属的原生窗口持有独立需求。
    pub(crate) fn playback<T: 'static>(window: &Window, cx: &mut Context<T>) -> Self {
        Self::new(Placement::Playback, None, window, cx)
    }
    /// 桌宠先登记整页素材，再停止旧位置，跨页同 ID 不出现中间零 owner。
    pub(crate) fn overlay<T: 'static>(window: &Window, cx: &mut Context<T>) -> Self {
        Self::new(Placement::Overlay, None, window, cx)
    }
    /// 图库使用每 entry 真实 Rc 身份，弱 anchor 保留 allocation 而不保留其值。
    pub(crate) fn catalog<T: 'static, V: 'static>(
        entry: &Rc<T>,
        window: &Window,
        cx: &mut Context<V>,
    ) -> Self {
        let identity = Rc::as_ptr(entry) as usize;
        let erased: Rc<dyn Any> = entry.clone();
        Self::new(
            Placement::Catalog(identity),
            Some(Rc::downgrade(&erased)),
            window,
            cx,
        )
    }
    /// 每 Entity 仅登记一次纯 release 回调，关闭窗口后也不会依赖 window.update 成功。
    fn new<T: 'static>(
        placement: Placement,
        allocation: Option<RcWeak<dyn Any>>,
        window: &Window,
        cx: &mut Context<T>,
    ) -> Self {
        ensure_registry(cx);
        let entity = cx.entity_id();
        if !cx.global::<Residencies>().entities.contains_key(&entity) {
            let release = cx.on_release(move |_, cx| {
                retire(|_, owner| owner.entity == entity, None, cx);
                if cx.has_global::<Residencies>() {
                    cx.global_mut::<Residencies>().entities.remove(&entity);
                }
            });
            cx.global_mut::<Residencies>()
                .entities
                .insert(entity, release);
        }
        Self {
            key: OwnerKey {
                window: window.window_handle().window_id(),
                entity,
                placement,
            },
            allocation,
        }
    }
}

/// 原始字节和解码结果都不复制；Weak 只供最终退役时取得已绘制的 GPU 句柄。
struct Residency {
    source: MonitorImageSource,
    owners: HashMap<OwnerKey, Option<RcWeak<dyn Any>>>,
    decoded: Weak<DecodedMonitorImage>,
    failed: bool,
}

/// App 内唯一登记器覆盖 pending 与 ready；订阅本身不持有任何 Entity 强引用。
struct Residencies {
    images: HashMap<String, Residency>,
    entities: HashMap<EntityId, Subscription>,
    _closed: Subscription,
}
impl Global for Residencies {}

/// 应用首次出现图片位置时绑定一次关闭观察，外部强持的已关闭 view 也失去窗口 owner。
fn ensure_registry(cx: &mut App) {
    if !cx.has_global::<Residencies>() {
        let closed =
            cx.on_window_closed(|cx, window| retire(|_, owner| owner.window == window, None, cx));
        cx.set_global(Residencies {
            images: HashMap::new(),
            entities: HashMap::new(),
            _closed: closed,
        });
    }
}

/// 必须在 use_asset 前登记真实可见需求，因此等待解码的窗口同样受共享保护。
pub(crate) fn acquire(source: &MonitorImageSource, owner: &ImageOwner, cx: &mut App) {
    ensure_registry(cx);
    let entry = cx
        .global_mut::<Residencies>()
        .images
        .entry(source.id.clone())
        .or_insert_with(|| Residency {
            source: source.clone(),
            owners: HashMap::new(),
            decoded: Weak::new(),
            failed: false,
        });
    entry.owners.insert(owner.key, owner.allocation.clone());
}

/// 只记录实际 Asset 回执；失败状态用于显式重试，成功弱引用不会增加像素驻留。
pub(crate) fn remember(
    source: &MonitorImageSource,
    result: &Result<Arc<DecodedMonitorImage>, &'static str>,
    cx: &mut App,
) {
    if let Some(entry) = cx.global_mut::<Residencies>().images.get_mut(&source.id) {
        match result {
            Ok(decoded) => {
                entry.decoded = Arc::downgrade(decoded);
                entry.failed = false;
            }
            Err(_) => entry.failed = true,
        }
    }
}

/// 单个位置释放幂等；退出 Global 借用后再操作 Asset 与原生图集。
pub(crate) fn release(id: &str, owner: &ImageOwner, window: Option<&mut Window>, cx: &mut App) {
    retire_source(id, |key| *key == owner.key, window, cx);
}

/// release_image 的 App 上下文用真实 entry 身份退役，不要求额外持有 Shell 强引用。
pub(crate) fn release_catalog<T>(id: &str, entry: &Rc<T>, window: &mut Window, cx: &mut App) {
    let identity = Rc::as_ptr(entry) as usize;
    let window_id = window.window_handle().window_id();
    retire_source(
        id,
        |key| key.window == window_id && key.placement == Placement::Catalog(identity),
        Some(window),
        cx,
    );
}

/// 用户刷新只取消已完成的失败 cache；保留健康 Ready、pending 与全部真实 owner 并通知重试。
pub(crate) fn retry_failed(source: &MonitorImageSource, cx: &mut App) {
    if !cx.has_global::<Residencies>() {
        return;
    }
    let owners = {
        let registry = cx.global_mut::<Residencies>();
        let Some(entry) = registry
            .images
            .get_mut(&source.id)
            .filter(|entry| entry.failed)
        else {
            return;
        };
        entry.failed = false;
        entry
            .owners
            .keys()
            .map(|owner| owner.entity)
            .collect::<HashSet<_>>()
    };
    cx.remove_asset::<MonitorImageDecoder>(source);
    for owner in owners {
        cx.notify(owner);
    }
}

/// 最后 owner 先取得弱 GPU 句柄再删 cache；App::drop_image 包含所有仍存活窗及当前借出的窗。
fn retire(
    mut matches: impl FnMut(&str, &OwnerKey) -> bool,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    if !cx.has_global::<Residencies>() {
        return;
    }
    let retired = {
        let registry = cx.global_mut::<Residencies>();
        for (id, entry) in &mut registry.images {
            entry.owners.retain(|owner, _| !matches(id, owner));
        }
        let empty = registry
            .images
            .iter()
            .filter(|(_, entry)| entry.owners.is_empty())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        empty
            .into_iter()
            .filter_map(|id| registry.images.remove(&id))
            .collect::<Vec<_>>()
    };
    cleanup(retired, window, cx);
}

/// 高频位置离屏只查本素材，不因保留的其它图库条目而反复扫描整个登记器。
fn retire_source(
    id: &str,
    mut matches: impl FnMut(&OwnerKey) -> bool,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    if !cx.has_global::<Residencies>() {
        return;
    }
    let retired = {
        let registry = cx.global_mut::<Residencies>();
        let Some(entry) = registry.images.get_mut(id) else {
            return;
        };
        entry.owners.retain(|owner, _| !matches(owner));
        if entry.owners.is_empty() {
            registry.images.remove(id)
        } else {
            None
        }
    };
    cleanup(retired, window, cx);
}

/// 全局状态已归还后清理真正退役的 cache 与图集，弱引用升级发生在删除 cache 之前。
fn cleanup(
    retired: impl IntoIterator<Item = Residency>,
    mut window: Option<&mut Window>,
    cx: &mut App,
) {
    for entry in retired {
        let decoded = entry.decoded.upgrade();
        cx.remove_asset::<MonitorImageDecoder>(&entry.source);
        if let Some(decoded) = decoded {
            cx.drop_image(decoded.image.clone(), window.as_deref_mut());
        }
    }
}
