# SVGCat

A minimal SVG viewer in Rust. Usage:

    svgcat <path-to-svg>

- Watches file for changes (& able to open a path that doesn't yet exist, and wait for a file)
- Fits SVG to window on resize / file update (& keeps updated)

**Key bindings**

- Pan using `hjkl` (vim-style)
- Zoom in/out using `+ / -`
- Fit-to-window using `r`
