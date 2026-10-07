# PhotoCraft Studio

An independent, clean-room raster graphics editor built in pure Rust, powered by the sovereign **Martensite** GPU-accelerated retained-mode GUI engine.

![PhotoCraft Studio on Martensite](brag/demo.gif)

## Architecture

PhotoCraft is built on a clean-room modular Rust architecture:

- **`crates/ui-martensite`**: Sovereign retained-mode interface built on Martensite's 64-byte `HotNode` generational arena, reactive signals, and Vello compute rasterization.
- **`crates/engine`**: Core raster compositing, history state management, and command dispatch pipeline.
- **`crates/psd`**: Standalone, clean-room reader and writer for PSD/PSB files built from public specifications.
- **`crates/cms`**: Color management system implementing ICC v4.3 and black point compensation.
- **`crates/codecs`**: Lossless and lossy raster image codecs (PNG, JPEG, TIFF, WebP, AVIF, BMP).
- **`crates/paint`**: High-performance brush engine with dynamics, scattering, and stylus pressure.

## Legal & Compliance Notice

PhotoCraft is an independent open-source raster graphics editor. It is not sponsored, endorsed, or affiliated with Adobe Inc. Adobe, Photoshop, and Creative Cloud are trademarks or registered trademarks of Adobe Inc. All workflows, keybindings, and functional controls operate in strict compliance with 17 U.S.C. § 102(b), *Lotus v. Borland*, and *Apple v. Microsoft*.

## License

Dual-licensed under MIT OR Apache-2.0.
