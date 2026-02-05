# SVG View

A minimal SVG viewer in Rust. Usage:

    svgview <path-to-svg>

- Watches file for changes (& able to open a path that doesn't yet exist, and wait for a file)
- Fits SVG to window on resize / file update (& keeps updated)
- pan using WASD, zoom using +/-
