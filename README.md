# oneko in rust

A rewrite of the classic [**oneko**](https://github.com/tie/oneko) desktop cat, in **Rust**, for **Arch Linux + Hyprland**.

A little pixel-art cat chases your cursor around the screen. When you stop moving the mouse it sits down, washes itself, and eventually falls asleep — just like the 1990s X11 original, but running natively on Wayland.

![demo](demo.gif)

## Why a rewrite?

The original oneko (and most clones) rely on X11 tricks — override-redirect windows and the SHAPE extension — that don't work under Wayland compositors like Hyprland. This version uses:

- **`wlr-layer-shell`** (via [smithay-client-toolkit](https://crates.io/crates/smithay-client-toolkit)) for an always-on-top overlay surface
- **ARGB transparency** instead of the X11 SHAPE extension
- **`hyprctl cursorpos`** to track the cursor globally
- An **empty input region**, so the cat never blocks your clicks or steals focus

The original 32×32 XBM sprites are embedded directly in the binary — no asset files needed.

## Requirements

- Arch Linux (or any Linux distro, really)
- [Hyprland](https://hypr.land) — the cursor tracking uses `hyprctl`; any other wlroots-based compositor would need a different cursor source
- Rust toolchain (`rustup` or `pacman -S rust`)

## Build & run

```sh
cargo build --release
./target/release/oneko-rust
```

## Install

Run the install script to build the release binary, copy it to `~/.local/bin`, and optionally add a Hyprland autostart entry:

```sh
./install.sh
```

## Autostart with Hyprland

Add the binary to your Hyprland autostart. Classic config (`hyprland.conf`):

```ini
exec-once = /path/to/oneko-rust/target/release/oneko-rust
```

Lua config (`hyprland.lua`, Hyprland ≥ 0.55):

```lua
hl.on("hyprland.start", function()
    hl.exec_cmd("/path/to/oneko-rust/target/release/oneko-rust")
end)
```

Stop it with `pkill oneko-rust`.

## Limitations

- Single-monitor: the layer surface lives on the monitor it spawns on, while cursor coordinates are global. Multi-monitor support would need per-output surfaces.

## Credits

Sprites and behavior are taken from the original [oneko](https://github.com/tie/oneko) by Masayuki Koba, which its maintainers describe as public domain software (no formal license file).

## License

This rewrite is licensed under the [GNU General Public License v3.0](LICENSE).
