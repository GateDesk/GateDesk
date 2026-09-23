use crate::client::translate;
#[cfg(windows)]
use crate::ipc::Data;
#[cfg(windows)]
use hbb_common::tokio;
use hbb_common::{allow_err, log};
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::Duration;

pub fn start_tray() {
    if crate::ui_interface::get_builtin_option(hbb_common::config::keys::OPTION_HIDE_TRAY) == "Y" {
        #[cfg(not(target_os = "macos"))]
        {
            return;
        }
    }

    #[cfg(target_os = "linux")]
    crate::server::check_zombie();

    allow_err!(make_tray());
}

/// The size the notification area asks for. Reading it beats hard-coding 16, and it is what
/// the tray scales to anyway.
#[cfg(windows)]
fn small_icon_size() -> (u32, u32) {
    use winapi::um::winuser::{GetSystemMetrics, SM_CXSMICON, SM_CYSMICON};
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
    (
        if w > 0 { w as u32 } else { 16 },
        if h > 0 { h as u32 } else { 16 },
    )
}

/// Build the tray icon the way Windows expects it, instead of going through
/// `tray_icon::Icon::from_rgba`.
///
/// That one ends in `CreateIcon`, which builds a DDB: a bitmap with no alpha channel, where
/// transparency can only be expressed by the 1bpp mask. Windows 10 and 11 composite the
/// tray icon by its alpha regardless, so it looks right there - but the Windows 7 tray does
/// not, and the fully transparent pixels (black, alpha 0) come out as a black square.
///
/// A 32bpp DIB carrying the alpha, handed over with `CreateIconIndirect`, is what every
/// Windows since Vista actually wants. `tray-icon` takes ownership of the handle and
/// destroys it when the icon is dropped.
#[cfg(windows)]
fn windows_icon_from_rgba(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
) -> hbb_common::ResultType<tray_icon::Icon> {
    use std::ptr;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::wingdi::{
        CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BI_RGB, DIB_RGB_COLORS,
    };
    use winapi::um::winuser::{CreateIconIndirect, ICONINFO};

    let (w, h) = (width as i32, height as i32);
    if w <= 0 || h <= 0 || rgba.len() < width as usize * height as usize * 4 {
        hbb_common::bail!("icon data is not {width}x{height}");
    }
    unsafe {
        // Top-down (negative height) so the rows arrive in the same order as `rgba`.
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of_val(&bmi.bmiHeader) as DWORD;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut winapi::ctypes::c_void = ptr::null_mut();
        let color = CreateDIBSection(
            ptr::null_mut(),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            ptr::null_mut(),
            0,
        );
        if color.is_null() {
            hbb_common::bail!(
                "CreateDIBSection failed: {}",
                std::io::Error::last_os_error()
            );
        }
        if !bits.is_null() {
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, rgba.len());
            for (px, out) in rgba.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                out[0] = px[2];
                out[1] = px[1];
                out[2] = px[0];
                out[3] = px[3];
            }
        }
        // All zeroes, which tells the shell to take the alpha from the colour bitmap.
        // `CreateBitmap` with a null pointer leaves the bits undefined, so pass ours.
        let stride = (width.div_ceil(32) * 4) as usize;
        let mask_bits = vec![0u8; stride * height as usize];
        let mask = CreateBitmap(w, h, 1, 1, mask_bits.as_ptr() as *const _);
        if mask.is_null() {
            DeleteObject(color as _);
            hbb_common::bail!("CreateBitmap failed: {}", std::io::Error::last_os_error());
        }
        let mut info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let hicon = CreateIconIndirect(&mut info);
        // The icon owns its copies of both bitmaps now.
        DeleteObject(mask as _);
        DeleteObject(color as _);
        if hicon.is_null() {
            hbb_common::bail!(
                "CreateIconIndirect failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(tray_icon::Icon::from_handle(hicon as isize))
    }
}

fn make_tray() -> hbb_common::ResultType<()> {
    // https://github.com/tauri-apps/tray-icon/blob/dev/examples/tao.rs
    use hbb_common::anyhow::Context;
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem},
        TrayIcon, TrayIconBuilder, TrayIconEvent as TrayEvent,
    };

    // Duplicated tray icons kept piling up through the blind spots of
    // `check_process("--tray", ..)`. https://github.com/rustdesk/rustdesk/issues/15689
    #[cfg(windows)]
    if !crate::platform::windows::try_lock_tray_single_instance() {
        log::info!("Another tray process is already running in this session, exit");
        return Ok(());
    }

    let icon;
    #[cfg(target_os = "macos")]
    {
        icon = include_bytes!("../res/mac-tray-dark-x2.png"); // use as template, so color is not important
    }
    #[cfg(not(target_os = "macos"))]
    {
        icon = include_bytes!("../res/tray-icon.ico");
    }

    let (icon_rgba, icon_width, icon_height) = {
        let image = load_icon_from_asset()
            .unwrap_or(image::load_from_memory(icon).context("Failed to open icon path")?)
            .into_rgba8();
        #[cfg(windows)]
        // Hand the tray the size it asks for. The .ico holds a much larger image, and
        // Windows 7's own downscaling is part of what went wrong there.
        let image = {
            let (w, h) = small_icon_size();
            if image.width() == w && image.height() == h {
                image
            } else {
                image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle)
            }
        };
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    #[cfg(windows)]
    let icon = windows_icon_from_rgba(icon_rgba, icon_width, icon_height)
        .context("Failed to build the tray icon")?;
    #[cfg(not(windows))]
    let icon = tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .context("Failed to open icon")?;

    let mut event_loop = EventLoopBuilder::new().build();

    let tray_menu = Menu::new();
    let hide_stop_service = crate::ui_interface::get_builtin_option(
        hbb_common::config::keys::OPTION_HIDE_STOP_SERVICE,
    ) == "Y";
    // Whether the controllable service is currently stopped. This toggles the label
    // of the dynamic menu item below: "Stop service" when running, "Enable service"
    // when stopped.
    let service_stopped = crate::ui_interface::get_option("stop-service") == "Y";
    // Dynamic item: toggles the `stop-service` flag WITHOUT killing processes or
    // exiting the tray. The main process picks the flag up on its next rendezvous
    // loop, so this takes effect live.
    let toggle_service_i = if !hide_stop_service {
        Some(MenuItem::new(
            if service_stopped {
                translate("Enable service".to_owned())
            } else {
                translate("Stop service".to_owned())
            },
            true,
            None,
        ))
    } else {
        None
    };
    // Exits the whole application (tray + main service) without persisting a
    // `stop-service` flag, so the next launch is still a controllable host.
    let exit_i = MenuItem::new(translate("Exit".to_owned()), true, None);
    let open_i = MenuItem::new(translate("Open".to_owned()), true, None);
    // Read-only local ID row: the default (headless) form has no window, so this is
    // the only place the service identity is visible without opening the main UI.
    let id_i = MenuItem::new(
        format!("{} {}", translate("ID:".to_owned()), crate::ipc::get_id()),
        false,
        None,
    );
    // The "Open" menu item is defined but intentionally NOT appended to the menu
    // (hidden). The open functionality (open_func / left-click handler below) stays
    // intact so it can be re-enabled later by simply adding `open_i` back into the
    // append list below.
    if let Some(toggle_service_i) = &toggle_service_i {
        // Add `&open_i` back here to restore the "Open" menu item.
        tray_menu
            .append_items(&[&id_i, toggle_service_i, &exit_i])
            .ok();
    } else {
        // Add `&open_i` back here to restore the "Open" menu item.
        tray_menu.append_items(&[&id_i, &exit_i]).ok();
    }
    let tooltip = |count: usize| {
        if count == 0 {
            format!(
                "{} {}",
                crate::get_app_name(),
                translate("Service is running".to_owned()),
            )
        } else {
            format!(
                "{} - {}\n{}",
                crate::get_app_name(),
                translate("Ready".to_owned()),
                translate("{".to_string() + &format!("{count}") + "} sessions"),
            )
        }
    };
    let mut _tray_icon: Arc<Mutex<Option<TrayIcon>>> = Default::default();

    let menu_channel = MenuEvent::receiver();
    let tray_channel = TrayEvent::receiver();
    #[cfg(windows)]
    let (ipc_sender, ipc_receiver) = std::sync::mpsc::channel::<Data>();

    let open_func = move || {
        if cfg!(not(feature = "flutter")) {
            // `--ui`: a plain launch is headless now, so opening the window has to be
            // asked for explicitly.
            crate::run_me::<&str>(vec!["--ui"]).ok();
            return;
        }
        #[cfg(target_os = "macos")]
        crate::platform::macos::handle_application_should_open_untitled_file();
        #[cfg(target_os = "windows")]
        {
            // Do not use "start uni link" way, it may not work on some Windows, and pop out error
            // dialog, I found on one user's desktop, but no idea why, Windows is shit.
            // Use `run_me` instead.
            // `allow_multiple_instances` in `flutter/windows/runner/main.cpp` allows only one instance without args.
            crate::run_me::<&str>(vec!["--ui"]).ok();
        }
        #[cfg(target_os = "linux")]
        {
            // Do not use "xdg-open", it won't read the config.
            if crate::dbus::invoke_new_connection(crate::get_uri_prefix()).is_err() {
                if let Ok(task) = crate::run_me::<&str>(vec!["--ui"]) {
                    crate::server::CHILD_PROCESS.lock().unwrap().push(task);
                }
            }
        }
    };

    #[cfg(windows)]
    std::thread::spawn(move || {
        start_query_session_count(ipc_sender.clone());
    });
    #[cfg(windows)]
    let mut last_click = std::time::Instant::now();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::EventLoopExtMacOS;
        event_loop.set_activation_policy(tao::platform::macos::ActivationPolicy::Accessory);
    }
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        );

        if let tao::event::Event::NewEvents(tao::event::StartCause::Init) = event {
            // for fixing https://github.com/rustdesk/rustdesk/discussions/10210#discussioncomment-14600745
            // so we start tray, but not to show it
            if crate::ui_interface::get_builtin_option(hbb_common::config::keys::OPTION_HIDE_TRAY) == "Y" {
                return;
            }
            // We create the icon once the event loop is actually running
            // to prevent issues like https://github.com/tauri-apps/tray-icon/issues/90
            let mut builder = TrayIconBuilder::new()
                .with_id(crate::get_app_name().to_lowercase())
                .with_menu(Box::new(tray_menu.clone()))
                .with_tooltip(tooltip(0))
                .with_icon(icon.clone());
            #[cfg(target_os = "macos")]
            {
                builder = builder.with_icon_as_template(true);
            }
            #[cfg(target_os = "windows")]
            {
                // Required since tray-icon 0.17
                // Fixes #15215, #15222, #15410
                builder = builder.with_menu_on_left_click(false);
            }
            let tray = builder.build();
            match tray {
                Ok(tray) => _tray_icon = Arc::new(Mutex::new(Some(tray))),
                Err(err) => {
                    log::error!("Failed to create tray icon: {}", err);
                }
            };

            // We have to request a redraw here to have the icon actually show up.
            // Tao only exposes a redraw method on the Window so we use core-foundation directly.
            #[cfg(target_os = "macos")]
            unsafe {
                use core_foundation::runloop::{CFRunLoopGetMain, CFRunLoopWakeUp};

                let rl = CFRunLoopGetMain();
                CFRunLoopWakeUp(rl);
            }
        }

        if let Ok(event) = menu_channel.try_recv() {
            if let Some(toggle_service_i) = &toggle_service_i {
                if event.id == toggle_service_i.id() {
                    // Toggle the controllable-host state WITHOUT killing processes or
                    // exiting the tray. `ipc::set_option` broadcasts `Data::Options` to
                    // the main process and writes the local config, so the rendezvous
                    // loop picks it up live. Empty value removes the flag.
                    let currently_stopped =
                        crate::ui_interface::get_option("stop-service") == "Y";
                    if currently_stopped {
                        crate::ipc::set_option("stop-service", "");
                        toggle_service_i.set_text(&translate("Stop service".to_owned()));
                    } else {
                        crate::ipc::set_option("stop-service", "Y");
                        toggle_service_i.set_text(&translate("Enable service".to_owned()));
                    }
                }
            }
            if event.id == exit_i.id() {
                // Remove the icon first: `exit_application()` ends this process with
                // `std::process::exit`, which skips the destructor that would remove
                // it, leaving a ghost icon behind.
                #[cfg(windows)]
                {
                    let _ = _tray_icon
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|t| t.set_visible(false));
                    crate::platform::windows::exit_application();
                }
                #[cfg(not(windows))]
                {
                    *control_flow = ControlFlow::Exit;
                }
            } else if event.id == open_i.id() {
                open_func();
            }
        }

        if let Ok(_event) = tray_channel.try_recv() {
            #[cfg(target_os = "windows")]
            match _event {
                TrayEvent::Click {
                    button,
                    button_state,
                    ..
                } => {
                    if button == tray_icon::MouseButton::Left
                        && button_state == tray_icon::MouseButtonState::Up
                    {
                        if last_click.elapsed() < std::time::Duration::from_secs(1) {
                            return;
                        }
                        // open_func();
                        last_click = std::time::Instant::now();
                    }
                }
                _ => {}
            }
        }

        #[cfg(windows)]
        if let Ok(data) = ipc_receiver.try_recv() {
            match data {
                Data::ControlledSessionCount(count) => {
                    _tray_icon
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|t| t.set_tooltip(Some(tooltip(count))));
                }
                _ => {}
            }
        }
    });
}

#[cfg(windows)]
#[tokio::main(flavor = "current_thread")]
async fn start_query_session_count(sender: std::sync::mpsc::Sender<Data>) {
    let mut last_count = 0;
    loop {
        if let Ok(mut c) = crate::ipc::connect(1000, "").await {
            let mut timer = crate::rustdesk_interval(tokio::time::interval(Duration::from_secs(1)));
            loop {
                tokio::select! {
                    res = c.next() => {
                        match res {
                            Err(err) => {
                                log::error!("ipc connection closed: {}", err);
                                break;
                            }

                            Ok(Some(Data::ControlledSessionCount(count))) => {
                                if count != last_count {
                                    last_count = count;
                                    sender.send(Data::ControlledSessionCount(count)).ok();
                                }
                            }
                            _ => {}
                        }
                    }

                    _ = timer.tick() => {
                        c.send(&Data::ControlledSessionCount(0)).await.ok();
                    }
                }
            }
        }
        hbb_common::sleep(1.).await;
    }
}

fn load_icon_from_asset() -> Option<image::DynamicImage> {
    let Some(path) = std::env::current_exe().map_or(None, |x| x.parent().map(|x| x.to_path_buf()))
    else {
        return None;
    };
    #[cfg(target_os = "macos")]
    let path = path.join("../Frameworks/App.framework/Resources/flutter_assets/assets/icon.png");
    #[cfg(windows)]
    let path = path.join(r"data\flutter_assets\assets\icon.png");
    #[cfg(target_os = "linux")]
    let path = path.join(r"data/flutter_assets/assets/icon.png");
    if path.exists() {
        if let Ok(image) = image::open(path) {
            return Some(image);
        }
    }
    None
}
