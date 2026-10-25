fn target_url(target: &str) -> Result<&'static str, String> {
    match target {
        "profile" => Ok("https://www.xiaohongshu.com/user/profile/5bed9e4201e65d00013a32bf"),
        "email" => Ok("mailto:chase_li@qq.com"),
        _ => Err("不支持的关于页链接。".into()),
    }
}

#[tauri::command]
pub async fn open_about_link(app: tauri::AppHandle, target: String) -> Result<(), String> {
    // Accept fixed destinations only, never a frontend-supplied URL or command.
    let url = target_url(&target)?;
    #[cfg(windows)]
    {
        // Shell extensions use the desktop UI thread's initialized COM apartment.
        let (sender, receiver) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = sender.send(open_native(url));
        })
        .map_err(|_| "无法调用系统打开链接。".to_owned())?;
        tauri::async_runtime::spawn_blocking(move || {
            receiver
                .recv()
                .map_err(|_| "系统打开链接未返回结果。".to_owned())?
        })
        .await
        .map_err(|_| "打开链接任务失败。".to_owned())?
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        tauri::async_runtime::spawn_blocking(move || open_native(url))
            .await
            .map_err(|_| "打开链接任务失败。".to_owned())?
    }
}

#[cfg(windows)]
fn open_native(url: &str) -> Result<(), String> {
    use std::ffi::c_void;
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            window: *mut c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    let target: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    // SAFETY: target is NUL-terminated and lives through the call; optional
    // pointers are null. The destination is selected from the fixed allowlist.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result > 32 {
        Ok(())
    } else {
        Err("无法打开链接，请检查系统默认浏览器或邮件应用。".into())
    }
}

#[cfg(not(windows))]
fn open_native(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let executable = "/usr/bin/open";
    #[cfg(not(target_os = "macos"))]
    let executable = "xdg-open";
    let result = std::process::Command::new(executable)
        .arg(url)
        .output()
        .map_err(|_| "无法打开链接，请检查系统默认浏览器或邮件应用。".to_owned())?;
    if result.status.success() {
        Ok(())
    } else {
        Err("无法打开链接，请检查系统默认浏览器或邮件应用。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_reviewed_destinations_can_be_opened() {
        assert!(target_url("profile")
            .unwrap()
            .starts_with("https://www.xiaohongshu.com/user/profile/"));
        assert_eq!(target_url("email").unwrap(), "mailto:chase_li@qq.com");
        for input in [
            "https://example.com",
            "file:///tmp/test",
            "javascript:alert(1)",
            "--args",
            "email;open",
        ] {
            assert!(target_url(input).is_err());
        }
    }
}
