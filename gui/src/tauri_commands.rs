use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;
use std::path::Path;
use std::process::Command;

use gui::{
    start_package_async, GuiPackageHandle, GuiPackageStatus, GuiSessionConfig, GuiSessionHandle,
    GuiSessionRunner, GuiStatus, PackageRequest,
};

#[derive(Default)]
pub struct GuiState {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<u64, GuiSessionHandle>>,
    packages: Mutex<HashMap<u64, GuiPackageHandle>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuiStatusDto {
    Started { session_name: String },
    Frame {
        step_index: u64,
        qpc_ts: u64,
        is_foreground: bool,
    },
    Finished { output_dir: String },
    Error { message: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuiPackageStatusDto {
    Started { total_files: u64, total_bytes: u64 },
    File {
        index: u64,
        total_files: u64,
        bytes: u64,
        path: String,
    },
    Finished { output_zip: String, deleted: bool },
    Error { message: String },
}

#[derive(Debug, Serialize)]
pub struct WindowEntryDto {
    pub hwnd: isize,
    pub title: String,
}

#[tauri::command]
pub fn start_session(config: GuiSessionConfig, state: State<GuiState>) -> Result<u64, String> {
    let handle = GuiSessionRunner::start_realtime_async(config).map_err(|err| err.to_string())?;
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let mut sessions = state.sessions.lock().map_err(|_| "lock poisoned")?;
    sessions.insert(id, handle);
    Ok(id)
}

#[tauri::command]
pub fn poll_session(id: u64, state: State<GuiState>) -> Result<Vec<GuiStatusDto>, String> {
    let sessions = state.sessions.lock().map_err(|_| "lock poisoned")?;
    let handle = sessions.get(&id).ok_or_else(|| "unknown session id".to_string())?;
    let mut out = Vec::new();
    for status in handle.rx.try_iter() {
        out.push(map_status(status));
    }
    Ok(out)
}

#[tauri::command]
pub fn join_session(id: u64, state: State<GuiState>) -> Result<String, String> {
    let handle = {
        let mut sessions = state.sessions.lock().map_err(|_| "lock poisoned")?;
        sessions
            .remove(&id)
            .ok_or_else(|| "unknown session id".to_string())?
    };
    handle
        .join()
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn stop_session(id: u64, state: State<GuiState>) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|_| "lock poisoned")?;
    let handle = sessions.get(&id).ok_or_else(|| "unknown session id".to_string())?;
    handle.stop();
    Ok(())
}

#[tauri::command]
pub fn set_thought(id: u64, text: String, state: State<GuiState>) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|_| "lock poisoned")?;
    let handle = sessions.get(&id).ok_or_else(|| "unknown session id".to_string())?;
    handle.set_thought(text).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn set_goals(
    id: u64,
    long_goal: String,
    mid_goal: String,
    state: State<GuiState>,
) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|_| "lock poisoned")?;
    let handle = sessions.get(&id).ok_or_else(|| "unknown session id".to_string())?;
    handle
        .set_goals(long_goal, mid_goal)
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn start_package(request: PackageRequest, state: State<GuiState>) -> Result<u64, String> {
    let handle = start_package_async(request).map_err(|err| err.to_string())?;
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let mut packages = state.packages.lock().map_err(|_| "lock poisoned")?;
    packages.insert(id, handle);
    Ok(id)
}

#[tauri::command]
pub fn poll_package(id: u64, state: State<GuiState>) -> Result<Vec<GuiPackageStatusDto>, String> {
    let packages = state.packages.lock().map_err(|_| "lock poisoned")?;
    let handle = packages.get(&id).ok_or_else(|| "unknown package id".to_string())?;
    let mut out = Vec::new();
    for status in handle.rx.try_iter() {
        out.push(map_package_status(status));
    }
    Ok(out)
}

#[tauri::command]
pub fn join_package(id: u64, state: State<GuiState>) -> Result<String, String> {
    let handle = {
        let mut packages = state.packages.lock().map_err(|_| "lock poisoned")?;
        packages
            .remove(&id)
            .ok_or_else(|| "unknown package id".to_string())?
    };
    handle
        .join()
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|err| err.to_string())
}

#[cfg(windows)]
#[tauri::command]
pub fn list_windows() -> Result<Vec<WindowEntryDto>, String> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextW, IsWindowVisible,
    };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut buf = vec![0u16; 512];
        let copied = GetWindowTextW(hwnd, &mut buf);
        let mut title = if copied > 0 {
            String::from_utf16_lossy(&buf[..copied as usize])
        } else {
            String::new()
        };
        if title.trim().is_empty() {
            let mut class_buf = vec![0u16; 256];
            let class_len = GetClassNameW(hwnd, &mut class_buf);
            if class_len > 0 {
                title = format!(
                    "[class] {}",
                    String::from_utf16_lossy(&class_buf[..class_len as usize])
                );
            } else {
                title = "[untitled window]".to_string();
            }
        }
        let entries = &mut *(lparam.0 as *mut Vec<WindowEntryDto>);
        entries.push(WindowEntryDto { hwnd: hwnd.0, title });
        BOOL(1)
    }

    let mut entries: Vec<WindowEntryDto> = Vec::new();
    let entries_ptr = &mut entries as *mut Vec<WindowEntryDto>;
    unsafe {
        EnumWindows(Some(enum_proc), LPARAM(entries_ptr as isize));
    }
    Ok(entries)
}

#[cfg(not(windows))]
#[tauri::command]
pub fn list_windows() -> Result<Vec<WindowEntryDto>, String> {
    Err("window listing is only supported on Windows".to_string())
}

#[tauri::command]
pub fn validate_ffmpeg(path: String) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("ffmpeg path is empty".to_string());
    }
    let path_obj = Path::new(trimmed);
    if path_obj.is_file() {
        return Ok(());
    }
    let output = Command::new(trimmed)
        .arg("-version")
        .output()
        .map_err(|err| format!("failed to run ffmpeg: {}", err))?;
    if output.status.success() {
        Ok(())
    } else {
        Err("ffmpeg returned an error".to_string())
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn validate_session_name(datasetRoot: String, sessionName: String) -> Result<(), String> {
    let root = Path::new(datasetRoot.trim());
    if !root.exists() {
        return Err("dataset root does not exist".to_string());
    }
    let sessions_dir = root.join("sessions");
    let candidate = sessions_dir.join(sessionName.trim());
    if candidate.exists() {
        return Err("session directory already exists".to_string());
    }
    Ok(())
}

fn map_status(status: GuiStatus) -> GuiStatusDto {
    match status {
        GuiStatus::Started { session_name } => GuiStatusDto::Started { session_name },
        GuiStatus::Frame {
            step_index,
            qpc_ts,
            is_foreground,
        } => GuiStatusDto::Frame {
            step_index,
            qpc_ts,
            is_foreground,
        },
        GuiStatus::Finished { output_dir } => GuiStatusDto::Finished {
            output_dir: output_dir.to_string_lossy().to_string(),
        },
        GuiStatus::Error { message } => GuiStatusDto::Error { message },
    }
}

fn map_package_status(status: GuiPackageStatus) -> GuiPackageStatusDto {
    match status {
        GuiPackageStatus::Started {
            total_files,
            total_bytes,
        } => GuiPackageStatusDto::Started {
            total_files,
            total_bytes,
        },
        GuiPackageStatus::File {
            index,
            total_files,
            bytes,
            path,
        } => GuiPackageStatusDto::File {
            index,
            total_files,
            bytes,
            path: path.to_string_lossy().to_string(),
        },
        GuiPackageStatus::Finished {
            output_zip,
            deleted,
        } => GuiPackageStatusDto::Finished {
            output_zip: output_zip.to_string_lossy().to_string(),
            deleted,
        },
        GuiPackageStatus::Error { message } => GuiPackageStatusDto::Error { message },
    }
}
