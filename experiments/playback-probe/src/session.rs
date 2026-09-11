//! Bounded, silent native session proof. The endpoint is a bearer capability.
use super::*;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::{
    io::Read,
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::mpsc,
};
use winit::{
    event::{ElementState, Event, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    platform::run_return::EventLoopExtRunReturn,
    window::WindowBuilder,
};

fn option(args: &mut Vec<String>, name: &str) -> Result<Option<String>> {
    if let Some(i) = args.iter().position(|s| s == name) {
        if i + 1 >= args.len() {
            bail!("missing {name} value")
        };
        args.remove(i);
        return Ok(Some(args.remove(i)));
    }
    Ok(None)
}
// A total deadline also bounds clients that drip bytes without a newline.
fn read_json(stream: &mut TcpStream) -> Result<serde_json::Value> {
    // BSD can inherit O_NONBLOCK from the listener. Deadline-based reads must
    // wait for delayed client bytes instead of rejecting a healthy connection.
    stream
        .set_nonblocking(false)
        .context("set TCP stream blocking")?;
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .context("request timed out")?;
        stream
            .set_read_timeout(Some(remaining))
            .with_context(|| format!("set TCP read timeout {remaining:?}"))?;
        let mut byte = [0];
        stream.read_exact(&mut byte).context("read command byte")?;
        bytes.push(byte[0]);
        if bytes.len() > 4096 {
            bail!("request exceeds 4096 bytes")
        }
        if byte[0] == b'\n' {
            return Ok(serde_json::from_slice(&bytes)?);
        }
    }
}
static OWNED_ENDPOINT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
pub fn deadline_cleanup() {
    if let Some(path) = OWNED_ENDPOINT.get() {
        let _ = std::fs::remove_file(path);
    }
}

fn send(stream: &mut TcpStream, value: serde_json::Value) -> Result<()> {
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .context("set TCP write timeout")?;
    writeln!(stream, "{value}").context("write command JSON")?;
    Ok(())
}
fn request(info: &serde_json::Value, command: &str) -> Result<serde_json::Value> {
    let port = info["port"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= 65535)
        .context("invalid port")?;
    let address = SocketAddr::from(([127, 0, 0, 1], port as u16));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))
        .context("connect to session endpoint")?;
    send(
        &mut stream,
        json!({"token":info["token"],"session_id":info["session_id"],"command":command}),
    )?;
    let response = read_json(&mut stream).context("read session response")?;
    if let Some(error) = response.get("error") {
        bail!("session rejected command: {error}")
    };
    Ok(response)
}
fn apply(state: &State, command: &str) -> Result<()> {
    match command {
        "status" => (),
        "pause" => state.paused.store(true, Ordering::Release),
        "resume" => state.paused.store(false, Ordering::Release),
        "toggle" => {
            state.paused.fetch_xor(true, Ordering::AcqRel);
        }
        "quit" => state.stop_requested.store(true, Ordering::Release),
        _ => bail!("unsupported command {command}"),
    };
    Ok(())
}
fn native_command(event: &MediaControlEvent) -> Option<&'static str> {
    match event {
        MediaControlEvent::Play => Some("resume"),
        MediaControlEvent::Pause => Some("pause"),
        MediaControlEvent::Toggle => Some("toggle"),
        MediaControlEvent::Stop | MediaControlEvent::Quit => Some("quit"),
        _ => None,
    }
}
fn audio_completion(receiver: &mpsc::Receiver<Result<()>>) -> Result<bool> {
    match receiver.try_recv() {
        Ok(result) => {
            result?;
            Ok(true)
        }
        Err(mpsc::TryRecvError::Empty) => Ok(false),
        Err(mpsc::TryRecvError::Disconnected) => {
            bail!("audio owner disconnected without reporting completion (possible panic)")
        }
    }
}
fn status(state: &State, id: &str, dbus: &str) -> serde_json::Value {
    json!({"session_id":id,"pid":std::process::id(),"dbus_name":dbus,"paused":state.paused.load(Ordering::Acquire),"status":if state.stop_requested.load(Ordering::Acquire){"stopping"}else if state.paused.load(Ordering::Acquire){"paused"}else{"playing"},"consumed_frames":state.consumed.load(Ordering::Relaxed),"underrun_frames":state.underrun_frames.load(Ordering::Relaxed)})
}
struct Endpoint(PathBuf);
impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn run(command: &str, mut args: Vec<String>) -> Result<()> {
    let path = PathBuf::from(option(&mut args, "--info")?.context("--info PATH required")?);
    if command != "session" {
        let info: serde_json::Value = serde_json::from_reader(File::open(&path)?)?;
        if command == "session-control" {
            if args.len() != 1 {
                bail!("session-control --info PATH status|pause|resume|toggle|quit")
            };
            println!("{}", request(&info, &args[0])?);
            return Ok(());
        }
        let seconds: f64 = option(&mut args, "--close-after")?
            .unwrap_or_else(|| "170".into())
            .parse()?;
        if !args.is_empty() || !seconds.is_finite() || !(0.1..=170.0).contains(&seconds) {
            bail!("invalid controller options")
        }
        return controller(info, Duration::from_secs_f64(seconds));
    }
    let seconds: u64 = option(&mut args, "--seconds")?
        .unwrap_or_else(|| "120".into())
        .parse()?;
    if !(1..=170).contains(&seconds) {
        bail!("--seconds must be 1..=170 (180 second process safety deadline)")
    }
    let device = option(&mut args, "--device")?;
    let backend =
        Backend::parse(&option(&mut args, "--decoder")?.unwrap_or_else(|| "symphonia".into()))?;
    if args.is_empty() || args.iter().any(|s| s.starts_with("--")) {
        bail!("session requires local fixture files; volume is fixed at zero")
    }
    // Repeat the finite generated sequence; no FFmpeg context crosses a thread.
    let mut format = None;
    let mut frames = 0;
    for path in &args {
        frames += backend
            .decode(path, |f, _| ensure_format(&mut format, f))?
            .1;
    }
    let f = format.context("empty audio")?;
    let repeats = ((seconds * f.rate as u64) / frames + 2).min(1000);
    if frames.saturating_mul(repeats) < seconds * f.rate as u64 {
        bail!("fixture sequence too short for session within 1000-repeat bound");
    }
    let files = (0..repeats).flat_map(|_| args.clone()).collect();
    let mut random = [0u8; 32];
    getrandom::getrandom(&mut random).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let id = token[..16].to_string();
    let name = format!("hifimule_probe_{}", std::process::id());
    let dbus = format!("org.mpris.MediaPlayer2.{name}");
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut endpoint_file = options
        .open(&path)
        .context("exclusive endpoint creation failed")?;
    let _ = OWNED_ENDPOINT.set(path.clone());
    let _endpoint = Endpoint(path);
    let mut events = EventLoop::new();
    let window = WindowBuilder::new()
        .with_visible(false)
        .with_title("HifiMule silent session owner")
        .build(&events)
        .context("create native event-loop window")?;
    #[cfg(target_os = "windows")]
    let hwnd = {
        use winit::platform::windows::WindowExtWindows;
        Some(window.hwnd() as *mut std::ffi::c_void)
    };
    #[cfg(not(target_os = "windows"))]
    let hwnd = None;
    let mut media = MediaControls::new(PlatformConfig {
        dbus_name: &name,
        display_name: "HifiMule playback proof",
        hwnd,
    })
    .map_err(|e| anyhow::anyhow!("native registration: {e:?}"))?;
    let (tx, rx) = mpsc::sync_channel(32);
    let dropped_events = Arc::new(AtomicU64::new(0));
    let callback_drops = dropped_events.clone();
    media
        .attach(move |event| {
            if tx.try_send(event).is_err() {
                callback_drops.fetch_add(1, Ordering::Relaxed);
            }
        })
        .map_err(|e| anyhow::anyhow!("native attachment: {e:?}"))?;
    media
        .set_metadata(MediaMetadata {
            title: Some("Generated silent continuity fixtures"),
            artist: Some("HifiMule feasibility experiment"),
            ..Default::default()
        })
        .map_err(|e| anyhow::anyhow!("native metadata: {e:?}"))?;
    let state = Arc::new(State::default());
    let audio_state = state.clone();
    let (audio_tx, audio_rx) = mpsc::channel();
    thread::spawn(move || {
        let result = play_controlled(files, 0.0, device, backend, Some(audio_state));
        let _ = audio_tx.send(result);
    });
    let start = Instant::now();
    let mut published = false;
    let mut audio_finished = false;
    let mut last_pause = None;
    let mut failure = None;
    events.run_return(|event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(20));
        if !matches!(event, Event::MainEventsCleared) {
            return;
        }
        let dropped = dropped_events.swap(0, Ordering::AcqRel);
        if dropped > 0 {
            println!("{}", json!({"source":"native","event":"queue_overflow","dropped_events":dropped}));
            failure = Some(anyhow::anyhow!("native event queue overflow: {dropped} commands lost"));
            state.stop_requested.store(true, Ordering::Release);
        }
        for event in rx.try_iter() {
            let command = native_command(&event);
            println!("{}", json!({"source":"native","event":format!("{event:?}"),"accepted":command.is_some()}));
            if let Some(command) = command {
                let _ = apply(&state, command);
            }
        }
        // At most one bounded request per iteration; disconnect is not a command.
        if let Ok((mut stream, _)) = listener.accept() {
            let response = (|| -> Result<serde_json::Value> {
                let value = read_json(&mut stream)?;
                if value["token"].as_str() != Some(token.as_str())
                    || value["session_id"].as_str() != Some(id.as_str())
                {
                    bail!("wrong session capability");
                }
                let command = value["command"].as_str().context("missing command")?;
                apply(&state, command)?;
                if command != "status" {
                    println!("{}", json!({"source":"controller","command":command}));
                }
                Ok(status(&state, &id, &dbus))
            })();
            let _ = send(&mut stream, response.unwrap_or_else(|e| json!({"error":e.to_string()})));
        }
        let paused = state.paused.load(Ordering::Acquire);
        if last_pause != Some(paused) {
            let playback = if paused {
                MediaPlayback::Paused { progress: None }
            } else {
                MediaPlayback::Playing { progress: None }
            };
            if let Err(e) = media.set_playback(playback) {
                failure = Some(anyhow::anyhow!("native status: {e:?}"));
                state.stop_requested.store(true, Ordering::Release);
            }
            last_pause = Some(paused);
        }
        if !published && state.consumed.load(Ordering::Acquire) > 0 {
            let info = json!({"port":listener.local_addr().unwrap().port(),"token":token,"session_id":id,"pid":std::process::id(),"dbus_name":dbus});
            if let Err(e) = writeln!(endpoint_file, "{info}").and_then(|_| endpoint_file.flush()) {
                failure = Some(e.into());
                state.stop_requested.store(true, Ordering::Release);
            } else {
                published = true;
                println!("{}", json!({"source":"session","event":"ready","session_id":id,"pid":std::process::id(),"dbus_name":dbus}));
            }
        }
        match audio_completion(&audio_rx) {
            Ok(false) => (),
            Ok(true) => {
                audio_finished = true;
                state.stop_requested.store(true, Ordering::Release);
            }
            Err(e) => {
                audio_finished = true;
                failure = Some(e);
                state.stop_requested.store(true, Ordering::Release);
            }
        }
        if start.elapsed() >= Duration::from_secs(seconds) {
            println!("{}", json!({"source":"session","event":"deadline"}));
            state.stop_requested.store(true, Ordering::Release);
        }
        if state.stop_requested.load(Ordering::Acquire) {
            *flow = ControlFlow::Exit;
        }
    });
    state.stop_requested.store(true, Ordering::Release);
    drop(media);
    drop(window);
    // Stop is the normal audio owner's cancellation path. Let it drop its stream
    // before forcing decoder cancellation; genuine errors remain errors.
    if !audio_finished {
        match audio_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(())) => (),
            Ok(Err(e)) => failure = Some(e),
            Err(e) => {
                failure = Some(anyhow::anyhow!(
                    "audio owner did not shut down cleanly: {e}"
                ))
            }
        }
    }
    state.cancel.store(true, Ordering::Release);
    println!("{}", status(&state, &id, &dbus));
    if let Some(e) = failure {
        return Err(e);
    };
    if !published {
        bail!("session ended before audio became ready")
    };
    Ok(())
}
fn controller(info: serde_json::Value, duration: Duration) -> Result<()> {
    println!("{}", request(&info, "status")?);
    let mut events = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("HifiMule controller: Space play/pause; Q quit owner; Escape close")
        .build(&events)
        .context("create native event-loop window")?;
    let start = Instant::now();
    let mut next = Instant::now();
    let mut error = None;
    events.run_return(|event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100));
        let command = match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *flow = ControlFlow::Exit;
                None
            }
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput { input, .. },
                ..
            } if input.state == ElementState::Pressed => match input.virtual_keycode {
                Some(VirtualKeyCode::Space) => Some("toggle"),
                Some(VirtualKeyCode::Q) => Some("quit"),
                Some(VirtualKeyCode::Escape) => {
                    *flow = ControlFlow::Exit;
                    None
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(command) = command {
            match request(&info, command) {
                Ok(_) if command == "quit" => {
                    *flow = ControlFlow::Exit;
                    return;
                }
                Ok(_) => (),
                Err(e) => {
                    error = Some(e);
                    *flow = ControlFlow::Exit;
                    return;
                }
            }
        }
        if Instant::now() >= next {
            match request(&info, "status") {
                Ok(value) => window.set_title(&format!(
                    "HifiMule controller | {} | {} frames | Space: play/pause, Escape: close",
                    value["status"], value["consumed_frames"]
                )),
                Err(e) => {
                    error = Some(e);
                    *flow = ControlFlow::Exit
                }
            }
            next = Instant::now() + Duration::from_secs(1);
        }
        if start.elapsed() >= duration {
            *flow = ControlFlow::Exit;
        }
    });
    if let Some(e) = error {
        return Err(e);
    };
    println!(
        "{}",
        json!({"controller_closed":true,"session_id":info["session_id"]})
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn socket_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let server = listener.accept().unwrap().0;
        (client, server)
    }
    #[test]
    fn delayed_bytes_on_nonblocking_accepted_socket_are_read() {
        let (mut client, mut server) = socket_pair();
        server.set_nonblocking(true).unwrap();
        let sender = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            client.write_all(b"{\"command\":\"status\"}\n").unwrap();
        });
        assert_eq!(read_json(&mut server).unwrap()["command"], "status");
        sender.join().unwrap();
    }
    #[test]
    fn malformed_and_oversized_requests_are_rejected() {
        for bytes in [b"not json\n".to_vec(), vec![b'x'; 4097]] {
            let (mut client, mut server) = socket_pair();
            client.write_all(&bytes).unwrap();
            assert!(read_json(&mut server).is_err());
        }
    }
    #[test]
    fn disconnected_read_does_not_mutate_command_state() {
        let (client, mut server) = socket_pair();
        let state = State::default();
        drop(client);
        assert!(read_json(&mut server).is_err());
        assert!(!state.stop_requested.load(Ordering::Acquire));
    }
    #[test]
    fn endpoint_guard_removes_owned_file() {
        let path = std::env::temp_dir().join(format!("probe-endpoint-test-{}", std::process::id()));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        let guard = Endpoint(path.clone());
        drop(file);
        drop(guard);
        assert!(!path.exists());
    }
    #[test]
    fn missing_audio_completion_is_a_failure() {
        let (sender, receiver) = mpsc::channel();
        assert!(!audio_completion(&receiver).unwrap());
        drop(sender); // Includes a panicked audio thread dropping its sender.
        assert!(audio_completion(&receiver).is_err());
    }
    #[test]
    fn audio_completion_preserves_success_and_failure() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(())).unwrap();
        assert!(audio_completion(&receiver).unwrap());
        sender.send(Err(anyhow::anyhow!("device failed"))).unwrap();
        assert!(audio_completion(&receiver).is_err());
    }
    #[test]
    fn invalid_command_preserves_state() {
        let s = State::default();
        assert!(apply(&s, "seek").is_err());
        assert!(!s.stop_requested.load(Ordering::Relaxed));
        assert!(!s.paused.load(Ordering::Relaxed));
    }
    #[test]
    fn native_seek_is_unsupported() {
        assert_eq!(native_command(&MediaControlEvent::Next), None);
        assert_eq!(native_command(&MediaControlEvent::Pause), Some("pause"));
    }
    #[test]
    fn command_transitions_do_not_write_consumption_counter() {
        let s = State::default();
        s.consumed.store(42, Ordering::Relaxed);
        apply(&s, "pause").unwrap();
        apply(&s, "resume").unwrap();
        assert_eq!(s.consumed.load(Ordering::Relaxed), 42);
    }
}
