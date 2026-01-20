use std::io;

use collector_core::{InputEvent, InputEventKind, MouseButton, QpcTimestamp};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::keyboard_key_name;

struct KeySpec {
    vk: u16,
    name: &'static str,
}

pub struct PollingCollector {
    keys: Vec<KeySpec>,
    key_state: Vec<bool>,
    mouse_state: [bool; 5],
    last_cursor: Option<POINT>,
}

impl PollingCollector {
    pub fn new() -> io::Result<Self> {
        let keys = build_key_specs();
        let key_state = vec![false; keys.len()];
        let last_cursor = cursor_pos();
        Ok(Self {
            keys,
            key_state,
            mouse_state: [false; 5],
            last_cursor,
        })
    }

    pub fn sample(
        &mut self,
        window_start: QpcTimestamp,
        window_end: QpcTimestamp,
        include_keyboard: bool,
        include_mouse: bool,
    ) -> io::Result<Vec<InputEvent>> {
        let ts = if window_end > window_start {
            window_end - 1
        } else {
            window_end
        };
        let mut events = Vec::new();

        for (idx, key) in self.keys.iter().enumerate() {
            let state = async_key_state(key.vk);
            let down = (state & 0x8000) != 0;
            let pressed_since = (state & 0x0001) != 0;
            let prev = self.key_state[idx];
            self.key_state[idx] = down;

            if !include_keyboard {
                continue;
            }

            if down {
                events.push(InputEvent {
                    qpc_ts: ts,
                    kind: InputEventKind::KeyDown {
                        key: key.name.to_string(),
                    },
                });
                continue;
            }

            if pressed_since {
                events.push(InputEvent {
                    qpc_ts: ts,
                    kind: InputEventKind::KeyDown {
                        key: key.name.to_string(),
                    },
                });
                events.push(InputEvent {
                    qpc_ts: ts,
                    kind: InputEventKind::KeyUp {
                        key: key.name.to_string(),
                    },
                });
                continue;
            }

            if prev {
                events.push(InputEvent {
                    qpc_ts: ts,
                    kind: InputEventKind::KeyUp {
                        key: key.name.to_string(),
                    },
                });
            }
        }

        let mouse_specs: [(u16, MouseButton); 5] = [
            (VK_LBUTTON.0, MouseButton::Left),
            (VK_RBUTTON.0, MouseButton::Right),
            (VK_MBUTTON.0, MouseButton::Middle),
            (VK_XBUTTON1.0, MouseButton::X1),
            (VK_XBUTTON2.0, MouseButton::X2),
        ];
        for (idx, (vk, button)) in mouse_specs.iter().enumerate() {
            let down = (async_key_state(*vk) & 0x8000) != 0;
            let prev = self.mouse_state[idx];
            if down != prev {
                self.mouse_state[idx] = down;
                if include_mouse {
                    events.push(InputEvent {
                        qpc_ts: ts,
                        kind: InputEventKind::MouseButton {
                            button: *button,
                            is_down: down,
                        },
                    });
                }
            }
        }

        if let Some(point) = cursor_pos() {
            if let Some(prev) = self.last_cursor {
                let dx = point.x - prev.x;
                let dy = point.y - prev.y;
                if include_mouse && (dx != 0 || dy != 0) {
                    events.push(InputEvent {
                        qpc_ts: ts,
                        kind: InputEventKind::MouseMove { dx, dy },
                    });
                }
            }
            self.last_cursor = Some(point);
        }

        Ok(events)
    }
}

fn build_key_specs() -> Vec<KeySpec> {
    let mut out = Vec::new();

    for vk in 0x41u16..=0x5A {
        if let Some(name) = keyboard_key_name(vk) {
            out.push(KeySpec { vk, name });
        }
    }
    for vk in 0x30u16..=0x39 {
        if let Some(name) = keyboard_key_name(vk) {
            out.push(KeySpec { vk, name });
        }
    }
    for vk in 0x60u16..=0x69 {
        if let Some(name) = keyboard_key_name(vk) {
            out.push(KeySpec { vk, name });
        }
    }
    for vk in 0x70u16..=0x7B {
        if let Some(name) = keyboard_key_name(vk) {
            out.push(KeySpec { vk, name });
        }
    }

    let extra = [
        0x10u16, 0x11u16, 0x12u16, 0x20u16, 0x1Bu16, 0x09u16, 0x0Du16, 0x08u16,
        0x2Du16, 0x2Eu16, 0x24u16, 0x23u16, 0x21u16, 0x22u16, 0x13u16, 0x2Cu16,
        0x14u16, 0x90u16, 0x91u16, 0x26u16, 0x28u16, 0x25u16, 0x27u16, 0x5Bu16,
        0x5Cu16, 0x5Du16, 0x6Au16, 0x6Bu16, 0x6Du16, 0x6Eu16, 0x6Fu16,
    ];
    for vk in extra {
        if let Some(name) = keyboard_key_name(vk) {
            out.push(KeySpec { vk, name });
        }
    }

    out
}

fn cursor_pos() -> Option<POINT> {
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point).is_ok() {
            Some(point)
        } else {
            None
        }
    }
}

fn async_key_state(vk: u16) -> u16 {
    unsafe { GetAsyncKeyState(vk as i32) as u16 }
}
