//! JNI bridge to Android's USB host API. Java owns each USB session for the
//! duration of this synchronous background call, including cancellation cleanup.
use super::{cancelled, Camera, Captured, Result, Session};
use jni::{
    jni_sig, jni_str,
    objects::{JObject, JString, JValue},
    refs::Global,
    Env, JavaVM,
};
use schist_i18n::{t, tf};
use std::sync::atomic::AtomicBool;

fn with_activity<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<R>,
) -> Result<R> {
    let app = gpui::android::app().ok_or_else(|| t("common.not_available").to_string())?;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) };
    let activity = app.activity_as_ptr() as jni::sys::jobject;
    vm.attach_current_thread(|env| -> jni::errors::Result<R> {
        let activity = unsafe { env.as_cast_raw::<Global<JObject<'static>>>(&activity)? };
        let result = f(env, &activity);
        if env.exception_check() {
            env.exception_clear();
        }
        result
    })
    .map_err(|error| tf!("tethered.failed", detail = error))
}
pub fn begin_android() {
    let _ = with_activity(|env, activity| {
        env.call_method(
            activity,
            jni_str!("tetheredBegin"),
            jni_sig!(() -> void),
            &[],
        )?;
        Ok(())
    });
}
pub fn cancel_android() {
    let _ = with_activity(|env, activity| {
        env.call_method(
            activity,
            jni_str!("tetheredCancel"),
            jni_sig!(() -> void),
            &[],
        )?;
        Ok(())
    });
}
#[derive(serde::Deserialize)]
struct Reply {
    #[serde(default)]
    cameras: Vec<Camera>,
    error: Option<String>,
}
fn run(operation: &str, port: &str, destination: &str, cancel: &AtomicBool) -> Result<Reply> {
    cancelled(cancel)?;
    let json = with_activity(|env, activity| {
        let operation = env.new_string(operation)?;
        let port = env.new_string(port)?;
        let destination = env.new_string(destination)?;
        let reply = env
            .call_method(
                activity,
                jni_str!("tetheredRun"),
                jni_sig!((operation: JString, port: JString, destination: JString) -> JString),
                &[
                    JValue::Object(&operation),
                    JValue::Object(&port),
                    JValue::Object(&destination),
                ],
            )?
            .l()?;
        env.as_cast::<JString>(&reply)?.try_to_string(env)
    })?;
    cancelled(cancel)?;
    let reply: Reply =
        serde_json::from_str(&json).map_err(|error| tf!("tethered.failed", detail = error))?;
    if let Some(error) = reply.error {
        return Err(match error.as_str() {
            "common.cancelled"
            | "common.not_available"
            | "tethered.timeout"
            | "tethered.unsupported"
            | "tethered.no_download"
            | "library.import.camera_disconnected" => t(&error).into(),
            _ => tf!("tethered.failed", detail = error),
        });
    }
    Ok(reply)
}
pub fn discover(cancel: &AtomicBool) -> Result<Vec<Camera>> {
    Ok(run("discover", "", "", cancel)?.cameras)
}
pub fn connect(camera: &Camera, cancel: &AtomicBool) -> Result<()> {
    run("connect", &camera.port, "", cancel)?;
    Ok(())
}
pub fn capture(camera: &Camera, session: &Session, cancel: &AtomicBool) -> Result<Captured> {
    let staging = super::staging(session)?;
    run(
        "capture",
        &camera.port,
        &staging.path().to_string_lossy(),
        cancel,
    )?;
    super::publish(staging.path(), session, cancel)
}
