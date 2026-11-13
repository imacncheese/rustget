//! Native RustGet window. The daemon and API run in-process; the window loads
//! the loopback UI directly, so no external browser is involved.
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod windows {
    use std::net::SocketAddr;
    use std::sync::mpsc;

    use anyhow::Context;
    use rustget_core::EngineConfig;
    use rustget_ipc::{ApiConfig, load_or_create_token, router};
    use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent};
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};
    use wry::WebViewBuilder;

    pub fn run() -> anyhow::Result<()> {
        let data_dir = std::env::var_os("RUSTGET_DATA_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("APPDATA")
                    .map(|root| std::path::PathBuf::from(root).join("RustGet"))
            })
            .context("cannot resolve APPDATA for the RustGet data directory")?;
        let download_dir = std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .map(|home| home.join("Downloads").join("RustGet"))
            .unwrap_or_else(|| data_dir.join("Downloads"));
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&download_dir)?;
        let _instance = rustget_platform::InstanceLock::acquire(data_dir.join("desktop.lock"))?;

        let manager_config = ManagerConfig {
            data_dir: data_dir.clone(),
            download_dir,
            engine: EngineConfig::default(),
            max_concurrent: 4,
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("cannot create runtime: {error}")));
                    return;
                }
            };
            runtime.block_on(async move {
                let manager = match DownloadManager::new(manager_config) {
                    Ok(manager) => manager,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let token = match load_or_create_token(&data_dir) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
                    Ok(listener) => listener,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let mut events = manager.subscribe();
                let notification_manager = std::sync::Arc::clone(&manager);
                tokio::spawn(async move {
                    while let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                        if status != DownloadStatus::Completed {
                            continue;
                        }
                        let name = notification_manager
                            .get(id)
                            .map(|record| record.filename)
                            .unwrap_or_else(|_| "download".into());
                        let _ = notify_rust::Notification::new()
                            .summary("RustGet")
                            .body(&format!("{name} downloaded"))
                            .show();
                    }
                });
                let mut config = ApiConfig::loopback(token.clone());
                config.logs = None;
                let app = router(manager, &config);
                let _ = ready_tx.send(Ok((addr, token)));
                let _ = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await;
            });
        });

        let (addr, token) = ready_rx
            .recv()
            .context("desktop server thread stopped before startup")?
            .map_err(anyhow::Error::msg)?;
        let url = format!("http://{addr}/#token={token}");
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .with_title("RustGet")
            .with_inner_size(tao::dpi::LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(tao::dpi::LogicalSize::new(900.0, 600.0))
            .build(&event_loop)?;
        let _webview = WebViewBuilder::new()
            .with_url(url)
            .with_devtools(cfg!(debug_assertions))
            .build(&window)?;
        let menu = Menu::new();
        let open_item = MenuItem::new("Open RustGet", true, None);
        let quit_item = MenuItem::new("Quit", true, None);
        let _ = menu.append(&open_item);
        let _ = menu.append(&quit_item);
        let _tray = TrayIconBuilder::new()
            .with_tooltip("RustGet")
            .with_menu(Box::new(menu))
            .with_icon(Icon::from_rgba(crate::tray_icon_rgba(), 16, 16)?)
            .build()?;
        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            if let Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } = event
            {
                window.set_visible(false);
            }
            while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
                if menu_event.id == open_item.id() {
                    window.set_visible(true);
                    window.set_focus();
                } else if menu_event.id == quit_item.id() {
                    *control_flow = ControlFlow::Exit;
                }
            }
        });
    }
}

#[cfg(windows)]
fn tray_icon_rgba() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let border = x < 1 || y < 1 || x >= 15 || y >= 15;
            let letter = (4..12).contains(&x) && (3..13).contains(&y) && (x + y) % 5 < 3;
            if border || letter {
                pixels.extend_from_slice(&[255, 122, 69, 255]);
            } else {
                pixels.extend_from_slice(&[20, 18, 16, 255]);
            }
        }
    }
    pixels
}

fn main() {
    #[cfg(windows)]
    {
        if let Err(error) = windows::run() {
            eprintln!("rustget-desktop: {error}");
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    eprintln!("rustget-desktop: the native desktop shell is currently Windows-only");
}
