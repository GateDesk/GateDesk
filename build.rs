#[cfg(windows)]
fn build_windows() {
    let file = "src/platform/windows.cc";
    let file2 = "src/platform/windows_delete_test_cert.cc";
    cc::Build::new().file(file).file(file2).compile("windows");
    println!("cargo:rustc-link-lib=WtsApi32");
    println!("cargo:rerun-if-changed={}", file);
    println!("cargo:rerun-if-changed={}", file2);
}

/// Place `sciter.dll` next to the built executable.
///
/// The Sciter-based UI (`ui::start`) locates the runtime either in PATH or in the
/// same directory as the exe (`exe.parent()`), never relative to the working
/// directory. A dev build leaves the exe in `target/<profile>` with no dll beside
/// it, so a `gatedesk.exe --ui` launch would panic with "sciter.dll was not found".
/// Copying the dll onto the output directory keeps `--ui` (and the rest of the
/// Sciter UI) working straight out of `cargo build --release`.
///
/// Candidate sources, in order of preference: the package dir, then its parent
/// (the repo commonly keeps `sciter.dll` one level above the crate).
///
/// Only a runtime whose architecture matches the target is shipped: the repo keeps a
/// single `sciter.dll` (x64), so an x86 build would otherwise inherit a dll it cannot
/// load and fail at UI startup for a reason that has nothing to do with the build.
/// `sciter-<arch>.dll` is looked for first, because the two architectures have to live
/// side by side and cannot share the bare name.
#[cfg(windows)]
fn ship_sciter_dll() {
    use std::fs;
    use std::path::Path;

    let Some((target_arch, target_machine)) = target_arch() else {
        return;
    };
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok();
    let mut candidates = Vec::new();
    if let Some(m) = &manifest {
        let names = [
            format!("sciter-{}.dll", target_arch),
            "sciter.dll".to_owned(),
            "sciter1.dll".to_owned(),
            "sciter2.dll".to_owned(),
        ];
        for name in &names {
            candidates.push(Path::new(m).join(name));
            candidates.push(Path::new(m).join("..").join(name));
        }
    }
    let Some(out_dir) = std::env::var("OUT_DIR").ok() else {
        return;
    };
    // OUT_DIR = <target>/<profile>/build/<pkg>/out, so the exe dir is three levels up.
    let exe_dir = Path::new(&out_dir).join("..").join("..").join("..");
    let dest = exe_dir.join("sciter.dll");
    if dest.exists() {
        // A dll is already shipped (fresh manual copy or an installed bundle);
        // don't clobber it.
        return;
    }
    for src in candidates {
        if !src.exists() {
            continue;
        }
        match pe_machine(&src) {
            Some(machine) if machine == target_machine => {
                if fs::copy(&src, &dest).is_ok() && dest.exists() {
                    println!("cargo:rerun-if-changed={}", src.display());
                }
                return;
            }
            Some(machine) => println!(
                "cargo:warning={} holds the {} runtime while this build targets {}; not \
                 shipping it. Put an {} sciter.dll next to {} to run the UI.",
                src.display(),
                pe_machine_name(machine),
                pe_machine_name(target_machine),
                pe_machine_name(target_machine),
                dest.display()
            ),
            None => println!(
                "cargo:warning={} is not a readable PE image; not shipping it.",
                src.display()
            ),
        }
    }
}

/// Product architecture name (the short one we build and ship by) and the PE `Machine`
/// value the Sciter runtime must carry to be loadable by this build.
#[cfg(windows)]
fn target_arch() -> Option<(&'static str, u16)> {
    match std::env::var("CARGO_CFG_TARGET_ARCH").ok()?.as_str() {
        "x86" => Some(("x86", 0x014c)),       // IMAGE_FILE_MACHINE_I386
        "x86_64" => Some(("x64", 0x8664)),    // IMAGE_FILE_MACHINE_AMD64
        "aarch64" => Some(("arm64", 0xaa64)), // IMAGE_FILE_MACHINE_ARM64
        _ => None,
    }
}

/// PE `Machine` value of a dll, `None` when the file is not a readable PE image.
#[cfg(windows)]
fn pe_machine(path: &std::path::Path) -> Option<u16> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(0x3c)).ok()?; // e_lfanew
    let mut offset = [0u8; 4];
    file.read_exact(&mut offset).ok()?;
    file.seek(SeekFrom::Start(u32::from_le_bytes(offset) as u64))
        .ok()?;
    let mut header = [0u8; 6]; // signature + Machine
    file.read_exact(&mut header).ok()?;
    if &header[..4] != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes([header[4], header[5]]))
}

#[cfg(windows)]
fn pe_machine_name(machine: u16) -> &'static str {
    match machine {
        0x014c => "x86",
        0x8664 => "x64",
        0xaa64 => "arm64",
        _ => "unknown-arch",
    }
}

#[cfg(target_os = "macos")]
fn build_mac() {
    let file = "src/platform/macos.mm";
    let mut b = cc::Build::new();
    if let Ok(os_version::OsVersion::MacOS(v)) = os_version::detect() {
        let v = v.version;
        if v.contains("10.14") {
            b.flag("-DNO_InputMonitoringAuthStatus=1");
        }
    }
    b.flag("-std=c++17").file(file).compile("macos");
    println!("cargo:rerun-if-changed={}", file);
}

#[cfg(all(windows, feature = "inline"))]
fn build_manifest() {
    use std::io::Write;
    if std::env::var("PROFILE").unwrap() == "release" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("res/icon.ico")
            .set_language(winapi::um::winnt::MAKELANGID(
                winapi::um::winnt::LANG_ENGLISH,
                winapi::um::winnt::SUBLANG_ENGLISH_US,
            ))
            .set_manifest_file("res/manifest.xml");
        match res.compile() {
            Err(e) => {
                write!(std::io::stderr(), "{}", e).unwrap();
                std::process::exit(1);
            }
            Ok(_) => {}
        }
    }
}

fn install_android_deps() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os != "android" {
        return;
    }
    let mut target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if target_arch == "x86_64" {
        target_arch = "x64".to_owned();
    } else if target_arch == "x86" {
        target_arch = "x86".to_owned();
    } else if target_arch == "aarch64" {
        target_arch = "arm64".to_owned();
    } else {
        target_arch = "arm".to_owned();
    }
    let target = format!("{}-android", target_arch);
    let vcpkg_root = std::env::var("VCPKG_ROOT").unwrap();
    let mut path: std::path::PathBuf = vcpkg_root.into();
    if let Ok(vcpkg_root) = std::env::var("VCPKG_INSTALLED_ROOT") {
        path = vcpkg_root.into();
    } else {
        path.push("installed");
    }
    path.push(target);
    println!(
        "cargo:rustc-link-search={}",
        path.join("lib").to_str().unwrap()
    );
    println!("cargo:rustc-link-lib=ndk_compat");
    println!("cargo:rustc-link-lib=c++");
    println!("cargo:rustc-link-lib=OpenSLES");
}

fn main() {
    hbb_common::gen_version();
    install_android_deps();
    #[cfg(all(windows, feature = "inline"))]
    build_manifest();
    #[cfg(windows)]
    build_windows();
    #[cfg(windows)]
    ship_sciter_dll();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os == "macos" {
        #[cfg(target_os = "macos")]
        build_mac();
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
