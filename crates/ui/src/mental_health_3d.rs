//! Pure-Rust WASM WebGL2 3D interactive chart (`glow` + `glam`) for Abbott Lingo CGM
//! and Mental Health check-ins.
//!
//! Geometry generation is target-independent and unit-tested on native CI; the
//! `WebGl2RenderingContext` renderer compiles exclusively for `wasm32` (`hydrate` / `csr`).

use leptos::prelude::*;

use crate::mental_health_cgm::{downsample_readings_for_chart, select_notable_spike_peaks};
use crate::mental_health_lingo::{LingoReading, MentalHealthCheckIn};

/// Interleaved `[x, y, z, r, g, b, a]` geometry buffers ready for WebGL2 upload.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chart3DScene {
    /// Opaque triangle vertices (7 floats per vertex: pos xyz + rgba).
    pub opaque_triangles: Vec<f32>,
    /// Translucent triangle vertices drawn with depth writes disabled (7 floats per vertex).
    pub translucent_triangles: Vec<f32>,
    /// Line segment vertices (7 floats per vertex: pos xyz + rgba).
    pub lines: Vec<f32>,
}

fn push_vertex(buf: &mut Vec<f32>, pos: [f32; 3], color: [f32; 4]) {
    buf.extend_from_slice(&pos);
    buf.extend_from_slice(&color);
}

fn push_quad(
    buf: &mut Vec<f32>,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    d: [f32; 3],
    color: [f32; 4],
) {
    push_vertex(buf, a, color);
    push_vertex(buf, b, color);
    push_vertex(buf, c, color);
    push_vertex(buf, a, color);
    push_vertex(buf, c, color);
    push_vertex(buf, d, color);
}

fn push_octahedron(buf: &mut Vec<f32>, center: [f32; 3], radius: f32, color: [f32; 4]) {
    let [cx, cy, cz] = center;
    let top = [cx, cy + radius, cz];
    let bot = [cx, cy - radius, cz];
    let ring = [
        [cx + radius, cy, cz],
        [cx, cy, cz + radius],
        [cx - radius, cy, cz],
        [cx, cy, cz - radius],
    ];
    let light_top = [
        (color[0] * 1.15).min(1.0),
        (color[1] * 1.15).min(1.0),
        (color[2] * 1.15).min(1.0),
        color[3],
    ];
    let shade_bot = [color[0] * 0.72, color[1] * 0.72, color[2] * 0.72, color[3]];

    for i in 0..4 {
        let next = (i + 1) % 4;
        push_vertex(buf, top, light_top);
        push_vertex(buf, ring[i], color);
        push_vertex(buf, ring[next], color);

        push_vertex(buf, bot, shade_bot);
        push_vertex(buf, ring[next], shade_bot);
        push_vertex(buf, ring[i], shade_bot);
    }
}

/// Build the 3D triangle & line scene for Abbott Lingo glucose + mood/anxiety check-ins.
pub fn build_lingo_3d_scene(
    readings: &[LingoReading],
    checkins: &[MentalHealthCheckIn],
) -> Chart3DScene {
    let mut scene = Chart3DScene::default();
    let max_hour = readings
        .iter()
        .map(|r| r.hour_offset)
        .chain(checkins.iter().map(|c| c.hour_offset))
        .fold(24.0_f64, f64::max)
        .max(1.0);
    let min_g = 55.0_f64;
    let max_g = readings
        .iter()
        .map(|r| r.glucose_mg_dl)
        .fold(180.0_f64, f64::max)
        .max(180.0);

    let x_for = |h: f64| -1.55 + ((h / max_hour) as f32) * 3.10;
    let y_for_g = |g: f64| (((g.clamp(min_g, max_g) - min_g) / (max_g - min_g)) as f32) * 1.55;
    let y_for_score = |s: u8| ((f32::from(s.clamp(1, 10)) - 1.0) / 9.0) * 1.45 + 0.05;

    // 1. Subtle floor grid (z in [-0.75, 0.75], x in [-1.55, 1.55])
    let grid_col = [0.35, 0.45, 0.58, 0.35];
    for step in 0..=6 {
        let x = -1.55 + (step as f32 / 6.0) * 3.10;
        push_vertex(&mut scene.lines, [x, 0.0, -0.75], grid_col);
        push_vertex(&mut scene.lines, [x, 0.0, 0.75], grid_col);
    }
    for step in 0..=4 {
        let z = -0.75 + (step as f32 / 4.0) * 1.50;
        push_vertex(&mut scene.lines, [-1.55, 0.0, z], grid_col);
        push_vertex(&mut scene.lines, [1.55, 0.0, z], grid_col);
    }

    // 2. Translucent emerald Abbott Lingo 70–140 mg/dL target planes + wireframe rails
    let y70 = y_for_g(70.0);
    let y140 = y_for_g(140.0);
    let rail_col = [0.20, 0.83, 0.60, 0.55];
    for y in [y70, y140] {
        push_vertex(&mut scene.lines, [-1.55, y, -0.14], rail_col);
        push_vertex(&mut scene.lines, [1.55, y, -0.14], rail_col);
        push_vertex(&mut scene.lines, [-1.55, y, 0.14], rail_col);
        push_vertex(&mut scene.lines, [1.55, y, 0.14], rail_col);
    }
    push_quad(
        &mut scene.translucent_triangles,
        [-1.55, y70, -0.14],
        [1.55, y70, -0.14],
        [1.55, y140, -0.14],
        [-1.55, y140, -0.14],
        [0.20, 0.83, 0.60, 0.14],
    );

    // 3. 3D Glucose ribbon + translucent curtain + spike diamonds (bounded for multi-week traces)
    let plot_readings = downsample_readings_for_chart(readings, 320);
    let spike_peaks = select_notable_spike_peaks(readings, 36);
    let z_front = 0.08_f32;
    let z_back = -0.08_f32;
    for pair in plot_readings.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        let x0 = x_for(a.hour_offset);
        let y0 = y_for_g(a.glucose_mg_dl);
        let x1 = x_for(b.hour_offset);
        let y1 = y_for_g(b.glucose_mg_dl);

        let is_spike = a.glucose_mg_dl > 140.0 || b.glucose_mg_dl > 140.0;
        let top_col = if is_spike {
            [0.98, 0.75, 0.14, 1.0] // amber spike
        } else {
            [0.13, 0.83, 0.93, 1.0] // cyan steady
        };
        let curtain_col = if is_spike {
            [0.98, 0.75, 0.14, 0.24]
        } else {
            [0.13, 0.83, 0.93, 0.18]
        };

        // Top 3D ribbon face (opaque)
        push_quad(
            &mut scene.opaque_triangles,
            [x0, y0, z_back],
            [x1, y1, z_back],
            [x1, y1, z_front],
            [x0, y0, z_front],
            top_col,
        );
        // Translucent vertical area curtain
        push_quad(
            &mut scene.translucent_triangles,
            [x0, 0.0, 0.0],
            [x1, 0.0, 0.0],
            [x1, y1, 0.0],
            [x0, y0, 0.0],
            curtain_col,
        );
        // Bright ridge line
        push_vertex(
            &mut scene.lines,
            [x0, y0 + 0.008, 0.0],
            [0.82, 0.98, 1.0, 1.0],
        );
        push_vertex(
            &mut scene.lines,
            [x1, y1 + 0.008, 0.0],
            [0.82, 0.98, 1.0, 1.0],
        );
    }

    for r in &spike_peaks {
        let x = x_for(r.hour_offset);
        let y = y_for_g(r.glucose_mg_dl);
        push_octahedron(
            &mut scene.opaque_triangles,
            [x, y + 0.05, 0.0],
            0.055,
            [0.98, 0.75, 0.14, 1.0],
        );
    }

    // 4. Mental health check-in 3D markers (Mood at z=+0.48 emerald, Anxiety at z=-0.48 rose)
    let mood_col = [0.20, 0.83, 0.60, 1.0];
    let anx_col = [0.98, 0.44, 0.52, 1.0];
    let stem_col = [0.58, 0.64, 0.72, 0.45];

    for c in checkins {
        let x = x_for(c.hour_offset);
        let my = y_for_score(c.mood);
        let ay = y_for_score(c.anxiety);

        push_octahedron(&mut scene.opaque_triangles, [x, my, 0.48], 0.065, mood_col);
        push_vertex(&mut scene.lines, [x, 0.0, 0.48], stem_col);
        push_vertex(&mut scene.lines, [x, my, 0.48], mood_col);

        push_octahedron(&mut scene.opaque_triangles, [x, ay, -0.48], 0.065, anx_col);
        push_vertex(&mut scene.lines, [x, 0.0, -0.48], stem_col);
        push_vertex(&mut scene.lines, [x, ay, -0.48], anx_col);
    }

    // Connect Mood and Anxiety trajectories in 3D depth lanes
    for pair in checkins.windows(2) {
        let x0 = x_for(pair[0].hour_offset);
        let x1 = x_for(pair[1].hour_offset);
        push_vertex(
            &mut scene.lines,
            [x0, y_for_score(pair[0].mood), 0.48],
            mood_col,
        );
        push_vertex(
            &mut scene.lines,
            [x1, y_for_score(pair[1].mood), 0.48],
            mood_col,
        );
        push_vertex(
            &mut scene.lines,
            [x0, y_for_score(pair[0].anxiety), -0.48],
            anx_col,
        );
        push_vertex(
            &mut scene.lines,
            [x1, y_for_score(pair[1].anxiety), -0.48],
            anx_col,
        );
    }

    scene
}

/// Interactive 3D WebGL2 (`glow` + `glam`) Abbott Lingo CGM & Mood/Anxiety viewport.
#[component]
pub fn LingoMood3DChart(
    readings: Signal<Vec<LingoReading>>,
    checkins: Signal<Vec<MentalHealthCheckIn>>,
) -> impl IntoView {
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let yaw = RwSignal::new(-0.42_f32);
    let pitch = RwSignal::new(0.36_f32);
    let distance = RwSignal::new(3.85_f32);
    let dragging = RwSignal::new(false);
    let last_xy = RwSignal::new((0_i32, 0_i32));
    let gl_error = RwSignal::new(Option::<String>::None);

    #[cfg(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr")))]
    Effect::new(move |_| {
        let r = readings.get();
        let c = checkins.get();
        let y = yaw.get();
        let p = pitch.get();
        let dist = distance.get();
        if let Some(canvas) = canvas_ref.get() {
            let scene = build_lingo_3d_scene(&r, &c);
            match render_webgl2_scene(&canvas, &scene, y, p, dist) {
                Ok(()) => gl_error.set(None),
                Err(err) => gl_error.set(Some(err)),
            }
        }
    });

    #[cfg(not(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr"))))]
    let _ = (&readings, &checkins, &gl_error);

    let on_mousedown = move |ev: leptos::ev::MouseEvent| {
        dragging.set(true);
        last_xy.set((ev.client_x(), ev.client_y()));
    };
    let on_mouseup = move |_: leptos::ev::MouseEvent| {
        dragging.set(false);
    };
    let on_mousemove = move |ev: leptos::ev::MouseEvent| {
        if dragging.get() {
            let (lx, ly) = last_xy.get();
            let dx = (ev.client_x() - lx) as f32;
            let dy = (ev.client_y() - ly) as f32;
            last_xy.set((ev.client_x(), ev.client_y()));
            yaw.update(|v| *v -= dx * 0.008);
            pitch.update(|v| *v = (*v + dy * 0.006).clamp(0.08, 1.35));
        }
    };
    let reset_view = move |_| {
        yaw.set(-0.42);
        pitch.set(0.36);
        distance.set(3.85);
    };
    let zoom_in = move |_| distance.update(|d| *d = (*d - 0.4).max(2.0));
    let zoom_out = move |_| distance.update(|d| *d = (*d + 0.4).min(6.5));

    view! {
        <div class="space-y-3">
            <div class="relative overflow-hidden rounded-xl border border-white/10 bg-slate-950">
                <canvas
                    node_ref=canvas_ref
                    width="900"
                    height="360"
                    class="block h-[300px] w-full cursor-grab active:cursor-grabbing"
                    aria-label="Interactive 3D WebGL2 Abbott Lingo glucose and mental state trajectory"
                    on:mousedown=on_mousedown
                    on:mouseup=on_mouseup
                    on:mouseleave=on_mouseup
                    on:mousemove=on_mousemove
                />
                <div class="pointer-events-none absolute top-3 left-3 rounded-lg bg-slate-950/75 px-2.5 py-1 text-[11px] text-slate-300 backdrop-blur">
                    "Pure-Rust WebGL2 (glow + glam) · Drag to orbit"
                </div>
                <div class="absolute right-3 bottom-3 flex gap-1.5">
                    <button
                        type="button"
                        on:click=zoom_in
                        class="rounded-md border border-white/15 bg-slate-900/90 px-2.5 py-1 text-xs text-slate-200 transition hover:bg-slate-800"
                    >
                        "+"
                    </button>
                    <button
                        type="button"
                        on:click=zoom_out
                        class="rounded-md border border-white/15 bg-slate-900/90 px-2.5 py-1 text-xs text-slate-200 transition hover:bg-slate-800"
                    >
                        "−"
                    </button>
                    <button
                        type="button"
                        on:click=reset_view
                        class="rounded-md border border-white/15 bg-slate-900/90 px-2.5 py-1 text-xs text-slate-200 transition hover:bg-slate-800"
                    >
                        "Reset 3D"
                    </button>
                </div>
            </div>
            {move || gl_error.get().map(|err| view! {
                <p class="text-xs text-amber-300">{format!("WebGL2 unavailable ({err}); switch to 2D SVG above.")}</p>
            })}
            <p class="text-xs text-slate-400">
                "X-axis: 24h timeline · Y-axis: Lingo glucose (mg/dL) with emerald 70–140 target plane & amber spikes · Z-axis depth: Mood lane (front, emerald) & Anxiety lane (back, rose)."
            </p>
        </div>
    }
}

#[cfg(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr")))]
fn render_webgl2_scene(
    canvas: &web_sys::HtmlCanvasElement,
    scene: &Chart3DScene,
    yaw: f32,
    pitch: f32,
    distance: f32,
) -> Result<(), String> {
    use glam::{Mat4, Vec3};
    use glow::HasContext;
    use wasm_bindgen::JsCast;

    let raw_ctx = canvas
        .get_context("webgl2")
        .map_err(|_| "getContext(webgl2) threw".to_string())?
        .ok_or_else(|| "browser returned no webgl2 context".to_string())?;
    let webgl2: web_sys::WebGl2RenderingContext = raw_ctx
        .dyn_into()
        .map_err(|_| "context is not WebGl2RenderingContext".to_string())?;

    let gl = glow::Context::from_webgl2_context(webgl2);

    const VERT_SRC: &str = r#"#version 300 es
        precision highp float;
        layout(location = 0) in vec3 a_pos;
        layout(location = 1) in vec4 a_color;
        uniform mat4 u_mvp;
        out vec4 v_color;
        void main() {
            v_color = a_color;
            gl_Position = u_mvp * vec4(a_pos, 1.0);
        }
    "#;
    const FRAG_SRC: &str = r#"#version 300 es
        precision highp float;
        in vec4 v_color;
        out vec4 frag_color;
        void main() {
            frag_color = v_color;
        }
    "#;

    let width = canvas.width().max(1) as i32;
    let height = canvas.height().max(1) as i32;
    let aspect = width as f32 / height as f32;

    let eye = Vec3::new(
        distance * pitch.cos() * yaw.sin(),
        0.55 + distance * pitch.sin(),
        distance * pitch.cos() * yaw.cos(),
    );
    let target = Vec3::new(0.0, 0.55, 0.0);
    let view = Mat4::look_at_rh(eye, target, Vec3::Y);
    let proj = Mat4::perspective_rh(42.0_f32.to_radians(), aspect, 0.1, 40.0);
    let mvp = (proj * view).to_cols_array();

    // SAFETY: All WebGL2 calls operate on a valid context bound to the canvas
    // and clean up shaders, program, VBO, and VAO before returning.
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| format!("create_program: {e}"))?;
        let vs = gl
            .create_shader(glow::VERTEX_SHADER)
            .map_err(|e| format!("create_shader vs: {e}"))?;
        gl.shader_source(vs, VERT_SRC);
        gl.compile_shader(vs);
        if !gl.get_shader_compile_status(vs) {
            let log = gl.get_shader_info_log(vs);
            gl.delete_shader(vs);
            gl.delete_program(program);
            return Err(format!("vertex shader error: {log}"));
        }

        let fs = gl
            .create_shader(glow::FRAGMENT_SHADER)
            .map_err(|e| format!("create_shader fs: {e}"))?;
        gl.shader_source(fs, FRAG_SRC);
        gl.compile_shader(fs);
        if !gl.get_shader_compile_status(fs) {
            let log = gl.get_shader_info_log(fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            gl.delete_program(program);
            return Err(format!("fragment shader error: {log}"));
        }

        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("program link error: {log}"));
        }

        gl.viewport(0, 0, width, height);
        gl.clear_color(0.008, 0.024, 0.090, 1.0);
        gl.enable(glow::DEPTH_TEST);
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);

        gl.use_program(Some(program));
        if let Some(loc) = gl.get_uniform_location(program, "u_mvp") {
            gl.uniform_matrix_4_f32_slice(Some(&loc), false, &mvp);
        }

        let vao = match gl.create_vertex_array() {
            Ok(vao) => vao,
            Err(e) => {
                gl.delete_program(program);
                return Err(format!("create_vao: {e}"));
            }
        };
        let vbo = match gl.create_buffer() {
            Ok(vbo) => vbo,
            Err(e) => {
                gl.delete_vertex_array(vao);
                gl.delete_program(program);
                return Err(format!("create_vbo: {e}"));
            }
        };
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

        let stride = (7 * std::mem::size_of::<f32>()) as i32;
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(
            1,
            4,
            glow::FLOAT,
            false,
            stride,
            (3 * std::mem::size_of::<f32>()) as i32,
        );

        // 1. Draw opaque 3D ribbons and mood/anxiety/spike octahedra with full depth writes.
        gl.depth_mask(true);
        if !scene.opaque_triangles.is_empty() {
            let bytes = floats_as_bytes(&scene.opaque_triangles);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            gl.draw_arrays(
                glow::TRIANGLES,
                0,
                (scene.opaque_triangles.len() / 7) as i32,
            );
        }
        // 2. Draw floor grid, target rails, stems, and trajectories.
        if !scene.lines.is_empty() {
            let bytes = floats_as_bytes(&scene.lines);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            gl.draw_arrays(glow::LINES, 0, (scene.lines.len() / 7) as i32);
        }
        // 3. Draw translucent target zone plane & glucose curtain without occluding objects behind them.
        if !scene.translucent_triangles.is_empty() {
            gl.depth_mask(false);
            let bytes = floats_as_bytes(&scene.translucent_triangles);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            gl.draw_arrays(
                glow::TRIANGLES,
                0,
                (scene.translucent_triangles.len() / 7) as i32,
            );
            gl.depth_mask(true);
        }

        gl.bind_vertex_array(None);
        gl.delete_buffer(vbo);
        gl.delete_vertex_array(vao);
        gl.delete_program(program);
    }

    Ok(())
}

#[cfg(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr")))]
fn floats_as_bytes(floats: &[f32]) -> &[u8] {
    // SAFETY: `f32` has no padding bytes and aligns cleanly to `u8`.
    unsafe {
        std::slice::from_raw_parts(floats.as_ptr().cast::<u8>(), std::mem::size_of_val(floats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_health_lingo::{parse_lingo_csv, sample_checkins, SAMPLE_LINGO_CSV};
    use leptos::prelude::RenderHtml;

    #[test]
    fn builds_valid_3d_triangles_and_lines_for_sample_lingo_data() {
        let readings = parse_lingo_csv(SAMPLE_LINGO_CSV).unwrap();
        let checkins = sample_checkins();
        let scene = build_lingo_3d_scene(&readings, &checkins);

        assert!(!scene.opaque_triangles.is_empty());
        assert!(!scene.translucent_triangles.is_empty());
        assert!(!scene.lines.is_empty());
        assert_eq!(scene.opaque_triangles.len() % (7 * 3), 0);
        assert_eq!(scene.translucent_triangles.len() % (7 * 3), 0);
        assert_eq!(scene.lines.len() % (7 * 2), 0);
        assert!(scene.opaque_triangles.iter().all(|v| v.is_finite()));
        assert!(scene.translucent_triangles.iter().all(|v| v.is_finite()));
        assert!(scene.lines.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn renders_3d_canvas_shell_on_ssr() {
        let readings = RwSignal::new(parse_lingo_csv(SAMPLE_LINGO_CSV).unwrap());
        let checkins = RwSignal::new(sample_checkins());
        let html = view! {
            <LingoMood3DChart readings=readings.into() checkins=checkins.into() />
        }
        .to_html();
        assert!(html.contains("<canvas"));
        assert!(html.contains("Pure-Rust WebGL2 (glow + glam)"));
        assert!(html.contains("Reset 3D"));
    }
}
