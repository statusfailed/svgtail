use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use minifb::{Key, Window, WindowOptions};
use notify_debouncer_mini::notify::RecursiveMode;
use resvg::tiny_skia;
use resvg::usvg;

fn load_svg(path: &PathBuf, opts: &usvg::Options) -> Option<usvg::Tree> {
    let data = fs::read(path).ok()?;
    usvg::Tree::from_data(&data, opts).ok()
}

fn render(
    tree: &usvg::Tree,
    width: u32,
    height: u32,
    pan: (f32, f32),
    zoom: f32,
    fit_scale: f32,
) -> Vec<u32> {
    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
    pixmap.fill(tiny_skia::Color::from_rgba8(0x33, 0x33, 0x33, 0xFF));

    let svg_size = tree.size();
    let eff_scale = fit_scale * zoom;
    let offset_x = (width as f32 - svg_size.width() * eff_scale) / 2.0 + pan.0;
    let offset_y = (height as f32 - svg_size.height() * eff_scale) / 2.0 + pan.1;

    let transform = tiny_skia::Transform::from_translate(offset_x, offset_y)
        .pre_scale(eff_scale, eff_scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());

    // Convert premultiplied RGBA bytes to 0x00RRGGBB u32 for minifb
    pixmap
        .data()
        .chunks_exact(4)
        .map(|px| {
            let (r, g, b, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
            if a == 0 {
                0x00333333 // background for fully transparent pixels
            } else {
                // Un-premultiply
                let r = (r * 255 / a).min(255);
                let g = (g * 255 / a).min(255);
                let b = (b * 255 / a).min(255);
                (r << 16) | (g << 8) | b
            }
        })
        .collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: svg-view <file.svg>");
        std::process::exit(1);
    }

    let svg_path = fs::canonicalize(&args[1]).unwrap_or_else(|_| PathBuf::from(&args[1]));

    let mut svg_opts = usvg::Options::default();
    svg_opts.fontdb_mut().load_system_fonts();

    let mut tree = load_svg(&svg_path, &svg_opts);

    // Set up file watcher on parent directory
    let (tx, rx) = mpsc::channel();
    let _debouncer = {
        let parent = svg_path.parent().unwrap_or(&svg_path);
        let mut debouncer =
            notify_debouncer_mini::new_debouncer(Duration::from_millis(200), tx).unwrap();
        debouncer
            .watcher()
            .watch(parent, RecursiveMode::NonRecursive)
            .unwrap();
        debouncer
    };

    let mut width: usize = 800;
    let mut height: usize = 600;

    let mut window = Window::new(
        "SVG View",
        width,
        height,
        WindowOptions {
            resize: true,
            ..Default::default()
        },
    )
    .expect("Failed to create window");

    window.set_target_fps(60);

    let mut pan = (0.0f32, 0.0f32);
    let mut zoom = 1.0f32;
    let mut auto_fit = true;
    let mut fit_scale = 1.0f32;
    let mut dirty = true;
    let mut buffer: Vec<u32> = vec![0; width * height];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // 1. Poll file watcher
        if let Ok(Ok(events)) = rx.try_recv() {
            let filename = svg_path.file_name();
            let changed = events
                .iter()
                .any(|e| e.path.file_name() == filename);
            if changed {
                if let Some(new_tree) = load_svg(&svg_path, &svg_opts) {
                    tree = Some(new_tree);
                    pan = (0.0, 0.0);
                    zoom = 1.0;
                    auto_fit = true;
                    dirty = true;
                }
            }
        }

        // 2. Check window resize
        let (new_w, new_h) = window.get_size();
        if new_w != width || new_h != height {
            width = new_w.max(1);
            height = new_h.max(1);
            buffer.resize(width * height, 0);
            dirty = true;
        }

        // 3. Recompute fit_scale when auto-fitting
        if auto_fit {
            if let Some(ref t) = tree {
                let svg_size = t.size();
                fit_scale = (width as f32 / svg_size.width())
                    .min(height as f32 / svg_size.height());
            }
        }

        // 4. Handle pan/zoom input
        let pan_speed = 10.0;
        if window.is_key_down(Key::K) {
            pan.1 += pan_speed;
            auto_fit = false;
            dirty = true;
        }
        if window.is_key_down(Key::J) {
            pan.1 -= pan_speed;
            auto_fit = false;
            dirty = true;
        }
        if window.is_key_down(Key::H) {
            pan.0 += pan_speed;
            auto_fit = false;
            dirty = true;
        }
        if window.is_key_down(Key::L) {
            pan.0 -= pan_speed;
            auto_fit = false;
            dirty = true;
        }
        if window.is_key_down(Key::Equal) || window.is_key_down(Key::NumPadPlus) {
            zoom *= 1.1;
            auto_fit = false;
            dirty = true;
        }
        if window.is_key_down(Key::Minus) || window.is_key_down(Key::NumPadMinus) {
            zoom /= 1.1;
            auto_fit = false;
            dirty = true;
        }
        if window.is_key_down(Key::R) {
            pan = (0.0, 0.0);
            zoom = 1.0;
            auto_fit = true;
            dirty = true;
        }

        // 5. Re-render if dirty
        if dirty {
            if let Some(ref t) = tree {
                buffer = render(t, width as u32, height as u32, pan, zoom, fit_scale);
            } else {
                buffer.fill(0x00333333);
            }
            dirty = false;
        }

        // 6. Update window
        window.update_with_buffer(&buffer, width, height).unwrap();
    }
}
