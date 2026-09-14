//! System tray presence.
//!
//! The window is frameless and can be hidden, so the tray icon is what tells the
//! user the app is still running — and it carries the actions that cannot be
//! reached once the window is out of the way.

use tauri::menu::{MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::app::{AppState, Status};
use crate::service::ServiceAction;

const TRAY_ID: &str = "main";

/// Build the tray icon and its menu.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
    let start = MenuItemBuilder::with_id("start", "启动服务").build(app)?;
    let stop = MenuItemBuilder::with_id("stop", "停止服务").build(app)?;
    let restart = MenuItemBuilder::with_id("restart", "重启服务").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&show, &start, &stop, &restart])
        .separator()
        .items(&[&quit])
        .build()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("EasyTier Manager")
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event);

    {
        // A left click should bring the window back rather than open the menu.
        let app = app.clone();
        builder = builder.on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(&app);
            }
        });
    }

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

/// Mirror the service state into the tray tooltip, so hovering the icon answers
/// "is it still running, and is the tunnel up?".
pub fn update_tooltip(app: &AppHandle, status: &Status) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let state = if status.running {
        "运行中"
    } else if status.installed {
        "已停止"
    } else {
        "未安装"
    };
    let version = if status.version.is_empty() {
        String::new()
    } else {
        format!(" v{}", status.version)
    };

    let _ = tray.set_tooltip(Some(format!("EasyTier Manager · {state}{version}")));
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "show" => show_window(app),
        // The window's close button only hides it, so this is the real exit.
        "quit" => app.exit(0),
        "start" | "stop" | "restart" => {
            let action = match event.id().as_ref() {
                "start" => ServiceAction::Start,
                "stop" => ServiceAction::Stop,
                _ => ServiceAction::Restart,
            };

            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                if let Err(err) = crate::platform::apply_service(state.inner(), action).await {
                    eprintln!("tray service action failed: {err}");
                }
            });
        }
        _ => {}
    }
}
