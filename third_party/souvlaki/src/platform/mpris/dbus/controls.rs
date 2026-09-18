use dbus::Path;
use dbus::arg::{RefArg, Variant};
use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::ffidisp::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged;
use dbus::message::SignalArgs;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::super::Error;
use crate::publication::{InternalEvent, OwnedMetadata, PendingUpdates};
use crate::{
    MediaControlCapabilities, MediaControlEvent, MediaMetadata, MediaPlayback, PlatformConfig,
};

/// A handle to OS media controls.
pub struct MediaControls {
    thread: Option<ServiceThreadHandle>,
    dbus_name: String,
    friendly_name: String,
    capabilities: MediaControlCapabilities,
}

struct ServiceThreadHandle {
    pending: Arc<Mutex<PendingUpdates>>,
    thread: JoinHandle<Result<(), Error>>,
}

#[derive(Debug)]
pub struct ServiceState {
    pub metadata: OwnedMetadata,
    pub metadata_dict: HashMap<String, Variant<Box<dyn RefArg>>>,
    pub playback_status: MediaPlayback,
    pub volume: f64,
    pub capabilities: MediaControlCapabilities,
}

impl ServiceState {
    pub fn set_metadata(&mut self, metadata: OwnedMetadata) {
        self.metadata_dict = create_metadata_dict(&metadata);
        self.metadata = metadata;
    }

    pub fn get_playback_status(&self) -> &'static str {
        match self.playback_status {
            MediaPlayback::Playing { .. } => "Playing",
            MediaPlayback::Paused { .. } => "Paused",
            MediaPlayback::Stopped => "Stopped",
        }
    }
}

pub fn create_metadata_dict(metadata: &OwnedMetadata) -> HashMap<String, Variant<Box<dyn RefArg>>> {
    let mut dict = HashMap::<String, Variant<Box<dyn RefArg>>>::new();

    let mut insert = |k: &str, v| dict.insert(k.to_string(), Variant(v));

    let OwnedMetadata {
        ref track_id,
        ref title,
        ref album,
        ref artist,
        ref cover_url,
        ref duration,
    } = metadata;

    let path = track_id
        .as_deref()
        .and_then(|value| Path::new(value).ok())
        .unwrap_or_else(|| Path::new("/org/mpris/MediaPlayer2/track/none").unwrap());

    // MPRIS
    insert("mpris:trackid", Box::new(path));

    if let Some(length) = duration {
        insert("mpris:length", Box::new(*length));
    }
    if let Some(cover_url) = cover_url {
        insert("mpris:artUrl", Box::new(cover_url.clone()));
    }

    // Xesam
    if let Some(title) = title {
        insert("xesam:title", Box::new(title.clone()));
    }
    if let Some(artist) = artist {
        insert("xesam:artist", Box::new(vec![artist.clone()]));
    }
    if let Some(album) = album {
        insert("xesam:album", Box::new(album.clone()));
    }

    dict
}

impl MediaControls {
    /// Create media controls with the specified config.
    pub fn new(config: PlatformConfig) -> Result<Self, Error> {
        let PlatformConfig {
            dbus_name,
            display_name,
            ..
        } = config;

        Ok(Self {
            thread: None,
            dbus_name: dbus_name.to_string(),
            friendly_name: display_name.to_string(),
            capabilities: MediaControlCapabilities::default(),
        })
    }

    /// Attach the media control events to a handler.
    pub fn attach<F>(&mut self, event_handler: F) -> Result<(), Error>
    where
        F: Fn(MediaControlEvent) + Send + 'static,
    {
        self.attach_checked(move |event| {
            event_handler(event);
            true
        })
    }

    pub fn attach_checked<F>(&mut self, event_handler: F) -> Result<(), Error>
    where
        F: Fn(MediaControlEvent) -> bool + Send + 'static,
    {
        self.detach()?;

        let dbus_name = self.dbus_name.clone();
        let friendly_name = self.friendly_name.clone();
        let pending = Arc::new(Mutex::new(PendingUpdates::default()));
        let worker_pending = pending.clone();

        // Check if the connection can be created BEFORE spawning the new thread
        let conn = Connection::new_session()?;
        let name = format!("org.mpris.MediaPlayer2.{}", dbus_name);
        conn.request_name(name, false, true, false)?;

        let capabilities = self.capabilities;
        self.thread = Some(ServiceThreadHandle {
            pending,
            thread: thread::spawn(move || {
                let result = run_service(
                    conn,
                    friendly_name,
                    capabilities,
                    event_handler,
                    &worker_pending,
                );
                worker_pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(InternalEvent::Kill);
                result
            }),
        });
        Ok(())
    }

    pub fn set_capabilities(
        &mut self,
        capabilities: MediaControlCapabilities,
    ) -> Result<(), Error> {
        self.capabilities = capabilities;
        if self.thread.is_some() {
            self.send_internal_event(InternalEvent::ChangeCapabilities(capabilities))?;
        }
        Ok(())
    }

    /// Detach the event handler.
    pub fn detach(&mut self) -> Result<(), Error> {
        if let Some(ServiceThreadHandle { pending, thread }) = self.thread.take() {
            // The terminal flag discards queued publications before joining.
            pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(InternalEvent::Kill);
            // One error in case the thread panics, and the other one in case the
            // thread has returned an error.
            thread.join().map_err(|_| Error::ThreadPanicked)??;
        }
        Ok(())
    }

    /// Set the current playback status.
    pub fn set_playback(&mut self, playback: MediaPlayback) -> Result<(), Error> {
        self.send_internal_event(InternalEvent::ChangePlayback(playback))
    }

    /// Set the metadata of the currently playing media item.
    pub fn set_metadata(&mut self, metadata: MediaMetadata) -> Result<(), Error> {
        self.send_internal_event(InternalEvent::ChangeMetadata(metadata.into()))
    }

    /// Set the volume level (0.0-1.0) (Only available on MPRIS)
    pub fn set_volume(&mut self, volume: f64) -> Result<(), Error> {
        self.send_internal_event(InternalEvent::ChangeVolume(volume))
    }

    /// Publish a confirmed media-time discontinuity.
    pub fn set_seeked(&mut self, position_micros: i64) -> Result<(), Error> {
        self.send_internal_event(InternalEvent::Seeked(position_micros))
    }

    fn send_internal_event(&mut self, event: InternalEvent) -> Result<(), Error> {
        let thread = &self.thread.as_ref().ok_or(Error::ThreadNotRunning)?;
        if thread.thread.is_finished() {
            return Err(Error::ThreadPanicked);
        }
        if thread
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(event)
        {
            Ok(())
        } else {
            Err(Error::ThreadNotRunning)
        }
    }
}

fn run_service<F>(
    conn: Connection,
    friendly_name: String,
    capabilities: MediaControlCapabilities,
    event_handler: F,
    pending: &Arc<Mutex<PendingUpdates>>,
) -> Result<(), Error>
where
    F: Fn(MediaControlEvent) -> bool + Send + 'static,
{
    let state = Arc::new(Mutex::new(ServiceState {
        metadata: Default::default(),
        metadata_dict: create_metadata_dict(&Default::default()),
        playback_status: MediaPlayback::Stopped,
        volume: 1.0,
        capabilities,
    }));
    let event_handler = Arc::new(Mutex::new(event_handler));
    let seeked_signal = Arc::new(Mutex::new(None));

    let mut cr = super::interfaces::register_methods(
        &state,
        &event_handler,
        friendly_name,
        seeked_signal.clone(),
    );

    conn.start_receive(
        dbus::message::MatchRule::new_method_call(),
        Box::new(move |msg, conn| {
            cr.handle_message(msg, conn).unwrap();
            true
        }),
    );

    loop {
        let Some(events) = pending.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            break;
        };
        if !events.is_empty() {
            let mut changed_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
            for event in events {
                match event {
                    InternalEvent::ChangeMetadata(metadata) => {
                        let mut state = state.lock().unwrap();
                        state.set_metadata(metadata);
                        changed_properties.insert(
                            "Metadata".to_owned(),
                            Variant(state.metadata_dict.box_clone()),
                        );
                    }
                    InternalEvent::ChangePlayback(playback) => {
                        let mut state = state.lock().unwrap();
                        state.playback_status = playback;
                        changed_properties.insert(
                            "PlaybackStatus".to_owned(),
                            Variant(Box::new(state.get_playback_status().to_string())),
                        );
                    }
                    InternalEvent::ChangeVolume(volume) => {
                        let mut state = state.lock().unwrap();
                        state.volume = volume;
                        changed_properties.insert("Volume".to_owned(), Variant(Box::new(volume)));
                    }
                    InternalEvent::ChangeCapabilities(capabilities) => {
                        let mut state = state.lock().unwrap();
                        state.capabilities = capabilities;
                        for property in [
                            "CanGoNext",
                            "CanGoPrevious",
                            "CanPlay",
                            "CanPause",
                            "CanSeek",
                            "CanControl",
                        ] {
                            changed_properties.insert(
                                property.to_owned(),
                                Variant(Box::new(match property {
                                    "CanGoNext" => capabilities.next,
                                    "CanGoPrevious" => capabilities.previous,
                                    "CanPlay" => capabilities.play,
                                    "CanPause" => capabilities.pause,
                                    "CanSeek" => capabilities.seek,
                                    _ => {
                                        capabilities.play
                                            || capabilities.pause
                                            || capabilities.toggle
                                            || capabilities.stop
                                    }
                                })),
                            );
                        }
                    }
                    InternalEvent::Seeked(position) => {
                        if let Some(signal) = seeked_signal
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .as_ref()
                        {
                            let path = Path::new("/org/mpris/MediaPlayer2").unwrap();
                            let _ = conn.send(signal(&path, &(position,)));
                        }
                    }
                    InternalEvent::Kill => (),
                }
            }
            let properties_changed = PropertiesPropertiesChanged {
                interface_name: "org.mpris.MediaPlayer2.Player".to_owned(),
                changed_properties,
                invalidated_properties: Vec::new(),
            };

            conn.send(
                properties_changed.to_emit_message(&Path::new("/org/mpris/MediaPlayer2").unwrap()),
            )
            .ok();
        }
        // Bound both publication latency and detach's join independently of
        // whether another process happens to send D-Bus traffic.
        conn.process(Duration::from_millis(20))?;
    }

    Ok(())
}
