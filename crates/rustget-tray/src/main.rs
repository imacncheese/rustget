//! Optional native tray shell. It does not own the daemon or any download
//! state; it opens the local UI and exits.
//!
//! Windows is the verified backend. Other platforms currently build a stub so
//! Linux CI does not need GTK/AppIndicator system libraries.
#![forbid(unsafe_code)]

#[cfg(windows)]
mod windows {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::WindowId;

    struct TrayApp {
        tray: Option<TrayIcon>,
        open_item: MenuItem,
        quit_item: MenuItem,
        ui_url: String,
    }

    impl ApplicationHandler for TrayApp {
        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
            if self.tray.is_some() {
                return;
            }
            let menu = Menu::new();
            let _ = menu.append(&self.open_item);
            let _ = menu.append(&self.quit_item);
            let icon = match Icon::from_rgba(icon_rgba(), 32, 32) {
                Ok(icon) => icon,
                Err(error) => {
                    eprintln!("rustget-tray: cannot build icon: {error}");
                    return;
                }
            };
            match TrayIconBuilder::new()
                .with_tooltip("RustGet")
                .with_menu(Box::new(menu))
                .with_icon(icon)
                .build()
            {
                Ok(tray) => self.tray = Some(tray),
                Err(error) => eprintln!("rustget-tray: cannot create tray: {error}"),
            }
        }

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            _event: WindowEvent,
        ) {
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == self.open_item.id() {
                    if let Err(error) = open::that_detached(&self.ui_url) {
                        eprintln!("rustget-tray: cannot open UI: {error}");
                    }
                } else if event.id == self.quit_item.id() {
                    event_loop.exit();
                }
            }
        }
    }

    fn icon_rgba() -> Vec<u8> {
        let mut pixels = Vec::with_capacity(32 * 32 * 4);
        for y in 0..32 {
            for x in 0..32 {
                let border = x < 2 || y < 2 || x >= 30 || y >= 30;
                let letter = (8..24).contains(&x) && (7..25).contains(&y) && (x + y) % 9 < 4;
                if border || letter {
                    pixels.extend_from_slice(&[255, 122, 69, 255]);
                } else {
                    pixels.extend_from_slice(&[20, 18, 16, 255]);
                }
            }
        }
        pixels
    }

    pub fn run() {
        let ui_url =
            std::env::var("RUSTGET_UI_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".to_string());
        let event_loop = match EventLoop::new() {
            Ok(event_loop) => event_loop,
            Err(error) => {
                eprintln!("rustget-tray: cannot create event loop: {error}");
                std::process::exit(1);
            }
        };
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = TrayApp {
            tray: None,
            open_item: MenuItem::new("Open RustGet", true, None),
            quit_item: MenuItem::new("Quit", true, None),
            ui_url,
        };
        if let Err(error) = event_loop.run_app(&mut app) {
            eprintln!("rustget-tray: event loop failed: {error}");
            std::process::exit(1);
        }
    }
}

fn main() {
    #[cfg(windows)]
    windows::run();
    #[cfg(not(windows))]
    eprintln!("rustget-tray: the native tray backend is currently Windows-only");
}
