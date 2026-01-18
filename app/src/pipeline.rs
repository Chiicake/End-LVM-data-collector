use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aggregator::{aggregate_window_with_compiled, AggregatorState, CursorProvider};
use collector_core::{InputEvent, Meta, Options, QpcTimestamp, StepIndex};
use writer::{SessionLayout, SessionWriter};

const DEFAULT_FLUSH_LINES: u64 = 10;
const DEFAULT_FLUSH_SECS: u64 = 1;

pub struct PipelineConfig {
    pub dataset_root: PathBuf,
    pub session_name: String,
    pub ffmpeg_path: PathBuf,
}

pub struct SessionPipeline {
    writer: SessionWriter,
    state: AggregatorState,
}

impl SessionPipeline {
    pub fn create(config: PipelineConfig) -> io::Result<Self> {
        let writer = SessionWriter::create(
            &config.dataset_root,
            &config.session_name,
            &config.ffmpeg_path,
            DEFAULT_FLUSH_LINES,
            Duration::from_secs(DEFAULT_FLUSH_SECS),
        )?;
        Ok(Self {
            writer,
            state: AggregatorState::new(),
        })
    }

    pub fn write_options_meta(&self, options: &Options, meta: &Meta) -> io::Result<()> {
        self.writer.write_options(options)?;
        self.writer.write_meta(meta)?;
        Ok(())
    }

    pub fn process_window(
        &mut self,
        events: &[InputEvent],
        window_start: QpcTimestamp,
        window_end: QpcTimestamp,
        step_index: StepIndex,
        is_foreground: bool,
        cursor: &CursorProvider,
        frame: &[u8],
        thought_content: Option<&str>,
    ) -> io::Result<()> {
        let aggregated = aggregate_window_with_compiled(
            events,
            window_start,
            window_end,
            step_index,
            is_foreground,
            cursor,
            &mut self.state,
        );

        self.writer.write_window(&aggregated)?;
        self.writer.write_frame(frame)?;
        let thought_line = format_thought_line(thought_content.unwrap_or_default());
        self.writer.write_thought(&thought_line)?;
        Ok(())
    }

    pub fn finalize(self) -> io::Result<SessionLayout> {
        self.writer.finalize()
    }
}

pub fn default_session_name(now: &str, run_id: u32) -> String {
    format!("{}_run{:03}", now, run_id)
}

pub fn format_thought_line(content: &str) -> String {
    if content.is_empty() {
        "<|thought_start|><|thought_end|>".to_string()
    } else if content.contains("<|thought_start|>") && content.contains("<|thought_end|>") {
        content.to_string()
    } else {
        format!("<|thought_start|>{} <|thought_end|>", content)
    }
}

pub fn ensure_dataset_root(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "dataset root does not exist",
        ));
    }
    Ok(())
}
