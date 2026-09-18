#![cfg(any(target_os = "macos", target_os = "ios"))]
#![allow(non_upper_case_globals)]

#[cfg(target_os = "ios")]
use std::fs;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use block::ConcreteBlock;
use cocoa::{
    base::{NO, YES, id, nil},
    foundation::{NSInteger, NSString, NSUInteger},
};
use core_graphics::geometry::CGSize;

use dispatch::{Queue, QueuePriority};
use objc::{class, msg_send, rc::StrongPtr, sel, sel_impl};

use crate::{
    MediaControlCapabilities, MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition,
    PlatformConfig,
};

/// A platform-specific error.
#[derive(Debug)]
pub struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "Error")
    }
}

impl std::error::Error for Error {}

/// A handle to OS media controls.
pub struct MediaControls {
    capabilities: MediaControlCapabilities,
}

impl MediaControls {
    /// Create media controls with the specified config.
    pub fn new(_config: PlatformConfig) -> Result<Self, Error> {
        Ok(Self {
            capabilities: MediaControlCapabilities::default(),
        })
    }

    /// Attach the media control events to a handler.
    pub fn attach<F>(&mut self, event_handler: F) -> Result<(), Error>
    where
        F: Fn(MediaControlEvent) + Send + 'static,
    {
        unsafe {
            attach_command_handlers(
                Arc::new(move |event| {
                    event_handler(event);
                    true
                }),
                self.capabilities,
            )
        };
        Ok(())
    }

    /// Attach a handler that can reject delivery synchronously.
    pub fn attach_checked<F>(&mut self, event_handler: F) -> Result<(), Error>
    where
        F: Fn(MediaControlEvent) -> bool + Send + 'static,
    {
        unsafe { attach_command_handlers(Arc::new(event_handler), self.capabilities) };
        Ok(())
    }

    pub fn set_capabilities(
        &mut self,
        capabilities: MediaControlCapabilities,
    ) -> Result<(), Error> {
        self.capabilities = capabilities;
        unsafe { set_command_capabilities(capabilities) };
        Ok(())
    }

    /// Detach the event handler.
    pub fn detach(&mut self) -> Result<(), Error> {
        unsafe {
            detach_command_handlers();
            clear_now_playing();
        };
        Ok(())
    }

    /// Set the current playback status.
    pub fn set_playback(&mut self, playback: MediaPlayback) -> Result<(), Error> {
        unsafe { set_playback_status(playback) };
        Ok(())
    }

    pub fn set_seeked(&mut self, _position_micros: i64) -> Result<(), Error> {
        Ok(())
    }

    /// Set the metadata of the currently playing media item.
    pub fn set_metadata(&mut self, metadata: MediaMetadata) -> Result<(), Error> {
        unsafe { set_playback_metadata(metadata) };
        Ok(())
    }
}

// MPNowPlayingPlaybackState
const MPNowPlayingPlaybackStatePlaying: NSUInteger = 1;
const MPNowPlayingPlaybackStatePaused: NSUInteger = 2;
const MPNowPlayingPlaybackStateStopped: NSUInteger = 3;

// MPRemoteCommandHandlerStatus
const MPRemoteCommandHandlerStatusSuccess: NSInteger = 0;
const MPRemoteCommandHandlerStatusCommandFailed: NSInteger = 200;

extern "C" {
    static MPMediaItemPropertyTitle: id; // NSString
    static MPMediaItemPropertyArtist: id; // NSString
    static MPMediaItemPropertyAlbumTitle: id; // NSString
    static MPMediaItemPropertyArtwork: id; // NSString
    static MPMediaItemPropertyPlaybackDuration: id; // NSString
    static MPNowPlayingInfoPropertyElapsedPlaybackTime: id; // NSString
}

unsafe fn set_playback_status(playback: MediaPlayback) {
    let media_center: id = msg_send!(class!(MPNowPlayingInfoCenter), defaultCenter);
    let state = match playback {
        MediaPlayback::Stopped => MPNowPlayingPlaybackStateStopped,
        MediaPlayback::Paused { .. } => MPNowPlayingPlaybackStatePaused,
        MediaPlayback::Playing { .. } => MPNowPlayingPlaybackStatePlaying,
    };
    let _: () = msg_send!(media_center, setPlaybackState: state);
    if let MediaPlayback::Paused {
        progress: Some(progress),
    }
    | MediaPlayback::Playing {
        progress: Some(progress),
    } = playback
    {
        set_playback_progress(progress.0);
    }
}

static GLOBAL_METADATA_COUNTER: AtomicUsize = AtomicUsize::new(1);

unsafe fn set_playback_metadata(metadata: MediaMetadata) {
    let prev_counter = GLOBAL_METADATA_COUNTER.fetch_add(1, Ordering::SeqCst);
    let media_center: id = msg_send!(class!(MPNowPlayingInfoCenter), defaultCenter);
    let now_playing: id = msg_send!(class!(NSMutableDictionary), dictionary);
    if let Some(title) = metadata.title {
        let title = ns_string(title);
        let _: () = msg_send!(now_playing, setObject: *title
                                              forKey: MPMediaItemPropertyTitle);
    }
    if let Some(artist) = metadata.artist {
        let artist = ns_string(artist);
        let _: () = msg_send!(now_playing, setObject: *artist
                                              forKey: MPMediaItemPropertyArtist);
    }
    if let Some(album) = metadata.album {
        let album = ns_string(album);
        let _: () = msg_send!(now_playing, setObject: *album
                                              forKey: MPMediaItemPropertyAlbumTitle);
    }
    if let Some(duration) = metadata.duration {
        let _: () = msg_send!(now_playing, setObject: ns_number(duration.as_secs_f64())
                                              forKey: MPMediaItemPropertyPlaybackDuration);
    }
    if let Some(cover_url) = metadata.cover_url {
        let cover_url = cover_url.to_owned();
        Queue::global(QueuePriority::Default).exec_async(move || {
            load_and_set_playback_artwork(cover_url, prev_counter + 1);
        });
    }
    let _: () = msg_send!(media_center, setNowPlayingInfo: now_playing);
}

unsafe fn load_and_set_playback_artwork(url: String, for_counter: usize) {
    let (image, size) = load_image_from_url(&url);
    let artwork = mp_artwork(image, size);
    if GLOBAL_METADATA_COUNTER.load(Ordering::SeqCst) == for_counter {
        set_playback_artwork(artwork);
    }
}

unsafe fn set_playback_artwork(artwork: id) {
    let media_center: id = msg_send!(class!(MPNowPlayingInfoCenter), defaultCenter);
    let now_playing: id = msg_send!(class!(NSMutableDictionary), dictionary);
    let prev_now_playing: id = msg_send!(media_center, nowPlayingInfo);
    let _: () = msg_send!(now_playing, addEntriesFromDictionary: prev_now_playing);
    let _: () = msg_send!(now_playing, setObject: artwork
                                          forKey: MPMediaItemPropertyArtwork);
    let _: () = msg_send!(media_center, setNowPlayingInfo: now_playing);
}

unsafe fn set_playback_progress(progress: Duration) {
    let media_center: id = msg_send!(class!(MPNowPlayingInfoCenter), defaultCenter);
    let now_playing: id = msg_send!(class!(NSMutableDictionary), dictionary);
    let prev_now_playing: id = msg_send!(media_center, nowPlayingInfo);
    let _: () = msg_send!(now_playing, addEntriesFromDictionary: prev_now_playing);
    let _: () = msg_send!(now_playing, setObject: ns_number(progress.as_secs_f64())
                                          forKey: MPNowPlayingInfoPropertyElapsedPlaybackTime);
    let _: () = msg_send!(media_center, setNowPlayingInfo: now_playing);
}

unsafe fn command_status(accepted: bool) -> NSInteger {
    if accepted {
        MPRemoteCommandHandlerStatusSuccess
    } else {
        MPRemoteCommandHandlerStatusCommandFailed
    }
}

unsafe fn attach_command_handlers(
    handler: Arc<dyn Fn(MediaControlEvent) -> bool>,
    capabilities: MediaControlCapabilities,
) {
    detach_command_handlers();
    let command_center: id = msg_send!(class!(MPRemoteCommandCenter), sharedCommandCenter);

    // togglePlayPauseCommand
    let play_pause_handler = ConcreteBlock::new({
        let handler = handler.clone();
        move |_event: id| -> NSInteger { command_status((handler)(MediaControlEvent::Toggle)) }
    })
    .copy();
    let cmd: id = msg_send!(command_center, togglePlayPauseCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.toggle { YES } else { NO });
    let _: () = msg_send!(cmd, addTargetWithHandler: play_pause_handler);

    // playCommand
    let play_handler = ConcreteBlock::new({
        let handler = handler.clone();
        move |_event: id| -> NSInteger { command_status((handler)(MediaControlEvent::Play)) }
    })
    .copy();
    let cmd: id = msg_send!(command_center, playCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.play { YES } else { NO });
    let _: () = msg_send!(cmd, addTargetWithHandler: play_handler);

    // pauseCommand
    let pause_handler = ConcreteBlock::new({
        let handler = handler.clone();
        move |_event: id| -> NSInteger { command_status((handler)(MediaControlEvent::Pause)) }
    })
    .copy();
    let cmd: id = msg_send!(command_center, pauseCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.pause { YES } else { NO });
    let _: () = msg_send!(cmd, addTargetWithHandler: pause_handler);

    // stopCommand (missing from upstream 0.8.3)
    let stop_handler = ConcreteBlock::new({
        let handler = handler.clone();
        move |_event: id| -> NSInteger { command_status((handler)(MediaControlEvent::Stop)) }
    })
    .copy();
    let cmd: id = msg_send!(command_center, stopCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.stop { YES } else { NO });
    let _: () = msg_send!(cmd, addTargetWithHandler: stop_handler);

    // previousTrackCommand
    let previous_track_handler = ConcreteBlock::new({
        let handler = handler.clone();
        move |_event: id| -> NSInteger { command_status((handler)(MediaControlEvent::Previous)) }
    })
    .copy();
    let cmd: id = msg_send!(command_center, previousTrackCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.previous { YES } else { NO });
    let _: () = msg_send!(cmd, addTargetWithHandler: previous_track_handler);

    // nextTrackCommand
    let next_track_handler = ConcreteBlock::new({
        let handler = handler.clone();
        move |_event: id| -> NSInteger { command_status((handler)(MediaControlEvent::Next)) }
    })
    .copy();
    let cmd: id = msg_send!(command_center, nextTrackCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.next { YES } else { NO });
    let _: () = msg_send!(cmd, addTargetWithHandler: next_track_handler);

    // changePlaybackPositionCommand
    let position_handler = ConcreteBlock::new({
        let handler = handler.clone();
        // event of type MPChangePlaybackPositionCommandEvent
        move |event: id| -> NSInteger {
            let position: f64 = msg_send![event, positionTime];
            if !position.is_finite() || position < 0.0 {
                return command_status(false);
            }
            command_status((handler)(MediaControlEvent::SetPosition(MediaPosition(
                Duration::from_secs_f64(position),
            ))))
        }
    })
    .copy();
    let cmd: id = msg_send!(command_center, changePlaybackPositionCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.seek { YES } else { NO });
    let _: () = msg_send!(cmd, addTargetWithHandler: position_handler);
}

unsafe fn detach_command_handlers() {
    let command_center: id = msg_send!(class!(MPRemoteCommandCenter), sharedCommandCenter);

    let cmd: id = msg_send!(command_center, togglePlayPauseCommand);
    let _: () = msg_send!(cmd, setEnabled: NO);
    let _: () = msg_send!(cmd, removeTarget: nil);

    let cmd: id = msg_send!(command_center, playCommand);
    let _: () = msg_send!(cmd, setEnabled: NO);
    let _: () = msg_send!(cmd, removeTarget: nil);

    let cmd: id = msg_send!(command_center, pauseCommand);
    let _: () = msg_send!(cmd, setEnabled: NO);
    let _: () = msg_send!(cmd, removeTarget: nil);

    let cmd: id = msg_send!(command_center, stopCommand);
    let _: () = msg_send!(cmd, setEnabled: NO);
    let _: () = msg_send!(cmd, removeTarget: nil);

    let cmd: id = msg_send!(command_center, previousTrackCommand);
    let _: () = msg_send!(cmd, setEnabled: NO);
    let _: () = msg_send!(cmd, removeTarget: nil);

    let cmd: id = msg_send!(command_center, nextTrackCommand);
    let _: () = msg_send!(cmd, setEnabled: NO);
    let _: () = msg_send!(cmd, removeTarget: nil);

    let cmd: id = msg_send!(command_center, changePlaybackPositionCommand);
    let _: () = msg_send!(cmd, setEnabled: NO);
    let _: () = msg_send!(cmd, removeTarget: nil);
}

unsafe fn set_command_capabilities(capabilities: MediaControlCapabilities) {
    let command_center: id = msg_send!(class!(MPRemoteCommandCenter), sharedCommandCenter);
    let cmd: id = msg_send!(command_center, togglePlayPauseCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.toggle { YES } else { NO });
    let cmd: id = msg_send!(command_center, playCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.play { YES } else { NO });
    let cmd: id = msg_send!(command_center, pauseCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.pause { YES } else { NO });
    let cmd: id = msg_send!(command_center, stopCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.stop { YES } else { NO });
    let cmd: id = msg_send!(command_center, previousTrackCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.previous { YES } else { NO });
    let cmd: id = msg_send!(command_center, nextTrackCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.next { YES } else { NO });
    let cmd: id = msg_send!(command_center, changePlaybackPositionCommand);
    let _: () = msg_send!(cmd, setEnabled: if capabilities.seek { YES } else { NO });
}

unsafe fn clear_now_playing() {
    GLOBAL_METADATA_COUNTER.fetch_add(1, Ordering::SeqCst);
    let media_center: id = msg_send!(class!(MPNowPlayingInfoCenter), defaultCenter);
    let _: () = msg_send!(media_center, setNowPlayingInfo: nil);
    let _: () = msg_send!(media_center, setPlaybackState: MPNowPlayingPlaybackStateStopped);
}

// Own the +1 allocation until the receiving Cocoa API has retained/copied it.
// This also works on threads without an autorelease pool.
unsafe fn ns_string(value: &str) -> StrongPtr {
    StrongPtr::new(NSString::alloc(nil).init_str(value))
}

unsafe fn ns_number(value: f64) -> id {
    let number: id = msg_send!(class!(NSNumber), numberWithDouble: value);
    number
}

unsafe fn ns_url(value: &str) -> id {
    let value = ns_string(value);
    let url: id = msg_send!(class!(NSURL), URLWithString: *value);
    url
}

#[cfg(target_os = "ios")]
unsafe fn load_image_from_url(url: &str) -> (id, CGSize) {
    let image_data = fs::read(&url).unwrap();
    let base64_data = base64::encode(image_data);
    let base64_ns_string = ns_string(&base64_data);

    let ns_data: id = msg_send!(class!(NSData), alloc);
    let ns_data: id = msg_send!(ns_data, initWithBase64EncodedString: *base64_ns_string
                                          options: 0);
    if ns_data == nil {
        return (nil, CGSize::new(0.0, 0.0));
    }
    let image: id = msg_send!(class!(UIImage), imageWithData: ns_data);
    if image == nil {
        return (nil, CGSize::new(0.0, 0.0));
    }
    let size: CGSize = msg_send!(image, size);
    (image, size)
}

#[cfg(target_os = "macos")]
unsafe fn load_image_from_url(url: &str) -> (id, CGSize) {
    let url = ns_url(url);
    let image: id = msg_send!(class!(NSImage), alloc);
    let image: id = msg_send!(image, initWithContentsOfURL: url);
    let size: CGSize = msg_send!(image, size);
    (image, CGSize::new(size.width, size.height))
}

#[cfg(target_os = "ios")]
unsafe fn mp_artwork(image: id, bounds: CGSize) -> id {
    let artwork: id = msg_send!(class!(MPMediaItemArtwork), alloc);
    let artwork: id = msg_send!(artwork, initWithImage: image);
    artwork
}

#[cfg(target_os = "macos")]
unsafe fn mp_artwork(image: id, bounds: CGSize) -> id {
    let handler = ConcreteBlock::new(move |_size: CGSize| -> id { image }).copy();
    let artwork: id = msg_send!(class!(MPMediaItemArtwork), alloc);
    let artwork: id = msg_send!(artwork, initWithBoundsSize: bounds
                                         requestHandler: handler);
    artwork
}
