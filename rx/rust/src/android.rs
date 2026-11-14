//! Android glue: an EGL/GLES display for the Rust-rendered frame, plus the JNI
//! calls that read the installed app list and the system status.
//!
//! Rendering is strictly on demand: the thread sleeps until the UI state is
//! dirty, paints one frame, uploads one texture and goes back to sleep. With a
//! static screen the GPU does nothing and the CPU is idle, which is what keeps
//! the device cool and the battery flat.

use crate::model::{Outcome, State};
use crate::scene::Images;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use jni::objects::{GlobalRef, JByteArray, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::JNIEnv;

use khronos_egl as egl;
use ndk::native_window::NativeWindow;

struct Engine {
    state: State,
    images: Images,
    dirty: bool,
    width: u32,
    height: u32,
    activity: Option<GlobalRef>,
    files_dir: String,
}

static ENGINE: OnceLock<Arc<(Mutex<Engine>, Condvar)>> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

fn engine() -> &'static Arc<(Mutex<Engine>, Condvar)> {
    ENGINE.get_or_init(|| {
        Arc::new((
            Mutex::new(Engine {
                state: State::new(Vec::new()),
                images: Images::new(),
                dirty: true,
                width: 1920,
                height: 1080,
                activity: None,
                files_dir: String::new(),
            }),
            Condvar::new(),
        ))
    })
}

/// A panic while a lock is held poisons the mutex; the launcher must keep
/// running, so take the data anyway instead of `unwrap()`.
fn lock_engine(arc: &Arc<(Mutex<Engine>, Condvar)>) -> std::sync::MutexGuard<'_, Engine> {
    arc.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A failed JNI call leaves a Java exception pending. If it is not cleared,
/// every later call silently fails and the exception surfaces when the native
/// method returns, which kills the process. Every boundary clears it.
fn clear(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

fn mark_dirty() {
    if let Some(arc) = ENGINE.get() {
        let mut e = lock_engine(arc);
        e.dirty = true;
        arc.1.notify_all();
    }
}

fn prefs_path(dir: &str) -> String {
    format!("{dir}/nxs.prefs")
}

fn load_dual(dir: &str) -> bool {
    std::fs::read_to_string(prefs_path(dir))
        .map(|s| s.contains("dual=1"))
        .unwrap_or(false)
}

fn save_dual(dir: &str, dual: bool) {
    let _ = std::fs::write(prefs_path(dir), if dual { "dual=1\n" } else { "dual=0\n" });
}

// ---------------------------------------------------------------- JNI entry

#[no_mangle]
pub extern "system" fn Java_com_nxslauncher_NxsActivity_nativeCreate(
    mut env: JNIEnv,
    this: JObject,
    surface: JObject,
    width: jint,
    height: jint,
    files_dir: JString,
    _package: JString,
) {
    let dir: String = env.get_string(&files_dir).map(Into::into).unwrap_or_default();

    let (vm, activity) = {
        let arc = engine();
        let mut e = lock_engine(&arc);
        e.activity = env.new_global_ref(&this).ok();
        e.files_dir = dir.clone();
        e.width = width.max(1) as u32;
        e.height = height.max(1) as u32;
        e.dirty = true;
        (env.get_java_vm().ok(), e.activity.clone())
    };
    // Nothing from here on may leave an exception pending for the caller.
    clear(&mut env);

    // Enumerating every installed app and rasterising its icon takes long enough
    // to trip an ANR when it runs inside surfaceCreated, so discovery gets its own
    // thread and the first frame is drawn straight away.
    if let (Some(vm), Some(activity)) = (vm, activity) {
        let dir = dir.clone();
        thread::Builder::new()
            .name("nxs-apps".into())
            .spawn(move || {
                let Ok(mut env) = vm.attach_current_thread() else {
                    return;
                };
                // Discovery runs outside the engine lock: holding it for the
                // whole scan would block the UI thread on the next touch and trip
                // an ANR. The lock is taken only to publish the result.
                let mut images = Images::new();
                let mut status = crate::model::StatusBar::default();
                let apps = platform::load_apps(&mut env, activity.as_obj(), &mut images, &dir);
                platform::load_status(&mut env, activity.as_obj(), &mut status, &dir);

                let arc = engine();
                let mut e = lock_engine(&arc);
                e.images = images;
                e.state.replace_apps(apps);
                e.state.status = status;
                e.state.dual = load_dual(&dir) || e.state.dual;
                e.dirty = true;
                arc.1.notify_all();
            })
            .ok();
    }

    STOP.store(false, Ordering::SeqCst);
    if RUNNING.swap(true, Ordering::SeqCst) {
        // A previous surface is still tearing down; wait for it so this one gets
        // its own render thread instead of rendering into a dead window.
        for _ in 0..50 {
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if RUNNING.load(Ordering::SeqCst) {
            mark_dirty();
            return;
        }
        RUNNING.store(true, Ordering::SeqCst);
    }

    let window = unsafe { NativeWindow::from_surface(env.get_native_interface(), surface.as_raw()) };
    let Some(window) = window else {
        RUNNING.store(false, Ordering::SeqCst);
        return;
    };

    thread::Builder::new()
        .name("nxs-render".into())
        .spawn(move || {
            render_loop(window);
            RUNNING.store(false, Ordering::SeqCst);
        })
        .ok();
}

#[no_mangle]
pub extern "system" fn Java_com_nxslauncher_NxsActivity_nativeResize(
    _env: JNIEnv,
    _this: JObject,
    width: jint,
    height: jint,
) {
    let arc = engine();
    {
        let mut e = lock_engine(&arc);
        e.width = width.max(1) as u32;
        e.height = height.max(1) as u32;
        e.dirty = true;
        arc.1.notify_all();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_nxslauncher_NxsActivity_nativeDestroy(_env: JNIEnv, _this: JObject) {
    STOP.store(true, Ordering::SeqCst);
    mark_dirty();
}

#[no_mangle]
pub extern "system" fn Java_com_nxslauncher_NxsActivity_nativeTouch(
    mut env: JNIEnv,
    this: JObject,
    action: jint,
    x: f32,
    y: f32,
    down_time: jlong,
    event_time: jlong,
) {
    let (dx, dy) = {
        let arc = engine();
        let e = lock_engine(&arc);
        let ctx = crate::ui::Ctx::new(e.width as f32, e.height as f32);
        ((x - ctx.ox) / ctx.s, (y - ctx.oy) / ctx.s)
    };

    let mut outcome = Outcome::None;
    let mut persist: Option<bool> = None;
    {
        let arc = engine();
        let mut e = lock_engine(&arc);
        match action {
            0 => e.state.touch_start(dx, dy, event_time),
            2 => e.state.touch_move(dx, dy, event_time),
            _ => {
                outcome = e.state.touch_end(dx, dy, action == 3, down_time, event_time);
                if e.state.persist {
                    e.state.persist = false;
                    persist = Some(e.state.dual);
                }
            }
        }
        e.dirty = true;
        arc.1.notify_all();
    }
    if let (Some(dual), arc) = (persist, engine()) {
        let dir = lock_engine(&arc).files_dir.clone();
        save_dual(&dir, dual);
    }

    match outcome {
        Outcome::Launch(pkg) => {
            if let Ok(s) = env.new_string(pkg) {
                let _ = env.call_method(
                    &this,
                    "launchPackage",
                    "(Ljava/lang/String;)Z",
                    &[JValue::Object(&JObject::from(s))],
                );
            }
        }
        Outcome::PickImage(kind) => {
            let _ = env.call_method(&this, "pickImage", "(I)V", &[JValue::Int(kind)]);
        }
        Outcome::HomeSettings => {
            let _ = env.call_method(&this, "openHomeSettings", "()V", &[]);
        }
        Outcome::None => {}
    }
}

#[no_mangle]
pub extern "system" fn Java_com_nxslauncher_NxsActivity_nativeKey(
    _env: JNIEnv,
    _this: JObject,
    code: jint,
    down: jboolean,
) {
    if down == 0 {
        return;
    }
    let arc = engine();
    let mut e = lock_engine(&arc);
    match code {
        21 => e.state.step(-1),          // DPAD_LEFT
        22 => e.state.step(1),           // DPAD_RIGHT
        19 => e.state.page_shift(-1),    // DPAD_UP
        20 => e.state.page_shift(1),     // DPAD_DOWN
        24 => {}                         // VOLUME_UP
        25 => {}                         // VOLUME_DOWN
        _ => {}
    }
    e.dirty = true;
    arc.1.notify_all();
}

#[no_mangle]
pub extern "system" fn Java_com_nxslauncher_NxsActivity_nativeBack(_env: JNIEnv, _this: JObject) {
    let arc = engine();
    {
        let mut e = lock_engine(&arc);
        if e.state.overlay != crate::model::Overlay::None {
            e.state.overlay = crate::model::Overlay::None;
        } else if e.state.dual {
            e.state.dual = false;
            e.state.persist = true;
        }
        e.dirty = true;
        arc.1.notify_all();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_nxslauncher_NxsActivity_onImagePicked(
    mut env: JNIEnv,
    _this: JObject,
    kind: jint,
    width: jint,
    height: jint,
    rgba: JByteArray,
) {
    let Ok(bytes) = env.convert_byte_array(&rgba) else { return };
    let w = width.max(1) as u32;
    let h = height.max(1) as u32;
    let expected = (w as usize) * (h as usize) * 4;
    if bytes.len() < expected {
        return;
    }
    let Some(mut pixmap) = tiny_skia::Pixmap::new(w, h) else { return };
    // Java handed back straight RGBA; tiny-skia stores premultiplied.
    for i in 0..(w * h) as usize {
        let r = bytes[i * 4] as u32;
        let g = bytes[i * 4 + 1] as u32;
        let b = bytes[i * 4 + 2] as u32;
        let a = bytes[i * 4 + 3] as u32;
        let px = &mut pixmap.data_mut()[i * 4..i * 4 + 4];
        px[0] = ((r * a) / 255) as u8;
        px[1] = ((g * a) / 255) as u8;
        px[2] = ((b * a) / 255) as u8;
        px[3] = a as u8;
    }
    // The stored copy has to be premultiplied like the in-memory pixmap, or a
    // reloaded image comes back brighter than the one that was just picked.
    let premultiplied = pixmap.data().to_vec();
    let arc = engine();
    let mut e = lock_engine(&arc);
    let id = e.images.push(pixmap);
    let selected = e.state.selected;
    let dir = e.files_dir.clone();
    if let Some(app) = e.state.apps.get_mut(selected) {
        if kind == 0 {
            app.boxart = id;
        } else {
            app.hero = id;
        }
        let pkg = app.package.clone();
        platform::write_media(&dir, &pkg, kind, &premultiplied, w, h);
    }
    e.dirty = true;
    arc.1.notify_all();
}

// ------------------------------------------------------------------ drawing

struct Drawer {
    gl: glow::Context,
    program: glow::Program,
    texture: glow::Texture,
    vao: Option<glow::VertexArray>,
    vbo: glow::Buffer,
    tex_w: u32,
    tex_h: u32,
}

fn render_loop(window: NativeWindow) {
    let Ok(instance) = (unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required() }) else { return };
    let Some(display) = (unsafe { instance.get_display(egl::DEFAULT_DISPLAY) }) else { return };
    if instance.initialize(display).is_err() {
        return;
    }
    let attrs = [
        egl::SURFACE_TYPE,
        egl::WINDOW_BIT,
        egl::RENDERABLE_TYPE,
        egl::OPENGL_ES2_BIT,
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::ALPHA_SIZE,
        8,
        egl::NONE,
    ];
    let Ok(Some(config)) = instance.choose_first_config(display, &attrs) else { return };
    let ctx_attrs = [egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE];
    let Ok(context) = instance.create_context(display, config, None, &ctx_attrs) else { return };
    let window_ptr = window.ptr().as_ptr();
    let Ok(surface) =
        (unsafe { instance.create_window_surface(display, config, window_ptr as *mut c_void, None) })
    else {
        return;
    };
    if instance
        .make_current(display, Some(surface), Some(surface), Some(context))
        .is_err()
    {
        return;
    }

    let gl = unsafe {
        glow::Context::from_loader_function(|name| {
            instance
                .get_proc_address(name)
                .map(|f| f as *const c_void)
                .unwrap_or(std::ptr::null())
        })
    };
    let Some(mut drawer) = (unsafe { build_gl(gl) }) else { return };

    let arc = engine();
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        let frame = {
            let mut guard = lock_engine(&arc);
            while !guard.dirty && !STOP.load(Ordering::SeqCst) {
                let (next, _) = arc
                    .1
                    .wait_timeout(guard, Duration::from_secs(30))
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard = next;
            }
            if STOP.load(Ordering::SeqCst) {
                break;
            }
            guard.dirty = false;
            crate::render(&guard.state, guard.width, guard.height, &guard.images)
        };
        if let Some(pixmap) = frame {
            unsafe { drawer.draw(&pixmap) };
            let _ = instance.swap_buffers(display, surface);
        }
    }

    unsafe { teardown(&mut drawer) };
    let _ = instance.make_current(display, None, None, None);
    let _ = instance.destroy_surface(display, surface);
    let _ = instance.destroy_context(display, context);
    let _ = instance.terminate(display);
}

unsafe fn build_gl(gl: glow::Context) -> Option<Drawer> {
    use glow::HasContext;
    let program = gl.create_program().ok()?;
    let vs = gl.create_shader(glow::VERTEX_SHADER).ok()?;
    gl.shader_source(
        vs,
        "attribute vec2 pos; attribute vec2 uv; varying vec2 v_uv;\
         void main() { v_uv = uv; gl_Position = vec4(pos, 0.0, 1.0); }",
    );
    gl.compile_shader(vs);
    let fs = gl.create_shader(glow::FRAGMENT_SHADER).ok()?;
    gl.shader_source(
        fs,
        "precision mediump float; varying vec2 v_uv; uniform sampler2D tex;\
         void main() { gl_FragColor = texture2D(tex, v_uv); }",
    );
    gl.compile_shader(fs);
    gl.attach_shader(program, vs);
    gl.attach_shader(program, fs);
    // Attribute locations must be bound before linking, or the shader may end up
    // with different indices than the ones the vertex arrays feed.
    gl.bind_attrib_location(program, 0, "pos");
    gl.bind_attrib_location(program, 1, "uv");
    gl.link_program(program);
    gl.use_program(Some(program));

    // Full-screen quad; v flipped so the pixmap's top-left is the screen's top-left.
    let verts: [f32; 16] = [
        -1.0, -1.0, 0.0, 1.0, //
        1.0, -1.0, 1.0, 1.0, //
        -1.0, 1.0, 0.0, 0.0, //
        1.0, 1.0, 1.0, 0.0,
    ];
    let vbo = gl.create_buffer().ok()?;
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
    gl.buffer_data_u8_slice(
        glow::ARRAY_BUFFER,
        std::slice::from_raw_parts(verts.as_ptr() as *const u8, 64),
        glow::STATIC_DRAW,
    );

    let vao = gl.create_vertex_array().ok();
    if let Some(vao) = vao {
        gl.bind_vertex_array(Some(vao));
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
    }

    let texture = gl.create_texture().ok()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
    gl.disable(glow::DEPTH_TEST);
    gl.disable(glow::BLEND);

    Some(Drawer {
        gl,
        program,
        texture,
        vao,
        vbo,
        tex_w: 0,
        tex_h: 0,
    })
}

impl Drawer {
    unsafe fn draw(&mut self, pixmap: &tiny_skia::Pixmap) {
        use glow::HasContext;
        let gl = &self.gl;
        gl.viewport(0, 0, pixmap.width() as i32, pixmap.height() as i32);
        gl.clear_color(0.0, 0.0, 0.0, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        if pixmap.width() != self.tex_w || pixmap.height() != self.tex_h {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                pixmap.width() as i32,
                pixmap.height() as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(pixmap.data()),
            );
            self.tex_w = pixmap.width();
            self.tex_h = pixmap.height();
        } else {
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                pixmap.width() as i32,
                pixmap.height() as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(pixmap.data()),
            );
        }
        gl.use_program(Some(self.program));
        if let Some(vao) = self.vao {
            gl.bind_vertex_array(Some(vao));
        } else {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
        }
        gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
    }
}

unsafe fn teardown(drawer: &mut Drawer) {
    use glow::HasContext;
    let gl = &drawer.gl;
    gl.delete_program(drawer.program);
    gl.delete_texture(drawer.texture);
    if let Some(vao) = drawer.vao {
        gl.delete_vertex_array(vao);
    }
    gl.delete_buffer(drawer.vbo);
}

// ------------------------------------------------------------------ platform

pub mod platform {
    use super::*;
    use crate::scene::ImgId;

    /// Reads a Java string. App labels arrive as `CharSequence` and are not
    /// always `java.lang.String` (SpannableString is common), and handing a
    /// non-String to `get_string` raises a ClassCastException that stays pending
    /// and eventually kills the process, so it is stringified first.
    fn jstr(env: &mut JNIEnv, obj: &JObject) -> String {
        if obj.is_null() {
            return String::new();
        }
        let text = match env.call_method(obj, "toString", "()Ljava/lang/String;", &[]) {
            Ok(v) => match v.l() {
                Ok(o) => o,
                Err(_) => {
                    clear(env);
                    return String::new();
                }
            },
            Err(_) => {
                clear(env);
                return String::new();
            }
        };
        match env.get_string(&unsafe { JString::from_raw(text.as_raw()) }) {
            Ok(v) => v.into(),
            Err(_) => {
                clear(env);
                String::new()
            }
        }
    }

    fn new_jstring<'a>(env: &mut JNIEnv<'a>, s: &str) -> Option<JObject<'a>> {
        env.new_string(s).ok().map(JObject::from)
    }

    /// Wraps discovery so that whatever fails inside, no exception is left
    /// pending when control returns to Java.
    pub fn load_apps(
        env: &mut JNIEnv,
        activity: &JObject,
        images: &mut Images,
        dir: &str,
    ) -> Vec<crate::model::AppEntry> {
        let apps = load_apps_inner(env, activity, images, dir);
        clear(env);
        apps
    }

    fn load_apps_inner(
        env: &mut JNIEnv,
        activity: &JObject,
        images: &mut Images,
        dir: &str,
    ) -> Vec<crate::model::AppEntry> {
        let mut apps = Vec::new();
        let Ok(pm_val) = env.call_method(activity, "getPackageManager", "()Landroid/content/pm/PackageManager;", &[]) else {
            return apps;
        };
        let Ok(pm) = pm_val.l() else { return apps };

        let Ok(intent_class) = env.find_class("android/content/Intent") else { return apps };
        let Some(action) = new_jstring(env, "android.intent.action.MAIN") else { return apps };
        let Ok(intent) = env.new_object(&intent_class, "(Ljava/lang/String;)V", &[JValue::Object(&action)]) else {
            return apps;
        };
        let Some(category) = new_jstring(env, "android.intent.category.LAUNCHER") else { return apps };
        let _ = env.call_method(
            &intent,
            "addCategory",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&category)],
        );

        let Ok(list_val) = env.call_method(
            &pm,
            "queryIntentActivities",
            "(Landroid/content/Intent;I)Ljava/util/List;",
            &[JValue::Object(&intent), JValue::Int(0)],
        ) else {
            return apps;
        };
        let Ok(list) = list_val.l() else { return apps };
        let Ok(size_val) = env.call_method(&list, "size", "()I", &[]) else { return apps };
        let Ok(size) = size_val.i() else { return apps };

        for i in 0..size {
            // A throw from one package (uninstalled mid-scan, hidden metadata)
            // must not poison the rest of the loop.
            clear(env);
            let Ok(ri_val) = env.call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(i)]) else {
                continue;
            };
            let Ok(ri) = ri_val.l() else { continue };
            let Ok(info_val) = env.get_field(&ri, "activityInfo", "Landroid/content/pm/ActivityInfo;") else {
                continue;
            };
            let Ok(info) = info_val.l() else { continue };
            let Ok(pkg_val) = env.get_field(&info, "packageName", "Ljava/lang/String;") else { continue };
            let Ok(pkg) = pkg_val.l() else { continue };
            let package = jstr(env, &pkg);
            if package.is_empty() {
                continue;
            }

            let label = match env.call_method(
                &ri,
                "loadLabel",
                "(Landroid/content/pm/PackageManager;)Ljava/lang/CharSequence;",
                &[JValue::Object(&pm)],
            ) {
                Ok(v) => match v.l() {
                    Ok(o) => {
                        let s = jstr(env, &o);
                        if s.is_empty() {
                            package.clone()
                        } else {
                            s
                        }
                    }
                    Err(_) => package.clone(),
                },
                Err(_) => package.clone(),
            };

            let mut version = String::from("1.0");
            let mut size_bytes = 0u64;
            let mut is_game = false;
            if let Some(pkg_str) = new_jstring(env, &package) {
                if let Ok(pi_val) = env.call_method(
                    &pm,
                    "getPackageInfo",
                    "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;",
                    &[JValue::Object(&pkg_str), JValue::Int(0)],
                ) {
                    if let Ok(pi) = pi_val.l() {
                        if let Ok(v) = env.get_field(&pi, "versionName", "Ljava/lang/String;") {
                            if let Ok(v) = v.l() {
                                let s = jstr(env, &v);
                                if !s.is_empty() {
                                    version = s;
                                }
                            }
                        }
                        if let Ok(ai_val) = env.get_field(&pi, "applicationInfo", "Landroid/content/pm/ApplicationInfo;") {
                            if let Ok(ai) = ai_val.l() {
                                if let Ok(src) = env.get_field(&ai, "sourceDir", "Ljava/lang/String;") {
                                    if let Ok(src) = src.l() {
                                        let path = jstr(env, &src);
                                        size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                                    }
                                }
                                                                if let Ok(cat) = env.get_field(&ai, "category", "I") {
                                    if let Ok(cat) = cat.i() {
                                        // ApplicationInfo.CATEGORY_GAME
                                        is_game = cat == 2;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let icon = load_icon(env, &ri, &pm, images).unwrap_or(ImgId::None);
            let custom_box = read_media(images, dir, &package, 0);
            let custom_hero = read_media(images, dir, &package, 1);

            apps.push(crate::model::AppEntry {
                package,
                label,
                version,
                size_bytes,
                play_ms: 0,
                last_played_min: 0,
                is_game,
                is_media: false,
                boxart: custom_box.unwrap_or(ImgId::None),
                hero: custom_hero.unwrap_or(ImgId::None),
                icon,
                seed: (apps.len() as u32 * 47) % 360,
            });
        }
        // Games first, then everything else, each alphabetically.
        apps.sort_by(|a, b| b.is_game.cmp(&a.is_game).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase())));
        apps
    }

    /// Custom media written by a previous run, as premultiplied RGBA.
    fn read_media(images: &mut Images, dir: &str, pkg: &str, kind: i32) -> Option<ImgId> {
        if dir.is_empty() {
            return None;
        }
        let name = if kind == 0 { "box" } else { "hero" };
        let path = format!("{dir}/media/{pkg}.{name}.rgba");
        let bytes = std::fs::read(path).ok()?;
        if bytes.len() < 8 {
            return None;
        }
        let w = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
        let h = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        let expected = (w as usize) * (h as usize) * 4;
        if w == 0 || h == 0 || bytes.len() < 8 + expected {
            return None;
        }
        let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
        pixmap.data_mut().copy_from_slice(&bytes[8..8 + expected]);
        Some(images.push(pixmap))
    }

    /// Rasterises a launcher icon. A package without a usable icon is normal
    /// (the card falls back to its procedural art), so every failure path ends
    /// by clearing whatever exception the attempt raised.
    fn load_icon(
        env: &mut JNIEnv,
        resolve_info: &JObject,
        pm: &JObject,
        images: &mut Images,
    ) -> Option<ImgId> {
        let out = load_icon_inner(env, resolve_info, pm, images);
        if out.is_none() {
            clear(env);
        }
        out
    }

    fn load_icon_inner(
        env: &mut JNIEnv,
        resolve_info: &JObject,
        pm: &JObject,
        images: &mut Images,
    ) -> Option<ImgId> {
        let drawable = env
            .call_method(
                resolve_info,
                "loadIcon",
                "(Landroid/content/pm/PackageManager;)Landroid/graphics/drawable/Drawable;",
                &[JValue::Object(pm)],
            )
            .ok()?
            .l()
            .ok()?;
        let size = 192i32;
        // createBitmap wants a real Config; a null one is rejected on some builds.
        let config_class = env.find_class("android/graphics/Bitmap$Config").ok()?;
        let config = env
            .get_static_field(&config_class, "ARGB_8888", "Landroid/graphics/Bitmap$Config;")
            .ok()?
            .l()
            .ok()?;
        let bitmap_class = env.find_class("android/graphics/Bitmap").ok()?;
        let bitmap = env
            .call_static_method(
                &bitmap_class,
                "createBitmap",
                "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
                &[JValue::Int(size), JValue::Int(size), JValue::Object(&config)],
            )
            .ok()?
            .l()
            .ok()?;
        let canvas_class = env.find_class("android/graphics/Canvas").ok()?;
        let canvas = env
            .new_object(&canvas_class, "(Landroid/graphics/Bitmap;)V", &[JValue::Object(&bitmap)])
            .ok()?;
        let _ = env.call_method(
            &drawable,
            "setBounds",
            "(IIII)V",
            &[JValue::Int(0), JValue::Int(0), JValue::Int(size), JValue::Int(size)],
        );
        let _ = env.call_method(&drawable, "draw", "(Landroid/graphics/Canvas;)V", &[JValue::Object(&canvas)]);

        let pixels = env.new_int_array((size * size) as i32).ok()?;
        let _ = env.call_method(
            &bitmap,
            "getPixels",
            "([IIIIIII)V",
            &[
                JValue::Object(&pixels),
                JValue::Int(0),
                JValue::Int(size),
                JValue::Int(0),
                JValue::Int(0),
                JValue::Int(size),
                JValue::Int(size),
            ],
        );
        let mut ints = vec![0i32; (size * size) as usize];
        env.get_int_array_region(&pixels, 0, &mut ints).ok()?;
        let mut pixmap = tiny_skia::Pixmap::new(size as u32, size as u32)?;
        for (i, p) in ints.iter().enumerate() {
            let a = ((p >> 24) & 0xFF) as u32;
            let r = ((p >> 16) & 0xFF) as u32;
            let g = ((p >> 8) & 0xFF) as u32;
            let b = (*p & 0xFF) as u32;
            let px = &mut pixmap.data_mut()[i * 4..i * 4 + 4];
            px[0] = r as u8;
            px[1] = g as u8;
            px[2] = b as u8;
            px[3] = a as u8;
        }
        Some(images.push(pixmap))
    }

    pub fn load_status(
        env: &mut JNIEnv,
        activity: &JObject,
        status: &mut crate::model::StatusBar,
        dir: &str,
    ) {
        load_status_inner(env, activity, status, dir);
        clear(env);
    }

    fn load_status_inner(
        env: &mut JNIEnv,
        activity: &JObject,
        status: &mut crate::model::StatusBar,
        dir: &str,
    ) {
        if let Some(name) = new_jstring(env, "batterymanager") {
            if let Ok(bm_val) = env.call_method(
                activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&name)],
            ) {
                if let Ok(bm) = bm_val.l() {
                    if let Ok(level) = env.call_method(&bm, "getIntProperty", "(I)I", &[JValue::Int(4)]) {
                        if let Ok(level) = level.i() {
                            if level > 0 {
                                status.battery = level.clamp(0, 100) as u8;
                            }
                        }
                    }
                }
            }
        }
        if let Ok(fmt_class) = env.find_class("java/text/SimpleDateFormat") {
            if let Some(pattern) = new_jstring(env, "h:mm a") {
                if let Ok(fmt) = env.new_object(&fmt_class, "(Ljava/lang/String;)V", &[JValue::Object(&pattern)]) {
                    if let Ok(date_class) = env.find_class("java/util/Date") {
                        if let Ok(date) = env.new_object(&date_class, "()V", &[]) {
                            if let Ok(text) = env.call_method(
                                &fmt,
                                "format",
                                "(Ljava/util/Date;)Ljava/lang/String;",
                                &[JValue::Object(&date)],
                            ) {
                                if let Ok(text) = text.l() {
                                    let s = jstr(env, &text);
                                    if !s.is_empty() {
                                        status.clock = s;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // Free space in the app's own filesystem.
        if !dir.is_empty() {
            if let Ok(out) = std::process::Command::new("/system/bin/df")
                .arg("-B1")
                .arg(dir)
                .output()
            {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(line) = text.lines().nth(1) {
                    let cols: Vec<&str> = line.split_whitespace().collect();
                    if cols.len() > 3 {
                        if let Ok(available) = cols[3].parse::<u64>() {
                            status.free_bytes = available;
                        }
                    }
                }
            }
        }
        status.online = true;
    }

    pub fn write_media(dir: &str, pkg: &str, kind: i32, rgba: &[u8], w: u32, h: u32) {
        if dir.is_empty() {
            return;
        }
        let folder = format!("{dir}/media");
        let _ = std::fs::create_dir_all(&folder);
        let name = if kind == 0 { "box" } else { "hero" };
        let path = format!("{folder}/{pkg}.{name}.rgba");
        let mut buf = Vec::with_capacity(rgba.len() + 8);
        buf.extend_from_slice(&w.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
        buf.extend_from_slice(rgba);
        let _ = std::fs::write(path, buf);
    }
}
