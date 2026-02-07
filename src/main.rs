use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use minifb::{Key, Window, WindowOptions};
use notify_debouncer_full::{
    DebounceEventResult, new_debouncer,
    notify::{
        RecursiveMode,
        event::{AccessKind, AccessMode, EventKind},
    },
};
use resvg::{tiny_skia, usvg};

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

    let transform =
        tiny_skia::Transform::from_translate(offset_x, offset_y).pre_scale(eff_scale, eff_scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());

    pixmap
        .data()
        .chunks_exact(4)
        .map(|px| {
            let (r, g, b, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
            if a == 0 {
                0x00333333
            } else {
                let r = (r * 255 / a).min(255);
                let g = (g * 255 / a).min(255);
                let b = (b * 255 / a).min(255);
                (r << 16) | (g << 8) | b
            }
        })
        .collect()
}

fn should_reload(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Open(AccessMode::Any)) => false,
        _ => true,
    }
}

fn wait_for_creation(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Ok(());
    }
    eprintln!(
        "{} does not exist, waiting for it to be created...",
        path.display()
    );

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(200), None, move |res| {
        let _ = tx.send(res);
    })?;

    debouncer.watch(&parent, RecursiveMode::NonRecursive)?;

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                if events.iter().any(|e| e.paths.iter().any(|p| p == path)) && path.exists() {
                    return Ok(());
                }
            }
            Ok(Err(_)) => {
                if path.exists() {
                    return Ok(());
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: svgtail <file.svg>");
        std::process::exit(1);
    }

    let svg_path = std::path::absolute(&args[1])?;
    wait_for_creation(&svg_path)?;

    let mut svg_opts = usvg::Options::default();
    svg_opts.fontdb_mut().load_system_fonts();

    let mut tree = load_svg(&svg_path, &svg_opts);

    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let _debouncer = {
        let tx = tx.clone();
        let mut debouncer = new_debouncer(Duration::from_millis(200), None, move |res| {
            let _ = tx.send(res);
        })?;
        debouncer.watch(&svg_path, RecursiveMode::NonRecursive)?;
        debouncer
    };

    let mut width: usize = 800;
    let mut height: usize = 600;

    let mut window = Window::new(
        "svgtail",
        width,
        height,
        WindowOptions {
            resize: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    let mut pan = (0.0f32, 0.0f32);
    let mut zoom = 1.0f32;
    let mut auto_fit = true;
    let mut fit_scale = 1.0f32;

    let mut dirty = true;
    let mut buffer: Vec<u32> = vec![0; width * height];

    // Needed for i3/X11: repaint once when window becomes active again.
    let mut was_active = window.is_active();

    // How often to poll window/input while idle (no redraw needed).
    // Not a "sleep every frame": we block on the watcher channel for up to this.
    let poll_active = Duration::from_millis(16); // ~60 Hz responsiveness
    let poll_inactive = Duration::from_millis(100);

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let active = window.is_active();
        if active && !was_active {
            dirty = true;
        }
        was_active = active;

        // 1) Drain watcher queue; reload at most once per iteration.
        let mut reload = false;
        while let Ok(res) = rx.try_recv() {
            match res {
                Ok(events) => {
                    for e in events {
                        if e.paths.iter().any(|p| p == &svg_path) && should_reload(&e.kind) {
                            reload = true;
                        }
                    }
                }
                Err(_) => reload = true,
            }
        }
        if reload {
            if let Some(new_tree) = load_svg(&svg_path, &svg_opts) {
                tree = Some(new_tree);
                pan = (0.0, 0.0);
                zoom = 1.0;
                auto_fit = true;
                dirty = true;
            }
        }

        // 2) Resize
        let (new_w, new_h) = window.get_size();
        if new_w != width || new_h != height {
            width = new_w.max(1);
            height = new_h.max(1);
            buffer.resize(width * height, 0);
            dirty = true;
        }

        // 3) Fit scale only when needed
        if dirty && auto_fit {
            if let Some(ref t) = tree {
                let svg_size = t.size();
                fit_scale =
                    (width as f32 / svg_size.width()).min(height as f32 / svg_size.height());
            }
        }

        // 4) Input
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

        // 5) Present if dirty, else block waiting for file events (with timeout to poll window).
        if dirty {
            if let Some(ref t) = tree {
                buffer = render(t, width as u32, height as u32, pan, zoom, fit_scale);
            } else {
                buffer.fill(0x00333333);
            }
            dirty = false;

            window
                .update_with_buffer(&buffer, width, height)
                .map_err(|e| format!("{e:?}"))?;
        } else {
            // Pump window events once (non-blocking)
            window.update();

            // Block until something happens (file change) or timeout to poll input again.
            let timeout = if window.is_active() {
                poll_active
            } else {
                poll_inactive
            };
            match rx.recv_timeout(timeout) {
                Ok(res) => {
                    // We got at least one fs event; push it back into handling by marking reload.
                    // Also drain any immediately-available events next loop iteration.
                    match res {
                        Ok(events) => {
                            if events.iter().any(|e| {
                                e.paths.iter().any(|p| p == &svg_path) && should_reload(&e.kind)
                            }) {
                                // reload next iteration
                                // (we could reload here, but keeping logic centralized above is simpler)
                                // mark dirty so if reload fails (parse error), we still repaint background.
                                dirty = true;
                            }
                        }
                        Err(_) => {
                            dirty = true;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }
        }
    }

    Ok(())
}
