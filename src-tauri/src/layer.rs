//! Where the widget sits in the window stack, and making sure it never keeps
//! keyboard focus.
//!
//! By default it lives on the desktop layer, behind every other window; the
//! "above other windows" setting pins it on top instead. Either way it must
//! not take the keyboard: the widget has nothing to type into, and an active
//! window that refuses focus leaves only system shortcuts working.
//!
//! Tauri's `alwaysOnBottom` / `alwaysOnTop` don't hold on their own on Windows
//! (activation and the shell can still reorder the window), and WebView2's
//! out-of-process content can activate the window on click despite
//! `WS_EX_NOACTIVATE`. So on Windows the window procedure is subclassed to pin
//! every z-order change, and to hand activation straight back to the window
//! that had it.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::WebviewWindow;

static ABOVE: AtomicBool = AtomicBool::new(false);

/// Applies the layer and focus rules. Safe to call repeatedly.
pub fn apply(window: &WebviewWindow, above: bool) {
    ABOVE.store(above, Ordering::SeqCst);
    let _ = window.set_always_on_bottom(!above);
    let _ = window.set_always_on_top(above);
    platform::install(window);
}

/// Shows the window without activating it.
pub fn show(window: &WebviewWindow, above: bool) {
    apply(window, above);
    platform::show_inactive(window);
}

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::Once;

    use tauri::WebviewWindow;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindow, PostMessageW, SetForegroundWindow,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, HWND_BOTTOM, HWND_TOPMOST,
        MA_NOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WA_INACTIVE, WINDOWPOS,
        WINEVENT_OUTOFCONTEXT, WM_ACTIVATE, WM_APP, WM_MOUSEACTIVATE, WM_WINDOWPOSCHANGING, WS_EX_APPWINDOW,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    use super::ABOVE;

    const SUBCLASS_ID: usize = 0x5749_4447; // "WIDG"
    /// Posted to ourselves so the hand-back happens after activation completes.
    const WM_GIVE_BACK_FOCUS: u32 = WM_APP + 0x57;

    /// The last foreground window that belongs to another process: where the
    /// keyboard goes back to if the widget ever gets activated. `WM_ACTIVATE`
    /// can't tell us, because it doesn't name windows of other processes.
    static LAST_FOREIGN_FOREGROUND: AtomicIsize = AtomicIsize::new(0);
    static WATCH_FOREGROUND: Once = Once::new();

    unsafe extern "system" fn on_foreground(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if hwnd.is_null() {
            return;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid != unsafe { GetCurrentProcessId() } {
            LAST_FOREIGN_FOREGROUND.store(hwnd as isize, Ordering::SeqCst);
        }
    }

    fn insert_after() -> HWND {
        if ABOVE.load(Ordering::SeqCst) {
            HWND_TOPMOST
        } else {
            HWND_BOTTOM
        }
    }

    unsafe extern "system" fn keep_layer(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_WINDOWPOSCHANGING => {
                // SAFETY: for this message lparam points at a valid WINDOWPOS owned by the caller.
                let pos = unsafe { &mut *(lparam as *mut WINDOWPOS) };
                if pos.flags & SWP_NOZORDER == 0 {
                    pos.hwndInsertAfter = insert_after();
                }
                pos.flags |= SWP_NOACTIVATE;
            }
            WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
            WM_ACTIVATE if (wparam & 0xFFFF) as u32 != WA_INACTIVE => {
                unsafe { PostMessageW(hwnd, WM_GIVE_BACK_FOCUS, 0, 0) };
            }
            WM_GIVE_BACK_FOCUS => {
                let previous = LAST_FOREIGN_FOREGROUND.load(Ordering::SeqCst) as HWND;
                unsafe {
                    if IsWindow(previous) != 0 {
                        SetForegroundWindow(previous);
                    } else if GetFocus() == hwnd {
                        SetFocus(std::ptr::null_mut());
                    }
                }
                return 0;
            }
            _ => {}
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    fn hwnd(window: &WebviewWindow) -> Option<HWND> {
        window.hwnd().ok().map(|h| h.0 as HWND)
    }

    pub fn install(window: &WebviewWindow) {
        // Runs on the main thread (setup), which has the message loop the hook needs.
        WATCH_FOREGROUND.call_once(|| unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            // Seed it with whatever is in front right now.
            on_foreground(std::ptr::null_mut(), EVENT_SYSTEM_FOREGROUND, GetForegroundWindow(), 0, 0, 0, 0);
        });
        let Some(hwnd) = hwnd(window) else { return };
        // SAFETY: plain Win32 calls on our own top-level window; the subclass
        // procedure is a static function, so it outlives the window.
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let ex = (ex | WS_EX_TOOLWINDOW as isize | WS_EX_NOACTIVATE as isize) & !(WS_EX_APPWINDOW as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
            // Same id again just updates the existing subclass, so this is idempotent.
            SetWindowSubclass(hwnd, Some(keep_layer), SUBCLASS_ID, 0);
            SetWindowPos(hwnd, insert_after(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    pub fn show_inactive(window: &WebviewWindow) {
        let Some(hwnd) = hwnd(window) else { return };
        // SAFETY: shows our own window without activating it.
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(hwnd, insert_after(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn install(_window: &tauri::WebviewWindow) {}

    pub fn show_inactive(window: &tauri::WebviewWindow) {
        let _ = window.show();
    }
}
