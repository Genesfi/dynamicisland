#![allow(unsafe_op_in_unsafe_fn)]

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        UI::{
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
};

pub const WM_TRAY_MSG: u32 = WM_APP + 2;
pub const WM_CONFIG_CHANGED: u32 = WM_APP + 3;

pub const ID_TRAY_SETTINGS: usize = 2001;
pub const ID_TRAY_STARTUP: usize = 2002;
pub const ID_TRAY_EXIT: usize = 2099;

pub unsafe fn add_tray_icon(hwnd: HWND) -> Result<()> {
    let mut tip = [0u16; 128];
    let tip_str = "Dynamic Island (Windows 11 Native)";
    for (i, c) in tip_str.encode_utf16().enumerate().take(127) {
        tip[i] = c;
    }

    let hicon = crate::icons::create_dynamic_island_icon(16)
        .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());

    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY_MSG,
        hIcon: hicon,
        szTip: tip,
        ..Default::default()
    };

    Shell_NotifyIconW(NIM_ADD, &nid).ok()
}

pub unsafe fn remove_tray_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

pub unsafe fn show_tray_menu(hwnd: HWND) -> Result<()> {
    let hmenu = CreatePopupMenu()?;

    AppendMenuW(hmenu, MF_STRING, ID_TRAY_SETTINGS, w!("Settings..."))?;

    let is_auto = crate::config::is_autostart_enabled();
    let check_flag = if is_auto { MF_CHECKED } else { MF_UNCHECKED };
    AppendMenuW(hmenu, MF_STRING | check_flag, ID_TRAY_STARTUP, w!("Start with Windows"))?;

    AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null())?;
    AppendMenuW(hmenu, MF_STRING, ID_TRAY_EXIT, w!("Exit"))?;

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(
        hmenu,
        TPM_RIGHTALIGN | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        None,
        hwnd,
        None,
    );
    let _ = DestroyMenu(hmenu);

    Ok(())
}
