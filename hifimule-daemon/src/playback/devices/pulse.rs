use super::*;
use crate::playback::config::OutputPreference;
use libpulse_binding as pulse;
use pulse::{
    callbacks::ListResult,
    context::{Context, FlagSet, State},
    mainloop::standard::{IterateResult, Mainloop},
};
use std::collections::BTreeMap;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

pub fn discover() -> Discovery {
    discover_inner().unwrap_or_else(|code| Discovery {
        outputs: vec![],
        error: Some(code),
    })
}

fn discover_inner() -> Result<Discovery, &'static str> {
    let mut mainloop = Mainloop::new().ok_or("OUTPUT_UNAVAILABLE")?;
    let mut context =
        Context::new(&mainloop, "HifiMule output discovery").ok_or("OUTPUT_UNAVAILABLE")?;
    context
        .connect(None, FlagSet::NOAUTOSPAWN, None)
        .map_err(|_| "OUTPUT_UNAVAILABLE")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while context.get_state() != State::Ready {
        step(&mut mainloop, &context, deadline)?;
    }
    let default = Rc::new(RefCell::new(None));
    let default_copy = default.clone();
    let mut server_op = context.introspect().get_server_info(move |info| {
        *default_copy.borrow_mut() = info.default_sink_name.as_ref().map(|s| s.to_string());
    });
    let inventory = Rc::new(RefCell::new(Discovery {
        outputs: vec![],
        error: None,
    }));
    let callback_inventory = inventory.clone();
    let mut list_op = context.introspect().get_sink_info_list(move |item| {
        let mut result = callback_inventory.borrow_mut();
        match item {
            ListResult::Item(info) => {
                if result.outputs.len() >= MAX_OUTPUTS {
                    result.error = Some("OUTPUT_ENUMERATION_TRUNCATED");
                    return;
                }
                let Some(name) = info
                    .name
                    .as_deref()
                    .filter(|s| !s.is_empty() && s.len() <= 4096)
                else {
                    result.error = Some("OUTPUT_IDENTITY_AMBIGUOUS");
                    return;
                };
                let mut properties = BTreeMap::new();
                for key in [
                    "device.serial",
                    "device.bus_path",
                    "device.bus",
                    "device.vendor.id",
                    "device.product.id",
                    "device.profile.name",
                    "device.string",
                ] {
                    if let Some(value) = info.proplist.get_str(key) {
                        if value.len() > 4096 {
                            result.error = Some("OUTPUT_IDENTITY_AMBIGUOUS");
                            return;
                        }
                        properties.insert(key.into(), value);
                    }
                }
                if let Some(port) = &info.active_port {
                    if let Some(name) = &port.name {
                        if name.len() > 4096 {
                            result.error = Some("OUTPUT_IDENTITY_AMBIGUOUS");
                            return;
                        }
                        properties.insert("port".into(), name.to_string());
                    }
                }
                let virtual_output = !info.flags.contains(pulse::def::SinkFlagSet::HARDWARE);
                let confident = virtual_output
                    || ["device.serial", "device.bus_path", "device.string"]
                        .iter()
                        .any(|k| properties.contains_key(*k));
                let display = info
                    .description
                    .as_deref()
                    .filter(|s| !s.is_empty() && s.len() <= 4096)
                    .unwrap_or("Audio output")
                    .to_owned();
                let detail = info
                    .active_port
                    .as_ref()
                    .and_then(|p| p.description.as_deref())
                    .unwrap_or(if virtual_output {
                        "Virtual output"
                    } else {
                        "PulseAudio shared output"
                    })
                    .chars()
                    .take(1024)
                    .collect();
                let preference = OutputPreference {
                    backend: "pulse".into(),
                    stable_id: name.into(),
                    display_name: display.clone(),
                    identity_properties: properties,
                };
                result.outputs.push(OutputDescriptor {
                    output_id: output_id(&preference),
                    display_name: display,
                    detail,
                    backend: "pulse".into(),
                    available: confident,
                    is_default: false,
                    identity_confidence: if confident { "stable" } else { "ambiguous" }.into(),
                    is_virtual: virtual_output,
                    preference: Some(preference),
                });
            }
            ListResult::Error => result.error = Some("OUTPUT_UNAVAILABLE"),
            ListResult::End => {}
        }
    });
    while server_op.get_state() == pulse::operation::State::Running
        || list_op.get_state() == pulse::operation::State::Running
    {
        if let Err(error) = step(&mut mainloop, &context, deadline) {
            server_op.cancel();
            list_op.cancel();
            return Err(error);
        }
    }
    let mut result = inventory.borrow().clone();
    for output in &mut result.outputs {
        output.is_default = output
            .preference
            .as_ref()
            .is_some_and(|p| Some(&p.stable_id) == default.borrow().as_ref());
    }
    context.disconnect();
    Ok(result)
}

fn step(mainloop: &mut Mainloop, context: &Context, deadline: Instant) -> Result<(), &'static str> {
    if Instant::now() >= deadline {
        return Err("OUTPUT_DISCOVERY_TIMEOUT");
    }
    if matches!(context.get_state(), State::Failed | State::Terminated) {
        return Err("OUTPUT_UNAVAILABLE");
    }
    if !matches!(mainloop.iterate(false), IterateResult::Success(_)) {
        return Err("OUTPUT_UNAVAILABLE");
    }
    std::thread::sleep(Duration::from_millis(5));
    Ok(())
}
