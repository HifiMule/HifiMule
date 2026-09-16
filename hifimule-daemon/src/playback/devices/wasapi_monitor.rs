//! Selected-endpoint COM subscription, owned by the same thread as its stream.
use super::endpoint_signal::EndpointSignal;
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};
use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Media::Audio::{
            DEVICE_STATE, DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDeviceEnumerator,
            IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
        },
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
        UI::Shell::PropertiesSystem::PROPERTYKEY,
    },
    core::{PCWSTR, implement},
};

struct Apartment {
    initialized: bool,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

pub struct SelectedEndpointMonitor {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    _apartment: Apartment,
}
impl SelectedEndpointMonitor {
    pub fn new(
        id: &str,
        gate: Arc<AtomicBool>,
        lost: Arc<AtomicBool>,
    ) -> Result<Self, &'static str> {
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
            return Err("OUTPUT_UNAVAILABLE");
        }
        let apartment = Apartment {
            initialized: initialized.is_ok(),
            _thread: PhantomData,
        };
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|_| "OUTPUT_UNAVAILABLE")?;
        let client: IMMNotificationClient = SelectedNotifications {
            signal: EndpointSignal::new(id, gate, lost),
        }
        .into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
            .map_err(|_| "OUTPUT_UNAVAILABLE")?;
        let result = Self {
            enumerator,
            client,
            _apartment: apartment,
        };
        // Subscribe first, then validate to close the enumerate/subscribe race.
        let wide: Vec<_> = id.encode_utf16().chain(Some(0)).collect();
        let device = unsafe { result.enumerator.GetDevice(PCWSTR(wide.as_ptr())) }
            .map_err(|_| "OUTPUT_UNAVAILABLE")?;
        if unsafe { device.GetState() }.map_err(|_| "OUTPUT_UNAVAILABLE")? != DEVICE_STATE_ACTIVE {
            return Err("OUTPUT_UNAVAILABLE");
        }
        Ok(result)
    }
}
impl Drop for SelectedEndpointMonitor {
    fn drop(&mut self) {
        // Unregister waits for callbacks. They only compare an ID and set atomics,
        // so they cannot call back into the owner or deadlock native retirement.
        while unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        }
        .is_err()
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
#[implement(IMMNotificationClient)]
struct SelectedNotifications {
    signal: EndpointSignal,
}
impl IMMNotificationClient_Impl for SelectedNotifications_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> windows::core::Result<()> {
        if state != DEVICE_STATE_ACTIVE {
            unsafe {
                self.signal.unavailable(id.as_ptr());
            }
        }
        Ok(())
    }
    fn OnDeviceRemoved(&self, id: &PCWSTR) -> windows::core::Result<()> {
        unsafe {
            self.signal.unavailable(id.as_ptr());
        }
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        _: EDataFlow,
        _: ERole,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}
