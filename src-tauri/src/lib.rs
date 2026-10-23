mod wsl;
mod cache_schedule;

use tauri::async_runtime::spawn_blocking;
use tauri::{Emitter, Manager};
use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};
use cache_schedule::{CacheSchedule, Schedule, Status};

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
async fn drop_caches(schedule: tauri::State<'_, Arc<CacheSchedule>>) -> Result<(), String> {
    let schedule = schedule.inner().clone();
    blocking(move || {
        if !schedule.state.lock().unwrap().begin(false, Instant::now()) {
            return Err("正在释放缓存，或没有运行中的发行版".into());
        }
        let result = wsl::drop_caches();
        schedule.state.lock().unwrap().finish(result.clone(), Instant::now());
        result
    }).await
}

#[tauri::command]
fn get_cache_schedule(schedule: tauri::State<'_, Arc<CacheSchedule>>) -> Status {
    schedule.state.lock().unwrap().status()
}

#[tauri::command]
fn set_cache_schedule(minutes: u32, schedule: tauri::State<'_, Arc<CacheSchedule>>) -> Result<Status, String> {
    if minutes > 1440 { return Err("请输入 1～1440 分钟，或选择关闭".into()); }
    let mut state = schedule.state.lock().unwrap();
    let temporary = schedule.path.with_extension("tmp");
    std::fs::write(&temporary, minutes.to_string()).map_err(|e| format!("保存定时设置失败：{e}"))?;
    std::fs::rename(&temporary, &schedule.path).map_err(|e| format!("保存定时设置失败：{e}"))?;
    state.configure(minutes, Instant::now())?;
    Ok(state.status())
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
        .setup(|app| {
            let directory = app.path().app_config_dir()?;
            std::fs::create_dir_all(&directory)?;
            let path = directory.join("cache-schedule.txt");
            let minutes = std::fs::read_to_string(&path).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
            let mut state = Schedule::default();
            let _ = state.configure(minutes, Instant::now());
            let schedule = Arc::new(CacheSchedule { state: Mutex::new(state), path });
            app.manage(schedule.clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut check_at = Instant::now();
                loop {
                    if Instant::now() >= check_at {
                        let available = !wsl::running_set().is_empty();
                        schedule.state.lock().unwrap().available(available, Instant::now());
                        check_at = Instant::now() + Duration::from_secs(5);
                    }
                    let started = schedule.state.lock().unwrap().begin(true, Instant::now());
                    if started {
                        let _ = handle.emit("cache-schedule", schedule.state.lock().unwrap().status());
                        let result = wsl::drop_caches();
                        schedule.state.lock().unwrap().finish(result, Instant::now());
                        let _ = handle.emit("cache-schedule", schedule.state.lock().unwrap().status());
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            drop_caches,
            get_cache_schedule,
            set_cache_schedule,
            terminate_distro,
            shutdown_wsl,
            set_default_distro,
            open_terminal
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
