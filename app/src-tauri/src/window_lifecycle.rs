//! One restoration path for startup, tray activation and a second instance.
use std::io::Write;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

const MAIN: &str = "main";

fn record(app: &tauri::AppHandle, detail: &str) {
    eprintln!("[main-window] {detail}");
    if let Ok(directory) = app.path().app_log_dir() {
        if std::fs::create_dir_all(&directory).is_ok() {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("window-lifecycle.log"))
            {
                let _ = writeln!(file, "{} {detail}", chrono::Local::now().to_rfc3339());
            }
        }
    }
}

fn report(app: &tauri::AppHandle, error: String) {
    record(app, &error);
    app.dialog()
        .message(format!(
            "主窗口无法显示：{error}\n请保留 window-lifecycle.log 以便诊断。"
        ))
        .title("IM-Board")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

pub(crate) fn show(app: &tauri::AppHandle) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if let Err(error) = restore(&handle) {
            report(&handle, error.to_string());
        }
    }) {
        // Dispatch itself failed; do not silently leave only a tray icon.
        record(app, &format!("无法提交主窗口恢复操作：{error}"));
    }
}

fn restore(app: &tauri::AppHandle) -> tauri::Result<()> {
    let window = if let Some(window) = app.get_webview_window(MAIN) {
        window
    } else {
        record(app, "主窗口不存在；按发行配置重新创建");
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|config| config.label == MAIN)
            .ok_or(tauri::Error::WindowNotFound)?;
        tauri::WebviewWindowBuilder::from_config(app, config)?.build()?
    };
    // Restore before showing: Windows can otherwise retain an iconic placement.
    window.unminimize()?;
    window.set_skip_taskbar(false)?;
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let monitors = window.available_monitors()?;
    let reachable = monitors.iter().any(|monitor| {
        let origin = monitor.position();
        let extent = monitor.size();
        title_is_reachable(
            (position.x, position.y, size.width),
            (origin.x, origin.y, extent.width, extent.height),
        )
    });
    if !reachable {
        record(app, "窗口标题区域不在当前显示器内；恢复到可见位置");
        let monitor = window
            .primary_monitor()?
            .or_else(|| monitors.first().cloned())
            .ok_or_else(|| tauri::Error::Anyhow(anyhow::anyhow!("无法获取可用显示器")))?;
        let origin = monitor.position();
        let extent = monitor.size();
        let position = recovery_position(
            (size.width, size.height),
            (origin.x, origin.y, extent.width, extent.height),
        );
        window.set_position(tauri::PhysicalPosition::new(position.0, position.1))?;
    }
    window.show()?;
    // Windows may reject foreground activation; visibility must still be restored.
    if let Err(error) = window.set_focus() {
        record(app, &format!("主窗口已显示，但系统拒绝前台聚焦：{error}"));
    }
    record(app, "主窗口恢复请求完成");
    Ok(())
}

fn title_is_reachable(window: (i32, i32, u32), monitor: (i32, i32, u32, u32)) -> bool {
    let (x, y, width) = (
        i64::from(window.0),
        i64::from(window.1),
        i64::from(window.2),
    );
    let (mx, my, mw, mh) = (
        i64::from(monitor.0),
        i64::from(monitor.1),
        i64::from(monitor.2),
        i64::from(monitor.3),
    );
    let horizontal = (x + width).min(mx + mw) - x.max(mx);
    let title_height = (y + 48).min(my + mh) - y.max(my);
    width > 0 && horizontal >= 64 && title_height >= 24
}

fn recovery_position(size: (u32, u32), monitor: (i32, i32, u32, u32)) -> (i32, i32) {
    // Oversized windows still need a reachable title; naïve centering can hide it.
    let x = i64::from(monitor.0) + i64::from(monitor.2.saturating_sub(size.0)) / 2;
    let y = i64::from(monitor.1) + i64::from(monitor.3.saturating_sub(size.1)) / 2;
    (
        x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::{recovery_position, title_is_reachable};
    #[test]
    fn oversized_window_recovers_with_its_title_inside_the_monitor() {
        let monitor = (-1920, 0, 1920, 1080);
        let (x, y) = recovery_position((2640, 1720), monitor);
        assert_eq!((x, y), (-1920, 0));
        assert!(title_is_reachable((x, y, 2640), monitor));
        assert_eq!(
            recovery_position((1320, 860), (0, 0, 1920, 1080)),
            (300, 110)
        );
    }
    #[test]
    fn detached_monitor_and_minimized_coordinates_are_unreachable() {
        assert!(!title_is_reachable((3000, 0, 1320), (0, 0, 1920, 1080)));
        assert!(!title_is_reachable(
            (-32000, -32000, 1320),
            (0, 0, 1920, 1080)
        ));
    }
    #[test]
    fn negative_origin_monitor_remains_valid() {
        assert!(title_is_reachable(
            (-1800, 100, 1320),
            (-1920, 0, 1920, 1080)
        ));
    }
    #[test]
    fn content_without_reachable_title_is_not_enough() {
        assert!(!title_is_reachable((100, -200, 1320), (0, 0, 1920, 1080)));
        assert!(!title_is_reachable((1910, 100, 1320), (0, 0, 1920, 1080)));
        assert!(title_is_reachable((-1200, 100, 1320), (0, 0, 1920, 1080)));
    }
}
