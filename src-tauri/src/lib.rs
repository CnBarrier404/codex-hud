mod usage;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, PhysicalPosition, PhysicalSize, WindowEvent,
};

struct Appearance {
    mica: bool,
}

#[tauri::command]
fn mica_enabled(appearance: tauri::State<'_, Appearance>) -> bool {
    appearance.mica
}

fn show_panel(app: &tauri::AppHandle, anchor: Option<PhysicalPosition<f64>>) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let anchor = anchor.or_else(|| {
        app.tray_by_id("main")
            .and_then(|tray| tray.rect().ok().flatten())
            .map(|rect| rect.position.to_physical::<f64>(1.0))
    });
    let monitor = match anchor {
        Some(point) => app.monitor_from_point(point.x, point.y)?,
        None => window.primary_monitor()?,
    };
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let gap = (8.4 * scale).round() as i32;
        let width = (319.2 * scale).round() as u32;
        let height = (302.4 * scale).round() as u32;
        let left = area.position.x + gap;
        let top = area.position.y + gap;
        let right = (area.position.x + area.size.width as i32 - width as i32 - gap).max(left);
        let bottom = (area.position.y + area.size.height as i32 - height as i32 - gap).max(top);
        let point = anchor.unwrap_or(PhysicalPosition::new(
            (right + width as i32) as f64,
            (bottom + height as i32) as f64,
        ));
        let x = (point.x as i32 - width as i32 / 2).clamp(left, right);
        let y = if point.y < (area.position.y + area.size.height as i32 / 2) as f64 {
            (point.y as i32 + gap).clamp(top, bottom)
        } else {
            (point.y as i32 - height as i32 - gap).clamp(top, bottom)
        };
        window.set_position(PhysicalPosition::new(x, y))?;
        window.set_size(PhysicalSize::new(width, height))?;
    }
    window.show()?;
    window.set_focus()
}

fn report(result: tauri::Result<()>) {
    if let Err(error) = result {
        eprintln!("Tray window error: {error}");
    }
}

pub fn run() {
    tauri::Builder::default()
        .manage(usage::UsageState::default())
        .invoke_handler(tauri::generate_handler![mica_enabled, usage::read_usage])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or("Missing main window")?;
            #[cfg(target_os = "windows")]
            let mica = match window_vibrancy::apply_mica(&window, Some(false)) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("Mica unavailable; using white background: {error}");
                    false
                }
            };
            #[cfg(not(target_os = "windows"))]
            let mica = false;
            if !mica {
                window.set_background_color(Some(tauri::window::Color(255, 255, 255, 255)))?;
            }
            app.manage(Appearance { mica });

            let show = MenuItem::with_id(app, "show", "Show Codex HUD", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("Missing tray icon")?;
            TrayIconBuilder::with_id("main")
                .icon(icon)
                .tooltip("Codex HUD")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => report(show_panel(app, None)),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        position,
                        ..
                    } = event
                    {
                        report(show_panel(tray.app_handle(), Some(position)));
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => report(window.hide()),
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                report(window.hide());
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running Codex HUD");
}
