use rencord_media::{ClientMessage, WorkerClient, WorkerMessage};
use std::time::Duration;

#[test]
fn the_worker_is_a_different_process_and_publishes_a_frame() {
    let bin = env!("CARGO_BIN_EXE_rencord-stream");
    let mut client = WorkerClient::spawn(bin).expect("spawn");
    assert_ne!(client.worker_pid(), std::process::id());
    let sources = client.request(&ClientMessage::ListSources).unwrap();
    match sources {
        WorkerMessage::Sources { sources } => {
            assert!(sources
                .iter()
                .any(|source| source.id == "synthetic:desktop"));
        }
        other => panic!("unexpected {other:?}"),
    }
    let started = client
        .request(&ClientMessage::Start {
            source_id: "synthetic:desktop".into(),
            width: 64,
            height: 36,
            fps: 30,
            bitrate_kbps: 2_500,
        })
        .unwrap();
    assert_eq!(started, WorkerMessage::Started);
    let mut frame = None;
    for _ in 0..40 {
        if let Some(next) = client.latest_frame() {
            frame = Some(next);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let frame = frame.expect("shared frame");
    assert_eq!(frame.width, 64);
    assert!(frame.bgra.iter().any(|byte| *byte != 0));
    client.shutdown().unwrap();
}
