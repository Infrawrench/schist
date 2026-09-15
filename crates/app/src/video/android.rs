//! MediaExtractor/MediaCodec through the packaged Java bridge. JNI locals
//! live for one call; only global references cross worker calls.
use super::*;
use jni::{
    jni_sig, jni_str,
    objects::{JByteArray, JClass, JIntArray, JObject, JString, JValue},
    refs::Global,
    Env, JavaVM,
};
use std::time::{Duration, Instant};

fn with_activity<R>(f: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> Result<R>) -> Result<R> {
    let app = gpui::android::app().context(t("video.decode_failed"))?;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let activity = app.activity_as_ptr() as jni::sys::jobject;
    vm.attach_current_thread(|env| {
        let activity = unsafe { env.as_cast_raw::<Global<JObject<'static>>>(&activity)? };
        let result = f(env, &activity);
        if env.exception_check() {
            env.exception_clear();
        }
        result
    })
}
fn native_error(error: anyhow::Error) -> anyhow::Error {
    log::warn!("Android video: {error:#}");
    anyhow::anyhow!("{}", t("video.decode_failed"))
}

pub fn probe(path: &Path, job: Arc<Job>) -> Result<Info> {
    let decoder = Decoder::open(path, 0.0, None, 0, job)?;
    Ok(Info {
        duration: decoder.duration,
    })
}
pub struct Decoder {
    native: Global<JObject<'static>>,
    duration: f64,
    rotation: [i32; 4],
    aspect: f64,
    edge: u32,
    job: Arc<Job>,
}
impl Decoder {
    pub fn open(
        path: &Path,
        start: f64,
        span: Option<f64>,
        edge: u32,
        job: Arc<Job>,
    ) -> Result<Self> {
        ensure!(!job.cancelled(), "{}", t("common.cancel"));
        let path = local_file(path)?;
        with_activity(|env, activity| {
            let loader = env
                .call_method(
                    activity,
                    jni_str!("getClassLoader"),
                    jni_sig!(() -> java.lang.ClassLoader),
                    &[],
                )?
                .l()?;
            let name = env.new_string("com.infrawrench.schist.VideoDecoder")?;
            let class = env
                .call_method(
                    &loader,
                    jni_str!("loadClass"),
                    jni_sig!((name: JString) -> java.lang.Class),
                    &[JValue::Object(&name)],
                )?
                .l()?;
            let class = env.as_cast::<JClass>(&class)?;
            let path = env.new_string(path.to_string_lossy())?;
            let end = span.map_or(i64::MAX, |s| ((start + s).max(0.0) * 1e6).round() as i64);
            let object = env.new_object(
                &*class,
                jni_sig!((path: JString, start: long, end: long)),
                &[
                    JValue::Object(&path),
                    JValue::Long((start.max(0.0) * 1e6).round() as i64),
                    JValue::Long(end),
                ],
            )?;
            let native = env.new_global_ref(object)?;
            // Close even if reading metadata fails after native construction.
            let mut decoder = Self {
                native,
                duration: 0.0,
                rotation: [1, 0, 0, 1],
                aspect: 1.0,
                edge,
                job,
            };
            decoder.duration = env
                .get_field(&decoder.native, jni_str!("durationUs"), jni_sig!(long))?
                .j()? as f64
                / 1e6;
            let rotation = env
                .get_field(&decoder.native, jni_str!("rotation"), jni_sig!(int))?
                .i()?;
            decoder.rotation = match rotation.rem_euclid(360) {
                90 => [0, 1, -1, 0],
                180 => [-1, 0, 0, -1],
                270 => [0, -1, 1, 0],
                _ => [1, 0, 0, 1],
            };
            let width = env
                .get_field(&decoder.native, jni_str!("aspectWidth"), jni_sig!(int))?
                .i()?;
            let height = env
                .get_field(&decoder.native, jni_str!("aspectHeight"), jni_sig!(int))?
                .i()?;
            decoder.aspect = width as f64 / height.max(1) as f64;
            Ok(decoder)
        })
        .map_err(native_error)
    }
    pub fn next(&mut self) -> Result<Option<Frame>> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if self.job.cancelled() {
                return Ok(None);
            }
            let (frame, ended) = with_activity(|env, _| {
                let object = env
                    .call_method(
                        &self.native,
                        jni_str!("step"),
                        jni_sig!(() -> com.infrawrench.schist.VideoDecoder::Frame),
                        &[],
                    )?
                    .l()?;
                let ended = env
                    .get_field(&self.native, jni_str!("ended"), jni_sig!(boolean))?
                    .z()?;
                if object.is_null() {
                    return Ok((None, ended));
                }
                let time = env
                    .get_field(&object, jni_str!("timeUs"), jni_sig!(long))?
                    .j()? as f64
                    / 1e6;
                let layout = env
                    .get_field(&object, jni_str!("layout"), jni_sig!([int]))?
                    .l()?;
                let layout = env.as_cast::<JIntArray>(&layout)?;
                let mut values = [0; 12];
                layout.get_region(env, 0, &mut values)?;
                let y = env
                    .get_field(&object, jni_str!("y"), jni_sig!([byte]))?
                    .l()?;
                let u = env
                    .get_field(&object, jni_str!("u"), jni_sig!([byte]))?
                    .l()?;
                let v = env
                    .get_field(&object, jni_str!("v"), jni_sig!([byte]))?
                    .l()?;
                let y = env.convert_byte_array(&*env.as_cast::<JByteArray>(&y)?)?;
                let u = env.convert_byte_array(&*env.as_cast::<JByteArray>(&u)?)?;
                let v = env.convert_byte_array(&*env.as_cast::<JByteArray>(&v)?)?;
                let frame = super::yuv::convert(time, &values, [&y, &u, &v], self.edge)?;
                Ok((
                    Some(display_frame(frame, self.aspect, self.rotation, self.edge)?),
                    ended,
                ))
            })
            .map_err(native_error)?;
            if frame.is_some() || ended {
                return Ok(frame);
            }
            ensure!(Instant::now() < deadline, "{}", t("video.decode_failed"));
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = with_activity(|env, _| {
            env.call_method(&self.native, jni_str!("close"), jni_sig!(()), &[])?;
            Ok(())
        });
    }
}

pub(crate) fn begin_import(destination: Option<&Path>) -> Result<()> {
    with_activity(|env, activity| {
        let destination = env.new_string(
            destination
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )?;
        env.call_method(
            activity,
            jni_str!("pickMedia"),
            jni_sig!((destination: JString)),
            &[JValue::Object(&destination)],
        )?;
        Ok(())
    })
}
#[derive(serde::Deserialize)]
pub(crate) struct ImportEvent {
    pub path: Option<PathBuf>,
    pub error: Option<String>,
    pub done: bool,
    pub open: bool,
}
pub(crate) fn take_imports() -> Result<Vec<ImportEvent>> {
    with_activity(|env, activity| {
        let events = env
            .call_method(
                activity,
                jni_str!("takeMediaImports"),
                jni_sig!(() -> JString),
                &[],
            )?
            .l()?;
        let text = env.as_cast::<JString>(&events)?.try_to_string(env)?;
        Ok(serde_json::from_str(&text)?)
    })
}
pub(crate) fn share(path: &Path) -> Result<()> {
    let path = local_file(path)?;
    with_activity(|env, activity| {
        let path = env.new_string(path.to_string_lossy())?;
        let title = env.new_string(t("video.open_another_app"))?;
        env.call_method(
            activity,
            jni_str!("shareVideo"),
            jni_sig!((path: JString, title: JString)),
            &[JValue::Object(&path), JValue::Object(&title)],
        )?;
        Ok(())
    })
}
