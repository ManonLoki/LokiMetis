// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#![allow(clippy::uninlined_format_args)]

//! tray-icon lets you create tray icons for desktop applications.
//!
//! # Platforms supported:
//!
//! - Windows
//! - macOS
//! - Linux and BSD (AppIndicator or KSNI)
//!
//! # Platform-specific notes:
//!
//! - On Windows and the Linux/BSD AppIndicator backend, an event loop must be running on the thread. The
//!   KSNI backend runs its D-Bus service on a worker thread and does not require a GTK event loop.
//! - When both the `libappindicator` and `ksni` features are enabled on Linux or BSD, tray-icon
//!   uses the KSNI backend and emits a Cargo warning.
//! - On macOS, an event loop must be running on the main thread so you also need to create the tray icon on the main thread. You must make sure that the event loop is already running and not just created before creating a TrayIcon to prevent issues with fullscreen apps. In Winit for example the earliest you can create icons is on [`StartCause::Init`](https://docs.rs/winit/latest/winit/event/enum.StartCause.html#variant.Init).
//!
//! # Dependencies (Linux/BSD)
//!
//! The default Linux backend uses GTK and `libappindicator` or
//! `libayatana-appindicator`. The `ksni` backend does not require these system libraries unless a
//!  GTK backend is also enabled.
//!
//! #### Arch Linux / Manjaro:
//!
//! ```sh
//! pacman -S gtk3 libappindicator-gtk3 #or libayatana-appindicator
//! ```
//!
//! #### Debian / Ubuntu:
//!
//! ```sh
//! sudo apt install libgtk-3-dev libappindicator3-dev #or libayatana-appindicator3-dev
//! ```
//!
//! # Examples
//!
//! #### Create a tray icon without a menu.
//!
//! ```no_run
//! use tray_icon::{TrayIconBuilder, Icon};
//!
//! # let icon = Icon::from_rgba(Vec::new(), 0, 0).unwrap();
//! let tray_icon = TrayIconBuilder::new()
//!     .with_tooltip("system-tray - tray icon library!")
//!     .with_icon(icon)
//!     .build()
//!     .unwrap();
//! ```
//!
//! #### Create a tray icon with a menu.
//!
//! ```no_run
//! use tray_icon::{TrayIconBuilder, menu::Menu,Icon};
//!
//! # let icon = Icon::from_rgba(Vec::new(), 0, 0).unwrap();
//! let tray_menu = Menu::new();
//! let tray_icon = TrayIconBuilder::new()
//!     .with_menu(Box::new(tray_menu))
//!     .with_tooltip("system-tray - tray icon library!")
//!     .with_icon(icon)
//!     .build()
//!     .unwrap();
//! ```
//!
//! # Processing tray events
//!
//! You can use [`TrayIconEvent::receiver`] to get a reference to the [`TrayIconEventReceiver`]
//! which you can use to listen to events when a click happens on the tray icon
//! ```no_run
//! use tray_icon::TrayIconEvent;
//!
//! if let Ok(event) = TrayIconEvent::receiver().try_recv() {
//!     println!("{:?}", event);
//! }
//! ```
//!
//! You can also listen for the menu events using [`MenuEvent::receiver`](crate::menu::MenuEvent::receiver) to get events for the tray context menu.
//!
//! ```no_run
//! use tray_icon::{TrayIconEvent, menu::MenuEvent};
//!
//! if let Ok(event) = TrayIconEvent::receiver().try_recv() {
//!     println!("tray event: {:?}", event);
//! }
//!
//! if let Ok(event) = MenuEvent::receiver().try_recv() {
//!     println!("menu event: {:?}", event);
//! }
//! ```
//!
//! ### Note for [winit] or [tao] users:
//!
//! You should use [`TrayIconEvent::set_event_handler`] and forward
//! the tray icon events to the event loop by using [`EventLoopProxy`]
//! so that the event loop is awakened on each tray icon event.
//! Same can be done for menu events using [`crate::menu::MenuEvent::set_event_handler`].
//!
//! ```no_run
//! # use winit::event_loop::EventLoop;
//! enum UserEvent {
//!   TrayIconEvent(tray_icon::TrayIconEvent),
//!   MenuEvent(tray_icon::menu::MenuEvent)
//! }
//!
//! let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
//!
//! let proxy = event_loop.create_proxy();
//! tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
//!     proxy.send_event(UserEvent::TrayIconEvent(event));
//! }));
//!
//! let proxy = event_loop.create_proxy();
//! tray_icon::menu::MenuEvent::set_event_handler(Some(move |event| {
//!     proxy.send_event(UserEvent::MenuEvent(event));
//! }));
//! ```
//!
//! [`EventLoopProxy`]: https://docs.rs/winit/latest/winit/event_loop/struct.EventLoopProxy.html
//! [winit]: https://docs.rs/winit
//! [tao]: https://docs.rs/tao

// 原始根属性与文档保留于此；模块和资源来自完整、经摘要验证的生成树。
include!(concat!(env!("OUT_DIR"), "/gpui-patched/src/lib.rs"));
