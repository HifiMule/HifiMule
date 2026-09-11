use windows::{Media::Control::GlobalSystemMediaTransportControlsSessionManager, Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED}};
fn main() -> windows::core::Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)?; }
    let action = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
    let sessions = manager.GetSessions()?;
    for index in 0..sessions.Size()? {
        let session = sessions.GetAt(index)?;
        let props = session.TryGetMediaPropertiesAsync()?.get()?;
        if props.Title()?.to_string() != "Generated silent continuity fixtures" { continue; }
        println!("HifiMule test session: {:?}", session.GetPlaybackInfo()?.PlaybackStatus()?);
        let accepted = match action.as_str() {
            "pause" => session.TryPauseAsync()?.get()?,
            "play" => session.TryPlayAsync()?.get()?,
            "status" => true,
            _ => { eprintln!("unsupported action"); std::process::exit(2) }
        };
        println!("native {action} accepted={accepted}");
        if !accepted { std::process::exit(3) }
        return Ok(());
    }
    eprintln!("HifiMule generated-fixture SMTC session not found");
    std::process::exit(4)
}
