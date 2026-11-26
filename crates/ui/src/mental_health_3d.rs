//! Pure-Rust WASM WebGL2 3D interactive chart (`glow` + `glam`) for Abbott Lingo CGM
//! and Mental Health check-ins.
//!
//! Geometry generation is target-independent and unit-tested on native CI; the
//! `WebGl2RenderingContext` renderer compiles exclusively for `wasm32` (`hydrate` / `csr`).

use std::sync::Arc;

use leptos::prelude::*;

use crate::mental_health_cgm::select_notable_spike_peaks;
use crate::mental_health_cgm_analytics::{downsample_segments_for_chart, DEFAULT_CHART_GAP_HOURS};
use crate::mental_health_lingo::{LingoReading, MentalHealthCheckIn};

const COLOR_STEADY: [f32; 4] = [0.13, 0.83, 0.93, 1.0];
const COLOR_STEADY_CURTAIN: [f32; 4] = [0.13, 0.83, 0.93, 0.18];
const COLOR_SPIKE: [f32; 4] = [0.98, 0.75, 0.14, 1.0];
const COLOR_SPIKE_CURTAIN: [f32; 4] = [0.98, 0.75, 0.14, 0.24];
const COLOR_HYPO: [f32; 4] = [0.98, 0.44, 0.52, 1.0];
const COLOR_HYPO_CURTAIN: [f32; 4] = [0.98, 0.44, 0.52, 0.22];
/// Distinct vibrant orange-red for ceiling-capped (`>200` / `≥201`) sensor saturation.
pub const COLOR_CEILING_CAP: [f32; 4] = [0.98, 0.45, 0.20, 1.0];
const COLOR_CEILING_CAP_CURTAIN: [f32; 4] = [0.98, 0.45, 0.20, 0.28];
/// Distinct crimson for floor-capped (`<55` / `≤54`) sensor saturation.
pub const COLOR_FLOOR_CAP: [f32; 4] = [0.96, 0.25, 0.37, 1.0];
const COLOR_FLOOR_CAP_CURTAIN: [f32; 4] = [0.96, 0.25, 0.37, 0.28];

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

/// Format a compact human-readable timeline span label (`"24h"`, `"14d"`, `"112.7d"`, etc.)
/// for the 3D chart caption based on the actual trace span.
pub fn format_trace_span_label(
    readings: &[LingoReading],
    checkins: &[MentalHealthCheckIn],
) -> String {
    let span_hours = if readings.len() >= 2 {
        let first = readings.first().map(|r| r.hour_offset).unwrap_or(0.0);
        let last = readings.last().map(|r| r.hour_offset).unwrap_or(0.0);
        (last - first).max(0.0)
    } else {
        readings
            .iter()
            .map(|r| r.hour_offset)
            .chain(checkins.iter().map(|c| c.hour_offset))
            .fold(0.0_f64, f64::max)
    };

    if span_hours <= 0.0 {
        return "24h".to_string();
    }
    if span_hours >= 48.0 {
        let days = span_hours / 24.0;
        if (days - days.round()).abs() < 0.05 {
            format!("{:.0}d", days.round())
        } else {
            format!("{days:.1}d")
        }
    } else if (span_hours - span_hours.round()).abs() < 0.05 {
        format!("{:.0}h", span_hours.round().max(1.0))
    } else {
        format!("{span_hours:.1}h")
    }
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

fn pair_ribbon_colors(a: &LingoReading, b: &LingoReading) -> ([f32; 4], [f32; 4], [f32; 4]) {
    if a.bound.is_floor_capped() || b.bound.is_floor_capped() {
        (COLOR_FLOOR_CAP, COLOR_FLOOR_CAP_CURTAIN, COLOR_FLOOR_CAP)
    } else if a.bound.is_ceiling_capped() || b.bound.is_ceiling_capped() {
        (
            COLOR_CEILING_CAP,
            COLOR_CEILING_CAP_CURTAIN,
            COLOR_CEILING_CAP,
        )
    } else if a.glucose_mg_dl < 70.0 || b.glucose_mg_dl < 70.0 {
        (COLOR_HYPO, COLOR_HYPO_CURTAIN, COLOR_HYPO)
    } else if a.glucose_mg_dl > 140.0 || b.glucose_mg_dl > 140.0 {
        (COLOR_SPIKE, COLOR_SPIKE_CURTAIN, [1.0, 0.92, 0.65, 1.0])
    } else {
        (COLOR_STEADY, COLOR_STEADY_CURTAIN, [0.82, 0.98, 1.0, 1.0])
    }
}

fn point_color(r: &LingoReading) -> [f32; 4] {
    if r.bound.is_floor_capped() {
        COLOR_FLOOR_CAP
    } else if r.bound.is_ceiling_capped() {
        COLOR_CEILING_CAP
    } else if r.glucose_mg_dl < 70.0 {
        COLOR_HYPO
    } else if r.glucose_mg_dl > 140.0 {
        COLOR_SPIKE
    } else {
        COLOR_STEADY
    }
}

/// Select one representative point per contiguous floor-capped run (up to `max_markers`).
fn select_notable_floor_caps(readings: &[LingoReading], max_markers: usize) -> Vec<LingoReading> {
    if readings.is_empty() || max_markers == 0 {
        return Vec::new();
    }
    const GAP_BREAK_HOURS: f64 = 1.5;
    let mut reps: Vec<usize> = Vec::new();
    let mut active_start: Option<usize> = None;
    let mut prev_idx: usize = 0;

    for (idx, r) in readings.iter().enumerate() {
        if r.bound.is_floor_capped() {
            match active_start {
                None => {
                    active_start = Some(idx);
                    prev_idx = idx;
                }
                Some(start) => {
                    if (r.hour_offset - readings[prev_idx].hour_offset).abs() > GAP_BREAK_HOURS {
                        reps.push(start + (prev_idx - start) / 2);
                        active_start = Some(idx);
                    }
                    prev_idx = idx;
                }
            }
        } else if let Some(start) = active_start.take() {
            reps.push(start + (prev_idx - start) / 2);
        }
    }
    if let Some(start) = active_start {
        reps.push(start + (prev_idx - start) / 2);
    }

    if reps.len() > max_markers {
        let step = reps.len() as f64 / max_markers as f64;
        reps = (0..max_markers)
            .map(|i| reps[((i as f64 * step).floor() as usize).min(reps.len() - 1)])
            .collect();
    }

    reps.into_iter().map(|i| readings[i].clone()).collect()
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
    // Lower floor to 45 mg/dL so Lingo floor-capped 54 mg/dL (<55) readings sit visibly above the grid.
    let min_g = 45.0_f64;
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

    // 2. Translucent emerald 70–140 mg/dL target planes + wireframe rails + subtle 180 mg/dL clinical rail
    let y70 = y_for_g(70.0);
    let y140 = y_for_g(140.0);
    let y180 = y_for_g(180.0);
    let rail_col = [0.20, 0.83, 0.60, 0.55];
    for y in [y70, y140] {
        push_vertex(&mut scene.lines, [-1.55, y, -0.14], rail_col);
        push_vertex(&mut scene.lines, [1.55, y, -0.14], rail_col);
        push_vertex(&mut scene.lines, [-1.55, y, 0.14], rail_col);
        push_vertex(&mut scene.lines, [1.55, y, 0.14], rail_col);
    }
    let hyper_rail_col = [0.98, 0.75, 0.14, 0.35];
    push_vertex(&mut scene.lines, [-1.55, y180, -0.14], hyper_rail_col);
    push_vertex(&mut scene.lines, [1.55, y180, -0.14], hyper_rail_col);
    push_quad(
        &mut scene.translucent_triangles,
        [-1.55, y70, -0.14],
        [1.55, y70, -0.14],
        [1.55, y140, -0.14],
        [-1.55, y140, -0.14],
        [0.20, 0.83, 0.60, 0.14],
    );

    // 3. Gap-aware 3D Glucose ribbons + curtains (each contiguous segment rendered independently
    //    so multi-hour / multi-day sensor-off gaps NEVER draw phantom bridges or curtains).
    let segments = downsample_segments_for_chart(readings, 320, DEFAULT_CHART_GAP_HOURS);
    let spike_peaks = select_notable_spike_peaks(readings, 36);
    let floor_caps = select_notable_floor_caps(readings, 24);
    let z_front = 0.08_f32;
    let z_back = -0.08_f32;

    for segment in &segments {
        if segment.len() == 1 {
            let r = &segment[0];
            let x = x_for(r.hour_offset);
            let y = y_for_g(r.glucose_mg_dl);
            push_octahedron(
                &mut scene.opaque_triangles,
                [x, y, 0.0],
                0.04,
                point_color(r),
            );
            continue;
        }
        for pair in segment.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            let x0 = x_for(a.hour_offset);
            let y0 = y_for_g(a.glucose_mg_dl);
            let x1 = x_for(b.hour_offset);
            let y1 = y_for_g(b.glucose_mg_dl);

            let (top_col, curtain_col, ridge_col) = pair_ribbon_colors(a, b);

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
            push_vertex(&mut scene.lines, [x0, y0 + 0.008, 0.0], ridge_col);
            push_vertex(&mut scene.lines, [x1, y1 + 0.008, 0.0], ridge_col);
        }
    }

    for r in &spike_peaks {
        let x = x_for(r.hour_offset);
        let y = y_for_g(r.glucose_mg_dl);
        let marker_col = if r.bound.is_ceiling_capped() {
            COLOR_CEILING_CAP
        } else {
            COLOR_SPIKE
        };
        push_octahedron(
            &mut scene.opaque_triangles,
            [x, y + 0.05, 0.0],
            0.055,
            marker_col,
        );
    }

    for r in &floor_caps {
        let x = x_for(r.hour_offset);
        let y = y_for_g(r.glucose_mg_dl);
        push_octahedron(
            &mut scene.opaque_triangles,
            [x, y + 0.045, 0.0],
            0.055,
            COLOR_FLOOR_CAP,
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

    // Memoize geometry construction strictly on data changes so camera orbit/zoom NEVER
    // clones readings or rebuilds the 3D scene on `mousemove`.
    let scene_memo = Memo::new(move |_| {
        readings.with(|r| checkins.with(|c| Arc::new(build_lingo_3d_scene(r, c))))
    });
    let span_label =
        Memo::new(move |_| readings.with(|r| checkins.with(|c| format_trace_span_label(r, c))));

    #[cfg(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr")))]
    {
        let renderer_cache = StoredValue::new_local(std::cell::RefCell::new(
            Option::<CachedWebGl2Renderer>::None,
        ));
        Effect::new(move |_| {
            let scene = scene_memo.get();
            let y = yaw.get();
            let p = pitch.get();
            let dist = distance.get();
            if let Some(canvas) = canvas_ref.get() {
                renderer_cache.with_value(|cell| {
                    let mut slot = cell.borrow_mut();
                    let init_res = if slot.is_none() {
                        match CachedWebGl2Renderer::new(&canvas) {
                            Ok(r) => {
                                *slot = Some(r);
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        Ok(())
                    };
                    match init_res {
                        Ok(()) => {
                            if let Some(renderer) = slot.as_mut() {
                                renderer.upload_if_needed(&scene);
                                renderer.draw(&canvas, y, p, dist);
                            }
                            gl_error.set(None);
                        }
                        Err(err) => gl_error.set(Some(err)),
                    }
                });
            }
        });
    }

    #[cfg(not(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr"))))]
    let _ = (&scene_memo, &gl_error);

    let on_mousedown = move |ev: leptos::ev::MouseEvent| {
        dragging.set(true);
        last_xy.set((ev.client_x(), ev.client_y()));
    };
    let on_mouseup = move |_: leptos::ev::MouseEvent| {
        dragging.set(false);
    };
    let on_mousemove = move |ev: leptos::ev::MouseEvent| {
        if dragging.get_untracked() {
            let (lx, ly) = last_xy.get_untracked();
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
                {move || format!(
                    "X-axis: {} timeline (gaps >2h broken cleanly) · Y-axis: glucose (mg/dL) with emerald 70–140 target plane, amber spikes, orange-red ceiling cap & crimson floor cap · Z-axis depth: Mood lane (front, emerald) & Anxiety lane (back, rose).",
                    span_label.get()
                )}
            </p>
        </div>
    }
}

#[cfg(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr")))]
struct CachedWebGl2Renderer {
    gl: glow::Context,
    program: glow::Program,
    mvp_loc: Option<glow::UniformLocation>,
    opaque_vao: glow::VertexArray,
    opaque_vbo: glow::Buffer,
    opaque_verts: i32,
    lines_vao: glow::VertexArray,
    lines_vbo: glow::Buffer,
    lines_verts: i32,
    translucent_vao: glow::VertexArray,
    translucent_vbo: glow::Buffer,
    translucent_verts: i32,
    uploaded_scene_ptr: usize,
}

#[cfg(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr")))]
impl CachedWebGl2Renderer {
    fn new(canvas: &web_sys::HtmlCanvasElement) -> Result<Self, String> {
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

        // SAFETY: All WebGL2 objects are created on a valid context bound to `canvas`
        // and cleaned up either on error or in `Drop`.
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

            let mvp_loc = gl.get_uniform_location(program, "u_mvp");
            let init_pair =
                |gl: &glow::Context| -> Result<(glow::VertexArray, glow::Buffer), String> {
                    let vao = gl
                        .create_vertex_array()
                        .map_err(|e| format!("create_vao: {e}"))?;
                    let vbo = gl.create_buffer().map_err(|e| {
                        gl.delete_vertex_array(vao);
                        format!("create_vbo: {e}")
                    })?;
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
                    gl.bind_vertex_array(None);
                    Ok((vao, vbo))
                };

            let (opaque_vao, opaque_vbo) = match init_pair(&gl) {
                Ok(pair) => pair,
                Err(e) => {
                    gl.delete_program(program);
                    return Err(e);
                }
            };
            let (lines_vao, lines_vbo) = match init_pair(&gl) {
                Ok(pair) => pair,
                Err(e) => {
                    gl.delete_buffer(opaque_vbo);
                    gl.delete_vertex_array(opaque_vao);
                    gl.delete_program(program);
                    return Err(e);
                }
            };
            let (translucent_vao, translucent_vbo) = match init_pair(&gl) {
                Ok(pair) => pair,
                Err(e) => {
                    gl.delete_buffer(lines_vbo);
                    gl.delete_vertex_array(lines_vao);
                    gl.delete_buffer(opaque_vbo);
                    gl.delete_vertex_array(opaque_vao);
                    gl.delete_program(program);
                    return Err(e);
                }
            };

            Ok(Self {
                gl,
                program,
                mvp_loc,
                opaque_vao,
                opaque_vbo,
                opaque_verts: 0,
                lines_vao,
                lines_vbo,
                lines_verts: 0,
                translucent_vao,
                translucent_vbo,
                translucent_verts: 0,
                uploaded_scene_ptr: 0,
            })
        }
    }

    fn upload_if_needed(&mut self, scene: &Arc<Chart3DScene>) {
        use glow::HasContext;
        let ptr = Arc::as_ptr(scene) as usize;
        if self.uploaded_scene_ptr == ptr {
            return;
        }
        // SAFETY: Buffers belong to `self.gl` and slices are tightly packed `f32` arrays.
        unsafe {
            self.gl
                .bind_buffer(glow::ARRAY_BUFFER, Some(self.opaque_vbo));
            self.gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                floats_as_bytes(&scene.opaque_triangles),
                glow::STATIC_DRAW,
            );
            self.opaque_verts = (scene.opaque_triangles.len() / 7) as i32;

            self.gl
                .bind_buffer(glow::ARRAY_BUFFER, Some(self.lines_vbo));
            self.gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                floats_as_bytes(&scene.lines),
                glow::STATIC_DRAW,
            );
            self.lines_verts = (scene.lines.len() / 7) as i32;

            self.gl
                .bind_buffer(glow::ARRAY_BUFFER, Some(self.translucent_vbo));
            self.gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                floats_as_bytes(&scene.translucent_triangles),
                glow::STATIC_DRAW,
            );
            self.translucent_verts = (scene.translucent_triangles.len() / 7) as i32;
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        self.uploaded_scene_ptr = ptr;
    }

    fn draw(&self, canvas: &web_sys::HtmlCanvasElement, yaw: f32, pitch: f32, distance: f32) {
        use glam::{Mat4, Vec3};
        use glow::HasContext;

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

        // SAFETY: Program, VAOs, and vertex counts were initialized on `self.gl`.
        unsafe {
            self.gl.viewport(0, 0, width, height);
            self.gl.clear_color(0.008, 0.024, 0.090, 1.0);
            self.gl.enable(glow::DEPTH_TEST);
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            self.gl
                .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);

            self.gl.use_program(Some(self.program));
            if let Some(loc) = self.mvp_loc.as_ref() {
                self.gl.uniform_matrix_4_f32_slice(Some(loc), false, &mvp);
            }

            // 1. Opaque ribbons & octahedra
            self.gl.depth_mask(true);
            if self.opaque_verts > 0 {
                self.gl.bind_vertex_array(Some(self.opaque_vao));
                self.gl.draw_arrays(glow::TRIANGLES, 0, self.opaque_verts);
            }
            // 2. Floor grid, rails, stems, and trajectories
            if self.lines_verts > 0 {
                self.gl.bind_vertex_array(Some(self.lines_vao));
                self.gl.draw_arrays(glow::LINES, 0, self.lines_verts);
            }
            // 3. Translucent target plane & glucose curtains
            if self.translucent_verts > 0 {
                self.gl.depth_mask(false);
                self.gl.bind_vertex_array(Some(self.translucent_vao));
                self.gl
                    .draw_arrays(glow::TRIANGLES, 0, self.translucent_verts);
                self.gl.depth_mask(true);
            }
            self.gl.bind_vertex_array(None);
        }
    }
}

#[cfg(all(target_arch = "wasm32", any(feature = "hydrate", feature = "csr")))]
impl Drop for CachedWebGl2Renderer {
    fn drop(&mut self) {
        use glow::HasContext;
        // SAFETY: Releases all GPU handles owned by this renderer once on component unmount.
        unsafe {
            self.gl.delete_buffer(self.opaque_vbo);
            self.gl.delete_vertex_array(self.opaque_vao);
            self.gl.delete_buffer(self.lines_vbo);
            self.gl.delete_vertex_array(self.lines_vao);
            self.gl.delete_buffer(self.translucent_vbo);
            self.gl.delete_vertex_array(self.translucent_vao);
            self.gl.delete_program(self.program);
        }
    }
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
    use crate::mental_health_lingo::{
        parse_lingo_csv, sample_checkins, GlucoseReadingBound, SAMPLE_LINGO_CSV,
    };
    use leptos::prelude::RenderHtml;

    fn synthetic_reading(
        hour_offset: f64,
        glucose_mg_dl: f64,
        bound: GlucoseReadingBound,
    ) -> LingoReading {
        LingoReading {
            timestamp: format!("2026-07-01T{:02}:00-07:00", (hour_offset as u32) % 24),
            hour_offset,
            glucose_mg_dl,
            lingo_count: None,
            tag: None,
            bound,
        }
    }

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
    fn breaks_3d_ribbons_and_curtains_across_sensor_gaps() {
        // Two segments separated by a 12-hour sensor-off gap (0..=4h and 16..=20h).
        let readings = vec![
            synthetic_reading(0.0, 95.0, GlucoseReadingBound::Measured),
            synthetic_reading(2.0, 105.0, GlucoseReadingBound::Measured),
            synthetic_reading(4.0, 110.0, GlucoseReadingBound::Measured),
            synthetic_reading(16.0, 120.0, GlucoseReadingBound::Measured),
            synthetic_reading(18.0, 115.0, GlucoseReadingBound::Measured),
            synthetic_reading(20.0, 100.0, GlucoseReadingBound::Measured),
        ];
        let scene = build_lingo_3d_scene(&readings, &[]);
        // 4 ribbon pairs total (2 per segment) -> 4 quads = 8 triangles = 24 vertices = 168 floats.
        // If the 12h gap had been bridged, there would be 5 quads (30 vertices = 210 floats).
        assert_eq!(scene.opaque_triangles.len(), 4 * 6 * 7);
        // Translucent: 1 target plane quad + 4 curtain quads = 5 quads.
        assert_eq!(scene.translucent_triangles.len(), 5 * 6 * 7);
    }

    #[test]
    fn renders_floor_and_ceiling_saturation_distinctly_above_floor() {
        let readings = vec![
            synthetic_reading(0.0, 54.0, GlucoseReadingBound::BelowSensorFloor),
            synthetic_reading(1.0, 54.0, GlucoseReadingBound::BelowSensorFloor),
            synthetic_reading(2.0, 100.0, GlucoseReadingBound::Measured),
            synthetic_reading(3.0, 201.0, GlucoseReadingBound::AboveSensorCeiling),
            synthetic_reading(4.0, 201.0, GlucoseReadingBound::AboveSensorCeiling),
        ];
        let scene = build_lingo_3d_scene(&readings, &[]);

        let has_floor_cap_color = scene.opaque_triangles.chunks_exact(7).any(|v| {
            (v[3] - COLOR_FLOOR_CAP[0]).abs() < 1e-4
                && (v[4] - COLOR_FLOOR_CAP[1]).abs() < 1e-4
                && (v[5] - COLOR_FLOOR_CAP[2]).abs() < 1e-4
                && v[1] > 0.05 // visibly above the 0.0 chart floor because min_g = 45.0
        });
        let has_ceiling_cap_color = scene.opaque_triangles.chunks_exact(7).any(|v| {
            (v[3] - COLOR_CEILING_CAP[0]).abs() < 1e-4
                && (v[4] - COLOR_CEILING_CAP[1]).abs() < 1e-4
                && (v[5] - COLOR_CEILING_CAP[2]).abs() < 1e-4
        });
        assert!(
            has_floor_cap_color,
            "expected crimson floor-cap geometry above y=0"
        );
        assert!(
            has_ceiling_cap_color,
            "expected orange-red ceiling-cap geometry"
        );
    }

    #[test]
    fn formats_dynamic_trace_span_labels_and_renders_3d_canvas_shell_on_ssr() {
        let d14 = vec![
            synthetic_reading(0.0, 100.0, GlucoseReadingBound::Measured),
            synthetic_reading(336.0, 105.0, GlucoseReadingBound::Measured),
        ];
        assert_eq!(format_trace_span_label(&d14, &[]), "14d");

        let long = vec![
            synthetic_reading(0.0, 100.0, GlucoseReadingBound::Measured),
            synthetic_reading(2704.25, 105.0, GlucoseReadingBound::Measured),
        ];
        assert_eq!(format_trace_span_label(&long, &[]), "112.7d");

        let readings = RwSignal::new(d14);
        let checkins = RwSignal::new(sample_checkins());
        let html = view! {
            <LingoMood3DChart readings=readings.into() checkins=checkins.into() />
        }
        .to_html();
        assert!(html.contains("<canvas"));
        assert!(html.contains("Pure-Rust WebGL2 (glow + glam)"));
        assert!(html.contains("Reset 3D"));
        assert!(html.contains("X-axis: 14d timeline"));
    }
}
