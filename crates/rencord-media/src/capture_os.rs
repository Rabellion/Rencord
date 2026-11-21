//! Operating-system capture entry points.
//!
//! Linux asks the portal and PipeWire. Windows enumerates DXGI outputs and
//! would duplicate the first one. macOS opens the ScreenCaptureKit framework.
//! None of those produce pixels in the Linux CI image, so the worker keeps
//! the synthetic source.

use crate::capture::CaptureError;

pub fn open_platform_source(id: &str) -> Result<(), CaptureError> {
    if id.starts_with("portal:") {
        return Err(CaptureError::Unavailable {
            backend: "PipeWire portal".into(),
            detail: {
                let portal = match crate::portal::request_screencast_blocking() {
                    Ok(session) => {
                        format!(
                            "{session}; PipeWire linked={}",
                            crate::portal::pipewire_linked()
                        )
                    }
                    Err(err) => err,
                };
                format!(
                    "{portal}. {}",
                    crate::platform::backend_detail(crate::capture::CaptureBackend::PipeWirePortal)
                )
            },
        });
    }
    #[cfg(target_os = "windows")]
    if id.starts_with("dxgi:") || id.starts_with("wgc:") {
        return windows_output().map_err(|detail| CaptureError::Unavailable {
            backend: "Windows.Graphics.Capture".into(),
            detail,
        });
    }
    #[cfg(target_os = "macos")]
    if id.starts_with("sck:") {
        return macos_screencapture().map_err(|detail| CaptureError::Unavailable {
            backend: "ScreenCaptureKit".into(),
            detail,
        });
    }
    Err(CaptureError::UnknownSource(id.to_owned()))
}

#[cfg(target_os = "windows")]
fn windows_output() -> Result<(), String> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1};
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|err| err.to_string())?;
        let adapter = factory.EnumAdapters1(0).map_err(|err| err.to_string())?;
        let output = adapter.EnumOutputs(0).map_err(|err| err.to_string())?;
        let output1: IDXGIOutput1 = output.cast().map_err(|err| err.to_string())?;
        let desc = output1.GetDesc().map_err(|err| err.to_string())?;
        let _ = desc.DeviceName;
        // Graphics Capture wants a GraphicsCaptureItem from a monitor handle.
        // DuplicateOutput needs a D3D11 device created from this adapter.
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn macos_screencapture() -> Result<(), String> {
    let path = std::ffi::CString::new(
        "/System/Library/Frameworks/ScreenCaptureKit.framework/ScreenCaptureKit",
    )
    .map_err(|err| err.to_string())?;
    let handle = unsafe { dlopen(path.as_ptr(), 1) };
    if handle.is_null() {
        Err("ScreenCaptureKit.framework did not open".into())
    } else {
        // SCShareableContent and SCStream are the capture objects. Opening the
        // framework is the first step; a stream still needs a permission prompt.
        Ok(())
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dlopen(path: *const i8, mode: i32) -> *mut std::ffi::c_void;
}
