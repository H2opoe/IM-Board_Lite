mod ai;
mod analysis;
mod bridge_runner;
mod commands;
mod connectors;
mod daily_cache;
mod diagnostics;
mod domain;
mod messages;
mod profile_manager;
mod runtime;
mod security;
mod storage;
mod sync;
mod version;

use storage::AppState;
#[cfg(not(target_os = "macos"))]
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri::{Manager, WindowEvent};

const MAIN_WINDOW_LABEL: &str = "main";
#[cfg(not(target_os = "macos"))]
const TRAY_MENU_SHOW: &str = "show-main-window";
#[cfg(not(target_os = "macos"))]
const TRAY_MENU_QUIT: &str = "quit-app";

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new().expect("初始化应用状态失败"))
        .setup(|_app| {
            sync::job::spawn_auto_sync_task(_app.handle().clone());

            #[cfg(not(target_os = "macos"))]
            {
                let show_item =
                    MenuItem::with_id(_app, TRAY_MENU_SHOW, "显示主窗口", true, None::<&str>)?;
                let quit_item =
                    MenuItem::with_id(_app, TRAY_MENU_QUIT, "退出IM-Board", true, None::<&str>)?;
                let tray_menu = Menu::with_items(_app, &[&show_item, &quit_item])?;
                let mut tray = TrayIconBuilder::with_id("main-tray")
                    .menu(&tray_menu)
                    .show_menu_on_left_click(true)
                    .tooltip("IM-Board正在后台运行")
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        TRAY_MENU_SHOW => show_main_window(app),
                        TRAY_MENU_QUIT => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
                        }
                    });

                if let Some(icon) = _app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }

                tray.build(_app)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW_LABEL {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::profiles::list_profiles,
            commands::profiles::upsert_profile,
            commands::profiles::delete_profile,
            commands::dashboard::get_dashboard,
            commands::dashboard::mark_action_item,
            commands::bridge::check_platform_cli_update,
            commands::bridge::cleanup_unused_platform_cli,
            commands::bridge::deploy_platform_bridge,
            commands::bridge::run_bridge_command,
            commands::bridge::update_platform_cli,
            commands::app::get_app_settings,
            commands::app::open_macos_privacy_settings,
            commands::app::export_diagnostic_package,
            commands::app::save_app_settings,
            commands::app::set_theme_dock_icon,
            commands::ai::get_ai_config,
            commands::ai::cancel_local_deepseek_download,
            commands::ai::clear_local_deepseek_model,
            commands::ai::get_local_deepseek_download_progress,
            commands::ai::get_local_deepseek_status,
            commands::ai::install_local_deepseek_model,
            commands::ai::save_ai_config,
            commands::ai::test_ai_connection,
            commands::sync::cancel_sync,
            commands::sync::set_auto_sync_frequency_minutes,
            commands::sync::run_sync_job
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = event
        {
            show_main_window(app);
        }
    });
}
