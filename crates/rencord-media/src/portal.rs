//! Linux ScreenCast.
//!
//! `ashpd` asks xdg-desktop-portal for a session. PipeWire then owns the fd.
//! Both calls fail closed when the desktop session or the library is absent.
//! Synthetic sources stay available for tests.

#[cfg(target_os = "linux")]
mod hosted {
    use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
    use ashpd::desktop::PersistMode;

    pub fn pipewire_linked() -> bool {
        // Constructing a main loop loads libpipewire. Drop it immediately.
        pipewire::main_loop::MainLoopBox::new(None).is_ok()
    }

    pub async fn request_screencast() -> Result<String, String> {
        let proxy = Screencast::new()
            .await
            .map_err(|err| format!("xdg-desktop-portal ScreenCast is not available: {err}"))?;
        let session = proxy
            .create_session()
            .await
            .map_err(|err| format!("ScreenCast CreateSession failed: {err}"))?;
        proxy
            .select_sources(
                &session,
                CursorMode::Hidden,
                SourceType::Monitor | SourceType::Window,
                false,
                None,
                PersistMode::DoNot,
            )
            .await
            .map_err(|err| format!("ScreenCast SelectSources failed: {err}"))?
            .response()
            .map_err(|err| format!("ScreenCast SelectSources was rejected: {err}"))?;
        let streams = proxy
            .start(&session, None)
            .await
            .map_err(|err| format!("ScreenCast Start failed: {err}"))?
            .response()
            .map_err(|err| format!("ScreenCast Start was rejected: {err}"))?;
        let node = streams
            .streams()
            .first()
            .map(|stream| stream.pipe_wire_node_id());
        let fd = proxy
            .open_pipe_wire_remote(&session)
            .await
            .map_err(|err| format!("OpenPipeWireRemote failed: {err}"))?;
        let (video_bytes, audio_samples) = pull_remote(Some(fd), node, true)?;
        Ok(format!(
        "portal copied {video_bytes} video bytes and {audio_samples} audio samples from node {node:?}"
    ))
    }

    /// Connect a PipeWire remote and pull one video buffer plus a monitor-audio buffer.
    ///
    /// `fd` is the portal remote. `None` uses the session default, which fails when
    /// no daemon is running. The call returns the copied byte count only when a
    /// buffer actually arrives.
    pub fn pull_remote(
        fd: Option<std::os::fd::OwnedFd>,
        node: Option<u32>,
        capture_sink: bool,
    ) -> Result<(usize, u32), String> {
        use pipewire::properties::properties;
        use pipewire::spa::pod::{serialize::PodSerializer, Pod, Value};
        use pipewire::stream::StreamFlags;
        use std::io::Cursor;
        use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
        use std::sync::Arc;

        pipewire::init();
        let mainloop = pipewire::main_loop::MainLoopBox::new(None)
            .map_err(|err| format!("PipeWire main loop failed: {err}"))?;
        let context = pipewire::context::ContextBox::new(mainloop.loop_(), None)
            .map_err(|err| format!("PipeWire context failed: {err}"))?;
        let core = match fd {
            Some(fd) => context.connect_fd(fd, None),
            None => context.connect(None),
        }
        .map_err(|err| format!("PipeWire connect failed: {err}"))?;

        let video_bytes = Arc::new(AtomicUsize::new(0));
        let audio_samples = Arc::new(AtomicU32::new(0));
        let video = pipewire::stream::StreamBox::new(
            &core,
            "rencord-screen",
            properties! {
                *pipewire::keys::MEDIA_TYPE => "Video",
                *pipewire::keys::MEDIA_CATEGORY => "Capture",
                *pipewire::keys::MEDIA_ROLE => "Screen",
            },
        )
        .map_err(|err| err.to_string())?;
        let copied = Arc::clone(&video_bytes);
        let _video_listener = video
            .add_local_listener_with_user_data(copied)
            .process(|stream, copied| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                if let Some(bytes) = datas.first_mut().and_then(|data| data.data()) {
                    if !bytes.is_empty() {
                        copied.store(bytes.len(), Ordering::Relaxed);
                    }
                }
            })
            .register()
            .map_err(|err| err.to_string())?;
        let video_pod = video_format_pod();
        let mut video_params = [Pod::from_bytes(&video_pod).ok_or("video format pod was empty")?];
        video
            .connect(
                pipewire::spa::utils::Direction::Input,
                node,
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
                &mut video_params,
            )
            .map_err(|err| format!("PipeWire video stream failed: {err}"))?;

        let mut audio_props = properties! {
            *pipewire::keys::MEDIA_TYPE => "Audio",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Music",
        };
        if capture_sink {
            audio_props.insert(*pipewire::keys::STREAM_CAPTURE_SINK, "true");
        }
        let audio = pipewire::stream::StreamBox::new(&core, "rencord-audio", audio_props)
            .map_err(|err| err.to_string())?;
        let samples = Arc::clone(&audio_samples);
        let _audio_listener = audio
            .add_local_listener_with_user_data(samples)
            .process(|stream, samples| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                if let Some(bytes) = datas.first_mut().and_then(|data| data.data()) {
                    let count = (bytes.len() / 4) as u32;
                    if count > 0 {
                        samples.store(count, Ordering::Relaxed);
                    }
                }
            })
            .register()
            .map_err(|err| err.to_string())?;
        let mut audio_info = pipewire::spa::param::audio::AudioInfoRaw::new();
        audio_info.set_format(pipewire::spa::param::audio::AudioFormat::F32LE);
        let audio_object = pipewire::spa::pod::Object {
            type_: pipewire::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: pipewire::spa::param::ParamType::EnumFormat.as_raw(),
            properties: audio_info.into(),
        };
        let audio_pod =
            PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(audio_object))
                .map_err(|err| format!("audio format pod failed: {err}"))?
                .0
                .into_inner();
        let mut audio_params = [Pod::from_bytes(&audio_pod).ok_or("audio format pod was empty")?];
        audio
            .connect(
                pipewire::spa::utils::Direction::Input,
                None,
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
                &mut audio_params,
            )
            .map_err(|err| format!("PipeWire monitor audio failed: {err}"))?;

        for _ in 0..4 {
            let _ = mainloop.loop_().iterate(pipewire::loop_::Timeout::Finite(
                std::time::Duration::from_millis(20),
            ));
        }
        let video_len = video_bytes.load(Ordering::Relaxed);
        let audio_len = audio_samples.load(Ordering::Relaxed);
        if video_len == 0 {
            return Err(
                "PipeWire connected but no screen buffer arrived, so no frame was copied".into(),
            );
        }
        Ok((video_len, audio_len))
    }

    fn video_format_pod() -> Vec<u8> {
        use pipewire::spa::pod::serialize::PodSerializer;
        use pipewire::spa::pod::Value;
        use std::io::Cursor;
        let obj = pipewire::spa::pod::object!(
            pipewire::spa::utils::SpaTypes::ObjectParamFormat,
            pipewire::spa::param::ParamType::EnumFormat,
            pipewire::spa::pod::property!(
                pipewire::spa::param::format::FormatProperties::MediaType,
                Id,
                pipewire::spa::param::format::MediaType::Video
            ),
            pipewire::spa::pod::property!(
                pipewire::spa::param::format::FormatProperties::MediaSubtype,
                Id,
                pipewire::spa::param::format::MediaSubtype::Raw
            ),
            pipewire::spa::pod::property!(
                pipewire::spa::param::format::FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                pipewire::spa::param::video::VideoFormat::BGRx,
                pipewire::spa::param::video::VideoFormat::BGRx,
                pipewire::spa::param::video::VideoFormat::RGBA,
                pipewire::spa::param::video::VideoFormat::I420
            ),
        );
        PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(obj))
            .map(|pod| pod.0.into_inner())
            .unwrap_or_default()
    }

    /// One attempt, bounded so a missing bus cannot stall the worker.
    pub fn request_screencast_blocking() -> Result<String, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| err.to_string())?;
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(400), request_screencast())
                .await
                .unwrap_or_else(|_| Err("ScreenCast timed out waiting for the portal".into()))
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn pipewire_library_is_linked() {
            assert!(pipewire_linked());
        }

        #[test]
        fn a_headless_bus_does_not_yield_a_stream() {
            let _ = request_screencast_blocking();
        }

        #[test]
        fn a_missing_daemon_copies_no_frame_or_audio() {
            let err = pull_remote(None, None, true).unwrap_err();
            assert!(
                err.contains("PipeWire") || err.contains("buffer") || err.contains("connect"),
                "{err}"
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub use hosted::{pipewire_linked, request_screencast_blocking};

#[cfg(not(target_os = "linux"))]
pub fn pipewire_linked() -> bool {
    false
}

#[cfg(not(target_os = "linux"))]
pub fn request_screencast_blocking() -> Result<String, String> {
    Err("ScreenCast uses the Linux desktop portal".into())
}
