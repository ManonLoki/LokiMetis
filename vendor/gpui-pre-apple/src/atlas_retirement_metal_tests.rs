// 用真正阻塞的 Metal 队列检查提交后删除；测试失败也必定解除 GPU 等待。

use super::*;
use gpui::{AtlasTile, Corners, PlatformAtlas, PolychromeSprite, RenderImage, RenderImageParams};
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

/// CPU 事件控制独立于渲染完成回调，panic 也不会把宿主 GPU 队列永久阻塞。
struct Gate(metal::SharedEvent);
impl Drop for Gate {
    /// 释放测试等待，command buffer 仍按正常 Metal 规则执行。
    fn drop(&mut self) {
        self.0.set_signaled_value(1);
    }
}

/// 实际整页 BGRA 图像让最后 key 的物理退役具有可观察的纹理槽。
fn upload(renderer: &MetalRenderer, image: &RenderImage) -> AtlasTile {
    renderer
        .sprite_atlas
        .get_or_insert_with(
            RenderImageParams {
                image_id: image.id,
                frame_index: 0,
            }
            .into(),
            &mut || {
                Ok(Some((
                    image.size(0),
                    Cow::Borrowed(image.as_bytes(0).unwrap()),
                )))
            },
        )
        .unwrap()
        .unwrap()
}

/// 真正的 Scene 消费刚上传的 tile，不用替代 renderer 或假完成计数。
fn image_scene(tile: AtlasTile) -> Scene {
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(32.), ScaledPixels(32.)),
    );
    let mut scene = Scene::default();
    scene.insert_primitive(PolychromeSprite {
        #[cfg(feature = "parent-rounded-clip")]
        parent_clip: Default::default(),
        order: 0,
        pad: 0,
        grayscale: false.into(),
        opacity: 1.,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        tile,
        rounded_clip_bounds: bounds,
    });
    scene.finish();
    scene
}

/// 逻辑 key 可立即失效，物理 texture 必须等实际提交的采样 command buffer 完成。
#[test]
fn submitted_atlas_texture_survives_until_gpu_completion() {
    objc2::rc::autoreleasepool(|_| {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let data = RenderImage::new(vec![image::Frame::new(image::RgbaImage::from_pixel(
            1024,
            1024,
            image::Rgba([0, 0, 255, 255]),
        ))]);
        let tile = upload(&renderer, &data);
        let gate = Gate(renderer.device.new_shared_event());
        let blocker = renderer.command_queue.new_command_buffer();
        blocker.encode_wait_for_event(&gate.0, 1);
        blocker.commit();
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(32);
        descriptor.set_height(32);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        descriptor.set_storage_mode(if renderer.is_apple_gpu {
            metal::MTLStorageMode::Shared
        } else {
            metal::MTLStorageMode::Managed
        });
        let target = renderer.device.new_texture(&descriptor);
        let command = renderer
            .render_frame(
                &image_scene(tile),
                &target,
                size(DevicePixels(32), DevicePixels(32)),
            )
            .unwrap();
        if !renderer.is_apple_gpu {
            let blit = command.new_blit_command_encoder();
            blit.synchronize_resource(&target);
            blit.end_encoding();
        }
        command.commit();
        let key = RenderImageParams {
            image_id: data.id,
            frame_index: 0,
        }
        .into();
        renderer.sprite_atlas.remove(&key);
        assert!(
            renderer
                .sprite_atlas
                .get_or_insert_with(key, &mut || Ok(None))
                .unwrap()
                .is_none()
        );
        assert!(
            renderer
                .sprite_atlas
                .metal_texture(tile.texture_id)
                .is_some(),
            "提交的GPU仍在等待，不能提前回收物理纹理槽"
        );
        gate.0.set_signaled_value(1);
        command.wait_until_completed();
        let deadline = Instant::now() + Duration::from_secs(2);
        while renderer
            .sprite_atlas
            .metal_texture(tile.texture_id)
            .is_some()
            && Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert!(
            renderer
                .sprite_atlas
                .metal_texture(tile.texture_id)
                .is_none(),
            "完成回调后应归还最后物理分配"
        );
        assert_eq!(
            read_texture_to_image(&target).unwrap().get_pixel(16, 16).0,
            [255, 0, 0, 255]
        );
    });
}

/// 保留另一个小 tile，避免整页释放掩盖 allocator 区域被过早复用。
#[test]
fn prepared_atlas_region_is_not_reused_before_later_submission() {
    objc2::rc::autoreleasepool(|_| {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let keeper = solid_image(64, [0, 255, 0, 255]);
        let keeper_tile = upload(&renderer, &keeper);
        let original = solid_image(700, [0, 0, 255, 255]);
        let original_tile = upload(&renderer, &original);
        assert_eq!(
            keeper_tile.texture_id, original_tile.texture_id,
            "必须实际共享图集页"
        );
        let gate = Gate(renderer.device.new_shared_event());
        let blocker = renderer.command_queue.new_command_buffer();
        blocker.encode_wait_for_event(&gate.0, 1);
        blocker.commit();
        let target = render_target(&renderer);
        let prepared = renderer
            .render_frame(
                &image_scene(original_tile),
                &target,
                size(DevicePixels(32), DevicePixels(32)),
            )
            .unwrap();
        if !renderer.is_apple_gpu {
            let blit = prepared.new_blit_command_encoder();
            blit.synchronize_resource(&target);
            blit.end_encoding();
        }
        let key = RenderImageParams {
            image_id: original.id,
            frame_index: 0,
        }
        .into();
        renderer.sprite_atlas.remove(&key);
        let replacement = solid_image(700, [255, 0, 0, 255]);
        // 同一个逻辑 key 再上传蓝色，不能占用尚未提交的红色采样区域。
        let replacement_tile = renderer
            .sprite_atlas
            .get_or_insert_with(key.clone(), &mut || {
                Ok(Some((
                    replacement.size(0),
                    Cow::Borrowed(replacement.as_bytes(0).unwrap()),
                )))
            })
            .unwrap()
            .unwrap();
        assert!(
            replacement_tile.texture_id != original_tile.texture_id
                || replacement_tile.bounds != original_tile.bounds
        );
        prepared.commit();
        gate.0.set_signaled_value(1);
        prepared.wait_until_completed();
        assert_eq!(
            read_texture_to_image(&target).unwrap().get_pixel(16, 16).0,
            [255, 0, 0, 255]
        );
        let current = renderer
            .sprite_atlas
            .get_or_insert_with(key, &mut || Ok(None))
            .unwrap()
            .unwrap();
        assert_eq!(
            current, replacement_tile,
            "旧 retirement 不得删除同 key 的新分配"
        );
        assert!(
            renderer
                .sprite_atlas
                .metal_texture(keeper_tile.texture_id)
                .is_some()
        );
    });
}

/// 从未提交的编码丢弃后不能永久占住物理分配。
#[test]
fn abandoned_encoded_frame_cancels_atlas_retirement() {
    objc2::rc::autoreleasepool(|_| {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let image = solid_image(1024, [0, 0, 255, 255]);
        let tile = upload(&renderer, &image);
        let target = render_target(&renderer);
        let prepared = renderer
            .render_frame(
                &image_scene(tile),
                &target,
                size(DevicePixels(32), DevicePixels(32)),
            )
            .unwrap();
        renderer.sprite_atlas.remove(
            &RenderImageParams {
                image_id: image.id,
                frame_index: 0,
            }
            .into(),
        );
        assert!(
            renderer
                .sprite_atlas
                .metal_texture(tile.texture_id)
                .is_some()
        );
        drop(prepared);
        assert!(
            renderer
                .sprite_atlas
                .metal_texture(tile.texture_id)
                .is_none()
        );
    });
}

/// 窗口 renderer 退出后，由提交完成回调独立收尾，不保持 UI 实体。
#[test]
fn submitted_frame_keeps_atlas_alive_after_renderer_drop() {
    objc2::rc::autoreleasepool(|_| {
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let image = solid_image(1024, [0, 0, 255, 255]);
        let tile = upload(&renderer, &image);
        let weak = Arc::downgrade(&renderer.sprite_atlas);
        let gate = Gate(renderer.device.new_shared_event());
        let blocker = renderer.command_queue.new_command_buffer();
        blocker.encode_wait_for_event(&gate.0, 1);
        blocker.commit();
        let target = render_target(&renderer);
        let command = renderer
            .render_frame(
                &image_scene(tile),
                &target,
                size(DevicePixels(32), DevicePixels(32)),
            )
            .unwrap();
        command.commit();
        renderer.sprite_atlas.remove(
            &RenderImageParams {
                image_id: image.id,
                frame_index: 0,
            }
            .into(),
        );
        drop(renderer);
        assert!(weak.upgrade().is_some());
        gate.0.set_signaled_value(1);
        command.wait_until_completed();
        let deadline = Instant::now() + Duration::from_secs(2);
        while weak.upgrade().is_some() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            weak.upgrade().is_none(),
            "完成后不得保留已关闭 renderer 的图集"
        );
    });
}

/// 输入保持与生产 RenderImage 相同的 BGRA 布局。
fn solid_image(side: u32, bgra: [u8; 4]) -> RenderImage {
    RenderImage::new(vec![image::Frame::new(image::RgbaImage::from_pixel(
        side,
        side,
        image::Rgba(bgra),
    ))])
}

/// Intel 使用显式同步，Apple GPU 使用共享存储。
fn render_target(renderer: &MetalRenderer) -> metal::Texture {
    let descriptor = metal::TextureDescriptor::new();
    descriptor.set_width(32);
    descriptor.set_height(32);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    descriptor.set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
    descriptor.set_storage_mode(if renderer.is_apple_gpu {
        metal::MTLStorageMode::Shared
    } else {
        metal::MTLStorageMode::Managed
    });
    renderer.device.new_texture(&descriptor)
}
