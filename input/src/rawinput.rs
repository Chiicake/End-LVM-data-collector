use std::collections::VecDeque;
use std::io;

use collector_core::InputEvent;

#[cfg(windows)]
use std::mem::size_of;
#[cfg(windows)]
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
#[cfg(windows)]
use std::thread::{self, JoinHandle};

#[cfg(windows)]
use collector_core::{InputEventKind, MouseButton, QpcTimestamp};

#[cfg(windows)]
use crate::keyboard_key_name;

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::System::Performance::QueryPerformanceCounter;
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentThreadId;
#[cfg(windows)]
use windows::Win32::UI::Input::RawInput::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RI_KEY_BREAK, RI_MOUSE_LEFT_BUTTON_DOWN,
    RI_MOUSE_LEFT_BUTTON_UP, RI_MOUSE_MIDDLE_BUTTON_DOWN, RI_MOUSE_MIDDLE_BUTTON_UP,
    RI_MOUSE_RIGHT_BUTTON_DOWN, RI_MOUSE_RIGHT_BUTTON_UP, RI_MOUSE_WHEEL, RI_MOUSE_X_BUTTON_DOWN,
    RI_MOUSE_X_BUTTON_UP, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    GetForegroundWindow, PostThreadMessageW, RegisterClassW, SetWindowLongPtrW,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, HMENU, MSG, WM_INPUT,
    WM_NCDESTROY, WM_QUIT, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

#[cfg(windows)]
pub struct RawInputCollectorImpl {
    rx: Receiver<InputEvent>,
    thread_id: u32,
    handle: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl RawInputCollectorImpl {
    pub fn new(target_hwnd: Option<isize>) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        let handle = thread::spawn(move || run_message_loop(tx, ready_tx, target_hwnd));

    let thread_id = ready_rx
        .recv()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "rawinput thread failed"))??;

        Ok(Self {
            rx,
            thread_id,
            handle: Some(handle),
        })
    }

    pub fn drain_into(&mut self, buffer: &mut VecDeque<InputEvent>) -> io::Result<()> {
        loop {
            match self.rx.try_recv() {
                Ok(event) => buffer.push_back(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "rawinput channel closed",
                    ))
                }
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for RawInputCollectorImpl {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(windows)]
struct RawInputContext {
    sender: Sender<InputEvent>,
    target_hwnd: Option<HWND>,
}

#[cfg(windows)]
fn run_message_loop(
    tx: Sender<InputEvent>,
    ready_tx: Sender<io::Result<u32>>,
    target_hwnd: Option<isize>,
) {
    unsafe {
        let class_name = to_wide("collector_rawinput_window");
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: windows::Win32::Foundation::HINSTANCE(0),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        let atom = RegisterClassW(&wnd_class);
        if atom == 0 {
            let _ = ready_tx.send(Err(io::Error::new(
                io::ErrorKind::Other,
                "RegisterClassW failed",
            )));
            return;
        }

        let hwnd = CreateWindowExW(
            Default::default(),
            windows::core::PCWSTR(class_name.as_ptr()),
            windows::core::PCWSTR(class_name.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            HWND(0),
            HMENU(0),
            windows::Win32::Foundation::HINSTANCE(0),
            std::ptr::null(),
        );
        if hwnd.0 == 0 {
            let _ = ready_tx.send(Err(io::Error::new(
                io::ErrorKind::Other,
                "CreateWindowExW failed",
            )));
            return;
        }

        let ctx = RawInputContext {
            sender: tx,
            target_hwnd: target_hwnd.map(|hwnd| HWND(hwnd)),
        };
        let tx_box = Box::new(ctx);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(tx_box) as isize);

        let devices = [
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
        ];
        if let Err(err) = RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32)
            .ok()
            .map_err(map_win_err)
        {
            let _ = ready_tx.send(Err(err));
            return;
        }

        let thread_id = GetCurrentThreadId();
        let _ = ready_tx.send(Ok(thread_id));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(0), 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT => {
            if let Err(err) = handle_raw_input(hwnd, lparam) {
                let _ = err;
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RawInputContext;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(windows)]
fn handle_raw_input(hwnd: HWND, lparam: LPARAM) -> io::Result<()> {
    unsafe {
        let ctx = context_from_hwnd(hwnd)?;
        if let Some(target) = ctx.target_hwnd {
            if GetForegroundWindow() != target {
                return Ok(());
            }
        }
        let mut size = 0u32;
        GetRawInputData(
            HRAWINPUT(lparam.0 as isize),
            RID_INPUT,
            None,
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        );
        if size == 0 {
            return Ok(());
        }
        let mut buffer = vec![0u8; size as usize];
        let read = GetRawInputData(
            HRAWINPUT(lparam.0 as isize),
            RID_INPUT,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        );
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "GetRawInputData failed",
            ));
        }

        let raw = &*(buffer.as_ptr() as *const RAWINPUT);
        let timestamp = qpc_now()?;
        let sender = ctx.sender.clone();

        match raw.header.dwType {
            RIM_TYPEKEYBOARD => {
                let keyboard = unsafe { raw.data.keyboard };
                let is_down = (keyboard.Flags & RI_KEY_BREAK) == 0;
                let vkey = keyboard.VKey;
                if vkey == 255 {
                    return Ok(());
                }
                if let Some(name) = keyboard_key_name(vkey) {
                    let event = InputEvent {
                        qpc_ts: timestamp,
                        kind: if is_down {
                            InputEventKind::KeyDown {
                                key: name.to_string(),
                            }
                        } else {
                            InputEventKind::KeyUp {
                                key: name.to_string(),
                            }
                        },
                    };
                    let _ = sender.send(event);
                }
            }
            RIM_TYPEMOUSE => {
                let mouse = unsafe { raw.data.mouse };
                if mouse.lLastX != 0 || mouse.lLastY != 0 {
                    let _ = sender.send(InputEvent {
                        qpc_ts: timestamp,
                        kind: InputEventKind::MouseMove {
                            dx: mouse.lLastX,
                            dy: mouse.lLastY,
                        },
                    });
                }
                let flags = mouse.usButtonFlags;
                emit_button(flags, RI_MOUSE_LEFT_BUTTON_DOWN, MouseButton::Left, true, timestamp, &sender);
                emit_button(flags, RI_MOUSE_LEFT_BUTTON_UP, MouseButton::Left, false, timestamp, &sender);
                emit_button(flags, RI_MOUSE_RIGHT_BUTTON_DOWN, MouseButton::Right, true, timestamp, &sender);
                emit_button(flags, RI_MOUSE_RIGHT_BUTTON_UP, MouseButton::Right, false, timestamp, &sender);
                emit_button(flags, RI_MOUSE_MIDDLE_BUTTON_DOWN, MouseButton::Middle, true, timestamp, &sender);
                emit_button(flags, RI_MOUSE_MIDDLE_BUTTON_UP, MouseButton::Middle, false, timestamp, &sender);
                emit_x_buttons(flags, mouse.usButtonData, true, timestamp, &sender);
                emit_x_buttons(flags, mouse.usButtonData, false, timestamp, &sender);
                if (flags & RI_MOUSE_WHEEL) != 0 {
                    let delta = (mouse.usButtonData as i16) as i32;
                    let _ = sender.send(InputEvent {
                        qpc_ts: timestamp,
                        kind: InputEventKind::MouseWheel { delta },
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(windows)]
fn emit_button(
    flags: u16,
    mask: u16,
    button: MouseButton,
    is_down: bool,
    ts: QpcTimestamp,
    sender: &Sender<InputEvent>,
) {
    if (flags & mask) != 0 {
        let _ = sender.send(InputEvent {
            qpc_ts: ts,
            kind: InputEventKind::MouseButton { button, is_down },
        });
    }
}

#[cfg(windows)]
fn emit_x_buttons(
    flags: u16,
    button_data: u16,
    is_down: bool,
    ts: QpcTimestamp,
    sender: &Sender<InputEvent>,
) {
    let mask = if is_down {
        RI_MOUSE_X_BUTTON_DOWN
    } else {
        RI_MOUSE_X_BUTTON_UP
    };
    if (flags & mask) == 0 {
        return;
    }
    let button = if (button_data & 0x0002) != 0 {
        MouseButton::X2
    } else {
        MouseButton::X1
    };
    let _ = sender.send(InputEvent {
        qpc_ts: ts,
        kind: InputEventKind::MouseButton { button, is_down },
    });
}

#[cfg(windows)]
fn context_from_hwnd(hwnd: HWND) -> io::Result<&'static RawInputContext> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RawInputContext;
        if ptr.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "missing rawinput context",
            ));
        }
        Ok(&*ptr)
    }
}

#[cfg(windows)]
fn qpc_now() -> io::Result<QpcTimestamp> {
    unsafe {
        let mut counter = 0i64;
        QueryPerformanceCounter(&mut counter).map_err(map_win_err)?;
        Ok(counter as u64)
    }
}

#[cfg(windows)]
fn map_win_err(err: windows::core::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{:?}", err))
}

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
pub struct RawInputCollectorImpl;

#[cfg(not(windows))]
impl RawInputCollectorImpl {
    pub fn new(_target_hwnd: Option<isize>) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "RawInput requires Windows",
        ))
    }

    pub fn drain_into(&mut self, _buffer: &mut VecDeque<InputEvent>) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "RawInput requires Windows",
        ))
    }
}
