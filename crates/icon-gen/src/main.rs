//! Generates the 1024x1024 source app icon or Google Play Store graphic assets
//! (512x512 hi-res PNG icon + 1024x500 feature graphic composited from
//! `src-tauri/icons/source.png`).
//!
//! Usage:
//! - `cargo run -p icon-gen [-- OUTPUT_PATH]` (default `src-tauri/icons/source.png`)
//! - `cargo run -p icon-gen -- --play-assets OUTPUT_DIR`

use std::path::Path;

use image::imageops::{overlay, FilterType};
use image::{Rgb, RgbImage, Rgba, RgbaImage};

const SIZE: u32 = 1024;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Shortest distance from point `(px, py)` to segment `a`-`b`.
fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    if dx == 0.0 && dy == 0.0 {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

fn generate_play_assets(out_dir: &Path) {
    std::fs::create_dir_all(out_dir).expect("failed to create Play assets directory");
    let source_path = Path::new("src-tauri/icons/source.png");
    let source = image::open(source_path)
        .unwrap_or_else(|err| panic!("failed to open {}: {err}", source_path.display()));

    // 1. 512x512 Google Play Store hi-res icon (32-bit PNG).
    let icon_512 = source.resize_exact(512, 512, FilterType::Lanczos3);
    let icon_out = out_dir.join("icon-512.png");
    icon_512
        .save(&icon_out)
        .expect("failed to write 512x512 Play Store icon");
    println!("wrote {} (512x512)", icon_out.display());

    // 2. 1024x500 Google Play Store feature graphic (dark slate-950 canvas with
    // emerald/cyan ambient glow + Dabney House elephant badge + clean UI preview cards).
    let (w, h) = (1024u32, 500u32);
    let mut fg = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32, y as f32);
            // Subtle radial emerald glow behind the elephant badge (left) and cyan glow (right).
            let d_em = ((fx - 260.0) / 360.0).hypot((fy - 250.0) / 300.0);
            let glow_em = (1.0 - d_em).clamp(0.0, 1.0).powi(2);
            let d_cy = ((fx - 790.0) / 420.0).hypot((fy - 140.0) / 280.0);
            let glow_cy = (1.0 - d_cy).clamp(0.0, 1.0).powi(2);

            let r = (2.0 + glow_em * 18.0 + glow_cy * 10.0).min(255.0) as u8;
            let g = (6.0 + glow_em * 78.0 + glow_cy * 64.0).min(255.0) as u8;
            let b = (23.0 + glow_em * 52.0 + glow_cy * 92.0).min(255.0) as u8;
            fg.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    // Subtle geometric accent cards on the right representing the 3 zero-PHI clinical tabs.
    let cards: [(u32, u32, u32, u32, [u8; 3]); 3] = [
        (510, 96, 430, 88, [16, 185, 129]),   // Emerald: Patient Demo
        (510, 206, 430, 88, [34, 211, 238]),  // Cyan: Physician Demo
        (510, 316, 430, 88, [167, 139, 250]), // Violet: Mood & CGM
    ];
    for (cx, cy, cw, ch, accent) in cards {
        for y in cy..(cy + ch) {
            for x in cx..(cx + cw) {
                let border = x < cx + 2 || x >= cx + cw - 2 || y < cy + 2 || y >= cy + ch - 2;
                let left_bar = x < cx + 10;
                let px = if left_bar {
                    Rgba([accent[0], accent[1], accent[2], 230])
                } else if border {
                    Rgba([accent[0] / 3, accent[1] / 3, accent[2] / 3, 255])
                } else {
                    Rgba([15, 23, 42, 235])
                };
                fg.put_pixel(x, y, px);
            }
        }
        // Minimal clean schematic lines inside each card.
        for y in (cy + 24)..(cy + 36) {
            for x in (cx + 28)..(cx + 220) {
                fg.put_pixel(x, y, Rgba([226, 232, 240, 230]));
            }
        }
        for y in (cy + 50)..(cy + 60) {
            for x in (cx + 28)..(cx + 350) {
                fg.put_pixel(x, y, Rgba([100, 116, 139, 210]));
            }
        }
    }

    // Composite the 300x300 Dabney House crest badge with rounded corners & subtle emerald border.
    let mut badge = source
        .resize_exact(300, 300, FilterType::Lanczos3)
        .to_rgba8();
    let (bw, bh) = (300.0_f32, 300.0_f32);
    let corner_r = 56.0_f32;
    for y in 0..300u32 {
        for x in 0..300u32 {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = if fx < corner_r {
                corner_r - fx
            } else if fx > bw - corner_r {
                fx - (bw - corner_r)
            } else {
                0.0
            };
            let dy = if fy < corner_r {
                corner_r - fy
            } else if fy > bh - corner_r {
                fy - (bh - corner_r)
            } else {
                0.0
            };
            let dist = dx.hypot(dy);
            if dist > corner_r {
                badge.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            } else if dist > corner_r - 3.0 {
                badge.put_pixel(x, y, Rgba([52, 211, 153, 180]));
            }
        }
    }
    overlay(&mut fg, &badge, 110, 100);

    let fg_out = out_dir.join("feature-graphic-1024x500.png");
    fg.save(&fg_out)
        .expect("failed to write 1024x500 Play Store feature graphic");
    println!("wrote {} (1024x500)", fg_out.display());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--play-assets") {
        let dir = args
            .get(1)
            .map(String::as_str)
            .unwrap_or("target/android-release/play-store-assets");
        generate_play_assets(Path::new(dir));
        return;
    }

    let out = args
        .first()
        .cloned()
        .unwrap_or_else(|| "src-tauri/icons/source.png".to_string());

    let bg = [2u8, 6, 23]; // slate-950
    let g_top = [34.0, 211.0, 238.0]; // cyan-400
    let g_bot = [37.0, 99.0, 235.0]; // blue-600
    let glyph = [255u8, 255, 255];

    let (lo, hi) = (96.0, SIZE as f32 - 96.0);
    let radius = 180.0;
    let c = SIZE as f32 / 2.0;
    let thickness = 46.0;
    // A ">" chevron (two segments) plus an underscore bar.
    let chev = [
        (c - 150.0, c - 170.0, c + 70.0, c),
        (c + 70.0, c, c - 150.0, c + 170.0),
    ];
    let bar = (c - 150.0, c + 240.0, c + 190.0, c + 240.0);

    let mut img = RgbImage::from_pixel(SIZE, SIZE, Rgb(bg));

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32, y as f32);
            if fx < lo || fx > hi || fy < lo || fy > hi {
                continue;
            }
            // Rounded corners.
            if (fx < lo + radius || fx > hi - radius) && (fy < lo + radius || fy > hi - radius) {
                let corx = if fx < lo + radius {
                    lo + radius
                } else {
                    hi - radius
                };
                let cory = if fy < lo + radius {
                    lo + radius
                } else {
                    hi - radius
                };
                if (fx - corx).hypot(fy - cory) > radius {
                    continue;
                }
            }

            let t = (fy - lo) / (hi - lo);
            let mut px = [
                lerp(g_top[0], g_bot[0], t) as u8,
                lerp(g_top[1], g_bot[1], t) as u8,
                lerp(g_top[2], g_bot[2], t) as u8,
            ];

            let d = dist_to_segment(fx, fy, chev[0].0, chev[0].1, chev[0].2, chev[0].3)
                .min(dist_to_segment(
                    fx, fy, chev[1].0, chev[1].1, chev[1].2, chev[1].3,
                ))
                .min(dist_to_segment(fx, fy, bar.0, bar.1, bar.2, bar.3));
            if d <= thickness / 2.0 {
                px = glyph;
            }

            img.put_pixel(x, y, Rgb(px));
        }
    }

    img.save(&out).expect("failed to write icon PNG");
    println!("wrote {out} ({SIZE}x{SIZE})");
}
