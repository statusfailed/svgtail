# svgtail

A minimal SVG viewer in Rust. Usage:

    svgtail <path>

This will:

- Watch `<path>` for changes (and will wait until `<path>` is created if it doesn't exist)
- Fit the SVG to window on window resize or file update

**Install**:

    cargo install svgtail

**Key bindings**

- Pan using `hjkl` (vim-style)
- Zoom in/out using `+` / `-`
- Reset with `r` (fits image to window)

# Why not `feh`?

I previously used `feh`, but renders SVGs at a fixed resolution so zooming in images is blurry.
