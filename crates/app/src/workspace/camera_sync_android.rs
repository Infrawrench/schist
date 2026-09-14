//! Android media permission and JobScheduler bridge. The job uses explicit
//! private-storage paths so it can run before an activity has set up gpui.

use super::camera_sync::{self, CameraSync, Engine, Outcome, Source};
use anyhow::{anyhow, Context as _, Result};
use jni::{
    jni_sig, jni_str,
    objects::{JByteArray, JClass, JObject, JString, JValue},
    refs::Global,
    Env, EnvUnowned, JavaVM,
};
use schist_cloud::{self as remote, Event};
use schist_i18n::t;
use std::{
    hash::Hasher,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex,
    },
    time::{Duration, Instant},
};

const JOB_ID: i32 = 1;
static JOB_RUNNING: AtomicBool = AtomicBool::new(false);
struct JobGuard(Arc<AtomicBool>);
impl Drop for JobGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
        JOB_RUNNING.store(false, Ordering::Release);
    }
}
/// Run on gpui's background executor before it reads the saved login.
pub(crate) fn hand_over_to_activity() {
    CANCEL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store(true, Ordering::Relaxed);
    while JOB_RUNNING.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(100));
    }
}
static CANCEL: LazyLock<Mutex<Arc<AtomicBool>>> =
    LazyLock::new(|| Mutex::new(Arc::new(AtomicBool::new(false))));

fn with_activity<R>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<R>,
) -> Result<R> {
    let app = gpui::android::app().ok_or_else(|| anyhow!("No Android activity"))?;
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
    .map_err(|e: jni::errors::Error| anyhow!(e.to_string()))
}

fn sdk(env: &mut Env<'_>) -> jni::errors::Result<i32> {
    env.get_static_field(
        jni_str!("android/os/Build$VERSION"),
        jni_str!("SDK_INT"),
        jni_sig!(int),
    )?
    .i()
}
fn permission(env: &mut Env<'_>, context: &JObject<'_>) -> jni::errors::Result<bool> {
    let name = if sdk(env)? >= 33 {
        "android.permission.READ_MEDIA_IMAGES"
    } else {
        "android.permission.READ_EXTERNAL_STORAGE"
    };
    let name = env.new_string(name)?;
    Ok(env
        .call_method(
            context,
            jni_str!("checkSelfPermission"),
            jni_sig!((name: JString) -> int),
            &[JValue::Object(&name)],
        )?
        .i()?
        == 0)
}
pub(crate) fn has_media_permission() -> bool {
    with_activity(permission).unwrap_or(false)
}
pub(crate) fn request_media_permission() {
    let result = with_activity(|env, activity| {
        let loader = env
            .call_method(
                activity,
                jni_str!("getClassLoader"),
                jni_sig!(() -> java.lang.ClassLoader),
                &[],
            )?
            .l()?;
        let name = env.new_string("com.infrawrench.schist.CameraSyncJob")?;
        let class = env
            .call_method(
                &loader,
                jni_str!("loadClass"),
                jni_sig!((name: JString) -> java.lang.Class),
                &[JValue::Object(&name)],
            )?
            .l()?;
        let class = env.as_cast::<JClass>(&class)?;
        env.call_static_method(
            &*class,
            jni_str!("requestMediaPermission"),
            jni_sig!((activity: android.app.Activity)),
            &[JValue::Object(activity)],
        )?;
        Ok(())
    });
    if let Err(error) = result {
        log::warn!("camera sync permission: {error}");
    }
}
fn path_of(env: &mut Env<'_>, file: &JObject<'_>) -> jni::errors::Result<PathBuf> {
    let path = env
        .call_method(
            file,
            jni_str!("getAbsolutePath"),
            jni_sig!(() -> JString),
            &[],
        )?
        .l()?;
    Ok(PathBuf::from(
        env.as_cast::<JString>(&path)?.try_to_string(env)?,
    ))
}
pub(crate) fn sources() -> Vec<(String, PathBuf)> {
    with_activity(|env, _| {
        let mut roots = Vec::new();
        for (name, label) in [
            ("DCIM", "cloud.sync.source_dcim"),
            ("Pictures", "cloud.sync.source_pictures"),
        ] {
            let directory = env.new_string(name)?;
            let file = env
                .call_static_method(
                    jni_str!("android/os/Environment"),
                    jni_str!("getExternalStoragePublicDirectory"),
                    jni_sig!((kind: JString) -> java.io.File),
                    &[JValue::Object(&directory)],
                )?
                .l()?;
            let path = path_of(env, &file)?;
            if name == "DCIM" {
                roots.push((t("cloud.sync.source_camera").into(), path.join("Camera")));
            }
            roots.push((t(label).into(), path));
        }
        Ok(roots)
    })
    .unwrap_or_default()
}
pub(crate) fn schedule_job() {
    let result = with_activity(|env, activity| {
        let name = env.new_string("jobscheduler")?;
        let scheduler = env
            .call_method(
                activity,
                jni_str!("getSystemService"),
                jni_sig!((name: JString) -> JObject),
                &[JValue::Object(&name)],
            )?
            .l()?;
        let class = env.new_string("com.infrawrench.schist.CameraSyncJob")?;
        let component = env.new_object(
            jni_str!("android/content/ComponentName"),
            jni_sig!((context: android.content.Context, name: JString)),
            &[JValue::Object(activity), JValue::Object(&class)],
        )?;
        let builder = env.new_object(
            jni_str!("android/app/job/JobInfo$Builder"),
            jni_sig!((id: int, component: android.content.ComponentName)),
            &[JValue::Int(JOB_ID), JValue::Object(&component)],
        )?;
        env.call_method(
            &builder,
            jni_str!("setPeriodic"),
            jni_sig!((interval: long) -> android.app.job.JobInfo::Builder),
            &[JValue::Long(15 * 60 * 1000)],
        )?;
        env.call_method(
            &builder,
            jni_str!("setRequiredNetworkType"),
            jni_sig!((kind: int) -> android.app.job.JobInfo::Builder),
            &[JValue::Int(1)],
        )?;
        env.call_method(
            &builder,
            jni_str!("setPersisted"),
            jni_sig!((persist: boolean) -> android.app.job.JobInfo::Builder),
            &[JValue::Bool(true)],
        )?;
        let job = env
            .call_method(
                &builder,
                jni_str!("build"),
                jni_sig!(() -> android.app.job.JobInfo),
                &[],
            )?
            .l()?;
        env.call_method(
            &scheduler,
            jni_str!("schedule"),
            jni_sig!((job: android.app.job.JobInfo) -> int),
            &[JValue::Object(&job)],
        )?
        .i()
    });
    if !matches!(result, Ok(1)) {
        log::warn!("camera sync scheduling failed: {result:?}");
    }
}
pub(crate) fn cancel_job() {
    CANCEL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .store(true, Ordering::Relaxed);
    let _ = with_activity(cancel_scheduled_job);
}

fn cancel_scheduled_job(env: &mut Env<'_>, context: &JObject<'_>) -> jni::errors::Result<()> {
    let name = env.new_string("jobscheduler")?;
    let scheduler = env
        .call_method(
            context,
            jni_str!("getSystemService"),
            jni_sig!((name: JString) -> JObject),
            &[JValue::Object(&name)],
        )?
        .l()?;
    env.call_method(
        &scheduler,
        jni_str!("cancel"),
        jni_sig!((id: int)),
        &[JValue::Int(JOB_ID)],
    )?;
    Ok(())
}

fn crypt(env: &mut Env<'_>, class: &JClass<'_>, encrypt: bool, bytes: &[u8]) -> Result<Vec<u8>> {
    let input = env.byte_array_from_slice(bytes)?;
    let output = env
        .call_static_method(
            class,
            jni_str!("crypt"),
            jni_sig!((encrypt: boolean, input: [byte]) -> [byte]),
            &[JValue::Bool(encrypt), JValue::Object(&input)],
        )?
        .l()?;
    let output = env.as_cast::<JByteArray>(&output)?;
    Ok(env.convert_byte_array(&*output)?)
}
fn credential_path(root: &Path) -> PathBuf {
    let mut hash = seahash::SeaHasher::new();
    hash.write(super::cloud::CREDENTIAL_KEY.as_bytes());
    root.join("gpui-credentials")
        .join(format!("{:016x}", hash.finish()))
}
fn save_credentials(
    env: &mut Env<'_>,
    class: &JClass<'_>,
    path: &Path,
    account: &remote::Account,
) -> Result<()> {
    let mut bytes = (account.domain.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(account.domain.as_bytes());
    bytes.extend_from_slice(&serde_json::to_vec(account)?);
    let encrypted = crypt(env, class, true, &bytes)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, encrypted)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Status {
    ledger_key: String,
    source: Option<Source>,
    folder_id: Option<String>,
    at: u64,
    error: Option<String>,
}
pub(crate) fn restore_status(rule: &mut CameraSync) {
    let Some(app) = gpui::android::app() else {
        return;
    };
    let Some(root) = app.internal_data_path() else {
        return;
    };
    let path = root.join(".local/state/schist/cloud/camera-sync-status.json");
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(status) = serde_json::from_slice::<Status>(&bytes) {
            if status.ledger_key == rule.ledger_key
                && status.source == rule.source
                && status.folder_id == rule.folder_id
                && status.at > rule.last_run.unwrap_or(0)
            {
                if status.error.is_none() {
                    rule.last_run = Some(status.at);
                }
                rule.last_error = status.error;
            }
        }
        let _ = std::fs::remove_file(path);
    }
}

fn run_job(env: &mut Env<'_>, class: &JClass<'_>, context: &JObject<'_>) -> Result<bool> {
    if !crate::feature_enabled("schist-cloud") {
        cancel_scheduled_job(env, context)?;
        return Ok(true);
    }
    // The open app already owns a cloud connection and drives this rule.
    if gpui::android::app().is_some() {
        return Ok(true);
    }
    let cancel = CANCEL.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if JOB_RUNNING
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Ok(true);
    }
    let _job = JobGuard(cancel.clone());
    if gpui::android::app().is_some() {
        return Ok(true);
    }
    let files = env
        .call_method(
            context,
            jni_str!("getFilesDir"),
            jni_sig!(() -> java.io.File),
            &[],
        )?
        .l()?;
    let root = path_of(env, &files)?;
    let prefs = root.join(".config/schist/preferences.json");
    let view: super::ViewOptions = match std::fs::read(&prefs) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e.into()),
    };
    let rule = view.camera_sync;
    if !rule.enabled || rule.source.is_none() || cancel.load(Ordering::Relaxed) {
        return Ok(true);
    }
    let state = root.join(".local/state/schist/cloud");
    std::fs::create_dir_all(&state)?;
    let result: Result<Outcome> = (|| {
        anyhow::ensure!(permission(env, context)?, t("cloud.sync.permission_denied"));
        let path = credential_path(&root);
        if !path.exists() {
            return Ok(Outcome::default());
        }
        let plain = crypt(env, class, false, &std::fs::read(&path)?)?;
        let (len, rest) = plain
            .split_first_chunk::<4>()
            .context("Invalid credential envelope")?;
        let account: remote::Account = serde_json::from_slice(
            rest.get(u32::from_be_bytes(*len) as usize..)
                .context("Invalid credential envelope")?,
        )?;
        let client = remote::Client::start(account);
        let started = Instant::now();
        while !client.handle.online() {
            for event in client.events.try_iter() {
                match event {
                    Event::Credentials(account) => save_credentials(env, class, &path, &account)?,
                    Event::AccountUnavailable => anyhow::bail!(t("cloud.error.sign_in_first")),
                    _ => {}
                }
            }
            anyhow::ensure!(!cancel.load(Ordering::Relaxed), t("cloud.upload.cancelled"));
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(60),
                t("cloud.transport.timed_out")
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let engine = Engine {
            source: rule.source.clone().unwrap(),
            folder_id: rule.folder_id.clone(),
            extensions: camera_sync::photo_extensions(rule.extensions.clone()),
            ledger_key: rule.ledger_key.clone(),
            ledger_path: state.join("camera-sync.json"),
            staging: root.join("camera-sync-staging"),
            cancel: cancel.clone(),
        };
        let handle = client.handle.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        remote::runtime::spawn(async move {
            let result = engine.run(&handle, Arc::new(|_, _, _| {})).await;
            let _ = tx.send(result);
        });
        loop {
            // Hand over to a newly opened activity before it restores credentials.
            if gpui::android::app().is_some() {
                cancel.store(true, Ordering::Relaxed);
            }
            for event in client.events.try_iter() {
                match event {
                    Event::Credentials(account) => save_credentials(env, class, &path, &account)?,
                    Event::AccountUnavailable => cancel.store(true, Ordering::Relaxed),
                    _ => {}
                }
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => return result,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => return Err(error.into()),
            }
        }
    })();
    let status = Status {
        ledger_key: rule.ledger_key,
        source: rule.source,
        folder_id: rule.folder_id,
        at: camera_sync::now_secs(),
        error: match &result {
            Ok(outcome) if outcome.pending > 0 || outcome.skipped > 0 => Some(outcome.message()),
            Err(error) => Some(error.to_string()),
            _ => None,
        },
    };
    let path = state.join("camera-sync-status.json");
    std::fs::write(path.with_extension("tmp"), serde_json::to_vec(&status)?)?;
    std::fs::rename(path.with_extension("tmp"), path)?;
    result.map(|o| o.pending == 0 && o.skipped == 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_infrawrench_schist_CameraSyncJob_prepareSync<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    env.with_env(|_| -> jni::errors::Result<()> {
        *CANCEL.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(AtomicBool::new(false));
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_infrawrench_schist_CameraSyncJob_runSync<'local>(
    mut env: EnvUnowned<'local>,
    class: JClass<'local>,
    context: JObject<'local>,
) -> jni::sys::jboolean {
    env.with_env(|env| -> jni::errors::Result<jni::sys::jboolean> {
        match run_job(env, &class, &context) {
            Ok(done) => Ok(done),
            Err(error) => {
                if env.exception_check() {
                    env.exception_clear();
                }
                log::warn!("camera sync job: {error}");
                Ok(false)
            }
        }
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_infrawrench_schist_CameraSyncJob_stopSync<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    env.with_env(|_| -> jni::errors::Result<()> {
        CANCEL
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .store(true, Ordering::Relaxed);
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
