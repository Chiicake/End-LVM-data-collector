use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use std::fs::{self, File};
use std::io::Read;

use collector_core::{Meta, Options};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use app::pipeline::{PipelineConfig, SessionPipeline};
#[cfg(windows)]
use capture::WgcCapture;
#[cfg(windows)]
use input::RawInputCollector;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiSessionConfig {
    pub dataset_root: PathBuf,
    pub session_name: String,
    pub ffmpeg_path: PathBuf,
    pub target_hwnd: isize,
    pub options: Options,
    pub meta: Meta,
    pub cursor_debug: bool,
}

pub struct GuiSessionRunner;

#[derive(Debug, Clone)]
pub enum GuiStatus {
    Started { session_name: String },
    Frame { step_index: u64, qpc_ts: u64, is_foreground: bool },
    Finished { output_dir: PathBuf },
    Error { message: String },
}

pub struct GuiSessionHandle {
    pub rx: mpsc::Receiver<GuiStatus>,
    join: JoinHandle<io::Result<PathBuf>>,
    thought: Arc<Mutex<String>>,
    goals: Arc<Mutex<GoalState>>,
    stop: Arc<AtomicBool>,
}

impl GuiSessionHandle {
    pub fn join(self) -> io::Result<PathBuf> {
        match self.join.join() {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "gui session thread panicked",
            )),
        }
    }

    pub fn set_thought(&self, text: String) -> io::Result<()> {
        let mut guard = self
            .thought
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "thought lock poisoned"))?;
        *guard = text;
        Ok(())
    }

    pub fn set_goals(&self, long_goal: String, mid_goal: String) -> io::Result<()> {
        let mut guard = self
            .goals
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "goals lock poisoned"))?;
        guard.long = long_goal;
        guard.mid = mid_goal;
        Ok(())
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[derive(Debug, Default, Clone)]
struct GoalState {
    long: String,
    mid: String,
}

#[derive(Debug, Clone)]
pub enum GuiPackageStatus {
    Started { total_files: u64, total_bytes: u64 },
    File { index: u64, total_files: u64, bytes: u64, path: PathBuf },
    Finished { output_zip: PathBuf, deleted: bool },
    Error { message: String },
}

pub struct GuiPackageHandle {
    pub rx: mpsc::Receiver<GuiPackageStatus>,
    join: JoinHandle<io::Result<PathBuf>>,
}

impl GuiPackageHandle {
    pub fn join(self) -> io::Result<PathBuf> {
        match self.join.join() {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "gui package thread panicked",
            )),
        }
    }
}

impl GuiSessionRunner {
    pub fn start_realtime_blocking(config: GuiSessionConfig) -> io::Result<PathBuf> {
        #[cfg(not(windows))]
        {
            let _ = config;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "GUI capture requires Windows",
            ));
        }
        #[cfg(windows)]
        {
            let pipeline = SessionPipeline::create(PipelineConfig {
                dataset_root: config.dataset_root.clone(),
                session_name: config.session_name.clone(),
                ffmpeg_path: config.ffmpeg_path.clone(),
                record_width: config.options.capture.record_resolution[0],
                record_height: config.options.capture.record_resolution[1],
                fps: config.options.capture.fps,
            })?;
            pipeline.write_options_meta(&config.options, &config.meta)?;

            let capture = WgcCapture::new(config.options.capture.clone(), config.target_hwnd)?;
            let input = RawInputCollector::new()?;

            let layout = app::pipeline::run_realtime_with_hwnd(
                capture,
                input,
                config.target_hwnd,
                config.cursor_debug,
                pipeline,
                config.options.timing.step_ms,
            )?;
            Ok(layout.root_dir)
        }
    }

    pub fn start_realtime_async(config: GuiSessionConfig) -> io::Result<GuiSessionHandle> {
        #[cfg(not(windows))]
        {
            let _ = config;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "GUI capture requires Windows",
            ));
        }
        #[cfg(windows)]
        {
            let (tx, rx) = mpsc::channel();
            let thought_state = Arc::new(Mutex::new(String::new()));
            let thought_state_thread = Arc::clone(&thought_state);
            let goals_state = Arc::new(Mutex::new(GoalState::default()));
            let goals_state_thread = Arc::clone(&goals_state);
            let stop_flag = Arc::new(AtomicBool::new(false));
            let stop_flag_thread = Arc::clone(&stop_flag);
            let handle = std::thread::spawn(move || {
                let pipeline = SessionPipeline::create(PipelineConfig {
                    dataset_root: config.dataset_root.clone(),
                    session_name: config.session_name.clone(),
                    ffmpeg_path: config.ffmpeg_path.clone(),
                    record_width: config.options.capture.record_resolution[0],
                    record_height: config.options.capture.record_resolution[1],
                    fps: config.options.capture.fps,
                })?;
                pipeline.write_options_meta(&config.options, &config.meta)?;
                let _ = tx.send(GuiStatus::Started {
                    session_name: config.session_name.clone(),
                });

                let capture = WgcCapture::new(config.options.capture.clone(), config.target_hwnd)?;
                let input = RawInputCollector::new()?;
                let tx_frame = tx.clone();

                let result = app::pipeline::run_realtime_with_hwnd_and_hook_and_thought_with_stop(
                    capture,
                    input,
                    config.target_hwnd,
                    config.cursor_debug,
                    pipeline,
                    &mut |frame, is_foreground, _cursor| {
                        let _ = tx_frame.send(GuiStatus::Frame {
                            step_index: frame.step_index,
                            qpc_ts: frame.qpc_ts,
                            is_foreground,
                        });
                    },
                    &mut || {
                        thought_state_thread
                            .lock()
                            .map(|value| value.clone())
                            .unwrap_or_default()
                    },
                    &mut || {
                        goals_state_thread
                            .lock()
                            .map(|value| (value.long.clone(), value.mid.clone()))
                            .unwrap_or_else(|_| (String::new(), String::new()))
                    },
                    &mut || stop_flag_thread.load(Ordering::SeqCst),
                    config.options.timing.step_ms,
                );

                match result {
                    Ok(layout) => {
                        let _ = tx.send(GuiStatus::Finished {
                            output_dir: layout.root_dir.clone(),
                        });
                        Ok(layout.root_dir)
                    }
                    Err(err) => {
                        let _ = tx.send(GuiStatus::Error {
                            message: err.to_string(),
                        });
                        Err(err)
                    }
                }
            });
            Ok(GuiSessionHandle {
                rx,
                join: handle,
                thought: thought_state,
                goals: goals_state,
                stop: stop_flag,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRequest {
    pub dataset_root: PathBuf,
    pub session_names: Vec<String>,
    pub output_zip: PathBuf,
    pub delete_after: bool,
}

pub fn package_sessions(request: PackageRequest) -> io::Result<PathBuf> {
    let sessions_dir = request.dataset_root.join("sessions");
    let targets = resolve_targets(&sessions_dir, &request.session_names)?;

    if targets.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no sessions found to package",
        ));
    }

    let files = collect_files(&request.dataset_root, &targets)?;
    let file = File::create(&request.output_zip)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default();

    for (path, _) in &files {
        let rel = path.strip_prefix(&request.dataset_root).map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "failed to compute relative path")
        })?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        zip.start_file(rel_str, options)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        let mut buffer = Vec::new();
        File::open(path)?.read_to_end(&mut buffer)?;
        zip.write_all(&buffer)?;
    }

    zip.finish()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    if request.delete_after {
        for target in &targets {
            if target.exists() {
                fs::remove_dir_all(target)?;
            }
        }
    }

    Ok(request.output_zip)
}

pub fn start_package_async(request: PackageRequest) -> io::Result<GuiPackageHandle> {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let sessions_dir = request.dataset_root.join("sessions");
        let targets = resolve_targets(&sessions_dir, &request.session_names)?;
        if targets.is_empty() {
            let err = io::Error::new(io::ErrorKind::NotFound, "no sessions found to package");
            let _ = tx.send(GuiPackageStatus::Error {
                message: err.to_string(),
            });
            return Err(err);
        }

        let files = collect_files(&request.dataset_root, &targets)?;
        let total_files = files.len() as u64;
        let total_bytes = files.iter().map(|(_, size)| *size).sum();
        let _ = tx.send(GuiPackageStatus::Started {
            total_files,
            total_bytes,
        });

        let file = File::create(&request.output_zip)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::default();

        for (index, (path, size)) in files.iter().enumerate() {
            let rel = path.strip_prefix(&request.dataset_root).map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to compute relative path")
            })?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            zip.start_file(rel_str, options)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            let mut buffer = Vec::new();
            File::open(path)?.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
            let _ = tx.send(GuiPackageStatus::File {
                index: (index + 1) as u64,
                total_files,
                bytes: *size,
                path: path.clone(),
            });
        }

        zip.finish()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        if request.delete_after {
            for target in &targets {
                if target.exists() {
                    fs::remove_dir_all(target)?;
                }
            }
        }

        let _ = tx.send(GuiPackageStatus::Finished {
            output_zip: request.output_zip.clone(),
            deleted: request.delete_after,
        });
        Ok(request.output_zip)
    });

    Ok(GuiPackageHandle { rx, join: handle })
}

fn list_session_dirs(root: &PathBuf) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && !is_tmp_dir(&path) {
            out.push(path);
        }
    }
    Ok(out)
}

fn resolve_targets(root: &PathBuf, names: &[String]) -> io::Result<Vec<PathBuf>> {
    if names.is_empty() {
        list_session_dirs(root)
    } else {
        Ok(names.iter().map(|name| root.join(name)).collect())
    }
}

fn is_tmp_dir(path: &PathBuf) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with(".tmp"))
        .unwrap_or(false)
}

fn collect_files(_base: &PathBuf, targets: &[PathBuf]) -> io::Result<Vec<(PathBuf, u64)>> {
    let mut files = Vec::new();
    let mut stack = targets.to_vec();
    while let Some(current) = stack.pop() {
        if is_tmp_dir(&current) {
            continue;
        }
        if current.is_dir() {
            for entry in fs::read_dir(&current)? {
                let entry = entry?;
                stack.push(entry.path());
            }
        } else if current.is_file() {
            let size = current.metadata().map(|meta| meta.len()).unwrap_or(0);
            files.push((current, size));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}
