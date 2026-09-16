//! Thread-confined Pulse stream. The caller owns and joins the containing worker.
//! DONT_MOVE pins the named sink; all transport transitions wait for server ACKs.
use libpulse_binding as pulse;
use pulse::{
    context::{Context, State as ContextState},
    def::BufferAttr,
    mainloop::standard::{IterateResult, Mainloop},
    sample::{Format, Spec},
    stream::{FlagSet, State, Stream},
};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct PinnedStream {
    stream: Stream,
    context: Context,
    mainloop: Mainloop,
    moved: Rc<Cell<bool>>,
    name: String,
    pub rate: u32,
    pub channels: u16,
    pub max_bytes: usize,
    corked: bool,
}

impl PinnedStream {
    pub fn open(
        preference: &super::OutputPreference,
        deadline: Instant,
    ) -> Result<Self, &'static str> {
        let name = &preference.stable_id;
        let mut mainloop = Mainloop::new().ok_or("OUTPUT_UNAVAILABLE")?;
        let mut context =
            Context::new(&mainloop, "HifiMule playback").ok_or("OUTPUT_UNAVAILABLE")?;
        context
            .connect(None, pulse::context::FlagSet::NOAUTOSPAWN, None)
            .map_err(|_| "OUTPUT_UNAVAILABLE")?;
        while context.get_state() != ContextState::Ready {
            if Instant::now() >= deadline {
                return Err("OUTPUT_UNAVAILABLE");
            }
            if matches!(
                context.get_state(),
                ContextState::Failed | ContextState::Terminated
            ) {
                return Err("OUTPUT_UNAVAILABLE");
            }
            if !matches!(mainloop.iterate(false), IterateResult::Success(_)) {
                return Err("OUTPUT_UNAVAILABLE");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        // Explicit float32 shared mixing is supported by Pulse/PipeWire. The
        // server converts to the sink format; no hardware format is changed.
        let rate = 48_000;
        let channels = 2;
        let spec = Spec {
            format: Format::FLOAT32NE,
            channels: channels as u8,
            rate,
        };
        let max_bytes = (rate as usize * channels as usize * 4 / 10).min(1024 * 1024);
        let attrs = BufferAttr {
            maxlength: max_bytes as u32,
            tlength: max_bytes as u32,
            prebuf: 0,
            minreq: rate * channels as u32 * 4 / 100,
            fragsize: 0,
        };
        let mut stream =
            Stream::new(&mut context, "Music", &spec, None).ok_or("OUTPUT_UNAVAILABLE")?;
        let moved = Rc::new(Cell::new(false));
        let signal = moved.clone();
        stream.set_moved_callback(Some(Box::new(move || signal.set(true))));
        stream
            .connect_playback(
                Some(name),
                Some(&attrs),
                FlagSet::DONT_MOVE
                    | FlagSet::START_CORKED
                    | FlagSet::AUTO_TIMING_UPDATE
                    | FlagSet::INTERPOLATE_TIMING,
                None,
                None,
            )
            .map_err(|_| "OUTPUT_UNAVAILABLE")?;
        let mut result = Self {
            stream,
            context,
            mainloop,
            moved,
            name: name.into(),
            rate,
            channels,
            max_bytes,
            corked: true,
        };
        while result.stream.get_state() != State::Ready {
            if Instant::now() >= deadline {
                return Err("OUTPUT_UNAVAILABLE");
            }
            result.pump()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        result.verify()?;
        // Validate the concrete sink actually opened, while it is still corked.
        // This targeted query shares the stream's connection and does not start
        // another inventory enumeration alongside the owned discovery worker.
        if result.verify_identity(preference, deadline)? {
            result.verify_discovery_admission(preference, deadline)?;
        }
        let actual = result
            .stream
            .get_buffer_attr()
            .ok_or("OUTPUT_SHARED_UNSUPPORTED")?;
        if actual.maxlength as usize > max_bytes
            || actual.tlength as usize > max_bytes
            || actual.maxlength == 0
            || actual.tlength == 0
            || actual.minreq > actual.tlength
        {
            return Err("OUTPUT_SHARED_UNSUPPORTED");
        }
        result.max_bytes = actual.maxlength as usize;
        Ok(result)
    }

    fn verify_identity(
        &mut self,
        preference: &super::OutputPreference,
        deadline: Instant,
    ) -> Result<bool, &'static str> {
        use pulse::callbacks::ListResult;
        let matches = Rc::new(Cell::new(None));
        let found = matches.clone();
        let expected = preference.clone();
        let mut operation =
            self.context
                .introspect()
                .get_sink_info_by_name(&self.name, move |item| {
                    if let ListResult::Item(info) = item {
                        let same = info.name.as_deref() == Some(expected.stable_id.as_str())
                            && expected.identity_properties.iter().all(|(key, value)| {
                                if key == "port" {
                                    info.active_port
                                        .as_ref()
                                        .and_then(|port| port.name.as_deref())
                                        == Some(value.as_str())
                                } else {
                                    info.proplist.get_str(key).as_deref() == Some(value.as_str())
                                }
                            });
                        if same {
                            let active_port_known = info
                                .active_port
                                .as_ref()
                                .and_then(|port| port.name.as_deref())
                                .is_some_and(|active| {
                                    info.ports
                                        .iter()
                                        .any(|port| port.name.as_deref() == Some(active))
                                });
                            found.set(Some(
                                info.flags.contains(pulse::def::SinkFlagSet::HARDWARE)
                                    && info.ports.len() > 1
                                    && active_port_known,
                            ));
                        }
                    }
                });
        while operation.get_state() == pulse::operation::State::Running {
            if Instant::now() >= deadline {
                operation.cancel();
                return Err("OUTPUT_UNAVAILABLE");
            }
            self.pump()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        matches.get().ok_or("OUTPUT_IDENTITY_AMBIGUOUS")
    }

    fn verify_discovery_admission(
        &self,
        preference: &super::OutputPreference,
        deadline: Instant,
    ) -> Result<(), &'static str> {
        let discovery = super::pulse::discover_until(deadline);
        let output = discovery
            .outputs
            .iter()
            .find(|output| output.output_id == super::output_id(preference) && output.available)
            .ok_or("OUTPUT_SHARED_UNSUPPORTED")?;
        if output.identity_confidence != "fallback" || discovery.error.is_some() {
            return Err("OUTPUT_SHARED_UNSUPPORTED");
        }
        Ok(())
    }

    fn verify(&self) -> Result<(), &'static str> {
        if self.moved.get()
            || (self.stream.get_state() == State::Ready
                && self.stream.get_device_name().as_deref() != Some(&self.name))
        {
            return Err("OUTPUT_LOST");
        }
        if matches!(self.stream.get_state(), State::Failed | State::Terminated)
            || matches!(
                self.context.get_state(),
                ContextState::Failed | ContextState::Terminated
            )
        {
            return Err("OUTPUT_LOST");
        }
        Ok(())
    }

    fn pump_transport(&mut self) -> Result<(), &'static str> {
        if !matches!(self.mainloop.iterate(false), IterateResult::Success(_))
            || matches!(self.stream.get_state(), State::Failed | State::Terminated)
            || matches!(
                self.context.get_state(),
                ContextState::Failed | ContextState::Terminated
            )
        {
            return Err("OUTPUT_LOST");
        }
        Ok(())
    }

    pub fn pump(&mut self) -> Result<(), &'static str> {
        self.pump_transport()?;
        self.verify()
    }

    pub fn writable_samples(&self) -> usize {
        self.stream.writable_size().unwrap_or(0).min(self.max_bytes) / (self.channels as usize * 4)
            * self.channels as usize
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<(), &'static str> {
        self.verify()?;
        if samples.len() * 4 > self.max_bytes
            || !samples.len().is_multiple_of(self.channels as usize)
        {
            return Err("OUTPUT_SWITCH_FAILED");
        }
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_ne_bytes())
            .collect();
        self.stream
            .write_copy(&bytes, 0, pulse::stream::SeekMode::Relative)
            .map_err(|_| "OUTPUT_LOST")
    }

    pub fn write_cursor_frames(&mut self) -> Option<u64> {
        let timing = self.stream.get_timing_info()?;
        if timing.write_index_corrupt != 0 || timing.write_index < 0 {
            return None;
        }
        Some(timing.write_index as u64 / (u64::from(self.channels) * 4))
    }

    pub fn played_frames(&self) -> Option<u64> {
        self.stream
            .get_time()
            .ok()
            .flatten()
            .map(|time| time.0.saturating_mul(u64::from(self.rate)) / 1_000_000)
    }

    pub fn set_paused(
        &mut self,
        paused: bool,
        on_stall: &mut impl FnMut(),
    ) -> Result<(), &'static str> {
        if !paused {
            self.verify()?;
        }
        if paused == self.corked {
            return Ok(());
        }
        let done = Rc::new(Cell::new(None));
        let callback = done.clone();
        let _operation = self
            .stream
            .set_corked_state(paused, Some(Box::new(move |ok| callback.set(Some(ok)))));
        self.wait_ack(done, on_stall)?;
        self.corked = paused;
        Ok(())
    }

    pub fn drain(&mut self, on_stall: &mut impl FnMut()) -> Result<(), &'static str> {
        let done = Rc::new(Cell::new(None));
        let callback = done.clone();
        let _operation = self
            .stream
            .drain(Some(Box::new(move |ok| callback.set(Some(ok)))));
        self.wait_ack(done, on_stall)
    }

    pub fn retire(&mut self, on_stall: &mut impl FnMut()) -> Result<(), &'static str> {
        self.set_paused(true, on_stall)?;
        let done = Rc::new(Cell::new(None));
        let callback = done.clone();
        let _operation = self
            .stream
            .flush(Some(Box::new(move |ok| callback.set(Some(ok)))));
        self.wait_ack(done, on_stall)?;
        self.stream.disconnect().map_err(|_| "OUTPUT_LOST")?;
        self.context.disconnect();
        Ok(())
    }

    fn wait_ack(
        &mut self,
        done: Rc<Cell<Option<bool>>>,
        on_stall: &mut impl FnMut(),
    ) -> Result<(), &'static str> {
        let start = Instant::now();
        super::ack::wait(
            || {
                // A moved stream must still be corked/flushed. Identity loss
                // prohibits playback, but only transport loss or ACK proves
                // native retirement; do not skip cleanup on a move notification.
                self.pump_transport()?;
                Ok(done.get())
            },
            || start.elapsed(),
            || std::thread::sleep(Duration::from_millis(2)),
            on_stall,
        )
    }
}
