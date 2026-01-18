use std::io;

use collector_core::{CaptureOptions, FrameRecord};

pub trait FrameSource {
    fn next_frame(&mut self) -> io::Result<FrameRecord>;
}

pub struct WgcCapture {
    options: CaptureOptions,
}

impl WgcCapture {
    pub fn new(options: CaptureOptions) -> io::Result<Self> {
        let _ = options;
        Err(io::Error::new(
            io::ErrorKind::Other,
            "WGC capture not implemented yet",
        ))
    }
}

impl FrameSource for WgcCapture {
    fn next_frame(&mut self) -> io::Result<FrameRecord> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "WGC capture not implemented yet",
        ))
    }
}

pub struct MockCapture {
    frames: Vec<FrameRecord>,
    index: usize,
}

impl MockCapture {
    pub fn new(frames: Vec<FrameRecord>) -> Self {
        Self { frames, index: 0 }
    }
}

impl FrameSource for MockCapture {
    fn next_frame(&mut self) -> io::Result<FrameRecord> {
        if self.index >= self.frames.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "no more frames",
            ));
        }
        let frame = self.frames[self.index].clone();
        self.index += 1;
        Ok(frame)
    }
}
