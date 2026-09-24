use super::{cancelled, Camera, Result, Session};
use gphoto2::{
    abilities::DeviceType, camera::CameraEvent, file::CameraFilePath, list::CameraDescriptor,
    task::Task, Camera as GpCamera, Context,
};
use std::{
    collections::BTreeSet,
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context as TaskContext, Poll, Waker},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(120);

type CameraKey = (String, String);

fn failure(error: impl std::fmt::Display) -> String {
    schist_i18n::tf!("tethered.failed", detail = error)
}

fn descriptor(camera: &Camera) -> CameraDescriptor {
    CameraDescriptor {
        model: camera.model.clone(),
        port: camera.port.clone(),
    }
}

fn run_task_result<T>(
    task: Task<gphoto2::Result<T>>,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<gphoto2::Result<T>>
where
    T: Send + 'static,
{
    cancelled(cancel)?;
    if Instant::now() >= deadline {
        return Err(schist_i18n::t("tethered.timeout").into());
    }
    let mut task = Box::pin(task);
    let waker = Waker::noop();
    let mut context = TaskContext::from_waker(waker);
    let mut terminal = None;
    loop {
        if terminal.is_none() {
            if cancel.load(Ordering::Acquire) {
                task.cancel();
                terminal = Some(schist_i18n::t("common.cancelled").to_string());
            } else if Instant::now() >= deadline {
                task.cancel();
                terminal = Some(schist_i18n::t("tethered.timeout").to_string());
            }
        }
        match task.as_mut().poll(&mut context) {
            Poll::Ready(result) => {
                return terminal.map_or(Ok(result), Err);
            }
            Poll::Pending => {
                // Keep the camera and staging directory alive until the native
                // worker acknowledges cancellation. Dropping a Task does not
                // stop its I/O or join the worker.
                thread::sleep(Duration::from_millis(40));
            }
        }
    }
}

fn run_task<T: Send + 'static>(
    task: Task<gphoto2::Result<T>>,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<T> {
    run_task_result(task, cancel, deadline)?.map_err(failure)
}

fn configured_camera(camera: &Camera, cancel: &AtomicBool, deadline: Instant) -> Result<GpCamera> {
    let context = Context::new().map_err(failure)?;
    let descriptor = descriptor(camera);
    let camera = run_task(context.get_camera(&descriptor), cancel, deadline)?;
    cancelled(cancel)?;
    if !matches!(camera.abilities().device_type(), DeviceType::Camera)
        || !camera.abilities().camera_operations().capture_image()
    {
        return Err(schist_i18n::t("tethered.unsupported").into());
    }
    Ok(camera)
}

pub fn discover(cancel: &AtomicBool) -> Result<Vec<Camera>> {
    let deadline = Instant::now() + TIMEOUT;
    let context = Context::new().map_err(failure)?;
    let cameras: Vec<_> = run_task(context.list_cameras(), cancel, deadline)?.collect();
    let mut supported = Vec::new();
    for camera in cameras {
        cancelled(cancel)?;
        if !camera.port.starts_with("usb:") {
            continue;
        }
        let candidate = Camera {
            id: None,
            model: camera.model,
            port: camera.port,
        };
        let descriptor = descriptor(&candidate);
        let camera = match run_task(context.get_camera(&descriptor), cancel, deadline) {
            Ok(camera) => camera,
            Err(error) => {
                cancelled(cancel)?;
                if Instant::now() >= deadline {
                    return Err(error);
                }
                continue;
            }
        };
        if matches!(camera.abilities().device_type(), DeviceType::Camera)
            && camera.abilities().camera_operations().capture_image()
        {
            supported.push(candidate);
        }
    }
    Ok(supported)
}

pub fn connect(camera: &Camera, cancel: &AtomicBool) -> Result<()> {
    cancelled(cancel)?;
    let deadline = Instant::now() + TIMEOUT;
    let camera = configured_camera(camera, cancel, deadline)?;
    // A descriptor is only configuration; touching the filesystem opens the
    // native session and catches locked/disconnected devices before selection.
    run_task(camera.fs().list_folders("/"), cancel, deadline)?;
    Ok(())
}

fn captured_files(
    camera: &GpCamera,
    captured: &CameraFilePath,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<CameraKey>> {
    let folder = captured.folder().into_owned();
    let mut files = BTreeSet::new();
    files.insert((folder, captured.name().into_owned()));
    // Drivers may deliver RAW+JPEG asynchronously. Wait for the companion
    // FileAdded events instead of listing the folder once immediately after
    // the shutter returns (which loses delayed RAW files).
    let mut last_file = Instant::now();
    while last_file.elapsed() < Duration::from_secs(2) {
        match run_task_result(
            camera.wait_event(Duration::from_millis(200)),
            cancel,
            deadline,
        )? {
            Ok(CameraEvent::NewFile(path)) => {
                if files.insert((path.folder().into_owned(), path.name().into_owned())) {
                    last_file = Instant::now();
                }
            }
            Err(error) if error.kind() == gphoto2::error::ErrorKind::NotSupported => break,
            Err(error) => return Err(failure(error)),
            Ok(_) => {}
        }
    }
    Ok(files.into_iter().collect())
}

pub fn capture(camera: &Camera, session: &Session, cancel: &AtomicBool) -> Result<super::Captured> {
    cancelled(cancel)?;
    let staging = super::staging(session)?;
    let deadline = Instant::now() + TIMEOUT;
    let camera = configured_camera(camera, cancel, deadline)?;
    let captured = run_task(camera.capture_image(), cancel, deadline)?;
    let files = captured_files(&camera, &captured, cancel, deadline)?;
    for (index, (folder, name)) in files.into_iter().enumerate() {
        cancelled(cancel)?;
        let path = super::download_path(staging.path(), index, &name)?;
        run_task(
            camera.fs().download_to(&folder, &name, &path),
            cancel,
            deadline,
        )?;
    }
    drop(camera);
    super::publish(staging.path(), session, cancel)
}
