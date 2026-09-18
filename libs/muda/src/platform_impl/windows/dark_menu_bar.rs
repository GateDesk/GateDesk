// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// this is a port of combination of https://github.com/hrydgard/ppsspp/blob/master/Windows/W32Util/UAHMenuBar.cpp and https://github.com/ysc3839/win32-darkmode/blob/master/win32-darkmode/DarkMode.h

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::cell::Cell;

use once_cell::sync::Lazy;
use windows_sys::{
    s,
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::{GetProcAddress, LoadLibraryA},
        UI::{
            Accessibility::HIGHCONTRASTA,
            Controls::*,
            WindowsAndMessaging::{
                GetClientRect, GetMenuBarInfo, GetMenuItemInfoW, GetWindowRect,
                SystemParametersInfoA, HMENU, MENUBARINFO, MENUITEMINFOW, MIIM_STRING, OBJID_MENU,
                SPI_GETHIGHCONTRAST, WM_NCACTIVATE, WM_NCPAINT,
            },
        },
    },
};

pub const WM_UAHDRAWMENU: u32 = 0x0091;
pub const WM_UAHDRAWMENUITEM: u32 = 0x0092;

#[repr(C)]
struct UAHMENUITEMMETRICS0 {
    cx: u32,
    cy: u32,
}

#[repr(C)]
struct UAHMENUITEMMETRICS {
    rgsizeBar: [UAHMENUITEMMETRICS0; 2],
    rgsizePopup: [UAHMENUITEMMETRICS0; 4],
}

#[repr(C)]
struct UAHMENUPOPUPMETRICS {
    rgcx: [u32; 4],
    fUpdateMaxWidths: u32,
}

#[repr(C)]
struct UAHMENU {
    hmenu: HMENU,
    hdc: HDC,
    dwFlags: u32,
}
#[repr(C)]
struct UAHMENUITEM {
    iPosition: u32,
    umim: UAHMENUITEMMETRICS,
    umpm: UAHMENUPOPUPMETRICS,
}
#[repr(C)]
struct UAHDRAWMENUITEM {
    dis: DRAWITEMSTRUCT,
    um: UAHMENU,
    umi: UAHMENUITEM,
}

#[derive(Debug)]
struct Win32Brush(Cell<HBRUSH>);

impl Win32Brush {
    const fn null() -> Win32Brush {
        Self(Cell::new(0 as _))
    }

    fn get_or_set(&self, color: u32) -> HBRUSH {
        if self.0.get().is_null() {
            self.0.set(unsafe { CreateSolidBrush(color) });
        }
        self.0.get()
    }
}

impl Drop for Win32Brush {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.0.get()) };
    }
}

fn background_brush() -> HBRUSH {
    thread_local! {
        static BACKGROUND_BRUSH: Win32Brush = const { Win32Brush::null() };
    }
    const BACKGROUND_COLOR: u32 = 2829099;
    BACKGROUND_BRUSH.with(|brush| brush.get_or_set(BACKGROUND_COLOR))
}

fn selected_background_brush() -> HBRUSH {
    thread_local! {
        static SELECTED_BACKGROUND_BRUSH: Win32Brush = const { Win32Brush::null() };
    }
    const SELECTED_BACKGROUND_COLOR: u32 = 4276545;
    SELECTED_BACKGROUND_BRUSH.with(|brush| brush.get_or_set(SELECTED_BACKGROUND_COLOR))
}

/// Draws a dark menu bar if needed and returns whether it draws it or not
pub fn draw(hwnd: super::Hwnd, msg: u32, _wparam: WPARAM, lparam: LPARAM) {
    match msg {
        // draw over the annoying white line blow menubar
        // ref: https://github.com/notepad-plus-plus/notepad-plus-plus/pull/9985
        WM_NCACTIVATE | WM_NCPAINT => {
            let mut mbi = MENUBARINFO {
                cbSize: std::mem::size_of::<MENUBARINFO>() as _,
                ..unsafe { std::mem::zeroed() }
            };
            unsafe { GetMenuBarInfo(hwnd as _, OBJID_MENU, 0, &mut mbi) };

            let mut client_rc: RECT = unsafe { std::mem::zeroed() };
            unsafe {
                GetClientRect(hwnd as _, &mut client_rc);
                MapWindowPoints(
                    hwnd as _,
                    std::ptr::null_mut(),
                    &mut client_rc as *mut _ as *mut _,
                    2,
                );
            };

            let mut window_rc: RECT = unsafe { std::mem::zeroed() };
            unsafe { GetWindowRect(hwnd as _, &mut window_rc) };

            unsafe { OffsetRect(&mut client_rc, -window_rc.left, -window_rc.top) };

            let mut annoying_rc = client_rc;
            annoying_rc.bottom = annoying_rc.top;
            annoying_rc.top -= 1;

            unsafe {
                let hdc = GetWindowDC(hwnd as _);
                FillRect(hdc, &annoying_rc, background_brush());
                ReleaseDC(hwnd as _, hdc);
            }
        }

        // draw menu bar background
        WM_UAHDRAWMENU => {
            let pudm = lparam as *const UAHMENU;

            // get the menubar rect
            let rc = {
                let mut mbi = MENUBARINFO {
                    cbSize: std::mem::size_of::<MENUBARINFO>() as _,
                    ..unsafe { std::mem::zeroed() }
                };
                unsafe { GetMenuBarInfo(hwnd as _, OBJID_MENU, 0, &mut mbi) };

                let mut window_rc: RECT = unsafe { std::mem::zeroed() };
                unsafe { GetWindowRect(hwnd as _, &mut window_rc) };

                let mut rc = mbi.rcBar;
                // the rcBar is offset by the window rect
                unsafe { OffsetRect(&mut rc, -window_rc.left, -window_rc.top) };
                rc.top -= 1;
                rc
            };

            unsafe { FillRect((*pudm).hdc, &rc, background_brush()) };
        }

        // draw menu bar items
        WM_UAHDRAWMENUITEM => {
            let pudmi = lparam as *mut UAHDRAWMENUITEM;

            // get the menu item string
            let (label, cch) = {
                let mut label = Vec::<u16>::with_capacity(256);
                let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
                info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as _;
                info.fMask = MIIM_STRING;
                info.dwTypeData = label.as_mut_ptr();
                info.cch = (std::mem::size_of_val(&label) / 2 - 1) as _;
                unsafe {
                    GetMenuItemInfoW(
                        (*pudmi).um.hmenu,
                        (*pudmi).umi.iPosition,
                        true.into(),
                        &mut info,
                    )
                };
                (label, info.cch)
            };

            // get the item state for drawing
            let mut dw_flags = DT_CENTER | DT_SINGLELINE | DT_VCENTER;
            let mut i_text_state_id = 0;
            let mut i_background_state_id = 0;

            unsafe {
                if (((*pudmi).dis.itemState & ODS_INACTIVE)
                    | ((*pudmi).dis.itemState & ODS_DEFAULT))
                    != 0
                {
                    // normal display
                    i_text_state_id = MPI_NORMAL;
                    i_background_state_id = MPI_NORMAL;
                }
                if (*pudmi).dis.itemState & ODS_HOTLIGHT != 0 {
                    // hot tracking
                    i_text_state_id = MPI_HOT;
                    i_background_state_id = MPI_HOT;
                }
                if (*pudmi).dis.itemState & ODS_SELECTED != 0 {
                    // clicked -- MENU_POPUPITEM has no state for this, though MENU_BARITEM does
                    i_text_state_id = MPI_HOT;
                    i_background_state_id = MPI_HOT;
                }
                if ((*pudmi).dis.itemState & ODS_GRAYED) != 0
                    || ((*pudmi).dis.itemState & ODS_DISABLED) != 0
                {
                    // disabled / grey text
                    i_text_state_id = MPI_DISABLED;
                    i_background_state_id = MPI_DISABLED;
                }
                if ((*pudmi).dis.itemState & ODS_NOACCEL) != 0 {
                    dw_flags |= DT_HIDEPREFIX;
                }

                let bg_brush = match i_background_state_id {
                    MPI_HOT => selected_background_brush(),
                    _ => background_brush(),
                };

                FillRect((*pudmi).um.hdc, &(*pudmi).dis.rcItem, bg_brush);

                const TEXT_COLOR: u32 = 16777215;
                const DISABLED_TEXT_COLOR: u32 = 7171437;

                let text_brush = match i_text_state_id {
                    MPI_DISABLED => DISABLED_TEXT_COLOR,
                    _ => TEXT_COLOR,
                };

                SetBkMode((*pudmi).um.hdc, 0);
                SetTextColor((*pudmi).um.hdc, text_brush);
                DrawTextW(
                    (*pudmi).um.hdc,
                    label.as_ptr(),
                    cch as _,
                    &mut (*pudmi).dis.rcItem,
                    dw_flags,
                );
            }
        }

        _ => {}
    };
}

pub fn should_use_dark_mode(hwnd: super::Hwnd) -> bool {
    dark_mode_api_available()
        && should_apps_use_dark_mode()
        && !is_high_contrast()
        && is_dark_mode_allowed_for_window(hwnd as _)
}

/// Whether this Windows has the dark mode API at all.
///
/// The two entry points below are undocumented and reached by ordinal, and those ordinals
/// only exist from Windows 10 1809 (build 17763) on. On older systems the same numbers
/// belong to other functions - Windows 7's `uxtheme.dll` answers ordinal 132 with
/// something that is not `ShouldAppsUseDarkMode` - and calling one of those as the dark
/// mode function (which takes no arguments and returns a `bool`) jumps into whatever it
/// finds and takes the process with it. Hence the build number, checked before any of the
/// ordinals is looked up at all.
fn dark_mode_api_available() -> bool {
    static AVAILABLE: Lazy<bool> = Lazy::new(|| windows_build_number() >= 17763);
    *AVAILABLE
}

/// The build number of the running Windows, or 0 when it cannot be read.
///
/// `RtlGetVersion` and not `GetVersionEx`: the latter is filtered through the application
/// manifest, and gets 8.1 and 10 applications told that they are 6.2.
fn windows_build_number() -> u32 {
    /// The name `GetProcAddress` is asked for, null terminated, as its caller in `util`
    /// passes names.
    const RTL_GET_VERSION: &str = "RtlGetVersion\0";

    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd: [u16; 128],
    }
    type RtlGetVersion = unsafe extern "system" fn(*mut OsVersionInfo) -> i32;

    static HNTDLL: Lazy<isize> = Lazy::new(|| unsafe { LoadLibraryA(s!("ntdll.dll")) as _ });
    static BUILD: Lazy<u32> = Lazy::new(|| unsafe {
        if *HNTDLL == 0 {
            return 0;
        }
        let Some(rtl_get_version) = GetProcAddress(*HNTDLL as *mut _, RTL_GET_VERSION.as_ptr())
        else {
            return 0;
        };
        let rtl_get_version: RtlGetVersion = std::mem::transmute(rtl_get_version);
        let mut version: OsVersionInfo = std::mem::zeroed();
        version.size = std::mem::size_of::<OsVersionInfo>() as u32;
        if rtl_get_version(&mut version) != 0 {
            return 0;
        }
        version.build
    });
    *BUILD
}

static HUXTHEME: Lazy<isize> = Lazy::new(|| unsafe { LoadLibraryA(s!("uxtheme.dll")) as _ });

fn should_apps_use_dark_mode() -> bool {
    const UXTHEME_SHOULDAPPSUSEDARKMODE_ORDINAL: u16 = 132;
    type ShouldAppsUseDarkMode = unsafe extern "system" fn() -> bool;
    static SHOULD_APPS_USE_DARK_MODE: Lazy<Option<ShouldAppsUseDarkMode>> = Lazy::new(|| unsafe {
        if *HUXTHEME == 0 {
            return None;
        }

        GetProcAddress(
            (*HUXTHEME) as *mut _,
            UXTHEME_SHOULDAPPSUSEDARKMODE_ORDINAL as usize as *mut _,
        )
        .map(|handle| std::mem::transmute(handle))
    });

    SHOULD_APPS_USE_DARK_MODE
        .map(|should_apps_use_dark_mode| unsafe { (should_apps_use_dark_mode)() })
        .unwrap_or(false)
}

fn is_dark_mode_allowed_for_window(hwnd: HWND) -> bool {
    const UXTHEME_ISDARKMODEALLOWEDFORWINDOW_ORDINAL: u16 = 137;
    type IsDarkModeAllowedForWindow = unsafe extern "system" fn(HWND) -> bool;
    static IS_DARK_MODE_ALLOWED_FOR_WINDOW: Lazy<Option<IsDarkModeAllowedForWindow>> =
        Lazy::new(|| unsafe {
            if *HUXTHEME == 0 {
                return None;
            }

            GetProcAddress(
                (*HUXTHEME) as *mut _,
                UXTHEME_ISDARKMODEALLOWEDFORWINDOW_ORDINAL as usize as *mut _,
            )
            .map(|handle| std::mem::transmute(handle))
        });

    if let Some(_is_dark_mode_allowed_for_window) = *IS_DARK_MODE_ALLOWED_FOR_WINDOW {
        unsafe { _is_dark_mode_allowed_for_window(hwnd) }
    } else {
        false
    }
}

fn is_high_contrast() -> bool {
    const HCF_HIGHCONTRASTON: u32 = 1;

    let mut hc = HIGHCONTRASTA {
        cbSize: 0,
        dwFlags: Default::default(),
        lpszDefaultScheme: std::ptr::null_mut(),
    };

    let ok = unsafe {
        SystemParametersInfoA(
            SPI_GETHIGHCONTRAST,
            std::mem::size_of_val(&hc) as _,
            &mut hc as *mut _ as _,
            Default::default(),
        )
    };

    ok != 0 && (HCF_HIGHCONTRASTON & hc.dwFlags) != 0
}
