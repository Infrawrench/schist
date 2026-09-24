//! Windows Portable Devices: the inbox PTP/MTP driver, without a vendor SDK.
//! All COM interfaces stay on the background worker's MTA apartment.
use super::{cancelled, Camera, Captured, Result, Session};
use schist_i18n::t;
use std::{
    collections::HashSet,
    fs::File,
    io::Write,
    sync::{atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};
use windows::{
    core::{implement, w, BSTR, PCWSTR, PROPVARIANT, PWSTR},
    Win32::{
        Devices::PortableDevices::*,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED, STGM_READ,
        },
        UI::Shell::PropertiesSystem::PROPERTYKEY,
    },
};

fn failure(error: impl std::fmt::Display) -> String {
    schist_i18n::tf!("tethered.failed", detail = error)
}
struct Apartment;
impl Apartment {
    fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.map_err(failure)?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}
struct Device(IPortableDevice);
impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Close();
        }
    }
}
struct ComString(PWSTR);
impl ComString {
    fn text(&self) -> Result<String> {
        if self.0.is_null() {
            return Err(t("tethered.no_download").into());
        }
        unsafe { self.0.to_string() }.map_err(failure)
    }
}
impl Drop for ComString {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0 .0.cast())) }
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn values() -> windows::core::Result<IPortableDeviceValues> {
    unsafe { CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER) }
}
fn check(cancel: &AtomicBool, deadline: Instant) -> Result<()> {
    cancelled(cancel)?;
    if Instant::now() >= deadline {
        return Err(t("tethered.timeout").into());
    }
    Ok(())
}
fn open(camera: &Camera) -> Result<Device> {
    unsafe {
        let info = values().map_err(failure)?;
        info.SetStringValue(&WPD_CLIENT_NAME, w!("Schist"))
            .map_err(failure)?;
        let device: IPortableDevice =
            CoCreateInstance(&PortableDeviceFTM, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
        let device = Device(device);
        device
            .0
            .Open(PCWSTR(wide(&camera.port).as_ptr()), &info)
            .map_err(failure)?;
        Ok(device)
    }
}
fn capture_target(device: &Device) -> Result<String> {
    unsafe {
        let capabilities = device.0.Capabilities().map_err(failure)?;
        let commands = capabilities.GetSupportedCommands().map_err(failure)?;
        // These COM out parameters are incorrectly projected as const pointers
        // by windows 0.58. Pass writable raw pointers, never shared references.
        let mut count = 0;
        commands.GetCount(&raw mut count).map_err(failure)?;
        let mut supported = false;
        for index in 0..count {
            let mut command = PROPERTYKEY::default();
            commands.GetAt(index, &raw mut command).map_err(failure)?;
            supported |= command == WPD_COMMAND_STILL_IMAGE_CAPTURE_INITIATE;
        }
        if !supported {
            return Err(t("tethered.unsupported").into());
        }
        let objects = capabilities
            .GetFunctionalObjects(&WPD_FUNCTIONAL_CATEGORY_STILL_IMAGE_CAPTURE)
            .map_err(failure)?;
        objects.GetCount(&raw mut count).map_err(failure)?;
        if count == 0 {
            return Err(t("tethered.unsupported").into());
        }
        let mut value = PROPVARIANT::default();
        objects.GetAt(0, &raw mut value).map_err(failure)?;
        BSTR::try_from(&value)
            .map(|s| s.to_string())
            .map_err(failure)
    }
}

pub fn discover(cancel: &AtomicBool) -> Result<Vec<Camera>> {
    cancelled(cancel)?;
    let _apartment = Apartment::new()?;
    let deadline = Instant::now() + Duration::from_secs(120);
    unsafe {
        let manager: IPortableDeviceManager =
            CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER)
                .map_err(failure)?;
        let mut count = 0;
        manager
            .GetDevices(std::ptr::null_mut(), &mut count)
            .map_err(failure)?;
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut ids = vec![PWSTR::null(); count as usize];
        let result = manager.GetDevices(ids.as_mut_ptr(), &mut count);
        // Own every allocation even if enumeration was interrupted by unplugging.
        let ids: Vec<_> = ids.into_iter().map(ComString).collect();
        result.map_err(failure)?;
        let mut cameras = Vec::new();
        for id in ids.into_iter().filter(|id| !id.0.is_null()) {
            check(cancel, deadline)?;
            let port = id.text()?;
            let mut size = 0;
            let _ = manager.GetDeviceFriendlyName(PCWSTR(id.0 .0), PWSTR::null(), &mut size);
            let mut name = vec![0u16; size as usize];
            let model = if size > 0
                && manager
                    .GetDeviceFriendlyName(PCWSTR(id.0 .0), PWSTR(name.as_mut_ptr()), &mut size)
                    .is_ok()
            {
                String::from_utf16_lossy(
                    &name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())],
                )
            } else {
                t("library.volume.camera").into()
            };
            let camera = Camera {
                id: None,
                model,
                port,
            };
            if open(&camera)
                .and_then(|device| capture_target(&device))
                .is_ok()
            {
                cameras.push(camera);
            }
        }
        check(cancel, deadline)?;
        Ok(cameras)
    }
}
pub fn connect(camera: &Camera, cancel: &AtomicBool) -> Result<()> {
    cancelled(cancel)?;
    let _apartment = Apartment::new()?;
    let device = open(camera)?;
    capture_target(&device)?;
    cancelled(cancel)
}

#[implement(IPortableDeviceEventCallback)]
struct Events(mpsc::Sender<Result<String>>);
impl IPortableDeviceEventCallback_Impl for Events_Impl {
    fn OnEvent(&self, parameters: Option<&IPortableDeviceValues>) -> windows::core::Result<()> {
        if let Some(parameters) = parameters {
            unsafe {
                let event = parameters.GetGuidValue(&WPD_EVENT_PARAMETER_EVENT_ID)?;
                if event == WPD_EVENT_OBJECT_ADDED {
                    let id = ComString(parameters.GetStringValue(&WPD_OBJECT_ID)?);
                    let _ = self.0.send(id.text());
                } else if event == WPD_EVENT_DEVICE_REMOVED {
                    let _ = self
                        .0
                        .send(Err(t("library.import.camera_disconnected").into()));
                }
            }
        }
        Ok(())
    }
}
struct Subscription<'a> {
    device: &'a Device,
    cookie: ComString,
}
impl Drop for Subscription<'_> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.0.Unadvise(PCWSTR(self.cookie.0 .0));
        }
    }
}
fn download(
    device: &Device,
    id: &str,
    staging: &std::path::Path,
    index: usize,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<bool> {
    unsafe {
        let content = device.0.Content().map_err(failure)?;
        let properties = content.Properties().map_err(failure)?;
        let id = wide(id);
        let properties = properties
            .GetValues(PCWSTR(id.as_ptr()), None)
            .map_err(failure)?;
        let kind = properties
            .GetGuidValue(&WPD_OBJECT_CONTENT_TYPE)
            .map_err(failure)?;
        if kind == WPD_CONTENT_TYPE_FOLDER || kind == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT {
            return Ok(false);
        }
        let name = ComString(
            properties
                .GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME)
                .map_err(failure)?,
        )
        .text()?;
        let size = properties
            .GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE)
            .map_err(failure)?;
        if size == 0 {
            return Err(t("tethered.no_download").into());
        }
        let path = super::download_path(staging, index, &name)?;
        let mut file = File::create(&path).map_err(super::io_error)?;
        let mut stream = None;
        let mut optimal = 0;
        content
            .Transfer()
            .map_err(failure)?
            .GetStream(
                PCWSTR(id.as_ptr()),
                &WPD_RESOURCE_DEFAULT,
                STGM_READ.0,
                &mut optimal,
                &mut stream,
            )
            .map_err(failure)?;
        let stream = stream.ok_or_else(|| t("tethered.no_download").to_string())?;
        let mut buffer = vec![0u8; optimal.clamp(4096, 1024 * 1024) as usize];
        let mut received = 0u64;
        loop {
            check(cancel, deadline)?;
            let mut read = 0;
            stream
                .Read(
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    Some(&mut read),
                )
                .ok()
                .map_err(failure)?;
            if read == 0 {
                break;
            }
            received += u64::from(read);
            if received > size {
                return Err(t("tethered.no_download").into());
            }
            file.write_all(&buffer[..read as usize])
                .map_err(super::io_error)?;
        }
        if received != size {
            return Err(t("tethered.no_download").into());
        }
        file.sync_all().map_err(super::io_error)?;
        Ok(true)
    }
}

pub fn capture(camera: &Camera, session: &Session, cancel: &AtomicBool) -> Result<Captured> {
    cancelled(cancel)?;
    let staging = super::staging(session)?;
    let _apartment = Apartment::new()?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let device = open(camera)?;
    let target = capture_target(&device)?;
    let (sender, receiver) = mpsc::channel();
    let callback: IPortableDeviceEventCallback = Events(sender).into();
    let cookie = unsafe { device.0.Advise(0, &callback, None) }.map_err(failure)?;
    let subscription = Subscription {
        device: &device,
        cookie: ComString(cookie),
    };
    check(cancel, deadline)?;
    unsafe {
        let command = values().map_err(failure)?;
        command
            .SetGuidValue(
                &WPD_PROPERTY_COMMON_COMMAND_CATEGORY,
                &WPD_COMMAND_STILL_IMAGE_CAPTURE_INITIATE.fmtid,
            )
            .map_err(failure)?;
        command
            .SetUnsignedIntegerValue(
                &WPD_PROPERTY_COMMON_COMMAND_ID,
                WPD_COMMAND_STILL_IMAGE_CAPTURE_INITIATE.pid,
            )
            .map_err(failure)?;
        command
            .SetStringValue(
                &WPD_PROPERTY_COMMON_COMMAND_TARGET,
                PCWSTR(wide(&target).as_ptr()),
            )
            .map_err(failure)?;
        device
            .0
            .SendCommand(0, &command)
            .map_err(failure)?
            .GetErrorValue(&WPD_PROPERTY_COMMON_HRESULT)
            .map_err(failure)?
            .ok()
            .map_err(failure)?;
    }
    let mut seen = HashSet::new();
    let mut downloaded = 0;
    let mut last = Instant::now();
    loop {
        check(cancel, deadline)?;
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(id) => {
                let id = id?;
                if seen.insert(id.clone())
                    && download(&device, &id, staging.path(), downloaded, cancel, deadline)?
                {
                    downloaded += 1;
                    last = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout)
                if downloaded > 0 && last.elapsed() >= Duration::from_secs(2) =>
            {
                break
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(t("library.import.camera_disconnected").into())
            }
        }
    }
    drop(subscription);
    drop(device);
    super::publish(staging.path(), session, cancel)
}
