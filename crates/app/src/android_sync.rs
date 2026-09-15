//! JNI exports stay in the Android shared library. The backup implementation
//! lives in schist-camera-sync, which is an ordinary Rust dependency.

use jni::{
    objects::{JClass, JObject},
    EnvUnowned,
};

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_infrawrench_schist_CameraSyncJob_prepareSync<'local>(
    env: EnvUnowned<'local>,
    class: JClass<'local>,
) {
    schist_camera_sync::android::prepare_sync(env, class);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_infrawrench_schist_CameraSyncJob_runSync<'local>(
    env: EnvUnowned<'local>,
    class: JClass<'local>,
    context: JObject<'local>,
) -> jni::sys::jboolean {
    schist_camera_sync::android::run_sync(env, class, context)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_infrawrench_schist_CameraSyncJob_stopSync<'local>(
    env: EnvUnowned<'local>,
    class: JClass<'local>,
) {
    schist_camera_sync::android::stop_sync(env, class);
}
