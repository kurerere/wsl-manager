use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt as _;
use tauri_plugin_updater::{Update, UpdaterExt};

const RELEASE_PAGE: &str = "https://github.com/kurerere/wsl-manager/releases/latest";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub available: Option<UpdateInfo>,
    pub installable: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

#[derive(Default)]
pub struct UpdaterState {
    pending: Mutex<Option<Update>>,
    status: Mutex<Option<UpdateStatus>>,
}

pub async fn check(app: &AppHandle, state: &UpdaterState) -> Result<UpdateStatus, String> {
    let update = app
        .updater_builder()
        .build()
        .map_err(|error| format!("检查更新失败：{error}"))?
        .check()
        .await
        .map_err(|error| format!("检查更新失败：{error}"))?;
    let status = UpdateStatus {
        current_version: app.package_info().version.to_string(),
        available: update.as_ref().map(|update| UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone(),
            date: update.date.map(|date| date.to_string()),
        }),
        installable: is_installed(&app.package_info().name),
    };
    *state.pending.lock().unwrap() = update;
    *state.status.lock().unwrap() = Some(status.clone());
    if status.available.is_some() {
        let _ = app.emit("update-available", status.clone());
    }
    Ok(status)
}

pub fn stored_status(state: &UpdaterState) -> Option<UpdateStatus> {
    state.status.lock().unwrap().clone()
}

pub fn available_version(state: &UpdaterState) -> Option<String> {
    stored_status(state)?.available.map(|update| update.version)
}

pub async fn install(app: &AppHandle, state: &UpdaterState) -> Result<(), String> {
    let installable = state
        .status
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|status| status.installable && status.available.is_some());
    if !installable {
        return Err("免安装版不支持在线安装，请前往下载页面".into());
    }
    let update = state
        .pending
        .lock()
        .unwrap()
        .take()
        .ok_or("更新信息已过期，请重新检查")?;
    let progress_app = app.clone();
    let installing_app = app.clone();
    let mut downloaded = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress_app.emit(
                    "update-progress",
                    UpdateProgress {
                        downloaded,
                        total,
                    },
                );
            },
            move || {
                let _ = installing_app.emit("update-installing", ());
            },
        )
        .await
        .map_err(|error| format!("下载或安装更新失败：{error}"))
}

pub fn open_release_page(app: &AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(RELEASE_PAGE, None::<&str>)
        .map_err(|error| format!("无法打开下载页面：{error}"))
}

#[cfg(windows)]
fn is_installed(product_name: &str) -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let Some(directory) = executable.parent() else {
        return false;
    };
    if directory.join("uninstall.exe").is_file() {
        return true;
    }
    let Ok(directory) = directory.canonicalize() else {
        return false;
    };
    const UNINSTALL_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().any(|hive| {
        let Ok(uninstall) = RegKey::predef(hive).open_subkey(UNINSTALL_PATH) else {
            return false;
        };
        uninstall.enum_keys().flatten().any(|subkey| {
            let Ok(entry) = uninstall.open_subkey(subkey) else {
                return false;
            };
            let display_name: String = entry.get_value("DisplayName").unwrap_or_default();
            if !display_name.eq_ignore_ascii_case(product_name) {
                return false;
            }
            let install_location: String = entry.get_value("InstallLocation").unwrap_or_default();
            let install_location = install_location.trim().trim_matches('"');
            std::path::Path::new(install_location)
                .canonicalize()
                .is_ok_and(|installed| {
                    installed
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&directory.to_string_lossy())
                })
        })
    })
}

#[cfg(not(windows))]
fn is_installed(_product_name: &str) -> bool {
    true
}
