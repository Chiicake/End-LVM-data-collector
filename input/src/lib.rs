use std::collections::{HashSet, VecDeque};
use std::io;

use collector_core::{InputEvent, InputEventKind, MouseButton, QpcTimestamp};

mod rawinput;

pub trait InputCollector {
    fn drain_events(&mut self, start: QpcTimestamp, end: QpcTimestamp) -> io::Result<Vec<InputEvent>>;
}

pub struct RawInputCollector {
    inner: rawinput::RawInputCollectorImpl,
    buffer: VecDeque<InputEvent>,
}

impl RawInputCollector {
    pub fn new() -> io::Result<Self> {
        Self::new_with_target(None)
    }

    pub fn new_with_target(target_hwnd: Option<isize>) -> io::Result<Self> {
        let inner = rawinput::RawInputCollectorImpl::new(target_hwnd)?;
        Ok(Self {
            inner,
            buffer: VecDeque::new(),
        })
    }
}

impl InputCollector for RawInputCollector {
    fn drain_events(&mut self, start: QpcTimestamp, end: QpcTimestamp) -> io::Result<Vec<InputEvent>> {
        self.inner.drain_into(&mut self.buffer)?;
        while matches!(self.buffer.front(), Some(ev) if ev.qpc_ts < start) {
            self.buffer.pop_front();
        }
        let mut out = Vec::new();
        while matches!(self.buffer.front(), Some(ev) if ev.qpc_ts < end) {
            if let Some(ev) = self.buffer.pop_front() {
                out.push(ev);
            }
        }
        Ok(out)
    }
}

pub struct MockInputCollector {
    events: Vec<InputEvent>,
    index: usize,
}

impl MockInputCollector {
    pub fn new(events: Vec<InputEvent>) -> Self {
        Self { events, index: 0 }
    }
}

impl InputCollector for MockInputCollector {
    fn drain_events(&mut self, start: QpcTimestamp, end: QpcTimestamp) -> io::Result<Vec<InputEvent>> {
        let mut out = Vec::new();
        while self.index < self.events.len() && self.events[self.index].qpc_ts < start {
            self.index += 1;
        }
        while self.index < self.events.len() && self.events[self.index].qpc_ts < end {
            out.push(self.events[self.index].clone());
            self.index += 1;
        }
        Ok(out)
    }
}
#[derive(Debug, Default)]
pub struct InputState {
    pub down_keys: HashSet<String>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            down_keys: HashSet::new(),
        }
    }

    pub fn apply_event(&mut self, event: &InputEvent) {
        match &event.kind {
            InputEventKind::KeyDown { key } => {
                self.down_keys.insert(key.clone());
            }
            InputEventKind::KeyUp { key } => {
                self.down_keys.remove(key);
            }
            InputEventKind::MouseButton { button, is_down } => {
                let key = mouse_button_name(*button).to_string();
                if *is_down {
                    self.down_keys.insert(key);
                } else {
                    self.down_keys.remove(&key);
                }
            }
            _ => {}
        }
    }
}

pub fn keyboard_key_name(vk: u16) -> Option<&'static str> {
    match vk {
        0x41..=0x5A => {
            const LETTERS: [&str; 26] = [
                "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
                "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
            ];
            let idx = (vk - 0x41) as usize;
            Some(LETTERS[idx])
        }
        0x31 => Some("one"),
        0x32 => Some("two"),
        0x33 => Some("three"),
        0x34 => Some("four"),
        0x35 => Some("five"),
        0x36 => Some("six"),
        0x37 => Some("seven"),
        0x38 => Some("eight"),
        0x39 => Some("nine"),
        0x70 => Some("One"),
        0x71 => Some("Two"),
        0x72 => Some("Three"),
        0x73 => Some("Four"),
        0x74 => Some("Five"),
        0x75 => Some("Six"),
        0x76 => Some("Seven"),
        0x77 => Some("Eight"),
        0x78 => Some("Nine"),
        0x79 => Some("Ten"),
        0x7A => Some("Eleven"),
        0x7B => Some("Twelve"),
        0x10 => Some("Shift"),
        0x11 => Some("Ctrl"),
        0x12 => Some("Alt"),
        0x20 => Some("Space"),
        0x1B => Some("Esc"),
        0x09 => Some("Tab"),
        0x0D => Some("Enter"),
        0x26 => Some("Up"),
        0x28 => Some("Down"),
        0x25 => Some("Left"),
        0x27 => Some("Right"),
        _ => None,
    }
}

pub fn mouse_button_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "MouseLeft",
        MouseButton::Right => "MouseRight",
        MouseButton::Middle => "MouseMiddle",
        MouseButton::X1 => "MouseX1",
        MouseButton::X2 => "MouseX2",
    }
}

pub fn make_key_event(qpc_ts: QpcTimestamp, key: &str, is_down: bool) -> InputEvent {
    let kind = if is_down {
        InputEventKind::KeyDown {
            key: key.to_string(),
        }
    } else {
        InputEventKind::KeyUp {
            key: key.to_string(),
        }
    };
    InputEvent { qpc_ts, kind }
}

pub fn make_mouse_button_event(
    qpc_ts: QpcTimestamp,
    button: MouseButton,
    is_down: bool,
) -> InputEvent {
    InputEvent {
        qpc_ts,
        kind: InputEventKind::MouseButton { button, is_down },
    }
}

pub fn make_mouse_move_event(qpc_ts: QpcTimestamp, dx: i32, dy: i32) -> InputEvent {
    InputEvent {
        qpc_ts,
        kind: InputEventKind::MouseMove { dx, dy },
    }
}

pub fn make_mouse_wheel_event(qpc_ts: QpcTimestamp, delta: i32) -> InputEvent {
    InputEvent {
        qpc_ts,
        kind: InputEventKind::MouseWheel { delta },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_state_tracks_down_keys() {
        let mut state = InputState::new();
        let down = make_key_event(10, "W", true);
        let up = make_key_event(20, "W", false);

        state.apply_event(&down);
        assert!(state.down_keys.contains("W"));

        state.apply_event(&up);
        assert!(!state.down_keys.contains("W"));
    }
}
