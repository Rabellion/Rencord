//! Which capture API each operating system is supposed to use.
//!
//! Linux probes the session bus and the PipeWire socket. Opening a ScreenCast
//! session still needs a portal consent dialog, so a headless machine keeps
//! the synthetic source. The Windows and macOS entry points are compiled on
//! those targets and report the system API they call.

use crate::capture::{preview_source, CaptureBackend, CaptureSource, SourceKind};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendSpec {
    pub os: &'static str,
    pub api: &'static str,
    pub fallback: &'static str,
}

pub fn capture_backends() -> &'static [BackendSpec] {
    &[
        BackendSpec {
            os: "windows",
            api: "Windows.Graphics.Capture",
            fallback: "DXGI Desktop Duplication",
        },
        BackendSpec {
            os: "linux",
            api: "PipeWire + xdg-desktop-portal ScreenCast",
            fallback: "synthetic preview",
        },
        BackendSpec {
            os: "macos",
            api: "ScreenCaptureKit",
            fallback: "synthetic preview",
        },
    ]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxPortalProbe {
    pub bus: Option<String>,
    pub pipewire_socket: Option<PathBuf>,
}

impl LinuxPortalProbe {
    pub fn ready(&self) -> bool {
        self.bus.is_some() && self.pipewire_socket.is_some()
    }
}

pub fn probe_linux_portal() -> LinuxPortalProbe {
    LinuxPortalProbe {
        bus: session_bus_path(),
        pipewire_socket: pipewire_socket(),
    }
}

pub fn session_bus_path() -> Option<String> {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok()?;
    parse_unix_bus(&address)
}

pub fn parse_unix_bus(address: &str) -> Option<String> {
    for part in address.split(',') {
        let part = part.trim();
        let rest = part
            .strip_prefix("unix:path=")
            .or_else(|| part.strip_prefix("path="));
        if let Some(rest) = rest {
            if !rest.is_empty() {
                return Some(rest.to_owned());
            }
        }
    }
    None
}

fn pipewire_socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let socket = PathBuf::from(runtime).join("pipewire-0");
    socket.exists().then_some(socket)
}

pub fn portal_source() -> Option<CaptureSource> {
    let probe = probe_linux_portal();
    if !probe.ready() {
        return None;
    }
    Some(preview_source(
        "portal:screen",
        SourceKind::Screen,
        "Screen (PipeWire portal)",
        1920,
        1080,
        CaptureBackend::PipeWirePortal,
    ))
}

/// PipeWire is how a granted ScreenCast fd becomes frames. Missing library
/// means the portal session, even if it connected, cannot be consumed yet.
pub fn pipewire_library() -> Result<String, String> {
    let path = std::ffi::CString::new("libpipewire-0.3.so.0").map_err(|err| err.to_string())?;
    let handle = unsafe { dlopen_pw(path.as_ptr(), 1) };
    if handle.is_null() {
        Err("libpipewire-0.3.so.0 is not installed, so a portal fd cannot be consumed".into())
    } else {
        Ok("libpipewire-0.3.so.0".into())
    }
}

unsafe extern "C" {
    #[link_name = "dlopen"]
    fn dlopen_pw(filename: *const i8, flags: i32) -> *mut std::ffi::c_void;
}

pub fn backend_detail(backend: CaptureBackend) -> String {
    match backend {
        CaptureBackend::WindowsGraphicsCapture => {
            "Windows.Graphics.Capture is selected on Windows. DXGI duplication is the fallback when capture-for-window is refused.".into()
        }
        CaptureBackend::PipeWirePortal => {
            let probe = probe_linux_portal();
            let library = pipewire_library().unwrap_or_else(|err| err);
            if probe.ready() {
                format!("xdg-desktop-portal ScreenCast socket is present. {library}. The consent dialog is not raised from this process, so the preview source stays available.")
            } else {
                format!("No session bus or PipeWire socket. {library}. The preview source is used.")
            }
        }
        CaptureBackend::ScreenCaptureKit => {
            "ScreenCaptureKit is selected on macOS. The system picker supplies the display or window.".into()
        }
        CaptureBackend::Synthetic => "In-process preview frames.".into(),
    }
}

/// Attempt the first step of a ScreenCast session: connect to the bus socket.
/// A missing socket is an error, not a silent success.
pub fn connect_session_bus() -> Result<String, String> {
    let path = session_bus_path().ok_or_else(|| {
        "DBUS_SESSION_BUS_ADDRESS is unset, so xdg-desktop-portal cannot be reached".to_owned()
    })?;
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(&path)
            .map(|_| format!("connected to {path}"))
            .map_err(|err| format!("session bus {path} refused the connection: {err}"))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err("the portal bus is a unix socket".into())
    }
}

#[cfg(target_os = "windows")]
pub fn probe_windows_capture() -> Result<&'static str, String> {
    // Windows.Graphics.Capture is the preferred path. DXGI duplication covers
    // the whole output when a window handle cannot be captured.
    let dxgi = windows_link("dxgi.dll", "CreateDXGIFactory1")?;
    if dxgi.is_null() {
        return Err("CreateDXGIFactory1 was not exported by dxgi.dll".into());
    }
    Ok("Windows.Graphics.Capture")
}

#[cfg(target_os = "windows")]
fn windows_link(module: &str, symbol: &str) -> Result<*mut std::ffi::c_void, String> {
    use std::ffi::CString;
    let module = CString::new(module).map_err(|err| err.to_string())?;
    let symbol = CString::new(symbol).map_err(|err| err.to_string())?;
    unsafe {
        let handle = LoadLibraryA(module.as_ptr());
        if handle.is_null() {
            return Err(format!("failed to load {}", module.to_string_lossy()));
        }
        Ok(GetProcAddress(handle, symbol.as_ptr()))
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn LoadLibraryA(name: *const i8) -> *mut std::ffi::c_void;
    fn GetProcAddress(module: *mut std::ffi::c_void, name: *const i8) -> *mut std::ffi::c_void;
}

#[cfg(target_os = "macos")]
pub fn probe_screen_capture_kit() -> Result<&'static str, String> {
    // ScreenCaptureKit (SCShareableContent) is the macOS path. This probe only
    // checks that the framework can be opened; the stream itself needs a
    // permission prompt.
    let path = std::ffi::CString::new(
        "/System/Library/Frameworks/ScreenCaptureKit.framework/ScreenCaptureKit",
    )
    .map_err(|err| err.to_string())?;
    let handle = unsafe { libc_dlopen(path.as_ptr()) };
    if handle.is_null() {
        Err("ScreenCaptureKit.framework did not open".into())
    } else {
        Ok("ScreenCaptureKit")
    }
}

#[cfg(target_os = "macos")]
unsafe fn libc_dlopen(path: *const i8) -> *mut std::ffi::c_void {
    extern "C" {
        fn dlopen(path: *const i8, mode: i32) -> *mut std::ffi::c_void;
    }
    dlopen(path, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_desktop_os_has_a_named_capturer() {
        let specs = capture_backends();
        assert!(specs
            .iter()
            .any(|spec| spec.os == "windows" && spec.api.contains("Graphics")));
        assert!(specs
            .iter()
            .any(|spec| spec.os == "linux" && spec.api.contains("PipeWire")));
        assert!(specs
            .iter()
            .any(|spec| spec.os == "macos" && spec.api.contains("ScreenCaptureKit")));
    }

    #[test]
    fn unix_bus_addresses_keep_the_socket_path() {
        assert_eq!(
            parse_unix_bus("unix:path=/run/user/1000/bus"),
            Some("/run/user/1000/bus".into())
        );
        assert_eq!(
            parse_unix_bus("unix:abstract=/tmp/dbus,path=/tmp/bus"),
            Some("/tmp/bus".into())
        );
        assert_eq!(parse_unix_bus("tcp:host=localhost"), None);
    }
}
