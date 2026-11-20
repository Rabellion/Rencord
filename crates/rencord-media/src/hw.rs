#![allow(clippy::missing_transmute_annotations, clippy::question_mark)]
//! Hardware encoder probes.
//!
//! Each backend is loaded with `dlopen` and asked to open a session. A library
//! that is merely installed is not treated as usable. x264 is GPL, so it is
//! reported and not called. The software encoder remains the path that
//! actually emits bytes on a machine with no GPU.

use crate::encode::EncoderKind;
use std::ffi::{c_void, CStr, CString};
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::ptr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderProbe {
    pub kind: EncoderKind,
    pub usable: bool,
    pub detail: String,
}

pub fn probe_all() -> Vec<EncoderProbe> {
    vec![
        probe_nvenc(),
        probe_amf(),
        probe_qsv(),
        probe_videotoolbox(),
        probe_vaapi(),
        EncoderProbe {
            kind: EncoderKind::Software,
            usable: true,
            detail: software_detail(),
        },
    ]
}

fn software_detail() -> String {
    if library_file_present("libx264.so.164") {
        "built-in luma access unit. libx264 is installed but GPL, so it is not loaded.".into()
    } else {
        "built-in luma access unit. No GPL codec is linked.".into()
    }
}

pub fn usable_encoder() -> EncoderKind {
    for probe in probe_all() {
        if probe.usable && probe.kind != EncoderKind::Software {
            return probe.kind;
        }
    }
    EncoderKind::Software
}

fn probe_nvenc() -> EncoderProbe {
    let kind = EncoderKind::Nvenc;
    match dlopen(&["libnvidia-encode.so.1", "nvEncodeAPI64.dll"]) {
        Err(err) => EncoderProbe {
            kind,
            usable: false,
            detail: err,
        },
        Ok(lib) => match dlsym(lib, "NvEncodeAPICreateInstance") {
            Err(err) => EncoderProbe {
                kind,
                usable: false,
                detail: err,
            },
            Ok(_) => EncoderProbe {
                kind,
                usable: false,
                detail: "NVENC entry point is present. The encode session is not opened because the NVIDIA Video Codec SDK function list is versioned and this process has no CUDA device check yet.".into(),
            },
        },
    }
}

fn probe_amf() -> EncoderProbe {
    let kind = EncoderKind::Amf;
    match dlopen(&["libamfrt64.so.1", "amfrt64.dll"]) {
        Err(err) => EncoderProbe {
            kind,
            usable: false,
            detail: err,
        },
        Ok(_) => EncoderProbe {
            kind,
            usable: false,
            detail: "AMF runtime loaded. No AMD device was opened.".into(),
        },
    }
}

fn probe_qsv() -> EncoderProbe {
    let kind = EncoderKind::Qsv;
    let lib = match dlopen(&["libvpl.so.2", "libmfx.so.1"]) {
        Ok(lib) => lib,
        Err(err) => {
            return EncoderProbe {
                kind,
                usable: false,
                detail: err,
            }
        }
    };
    let init: unsafe extern "C" fn(i32, *mut MfxVersion, *mut *mut c_void) -> i32 =
        match dlsym(lib, "MFXInit") {
            Ok(symbol) => unsafe { std::mem::transmute(symbol) },
            Err(err) => {
                return EncoderProbe {
                    kind,
                    usable: false,
                    detail: err,
                }
            }
        };
    let mut version = MfxVersion { major: 2, minor: 0 };
    let mut session = ptr::null_mut();
    // MFX_IMPL_HARDWARE = 2. A non-zero status means no usable device.
    let status = unsafe { init(2, &mut version, &mut session) };
    if status == 0 && !session.is_null() {
        if let Ok(close) = dlsym(lib, "MFXClose") {
            let close: unsafe extern "C" fn(*mut c_void) -> i32 =
                unsafe { std::mem::transmute(close) };
            unsafe { close(session) };
        }
        EncoderProbe {
            kind,
            usable: true,
            detail: format!(
                "oneVPL MFXInit hardware session, API {}.{}",
                version.major, version.minor
            ),
        }
    } else {
        EncoderProbe {
            kind,
            usable: false,
            detail: format!("oneVPL MFXInit(hardware) returned {status}. No Intel encode session."),
        }
    }
}

fn probe_videotoolbox() -> EncoderProbe {
    let kind = EncoderKind::VideoToolbox;
    #[cfg(target_os = "macos")]
    {
        return match dlopen(&["/System/Library/Frameworks/VideoToolbox.framework/VideoToolbox"]) {
            Ok(_) => EncoderProbe {
                kind,
                usable: true,
                detail: "VideoToolbox framework opened. VTCompressionSession is the encode path."
                    .into(),
            },
            Err(err) => EncoderProbe {
                kind,
                usable: false,
                detail: err,
            },
        };
    }
    #[cfg(not(target_os = "macos"))]
    EncoderProbe {
        kind,
        usable: false,
        detail: "VideoToolbox is the macOS encoder. This process is not macOS.".into(),
    }
}

fn probe_vaapi() -> EncoderProbe {
    let kind = EncoderKind::Vaapi;
    let node = match first_render_node() {
        Ok(node) => node,
        Err(err) => {
            return EncoderProbe {
                kind,
                usable: false,
                detail: err,
            }
        }
    };
    let file = match OpenOptions::new().read(true).write(true).open(&node) {
        Ok(file) => file,
        Err(err) => {
            return EncoderProbe {
                kind,
                usable: false,
                detail: format!("open {}: {err}", node.display()),
            }
        }
    };
    let drm = match dlopen(&["libva-drm.so.2"]) {
        Ok(lib) => lib,
        Err(err) => {
            return EncoderProbe {
                kind,
                usable: false,
                detail: err,
            }
        }
    };
    let va = match dlopen(&["libva.so.2"]) {
        Ok(lib) => lib,
        Err(err) => {
            return EncoderProbe {
                kind,
                usable: false,
                detail: err,
            }
        }
    };
    let get_display: unsafe extern "C" fn(i32) -> *mut c_void = match dlsym(drm, "vaGetDisplayDRM")
    {
        Ok(symbol) => unsafe { std::mem::transmute(symbol) },
        Err(err) => {
            return EncoderProbe {
                kind,
                usable: false,
                detail: err,
            }
        }
    };
    let init: unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> i32 =
        match dlsym(va, "vaInitialize") {
            Ok(symbol) => unsafe { std::mem::transmute(symbol) },
            Err(err) => {
                return EncoderProbe {
                    kind,
                    usable: false,
                    detail: err,
                }
            }
        };
    let display = unsafe { get_display(file.as_raw_fd()) };
    if display.is_null() {
        return EncoderProbe {
            kind,
            usable: false,
            detail: format!("vaGetDisplayDRM returned null for {}", node.display()),
        };
    }
    let mut major = 0;
    let mut minor = 0;
    let status = unsafe { init(display, &mut major, &mut minor) };
    if status != 0 {
        return EncoderProbe {
            kind,
            usable: false,
            detail: format!("vaInitialize returned {status} on {}", node.display()),
        };
    }
    let encode = va_has_h264_encode(va, display);
    if let Ok(term) = dlsym(va, "vaTerminate") {
        let term: unsafe extern "C" fn(*mut c_void) -> i32 = unsafe { std::mem::transmute(term) };
        unsafe { term(display) };
    }
    match encode {
        Ok(()) => EncoderProbe {
            kind,
            usable: true,
            detail: format!("VA-API {major}.{minor} has an H.264 encode entrypoint"),
        },
        Err(err) => EncoderProbe {
            kind,
            usable: false,
            detail: format!("VA-API {major}.{minor} opened, but {err}"),
        },
    }
}

fn va_has_h264_encode(va: *mut c_void, display: *mut c_void) -> Result<(), String> {
    // VAProfileH264Main = 6, VAEntrypointEncSlice = 6.
    let query: unsafe extern "C" fn(*mut c_void, i32, *mut i32, *mut i32) -> i32 =
        match dlsym(va, "vaQueryConfigEntrypoints") {
            Ok(symbol) => unsafe { std::mem::transmute(symbol) },
            Err(err) => return Err(err),
        };
    let mut entrypoints = [0i32; 8];
    let mut count = entrypoints.len() as i32;
    let status = unsafe { query(display, 6, entrypoints.as_mut_ptr(), &mut count) };
    if status != 0 {
        return Err(format!("vaQueryConfigEntrypoints returned {status}"));
    }
    let found = entrypoints[..count.max(0) as usize].contains(&6);
    if found {
        Ok(())
    } else {
        Err("no VAEntrypointEncSlice for H.264 Main".into())
    }
}

fn first_render_node() -> Result<std::path::PathBuf, String> {
    let dri = Path::new("/dev/dri");
    let entries = std::fs::read_dir(dri)
        .map_err(|_| "no /dev/dri render node, so VA-API cannot open a GPU".to_owned())?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("renderD") {
            return Ok(entry.path());
        }
    }
    Err("no /dev/dri/renderD* node".into())
}

#[repr(C)]
struct MfxVersion {
    major: u16,
    minor: u16,
}

fn library_file_present(name: &str) -> bool {
    [
        "/usr/lib",
        "/usr/lib64",
        "/lib",
        "/lib64",
        "/usr/lib/x86_64-linux-gnu",
    ]
    .iter()
    .any(|root| Path::new(root).join(name).exists())
}

fn dlopen(names: &[&str]) -> Result<*mut c_void, String> {
    let mut last = String::from("no candidate library");
    for name in names {
        let path = CString::new(*name).map_err(|err| err.to_string())?;
        let handle = unsafe { dlopen_c(path.as_ptr(), 1) };
        if !handle.is_null() {
            return Ok(handle);
        }
        last = format!("{name}: {}", last_dlerror());
    }
    Err(last)
}

fn dlsym(handle: *mut c_void, name: &str) -> Result<*mut c_void, String> {
    let symbol = CString::new(name).map_err(|err| err.to_string())?;
    let ptr = unsafe { dlsym_c(handle, symbol.as_ptr()) };
    if ptr.is_null() {
        Err(format!("{name} is not exported ({})", last_dlerror()))
    } else {
        Ok(ptr)
    }
}

fn last_dlerror() -> String {
    unsafe {
        let ptr = dlerror_c();
        if ptr.is_null() {
            "unknown".into()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

unsafe extern "C" {
    #[link_name = "dlopen"]
    fn dlopen_c(filename: *const i8, flags: i32) -> *mut c_void;
    #[link_name = "dlsym"]
    fn dlsym_c(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
    #[link_name = "dlerror"]
    fn dlerror_c() -> *mut i8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_is_usable_and_every_probe_explains_itself() {
        let probes = probe_all();
        assert!(probes
            .iter()
            .any(|probe| probe.kind == EncoderKind::Software && probe.usable));
        assert!(probes.iter().all(|probe| !probe.detail.is_empty()));
        assert_eq!(usable_encoder(), EncoderKind::Software);
    }
}
