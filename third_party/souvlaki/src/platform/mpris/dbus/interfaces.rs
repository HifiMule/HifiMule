use std::{
    convert::{TryFrom, TryInto},
    sync::{Arc, Mutex},
    time::Duration,
};

use dbus::Path;
use dbus_crossroads::{Crossroads, IfaceBuilder};

use crate::{MediaControlEvent, MediaPlayback, MediaPosition, SeekDirection};

use super::controls::{ServiceState, create_metadata_dict};

// TODO: This type is super messed up, but it's the only way to get seeking working properly
// on graphical media controls using dbus-crossroads.
pub type SeekedSignal =
    Arc<Mutex<Option<Box<dyn Fn(&Path<'_>, &(i64,)) -> dbus::Message + Send + Sync>>>>;

pub fn register_methods<F>(
    state: &Arc<Mutex<ServiceState>>,
    event_handler: &Arc<Mutex<F>>,
    friendly_name: String,
    seeked_signal: SeekedSignal,
) -> Crossroads
where
    F: Fn(MediaControlEvent) -> bool + Send + 'static,
{
    let mut cr = Crossroads::new();
    let app_interface = cr.register("org.mpris.MediaPlayer2", {
        move |b| {
            b.property("Identity")
                .get(move |_, _| Ok(friendly_name.clone()));

            b.property("CanQuit")
                .get(|_, _| Ok(false))
                .emits_changed_true();
            b.property("CanRaise")
                .get(|_, _| Ok(false))
                .emits_changed_true();
            b.property("HasTracklist")
                .get(|_, _| Ok(false))
                .emits_changed_true();
            b.property("SupportedUriSchemes")
                .get(move |_, _| Ok(&[] as &[String]))
                .emits_changed_true();
            b.property("SupportedMimeTypes")
                .get(move |_, _| Ok(&[] as &[String]))
                .emits_changed_true();
        }
    });

    let player_interface = cr.register("org.mpris.MediaPlayer2.Player", |b| {
        register_method(b, event_handler, "Next", MediaControlEvent::Next);
        register_method(b, event_handler, "Previous", MediaControlEvent::Previous);
        register_method(b, event_handler, "Pause", MediaControlEvent::Pause);
        register_method(b, event_handler, "PlayPause", MediaControlEvent::Toggle);
        register_method(b, event_handler, "Stop", MediaControlEvent::Stop);
        register_method(b, event_handler, "Play", MediaControlEvent::Play);

        b.method("Seek", ("Offset",), (), {
            let state = state.clone();
            let event_handler = event_handler.clone();

            move |_, _, (offset,): (i64,)| {
                let abs_offset = offset.unsigned_abs();
                let direction = if offset > 0 {
                    SeekDirection::Forward
                } else {
                    SeekDirection::Backward
                };

                let capabilities = state.lock().unwrap().capabilities;
                if !capabilities.seek
                    || !(event_handler.lock().unwrap())(MediaControlEvent::SeekBy(
                        direction,
                        Duration::from_micros(abs_offset),
                    ))
                {
                    return Err(dbus::MethodErr::failed("seeking is unsupported"));
                }
                Ok(())
            }
        });

        b.method("SetPosition", ("TrackId", "Position"), (), {
            let state = state.clone();
            let event_handler = event_handler.clone();

            move |_, _, (trackid, position): (Path, i64)| {
                let state = state.lock().unwrap();

                // According to the MPRIS specification:

                if state.metadata.track_id.as_deref() != Some(trackid.to_string().as_str()) {
                    return Ok(());
                }

                if let Some(duration) = state.metadata.duration {
                    // If the Position argument is greater than the track length, do nothing.
                    if position > duration {
                        return Ok(());
                    }
                }

                // If the Position argument is less than 0, do nothing.
                if let Ok(position) = u64::try_from(position) {
                    let position = Duration::from_micros(position);

                    if !state.capabilities.seek {
                        return Err(dbus::MethodErr::failed("seeking is unsupported"));
                    }
                    if !(event_handler.lock().unwrap())(MediaControlEvent::SetPosition(
                        MediaPosition(position),
                    )) {
                        return Err(dbus::MethodErr::failed("seek was rejected"));
                    }
                }
                Ok(())
            }
        });

        b.method("OpenUri", ("Uri",), (), {
            move |_, _, (uri,): (String,)| {
                let _ = uri;
                Err::<(), _>(dbus::MethodErr::failed("open URI is unsupported"))
            }
        });

        *seeked_signal.lock().unwrap() =
            Some(b.signal::<(i64,), _>("Seeked", ("Position",)).msg_fn());

        b.property("PlaybackStatus")
            .get({
                let state = state.clone();
                move |_, _| {
                    let state = state.lock().unwrap();
                    Ok(state.get_playback_status().to_string())
                }
            })
            .emits_changed_true();

        b.property("Rate").get(|_, _| Ok(1.0)).emits_changed_true();

        b.property("Metadata")
            .get({
                let state = state.clone();
                move |_, _| Ok(create_metadata_dict(&state.lock().unwrap().metadata))
            })
            .emits_changed_true();

        b.property("Volume")
            .get({
                let state = state.clone();
                move |_, _| {
                    let state = state.lock().unwrap();
                    Ok(state.volume)
                }
            })
            .set({
                let event_handler = event_handler.clone();
                move |_, _, volume: f64| {
                    if (event_handler.lock().unwrap())(MediaControlEvent::SetVolume(volume)) {
                        Ok(Some(volume))
                    } else {
                        Err(dbus::MethodErr::failed("volume control is unsupported"))
                    }
                }
            })
            .emits_changed_true();

        b.property("Position").get({
            let state = state.clone();
            move |_, _| {
                let state = state.lock().unwrap();
                let progress: i64 = match state.playback_status {
                    MediaPlayback::Playing {
                        progress: Some(progress),
                    }
                    | MediaPlayback::Paused {
                        progress: Some(progress),
                    } => progress.0.as_micros(),
                    _ => 0,
                }
                .try_into()
                .unwrap();
                Ok(progress)
            }
        });

        b.property("MinimumRate")
            .get(|_, _| Ok(1.0))
            .emits_changed_true();
        b.property("MaximumRate")
            .get(|_, _| Ok(1.0))
            .emits_changed_true();

        b.property("CanGoNext")
            .get({
                let state = state.clone();
                move |_, _| Ok(state.lock().unwrap().capabilities.next)
            })
            .emits_changed_true();
        b.property("CanGoPrevious")
            .get({
                let state = state.clone();
                move |_, _| Ok(state.lock().unwrap().capabilities.previous)
            })
            .emits_changed_true();
        b.property("CanPlay")
            .get({
                let state = state.clone();
                move |_, _| Ok(state.lock().unwrap().capabilities.play)
            })
            .emits_changed_true();
        b.property("CanPause")
            .get({
                let state = state.clone();
                move |_, _| Ok(state.lock().unwrap().capabilities.pause)
            })
            .emits_changed_true();
        b.property("CanSeek")
            .get({
                let state = state.clone();
                move |_, _| Ok(state.lock().unwrap().capabilities.seek)
            })
            .emits_changed_true();
        b.property("CanControl")
            .get({
                let state = state.clone();
                move |_, _| {
                    let c = state.lock().unwrap().capabilities;
                    Ok(c.play || c.pause || c.toggle || c.stop)
                }
            })
            .emits_changed_true();
    });

    cr.insert(
        "/org/mpris/MediaPlayer2",
        &[app_interface, player_interface],
        (),
    );

    seeked_signal.lock().ok();

    cr
}

fn register_method<F>(
    b: &mut IfaceBuilder<()>,
    event_handler: &Arc<Mutex<F>>,
    name: &'static str,
    event: MediaControlEvent,
) where
    F: Fn(MediaControlEvent) -> bool + Send + 'static,
{
    let event_handler = event_handler.clone();

    b.method(name, (), (), move |_, _, _: ()| {
        if (event_handler.lock().unwrap())(event.clone()) {
            Ok(())
        } else {
            Err(dbus::MethodErr::failed("command was rejected"))
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaControlCapabilities;
    use crate::publication::OwnedMetadata;
    use dbus::{
        Message, MessageType,
        arg::{PropMap, Variant},
    };
    use std::cell::RefCell;

    fn dispatch(cr: &mut Crossroads, mut message: Message) -> Message {
        message.set_serial(1);
        let replies = RefCell::new(Vec::new());
        cr.handle_message(message, &replies).unwrap();
        let mut replies = replies.into_inner();
        assert_eq!(replies.len(), 1);
        replies.remove(0)
    }

    fn call(method: &str) -> Message {
        Message::new_method_call(
            "org.mpris.MediaPlayer2.hifimule",
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
            method,
        )
        .unwrap()
    }

    fn property(name: &str) -> Message {
        Message::new_method_call(
            "org.mpris.MediaPlayer2.hifimule",
            "/org/mpris/MediaPlayer2",
            "org.freedesktop.DBus.Properties",
            "Get",
        )
        .unwrap()
        .append2("org.mpris.MediaPlayer2.Player", name)
    }

    fn fixture<F: Fn(MediaControlEvent) -> bool + Send + 'static>(
        handler: F,
    ) -> (Arc<Mutex<ServiceState>>, Crossroads) {
        let state = Arc::new(Mutex::new(ServiceState {
            metadata: Default::default(),
            metadata_dict: Default::default(),
            playback_status: MediaPlayback::Stopped,
            volume: 1.0,
            capabilities: MediaControlCapabilities::default(),
        }));
        let cr = register_methods(
            &state,
            &Arc::new(Mutex::new(handler)),
            "HifiMule".into(),
            Arc::new(Mutex::new(None)),
        );
        (state, cr)
    }

    #[test]
    fn transport_calls_preserve_callback_admission_results() {
        for accepted in [true, false] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let captured = events.clone();
            let (_, mut cr) = fixture(move |event| {
                captured.lock().unwrap().push(event);
                accepted
            });
            let commands = [
                ("Play", MediaControlEvent::Play),
                ("Pause", MediaControlEvent::Pause),
                ("PlayPause", MediaControlEvent::Toggle),
                ("Stop", MediaControlEvent::Stop),
            ];
            for (name, expected) in commands {
                let reply = dispatch(&mut cr, call(name));
                assert_eq!(
                    reply.msg_type(),
                    if accepted {
                        MessageType::MethodReturn
                    } else {
                        MessageType::Error
                    }
                );
                assert_eq!(events.lock().unwrap().pop(), Some(expected));
            }
        }
    }

    #[test]
    fn properties_follow_capabilities_and_replace_metadata() {
        let (state, mut cr) = fixture(|_| true);
        let initial: Variant<bool> = dispatch(&mut cr, property("CanPlay")).read1().unwrap();
        assert!(!initial.0);
        {
            let mut state = state.lock().unwrap();
            state.capabilities.play = true;
            state.set_metadata(OwnedMetadata {
                title: Some("first".into()),
                artist: Some("artist".into()),
                duration: Some(9_000_000),
                ..Default::default()
            });
        }
        let ready: Variant<bool> = dispatch(&mut cr, property("CanPlay")).read1().unwrap();
        assert!(ready.0);
        let rich: Variant<PropMap> = dispatch(&mut cr, property("Metadata")).read1().unwrap();
        assert!(rich.0.contains_key("xesam:artist"));
        assert!(rich.0.contains_key("mpris:length"));
        state.lock().unwrap().set_metadata(OwnedMetadata::default());
        let cleared: Variant<PropMap> = dispatch(&mut cr, property("Metadata")).read1().unwrap();
        assert!(!cleared.0.contains_key("xesam:title"));
        assert!(!cleared.0.contains_key("xesam:artist"));
        assert!(!cleared.0.contains_key("mpris:length"));
    }

    #[test]
    fn seek_methods_preserve_signed_units_and_fence_stale_track_ids() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let (state, mut cr) = fixture(move |event| {
            captured.lock().unwrap().push(event);
            true
        });
        {
            let mut state = state.lock().unwrap();
            state.capabilities.seek = true;
            state.set_metadata(OwnedMetadata {
                track_id: Some("/org/mpris/MediaPlayer2/track/current".into()),
                duration: Some(9_000_000),
                ..Default::default()
            });
        }
        assert_eq!(
            dispatch(&mut cr, call("Seek").append1(-1_500_000_i64)).msg_type(),
            MessageType::MethodReturn
        );
        assert_eq!(
            dispatch(
                &mut cr,
                call("SetPosition").append2(
                    Path::new("/org/mpris/MediaPlayer2/track/stale").unwrap(),
                    2_000_000_i64,
                )
            )
            .msg_type(),
            MessageType::MethodReturn
        );
        assert_eq!(
            dispatch(
                &mut cr,
                call("SetPosition").append2(
                    Path::new("/org/mpris/MediaPlayer2/track/current").unwrap(),
                    2_000_000_i64,
                )
            )
            .msg_type(),
            MessageType::MethodReturn
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                MediaControlEvent::SeekBy(
                    SeekDirection::Backward,
                    Duration::from_micros(1_500_000)
                ),
                MediaControlEvent::SetPosition(MediaPosition(Duration::from_micros(2_000_000))),
            ]
        );
    }

    #[test]
    fn unsupported_seek_and_open_uri_do_not_reach_callback() {
        let (_, mut cr) = fixture(|_| panic!("unsupported event reached callback"));
        assert_eq!(
            dispatch(&mut cr, call("Seek").append1(1_000_i64)).msg_type(),
            MessageType::Error
        );
        assert_eq!(
            dispatch(
                &mut cr,
                call("SetPosition").append2(Path::new("/").unwrap(), 1_000_i64)
            )
            .msg_type(),
            MessageType::Error
        );
        assert_eq!(
            dispatch(&mut cr, call("OpenUri").append1("file:///unused")).msg_type(),
            MessageType::Error
        );
    }
}
