use std::io;
use std::path::PathBuf;

use collector_core::{Meta, Options};
#[cfg(windows)]
use app::pipeline::{PipelineConfig, SessionPipeline};
#[cfg(windows)]
use capture::WgcCapture;
#[cfg(windows)]
use input::RawInputCollector;

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
        })?;
        pipeline.write_options_meta(&config.options, &config.meta)?;

        let capture = WgcCapture::new(config.options.capture.clone(), config.target_hwnd)?;
        let input = RawInputCollector::new_with_target(Some(config.target_hwnd))?;

        let layout = app::pipeline::run_realtime_with_hwnd(
            capture,
            input,
            config.target_hwnd,
            config.cursor_debug,
            pipeline,
        )?;
        Ok(layout.root_dir)
        }
    }
}
