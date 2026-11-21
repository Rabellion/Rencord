//! Provider requests against a local HTTP server. Nothing here contacts a real host.

use rencord_upload::{
    discord_attachment_call, fetch_limit, run_job, CustomTarget, DiscordTarget, Hosts, JobOutput,
    JobSpec, LitterboxTime, ProviderKind, UploadPlan, NITRO_BASIC_LIMIT_BYTES,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

struct Hit {
    method: String,
    path: String,
    headers: String,
    body: Vec<u8>,
}

fn serve(
    handler: impl Fn(&Hit) -> (u16, Vec<(String, String)>, Vec<u8>) + Send + Sync + 'static,
) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let Ok(hit) = read_request(&mut stream) else {
                continue;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            let (status, headers, body) = handler(&hit);
            let _ = write_response(&mut stream, status, &headers, &body);
        }
    });
    (format!("http://{address}"), hits)
}

fn read_request(stream: &mut impl Read) -> std::io::Result<Hit> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 1024];
    let header_end;
    loop {
        let read = stream.read(&mut buf)?;
        if read == 0 {
            return Err(std::io::Error::other("closed"));
        }
        raw.extend_from_slice(&buf[..read]);
        if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
        if raw.len() > 1024 * 1024 {
            return Err(std::io::Error::other("headers too large"));
        }
    }
    let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let mut lines = head.lines();
    let request = lines.next().unwrap_or("");
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = raw[header_end..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buf)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buf[..read]);
    }
    body.truncate(length);
    Ok(Hit {
        method,
        path,
        headers: head,
        body,
    })
}

fn write_response(
    stream: &mut impl Write,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "ERR" };
    write!(stream, "HTTP/1.1 {status} {reason}\r\n")?;
    write!(stream, "Content-Length: {}\r\n", body.len())?;
    write!(stream, "Connection: close\r\n")?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(body)?;
    Ok(())
}

fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rencord-upload-http-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn hosts(base: &str) -> Hosts {
    Hosts {
        catbox: base.to_owned(),
        litterbox: base.to_owned(),
        zero_x: base.to_owned(),
    }
}

#[test]
fn mock_servers_see_catbox_litterbox_zero_x_and_presigned_put() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let record = seen.clone();
    let (base, hits) = serve(move |hit| {
        record.lock().unwrap().push(format!(
            "{} {} {}",
            hit.method,
            hit.path,
            String::from_utf8_lossy(&hit.body)
        ));
        let url = if hit.path.contains("api.php") {
            "https://files.example/ok"
        } else if hit.method == "PUT" {
            "stored"
        } else {
            "https://0x0.example/abc"
        };
        (200, Vec::new(), url.as_bytes().to_vec())
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let file = temp_file("clip.bin", b"hello-rencord");
    let cases = [
        (ProviderKind::Catbox, None),
        (ProviderKind::Litterbox, None),
        (ProviderKind::ZeroX, None),
        (
            ProviderKind::Custom,
            Some(CustomTarget::PresignedPut {
                url: format!("{base}/presigned"),
            }),
        ),
    ];
    for (provider, custom) in cases {
        let output = run_job(
            JobSpec::External {
                path: file.clone(),
                plan: UploadPlan {
                    provider,
                    hosts: hosts(&base),
                    litterbox: LitterboxTime::H1,
                    zero_x_hours: Some(12),
                    custom,
                    file_name: "clip.bin".into(),
                },
                consent: true,
                resume_dir: file.parent().unwrap().join("resume"),
                history_path: file.parent().unwrap().join("history.json"),
            },
            &cancel,
            |_| {},
        )
        .unwrap();
        assert!(matches!(output, JobOutput::Link { .. }));
    }
    assert!(hits.load(Ordering::SeqCst) >= 4);
    let log = seen.lock().unwrap().join("\n");
    assert!(
        log.contains("reqtype") && log.contains("fileupload"),
        "{log}"
    );
    assert!(log.contains("fileToUpload"), "{log}");
    assert!(log.contains("name=\"time\"") && log.contains("1h"), "{log}");
    assert!(
        log.contains("name=\"expires\"") && log.contains("12"),
        "{log}"
    );
    assert!(log.contains("PUT /presigned"), "{log}");
    assert!(log.contains("hello-rencord"), "{log}");
    let history = std::fs::read_to_string(file.parent().unwrap().join("history.json")).unwrap();
    assert!(history.contains("https://"));
}

#[test]
fn consent_false_never_reaches_the_server() {
    let (base, hits) = serve(|_| (500, Vec::new(), b"nope".to_vec()));
    let file = temp_file("secret.bin", b"do-not-send");
    let error = run_job(
        JobSpec::External {
            path: file,
            plan: UploadPlan {
                provider: ProviderKind::Catbox,
                hosts: hosts(&base),
                litterbox: LitterboxTime::H24,
                zero_x_hours: None,
                custom: None,
                file_name: "secret.bin".into(),
            },
            consent: false,
            resume_dir: std::env::temp_dir(),
            history_path: std::env::temp_dir().join("rencord-should-not-write-history.json"),
        },
        &Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("consent"));
    thread::sleep(Duration::from_millis(50));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn s3_multipart_resumes_after_a_failed_part() {
    let part2_failures = Arc::new(AtomicUsize::new(1));
    let successes = Arc::new(Mutex::new(Vec::<u32>::new()));
    let fail = part2_failures.clone();
    let ok_parts = successes.clone();
    let (base, _) = serve(move |hit| {
        if hit.path.contains("?uploads") {
            return (
                200,
                Vec::new(),
                b"<InitiateMultipartUploadResult><UploadId>upload-9</UploadId></InitiateMultipartUploadResult>"
                    .to_vec(),
            );
        }
        if let Some(number) = part_number(&hit.path) {
            if number == 2 && fail.fetch_sub(1, Ordering::SeqCst) > 0 {
                // fetch_sub returns previous. First time previous is 1, we fail.
                return (500, Vec::new(), b"try again".to_vec());
            }
            ok_parts.lock().unwrap().push(number);
            return (
                200,
                vec![("ETag".into(), format!("\"p{number}\""))],
                Vec::new(),
            );
        }
        if hit.path.contains("uploadId=") && hit.method == "POST" {
            let body = String::from_utf8_lossy(&hit.body);
            assert!(body.contains("<PartNumber>1</PartNumber>"), "{body}");
            assert!(body.contains("<PartNumber>2</PartNumber>"), "{body}");
            assert!(body.contains("<PartNumber>3</PartNumber>"), "{body}");
            return (200, Vec::new(), b"complete".to_vec());
        }
        (404, Vec::new(), b"unknown".to_vec())
    });

    let file = temp_file("blob.bin", &vec![9u8; 2500]);
    let mut custom =
        CustomTarget::parse(&format!("s3 {base} bucket us-east-1 AKIA s3cr3t pref")).unwrap();
    if let CustomTarget::S3 { part_size, .. } = &mut custom {
        *part_size = 1000;
    }
    let spec = JobSpec::External {
        path: file.clone(),
        plan: UploadPlan {
            provider: ProviderKind::Custom,
            hosts: hosts(&base),
            litterbox: LitterboxTime::H24,
            zero_x_hours: None,
            custom: Some(custom),
            file_name: "blob.bin".into(),
        },
        consent: true,
        resume_dir: file.parent().unwrap().join("resume"),
        history_path: file.parent().unwrap().join("history.json"),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let first = run_job(spec.clone(), &cancel, |_| {});
    assert!(first.is_err(), "{first:?}");
    let second = run_job(spec, &cancel, |_| {}).unwrap();
    assert!(matches!(second, JobOutput::Link { .. }));
    let parts = successes.lock().unwrap().clone();
    assert_eq!(
        parts.iter().filter(|number| **number == 1).count(),
        1,
        "{parts:?}"
    );
    assert!(parts.contains(&2) && parts.contains(&3), "{parts:?}");
}

fn part_number(path: &str) -> Option<u32> {
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("partNumber=") {
            return value.parse().ok();
        }
    }
    None
}

#[test]
fn discord_attachment_post_carries_the_files_and_the_note() {
    let (base, _) = serve(|hit| {
        let body = String::from_utf8_lossy(&hit.body);
        assert!(hit.headers.contains("secret-token"), "{}", hit.headers);
        assert!(body.contains("payload_json"), "{body}");
        assert!(body.contains("rencord-split v1"), "{body}");
        assert!(body.contains("files[0]"), "{body}");
        assert!(body.contains("volume-bytes"), "{body}");
        (200, Vec::new(), b"{\"id\":\"1\"}".to_vec())
    });
    let file = temp_file("lecture.mp4.zip.001", b"volume-bytes");
    let output = run_job(
        JobSpec::Discord {
            target: DiscordTarget {
                base: base.clone(),
                token: "secret-token".into(),
                channel_id: 7,
            },
            content: "rencord-split v1\nparts: 1".into(),
            files: vec![file],
        },
        &Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert!(matches!(output, JobOutput::Posted { .. }));
    let call = discord_attachment_call(
        &DiscordTarget {
            base,
            token: "secret-token".into(),
            channel_id: 7,
        },
        "rencord-split v1",
        &[("lecture.mp4.zip.001".into(), String::new())],
    );
    assert!(call.url.ends_with("/channels/7/messages"));
}

#[test]
fn fetch_limit_reads_premium_type_and_boost_tier() {
    let (base, _) = serve(|hit| {
        let body = if hit.path.contains("/users/") {
            br#"{"id":"1","username":"ada","premium_type":3}"#.to_vec()
        } else {
            br#"{"id":"9","name":"Engines","premium_tier":2}"#.to_vec()
        };
        (200, Vec::new(), body)
    });
    let limit = fetch_limit(&base, "token-value", Some(9), false).unwrap();
    assert_eq!(limit.bytes, NITRO_BASIC_LIMIT_BYTES.max(50 * 1024 * 1024));
    assert_eq!(limit.premium_type, 3);
    assert_eq!(limit.guild_tier, Some(2));
}
