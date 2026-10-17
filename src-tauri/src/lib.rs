mod wsl;

use tauri::async_runtime::spawn_blocking;

/// 在阻塞线程池里执行 wsl.exe 调用，避免卡住异步运行时
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_overview() -> Result<wsl::Overview, String> {
    blocking(wsl::overview).await
}

#[tauri::command]
async fn drop_caches() -> Result<(), String> {
    blocking(wsl::drop_caches).await
}

#[tauri::command]
async fn terminate_distro(name: String) -> Result<(), String> {
    blocking(move || wsl::terminate(&name)).await
}

#[tauri::command]
async fn shutdown_wsl() -> Result<(), String> {
    blocking(wsl::shutdown).await
}

#[tauri::command]
async fn set_default_distro(name: String) -> Result<(), String> {
    blocking(move || wsl::set_default(&name)).await
}

#[tauri::command]
async fn open_terminal(name: String) -> Result<(), String> {
    blocking(move || wsl::open_terminal(&name)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_overview,
            drop_caches,
            terminate_distro,
            shutdown_wsl,
            set_default_distro,
            open_terminal
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
