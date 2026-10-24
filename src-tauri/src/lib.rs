mod wsl;
mod cache_schedule;
mod tray_status;
mod updater;

use tauri::async_runtime::spawn_blocking;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use std::{path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}}, time::{Duration, Instant}};
use cache_schedule::{CacheSchedule, Schedule, Status};
use serde::Serialize;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};

const TRAY_ID: &str = "main-tray";

struct AppState {
    close_to_tray: AtomicBool,
    close_to_tray_path: PathBuf,
    window_visible: AtomicBool,
    running_count: AtomicUsize,
    tooltip_refreshing: AtomicBool,
    last_tooltip: Mutex<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    autostart: bool,
    close_to_tray: bool,
}

/// 在阻塞线程池里执行 wsl.exe 调用，避免卡住异步运行时
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_overview(app: AppHandle) -> Result<wsl::Overview, String> {
    let overview = blocking(wsl::overview).await?;
    refresh_tray_tooltip(&app);
    Ok(overview)
}

#[tauri::command]
async fn drop_caches(schedule: tauri::State<'_, Arc<CacheSchedule>>) -> Result<(), String> {
    let schedule = schedule.inner().clone();
    blocking(move || release_caches(&schedule, |_| {})).await
}

fn release_caches(
    schedule: &CacheSchedule,
    emit_status: impl FnMut(Status),
) -> Result<(), String> {
    if !schedule.state.lock().unwrap().begin(false, Instant::now()) {
        return Err("正在释放缓存，或没有运行中的发行版".into());
    }
    release_started_caches(schedule, emit_status)
}

fn release_started_caches(
    schedule: &CacheSchedule,
    mut emit_status: impl FnMut(Status),
) -> Result<(), String> {
    let status = schedule.state.lock().unwrap().status();
    emit_status(status);
    let result = wsl::drop_caches();
    let _ = wsl::sample_vm_once();
    schedule.state.lock().unwrap().finish(result.clone(), Instant::now());
    let status = schedule.state.lock().unwrap().status();
    emit_status(status);
    result
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

#[tauri::command]
async fn compact_distro(name: String) -> Result<wsl::CompactResult, String> {
    blocking(move || wsl::compact(&name)).await
}

#[tauri::command]
async fn check_update(
    app: AppHandle,
    state: tauri::State<'_, Arc<updater::UpdaterState>>,
) -> Result<updater::UpdateStatus, String> {
    let status = updater::check(&app, state.inner()).await?;
    refresh_tray_tooltip(&app);
    Ok(status)
}

#[tauri::command]
fn get_update_status(
    state: tauri::State<'_, Arc<updater::UpdaterState>>,
) -> Option<updater::UpdateStatus> {
    updater::stored_status(state.inner())
}

#[tauri::command]
async fn install_update(
    app: AppHandle,
    state: tauri::State<'_, Arc<updater::UpdaterState>>,
) -> Result<(), String> {
    updater::install(&app, state.inner()).await
}

#[tauri::command]
fn open_release_page(app: AppHandle) -> Result<(), String> {
    updater::open_release_page(&app)
}

fn app_settings(app: &AppHandle, state: &AppState) -> Result<AppSettings, String> {
    Ok(AppSettings {
        autostart: app.autolaunch().is_enabled().map_err(|e| e.to_string())?,
        close_to_tray: state.close_to_tray.load(Ordering::Relaxed),
    })
}

#[tauri::command]
fn get_app_settings(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    app_settings(&app, state.inner())
}

#[tauri::command]
fn set_autostart(enabled: bool, app: AppHandle, state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    if enabled {
        app.autolaunch().enable().map_err(|e| e.to_string())?;
    } else {
        app.autolaunch().disable().map_err(|e| e.to_string())?;
    }
    app_settings(&app, state.inner())
}

#[tauri::command]
fn set_close_to_tray(
    enabled: bool,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, String> {
    let temporary = state.close_to_tray_path.with_extension("tmp");
    std::fs::write(&temporary, enabled.to_string())
        .map_err(|e| format!("保存关闭到托盘设置失败：{e}"))?;
    std::fs::rename(&temporary, &state.close_to_tray_path)
        .map_err(|e| format!("保存关闭到托盘设置失败：{e}"))?;
    state.close_to_tray.store(enabled, Ordering::Relaxed);
    app_settings(&app, state.inner())
}

#[tauri::command]
fn get_window_visibility(state: tauri::State<'_, AppState>) -> bool {
    state.window_visible.load(Ordering::Relaxed)
}

fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.show().is_err() {
        return;
    }
    let _ = window.unminimize();
    let _ = window.set_focus();
    if let Some(state) = app.try_state::<AppState>() {
        state.window_visible.store(true, Ordering::Relaxed);
    }
    let _ = app.emit("window-visibility", true);
}

fn refresh_tray_tooltip(app: &AppHandle) {
    let state = app.state::<AppState>();
    let schedule = app.state::<Arc<CacheSchedule>>();
    let status = schedule.state.lock().unwrap().status();
    let update_version = app
        .try_state::<Arc<updater::UpdaterState>>()
        .and_then(|state| updater::available_version(state.inner()));
    let text = tray_status::tooltip_text(
        wsl::last_vm_sample().as_ref(),
        state.running_count.load(Ordering::Relaxed),
        &status,
        Instant::now(),
        update_version.as_deref(),
    );
    let mut last = state.last_tooltip.lock().unwrap();
    if *last == text {
        return;
    }
    *last = text.clone();
    drop(last);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

fn refresh_vm_sample_on_hover(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.running_count.load(Ordering::Relaxed) == 0
        || wsl::last_vm_sample()
            .is_some_and(|sample| sample.at.elapsed() <= Duration::from_secs(10))
        || state
            .tooltip_refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
    {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = wsl::sample_vm_once();
        app.state::<AppState>()
            .tooltip_refreshing
            .store(false, Ordering::Release);
        refresh_tray_tooltip(&app);
    });
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let release = MenuItem::with_id(app, "release-caches", "立即释放缓存", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &release, &separator, &quit])?;
    let icon = app.default_window_icon().cloned().ok_or_else(|| {
        std::io::Error::other("默认窗口图标不可用")
    })?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("WSL Manager")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "release-caches" => {
                let schedule = app.state::<Arc<CacheSchedule>>().inner().clone();
                let handle = app.clone();
                std::thread::spawn(move || {
                    let _ = release_caches(&schedule, |status| {
                        let _ = handle.emit("cache-schedule", status);
                        refresh_tray_tooltip(&handle);
                    });
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Enter { .. } => refresh_vm_sample_on_hover(tray.app_handle()),
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let directory = app.path().app_config_dir()?;
            std::fs::create_dir_all(&directory)?;
            let path = directory.join("cache-schedule.txt");
            let minutes = std::fs::read_to_string(&path).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
            let mut state = Schedule::default();
            let _ = state.configure(minutes, Instant::now());
            let schedule = Arc::new(CacheSchedule { state: Mutex::new(state), path });
            app.manage(schedule.clone());
            let close_to_tray_path = directory.join("close-to-tray.txt");
            let close_to_tray = std::fs::read_to_string(&close_to_tray_path)
                .ok()
                .and_then(|value| value.trim().parse::<bool>().ok())
                .unwrap_or(true);
            let autostart = std::env::args().any(|arg| arg == "--autostart");
            app.manage(AppState {
                close_to_tray: AtomicBool::new(close_to_tray),
                close_to_tray_path,
                window_visible: AtomicBool::new(!autostart),
                running_count: AtomicUsize::new(0),
                tooltip_refreshing: AtomicBool::new(false),
                last_tooltip: Mutex::new(String::new()),
            });
            app.manage(Arc::new(updater::UpdaterState::default()));
            setup_tray(app.handle())?;
            if !autostart {
                show_main_window(app.handle());
            }
            if !cfg!(debug_assertions) {
                let handle = app.handle().clone();
                let state = app.state::<Arc<updater::UpdaterState>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    if updater::check(&handle, &state).await.is_ok() {
                        refresh_tray_tooltip(&handle);
                    }
                });
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut check_at = Instant::now();
                loop {
                    if Instant::now() >= check_at {
                        let running = wsl::running_set();
                        schedule.state.lock().unwrap().available(!running.is_empty(), Instant::now());
                        handle.state::<AppState>().running_count.store(running.len(), Ordering::Relaxed);
                        check_at = Instant::now() + Duration::from_secs(5);
                    }
                    let started = schedule.state.lock().unwrap().begin(true, Instant::now());
                    if started {
                        let _ = release_started_caches(&schedule, |status| {
                            let _ = handle.emit("cache-schedule", status);
                            refresh_tray_tooltip(&handle);
                        });
                    }
                    refresh_tray_tooltip(&handle);
                    std::thread::sleep(Duration::from_secs(1));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.close_to_tray.load(Ordering::Relaxed) && window.hide().is_ok() {
                    api.prevent_close();
                    state.window_visible.store(false, Ordering::Relaxed);
                    let _ = window.emit("window-visibility", false);
                    std::thread::spawn(wsl::close_sessions);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            drop_caches,
            get_cache_schedule,
            set_cache_schedule,
            terminate_distro,
            shutdown_wsl,
            set_default_distro,
            open_terminal,
            compact_distro,
            check_update,
            get_update_status,
            install_update,
            open_release_page,
            get_app_settings,
            set_autostart,
            set_close_to_tray,
            get_window_visibility
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
