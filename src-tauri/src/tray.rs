use std::collections::HashMap;

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::mount::MountStatus;
use crate::settings::RemoteSpec;

pub const TRAY_ID: &str = "ylrclone-tray";

fn state_mark(status: Option<&MountStatus>) -> &'static str {
    match status.map(|s| s.state.as_str()) {
        Some("mounted") => "●",
        Some("error") => "!",
        _ => "○",
    }
}

pub fn build_menu(
    app: &AppHandle,
    remotes: &[RemoteSpec],
    statuses: &HashMap<String, MountStatus>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let mut items: Vec<MenuItem<tauri::Wry>> = Vec::new();

    if remotes.is_empty() {
        items.push(MenuItem::with_id(
            app,
            "none",
            "还没有挂载项",
            false,
            None::<&str>,
        )?);
    } else {
        for remote in remotes {
            let label = format!(
                "{} {} ({})",
                state_mark(statuses.get(&remote.name)),
                remote.name,
                remote.mount_point
            );
            items.push(MenuItem::with_id(
                app,
                format!("toggle:{}", remote.name),
                label,
                true,
                None::<&str>,
            )?);
        }
    }

    let open = MenuItem::with_id(app, "open", "打开窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出（断开全部挂载）", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let mut refs: Vec<&dyn IsMenuItem<tauri::Wry>> = Vec::new();
    for item in &items {
        refs.push(item);
    }
    refs.push(&separator);
    refs.push(&open);
    refs.push(&quit);

    Menu::with_items(app, &refs)
}

pub fn create(app: &AppHandle) -> Result<(), String> {
    let menu =
        build_menu(app, &[], &HashMap::new()).map_err(|e| format!("创建托盘菜单失败：{e}"))?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("ylrclone · WebDAV 挂载")
        .show_menu_on_left_click(false);

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();
            crate::handle_menu_event(app, &id);
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| format!("创建托盘图标失败：{e}"))?;

    Ok(())
}

pub fn refresh(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let remotes = state.settings.lock().remotes.clone();
    let statuses = state.statuses.lock().clone();

    let menu =
        build_menu(app, &remotes, &statuses).map_err(|e| format!("重建托盘菜单失败：{e}"))?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))
            .map_err(|e| format!("更新托盘菜单失败：{e}"))?;
    }
    Ok(())
}