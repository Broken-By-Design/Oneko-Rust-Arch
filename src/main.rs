use std::{thread, time::Duration};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_output, wl_pointer::WlPointer, wl_seat::WlSeat, wl_shm, wl_surface},
        Connection, QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

mod pet;
mod sprites;

use pet::{dir_from_delta, select_sprite, CatState, Dir, PetAction};
use sprites::{BLANK, SIZE};

// Autodetects the desktop environment and grabs a silent full screenshot in PPM format
fn capture_screenshot_ppm() -> Option<Vec<u8>> {
    use std::fs;
    use std::process::Command;

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();
    println!("[DEBUG] Detecting desktop environment: '{}'", desktop);

    // Backend 1: Try grim (wlroots - Hyprland, Sway)
    println!("[DEBUG] Trying 'grim' backend...");
    if let Ok(output) = Command::new("grim").args(&["-t", "ppm", "-"]).output() {
        if output.status.success() {
            println!("[DEBUG] 'grim' screenshot succeeded.");
            return Some(output.stdout);
        } else {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            println!("[DEBUG] 'grim' failed: {}", err_msg.trim());
        }
    }

    // Backend 2: KDE Spectacle (saves directly as PPM in the background)
    if desktop.contains("kde") || desktop.contains("plasma") {
        println!("[DEBUG] KDE detected. Trying 'spectacle' backend...");
        let temp_ppm = "/tmp/cat_screenshot.ppm";
        let _ = fs::remove_file(temp_ppm); // Clear any old file

        if let Ok(status) = Command::new("spectacle")
            .args(&["--background", "--nonotify", "--output", temp_ppm])
            .status()
        {
            if status.success() {
                if let Ok(data) = fs::read(temp_ppm) {
                    println!("[DEBUG] 'spectacle' screenshot succeeded.");
                    let _ = fs::remove_file(temp_ppm);
                    return Some(data);
                }
            }
        }
        println!("[DEBUG] 'spectacle' backend failed.");
    }

    // Backend 3: GNOME (DBus Screenshot -> PNG -> ImageMagick Convert to PPM)
    if desktop.contains("gnome") || desktop.contains("unity") {
        println!("[DEBUG] GNOME detected. Trying DBus + ImageMagick backend...");
        let temp_png = "/tmp/cat_screenshot.png";
        let _ = fs::remove_file(temp_png);

        // GNOME DBus call takes a silent, non-interactive full screenshot
        let dbus_status = Command::new("gdbus")
            .args(&[
                "call",
                "--session",
                "--dest",
                "org.gnome.Shell.Screenshot",
                "--object-path",
                "/org/gnome/Shell/Screenshot",
                "--method",
                "org.gnome.Shell.Screenshot.Screenshot",
                "true",
                "false",
                temp_png,
            ])
            .status();

        if let Ok(status) = dbus_status {
            if status.success() {
                println!("[DEBUG] GNOME DBus screenshot saved to {}", temp_png);
                // Try converting PNG to PPM using ImageMagick
                for cmd in &["magick", "convert"] {
                    if let Ok(output) = Command::new(cmd).args(&[temp_png, "ppm:-"]).output() {
                        if output.status.success() {
                            println!("[DEBUG] Converted PNG to PPM using '{}'.", cmd);
                            let _ = fs::remove_file(temp_png);
                            return Some(output.stdout);
                        }
                    }
                }
                println!("[DEBUG] Failed to convert PNG to PPM. Please install 'imagemagick' (sudo apt install imagemagick / pacman -S imagemagick).");
            }
        }
        println!("[DEBUG] GNOME DBus backend failed.");
    }

    println!("[DEBUG] No successful screenshot backend found on this system.");
    None
}

// Scans a screenshot in memory for a solid horizontal line > 300 pixels long
fn find_ledge() -> Option<(f32, f32, f32)> {
    return None;
    // Returns (x, y, length)
    let data = capture_screenshot_ppm()?;

    // Fast, basic PPM image parser
    let mut idx = 0;

    let mut read_token = |idx: &mut usize| -> Option<String> {
        // Skip whitespace
        while *idx < data.len() && data[*idx].is_ascii_whitespace() {
            *idx += 1;
        }
        // Read token
        let start = *idx;
        while *idx < data.len() && !data[*idx].is_ascii_whitespace() {
            *idx += 1;
        }
        if start == *idx {
            None
        } else {
            Some(String::from_utf8_lossy(&data[start..*idx]).to_string())
        }
    };

    if read_token(&mut idx)? != "P6" {
        println!("[DEBUG] Image format is not P6.");
        return None;
    }
    let w: usize = read_token(&mut idx)?.parse().ok()?;
    let h: usize = read_token(&mut idx)?.parse().ok()?;
    let maxval: usize = read_token(&mut idx)?.parse().ok()?;

    println!("[DEBUG] Parsed PPM header: {}x{} maxval: {}", w, h, maxval);

    if maxval != 255 {
        println!("[DEBUG] Unsupported maxval: {}", maxval);
        return None;
    }
    idx += 1; // Skip trailing newline

    let pixels = &data[idx..];
    if pixels.len() < w * h * 3 {
        println!(
            "[DEBUG] Pixel data truncated. Expected {}, got {}",
            w * h * 3,
            pixels.len()
        );
        return None;
    }

    let mut max_edge_found = 0;

    // Scan every 20th row to find a window border
    for y in (50..h).step_by(20) {
        let mut edge_len = 0;
        let mut start_x = 0;
        let mut last_color: Option<(u8, u8, u8)> = None;

        for x in 0..w {
            let p_idx = (y * w + x) * 3;
            let above_idx = ((y - 2) * w + x) * 3; // Check 2 pixels above for contrast

            if p_idx + 2 >= pixels.len() || above_idx + 2 >= pixels.len() {
                break;
            }

            let color = (pixels[p_idx], pixels[p_idx + 1], pixels[p_idx + 2]);
            let above_color = (
                pixels[above_idx],
                pixels[above_idx + 1],
                pixels[above_idx + 2],
            );

            // Vertical contrast (Is it an edge?)
            let v_diff = (color.0 as i32 - above_color.0 as i32).abs()
                + (color.1 as i32 - above_color.1 as i32).abs()
                + (color.2 as i32 - above_color.2 as i32).abs();

            // Horizontal consistency (Is it a solid straight line?)
            let h_diff = if let Some(lc) = last_color {
                (color.0 as i32 - lc.0 as i32).abs()
                    + (color.1 as i32 - lc.1 as i32).abs()
                    + (color.2 as i32 - lc.2 as i32).abs()
            } else {
                0
            };

            last_color = Some(color);

            if v_diff > 60 && h_diff < 15 {
                edge_len += 1;

                if edge_len > max_edge_found {
                    max_edge_found = edge_len;
                }

                if edge_len > 300 {
                    // We found a flat ledge at least 300 pixels long!
                    println!(
                        "[DEBUG] >>> LEDGE FOUND! <<< at x: {}, y: {}, len: {}",
                        start_x, y, edge_len
                    );
                    return Some((start_x as f32, y as f32, edge_len as f32));
                }
            } else {
                edge_len = 0;
                start_x = x + 1;
                last_color = None;
            }
        }
    }

    println!(
        "[DEBUG] Scan complete. No ledges found on screen (Max unbroken line was {}px long).",
        max_edge_found
    );
    None
}

struct OutputSurface {
    output_id: u32,
    output: wl_output::WlOutput,
    layer: LayerSurface,
    configured: bool,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    local_x: f32,
    local_y: f32,
    // Track last drawn state to avoid redundant compositor commits
    last_drawn_x: Option<i32>,
    last_drawn_y: Option<i32>,
    last_sprite: Option<*const u8>,
    last_active: Option<bool>,
}

struct Cat {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    outputs: Vec<OutputSurface>,
    active_output_id: Option<u32>,
    exit: bool,
    win_x: f32,
    win_y: f32,
    dir: Dir,
    frame: bool,

    action: PetAction,
    action_ticks: u32,
    mouse_down: bool,
    target_pos: Option<(f32, f32)>,
    ledge_timer: u32, // <-- Added this
}

impl Cat {
    fn configure_layer(layer: &LayerSurface) {
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_size(SIZE, SIZE);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    }

    fn output_geometry(&self, output: &wl_output::WlOutput) -> Option<(u32, f32, f32, f32, f32)> {
        let info = self.output_state.info(output)?;
        let (x, y) = info.logical_position.unwrap_or(info.location);
        let (w, h) = info
            .logical_size
            .or_else(|| info.modes.iter().find(|m| m.current).map(|m| m.dimensions))
            .or_else(|| info.modes.first().map(|m| m.dimensions))?;

        Some((info.id, x as f32, y as f32, w as f32, h as f32))
    }

    fn ensure_output_surface(&mut self, qh: &QueueHandle<Self>, output: &wl_output::WlOutput) {
        let Some((output_id, x, y, w, h)) = self.output_geometry(output) else {
            return;
        };

        if let Some(idx) = self
            .outputs
            .iter()
            .position(|entry| entry.output == *output)
        {
            let entry = &mut self.outputs[idx];
            entry.output_id = output_id;
            entry.x = x;
            entry.y = y;
            entry.w = w;
            entry.h = h;
            return;
        }

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("oneko"),
            Some(output),
        );
        Self::configure_layer(&layer);

        // We intentionally don't set an empty input region anymore so it can be clicked
        layer.commit();

        self.outputs.push(OutputSurface {
            output_id,
            output: output.clone(),
            layer,
            configured: false,
            x,
            y,
            w,
            h,
            local_x: 0.0,
            local_y: 0.0,
            last_drawn_x: None,
            last_drawn_y: None,
            last_sprite: None,
            last_active: None,
        });
    }

    fn output_index_by_id(&self, output_id: u32) -> Option<usize> {
        self.outputs
            .iter()
            .position(|entry| entry.output_id == output_id)
    }

    fn pick_output_id_for_point(&self, x: f32, y: f32) -> Option<u32> {
        if let Some(entry) = self.outputs.iter().find(|entry| {
            x >= entry.x && x < entry.x + entry.w && y >= entry.y && y < entry.y + entry.h
        }) {
            return Some(entry.output_id);
        }

        self.outputs
            .iter()
            .min_by(|a, b| {
                let ax = a.x + a.w * 0.5;
                let ay = a.y + a.h * 0.5;
                let bx = b.x + b.w * 0.5;
                let by = b.y + b.h * 0.5;
                let da = (x - ax) * (x - ax) + (y - ay) * (y - ay);
                let db = (x - bx) * (x - bx) + (y - by) * (y - by);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|entry| entry.output_id)
    }

    fn tick(&mut self) {
        if self.outputs.is_empty() {
            return;
        }

        // Identify which output the cat is currently on (respecting explicit transitions first)
        let current_output_id = self
            .active_output_id
            .or_else(|| self.outputs.first().map(|entry| entry.output_id));

        let Some(current_output_id) = current_output_id else {
            return;
        };

        if self.active_output_id.is_none() {
            self.active_output_id = Some(current_output_id);
        }

        let Some(current_idx) = self.output_index_by_id(current_output_id) else {
            return;
        };

        let (cur_min_x, cur_max_x, cur_min_y, cur_max_y) = {
            let output = &self.outputs[current_idx];
            let min_x = output.x;
            let min_y = output.y;
            let max_x = (output.x + output.w - SIZE as f32).max(min_x);
            let max_y = (output.y + output.h - SIZE as f32).max(min_y);
            (min_x, max_x, min_y, max_y)
        };

        self.frame = !self.frame;

        // --- NEW LEDGE FINDING LOGIC ---
        if !self.mouse_down {
            if self.ledge_timer == 0 {
                if let Some((lx, ly, lw)) = find_ledge() {
                    // Send the cat to sit directly on the middle of the newly found ledge
                    self.target_pos = Some((lx + lw / 2.0, ly - SIZE as f32));
                }
                self.ledge_timer = 8 * 60; // Wait 60 seconds before scanning again
            } else {
                self.ledge_timer -= 1;
            }
        }
        // -------------------------------

        let mut chasing_pointer = false;

        if let Some((mut tx, mut ty)) = self.target_pos {
            // Find which output contains the target position (or default to current output)
            let target_output_idx = self
                .pick_output_id_for_point(tx, ty)
                .and_then(|id| self.output_index_by_id(id))
                .unwrap_or(current_idx);

            let (t_min_x, t_max_x, t_min_y, t_max_y) = {
                let output = &self.outputs[target_output_idx];
                let min_x = output.x;
                let min_y = output.y;
                let max_x = (output.x + output.w - SIZE as f32).max(min_x);
                let max_y = (output.y + output.h - SIZE as f32).max(min_y);
                (min_x, max_x, min_y, max_y)
            };

            // Clamp target to the boundaries of the output it belongs to
            tx = tx.clamp(t_min_x + SIZE as f32 * 0.5, t_max_x + SIZE as f32 * 0.5);
            ty = ty.clamp(t_min_y + SIZE as f32 * 0.5, t_max_y + SIZE as f32 * 0.5);

            let dx = tx - (self.win_x + SIZE as f32 * 0.5);
            let dy = ty - (self.win_y + SIZE as f32 * 0.5);
            let dist = (dx * dx + dy * dy).sqrt();

            // If we reached the target and the mouse is released, stop chasing
            if dist < 4.0 && !self.mouse_down {
                self.target_pos = None;
                self.action = PetAction::Sitting; // Force the cat to sit when it arrives at the ledge/mouse drop!
                self.action_ticks = 8 * 20; // Sit for 20 seconds
            } else {
                chasing_pointer = true;
                if dx.abs() > 2.0 || dy.abs() > 2.0 {
                    self.dir = dir_from_delta(dx, dy);
                }

                // Normal, constant speed instead of teleporting
                let speed = 12.0;
                if dist > speed {
                    self.win_x += (dx / dist) * speed;
                    self.win_y += (dy / dist) * speed;
                } else {
                    self.win_x += dx;
                    self.win_y += dy;
                }
            }
        }

        let state = if chasing_pointer {
            CatState::Chasing
        } else {
            if self.action_ticks == 0 {
                // Pseudo-random action generator based on system time
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .subsec_nanos();
                let rand = time % 100;

                self.action = match self.action {
                    PetAction::Wandering(_) => {
                        if rand < 30 {
                            self.action_ticks = 20;
                            PetAction::Sitting
                        } else {
                            let dirs = [
                                Dir::N,
                                Dir::NE,
                                Dir::E,
                                Dir::SE,
                                Dir::S,
                                Dir::SW,
                                Dir::W,
                                Dir::NW,
                            ];
                            self.action_ticks = 15 + (time % 15);
                            PetAction::Wandering(dirs[(time % 8) as usize])
                        }
                    }
                    PetAction::Sitting => {
                        if rand < 30 {
                            self.action_ticks = 20;
                            PetAction::Washing // Only wash after sitting
                        } else if rand < 60 {
                            let dirs = [
                                Dir::N,
                                Dir::NE,
                                Dir::E,
                                Dir::SE,
                                Dir::S,
                                Dir::SW,
                                Dir::W,
                                Dir::NW,
                            ];
                            self.action_ticks = 15 + (time % 15);
                            PetAction::Wandering(dirs[(time % 8) as usize])
                        } else {
                            self.action_ticks = 20;
                            PetAction::Sitting
                        }
                    }
                    PetAction::Washing => {
                        if rand < 50 {
                            self.action_ticks = 40;
                            PetAction::Sleeping // Only sleep after washing
                        } else if rand < 70 {
                            self.action_ticks = 20;
                            PetAction::Sitting
                        } else {
                            let dirs = [
                                Dir::N,
                                Dir::NE,
                                Dir::E,
                                Dir::SE,
                                Dir::S,
                                Dir::SW,
                                Dir::W,
                                Dir::NW,
                            ];
                            self.action_ticks = 15 + (time % 15);
                            PetAction::Wandering(dirs[(time % 8) as usize])
                        }
                    }
                    PetAction::Sleeping => {
                        if rand < 20 {
                            self.action_ticks = 20;
                            PetAction::Sitting // Wake up and sit
                        } else {
                            self.action_ticks = 40;
                            PetAction::Sleeping
                        }
                    }
                };

                if let PetAction::Wandering(d) = self.action {
                    self.dir = d;
                }
            } else {
                self.action_ticks -= 1;
            }

            match self.action {
                PetAction::Sitting => CatState::Sitting,
                PetAction::Washing => CatState::Washing,
                PetAction::Sleeping => CatState::Sleeping,
                PetAction::Wandering(d) => {
                    let speed = 8.0;
                    let (dx, dy) = match d {
                        Dir::N => (0.0, -speed),
                        Dir::S => (0.0, speed),
                        Dir::E => (speed, 0.0),
                        Dir::W => (-speed, 0.0),
                        Dir::NE => (speed, -speed),
                        Dir::NW => (-speed, -speed),
                        Dir::SE => (speed, speed),
                        Dir::SW => (-speed, speed),
                    };
                    self.win_x += dx;
                    self.win_y += dy;

                    // If it hits an edge, force a new action next tick
                    if self.win_x <= cur_min_x
                        || self.win_x >= cur_max_x
                        || self.win_y <= cur_min_y
                        || self.win_y >= cur_max_y
                    {
                        self.action_ticks = 0;
                    }

                    CatState::Chasing
                }
            }
        };

        // If wandering (not chasing), clamp coordinates to the current active monitor
        if !chasing_pointer {
            if let Some(active_id) = self.active_output_id {
                if let Some(idx) = self.output_index_by_id(active_id) {
                    let output = &self.outputs[idx];
                    let min_x = output.x;
                    let min_y = output.y;
                    let max_x = (output.x + output.w - SIZE as f32).max(min_x);
                    let max_y = (output.y + output.h - SIZE as f32).max(min_y);
                    self.win_x = self.win_x.clamp(min_x, max_x);
                    self.win_y = self.win_y.clamp(min_y, max_y);
                }
            }
        }

        let (sprite, mask) = select_sprite(state, self.dir, self.frame, self.action_ticks);

        // Determine the active output after movement
        let active_output_id = self.pick_output_id_for_point(self.win_x, self.win_y);
        self.active_output_id = active_output_id;

        // Draw on all outputs
        let num_outputs = self.outputs.len();
        for idx in 0..num_outputs {
            let output_id = self.outputs[idx].output_id;
            let is_active = Some(output_id) == self.active_output_id;
            let configured = self.outputs[idx].configured;

            if configured {
                let layer = self.outputs[idx].layer.clone();
                let origin_x = self.outputs[idx].x;
                let origin_y = self.outputs[idx].y;
                let local_max_x = (self.outputs[idx].w - SIZE as f32).max(0.0);
                let local_max_y = (self.outputs[idx].h - SIZE as f32).max(0.0);
                let local_x = (self.win_x - origin_x).clamp(0.0, local_max_x);
                let local_y = (self.win_y - origin_y).clamp(0.0, local_max_y);

                self.outputs[idx].local_x = local_x;
                self.outputs[idx].local_y = local_y;

                let target_sprite = if is_active { sprite } else { &BLANK };
                let target_mask = if is_active { mask } else { &BLANK };

                let x_int = local_x as i32;
                let y_int = local_y as i32;
                let sprite_ptr = target_sprite.as_ptr();

                // Skip commits if state did not change
                let changed = self.outputs[idx].last_drawn_x != Some(x_int)
                    || self.outputs[idx].last_drawn_y != Some(y_int)
                    || self.outputs[idx].last_sprite != Some(sprite_ptr)
                    || self.outputs[idx].last_active != Some(is_active);

                if changed {
                    layer.set_margin(y_int, 0, 0, x_int);
                    self.draw(&layer, target_sprite, target_mask, is_active);

                    self.outputs[idx].last_drawn_x = Some(x_int);
                    self.outputs[idx].last_drawn_y = Some(y_int);
                    self.outputs[idx].last_sprite = Some(sprite_ptr);
                    self.outputs[idx].last_active = Some(is_active);
                }
            }
        }
    }

    // XBM layout: 4 bytes per row, LSB of each byte is the leftmost pixel.
    // mask bit set + sprite bit set => black, mask only => white, else transparent.
    // XBM layout: 4 bytes per row, LSB of each byte is the leftmost pixel.
    // mask bit set + sprite bit set => black, mask only => white, else transparent.
    fn draw(
        &mut self,
        layer: &LayerSurface,
        sprite: &[u8; 128],
        mask: &[u8; 128],
        is_active: bool,
    ) {
        let (buffer, canvas) = self
            .pool
            .create_buffer(
                SIZE as i32,
                SIZE as i32,
                (SIZE * 4) as i32,
                wl_shm::Format::Argb8888,
            )
            .expect("create shm buffer");

        // Create an input region
        let input_region = Region::new(&self.compositor).expect("create input region");

        for y in 0..SIZE as i32 {
            let mut start_x = None;
            for x in 0..SIZE as i32 {
                let byte = (y as usize) * 4 + (x as usize) / 8;
                let bit = 1u8 << ((x as usize) % 8);
                let is_solid = is_active && (mask[byte] & bit != 0);

                // Draw pixel colors
                let px: u32 = if is_solid {
                    if sprite[byte] & bit != 0 {
                        0xFF00_0000
                    } else {
                        0xFFFF_FFFF
                    }
                } else {
                    0
                };
                let idx = ((y as usize) * SIZE as usize + (x as usize)) * 4;
                canvas[idx..idx + 4].copy_from_slice(&px.to_le_bytes());

                // Build input region shape:
                // Active surfaces match the exact shape of the sprite.
                // Inactive surfaces are fully solid 32x32 to catch pointer entries.
                let is_input_pixel = if is_active { is_solid } else { true };

                match (start_x, is_input_pixel) {
                    (None, true) => start_x = Some(x),
                    (Some(sx), false) => {
                        input_region.add(sx, y, x - sx, 1);
                        start_x = None;
                    }
                    _ => {}
                }
            }
            if let Some(sx) = start_x {
                input_region.add(sx, y, SIZE as i32 - sx, 1);
            }
        }

        let surface = layer.wl_surface();

        // Apply the exact bounds of the sprite to allow clicking through transparent areas!
        surface.set_input_region(Some(input_region.wl_region()));
        surface.damage_buffer(0, 0, SIZE as i32, SIZE as i32);

        buffer.attach_to(surface).expect("attach buffer");
        layer.commit();
    }
}

impl CompositorHandler for Cat {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Cat {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.ensure_output_surface(qh, &output);
    }

    fn update_output(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.ensure_output_surface(qh, &output);
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(idx) = self.outputs.iter().position(|entry| entry.output == output) {
            let removed = self.outputs.remove(idx);
            if self.active_output_id == Some(removed.output_id) {
                self.active_output_id = None;
            }
        }
    }
}

impl LayerShellHandler for Cat {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(idx) = self.outputs.iter().position(|entry| entry.layer == *layer) {
            let removed = self.outputs.remove(idx);
            if self.active_output_id == Some(removed.output_id) {
                self.active_output_id = None;
            }
        }
        if self.outputs.is_empty() {
            self.exit = true;
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(entry) = self.outputs.iter_mut().find(|entry| entry.layer == *layer) {
            entry.configured = true;
        }
    }
}

impl ShmHandler for Cat {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Cat {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

impl SeatHandler for Cat {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.seat_state
                .get_pointer(qh, &seat)
                .expect("Failed to get pointer");
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: WlSeat,
        _: Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl PointerHandler for Cat {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    if let Some(entry) = self
                        .outputs
                        .iter()
                        .find(|entry| entry.layer.wl_surface() == &event.surface)
                    {
                        self.target_pos = Some((
                            entry.x + entry.local_x + event.position.0 as f32,
                            entry.y + entry.local_y + event.position.1 as f32,
                        ));
                    }
                }
                PointerEventKind::Press { .. } => {
                    self.mouse_down = true;
                    if let Some(entry) = self
                        .outputs
                        .iter()
                        .find(|entry| entry.layer.wl_surface() == &event.surface)
                    {
                        self.target_pos = Some((
                            entry.x + entry.local_x + event.position.0 as f32,
                            entry.y + entry.local_y + event.position.1 as f32,
                        ));
                    }
                    self.action = PetAction::Sitting;
                    self.action_ticks = 0;
                }
                PointerEventKind::Release { .. } => {
                    self.mouse_down = false;
                }
                _ => {}
            }
        }
    }
}

delegate_compositor!(Cat);
delegate_output!(Cat);
delegate_shm!(Cat);
delegate_layer!(Cat);
delegate_registry!(Cat);
delegate_seat!(Cat);
delegate_pointer!(Cat);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;

    let pool = SlotPool::new((SIZE * SIZE * 4) as usize, &shm)?;

    let mut cat = Cat {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        compositor,
        layer_shell,
        shm,
        pool,
        outputs: Vec::new(),
        active_output_id: None,
        exit: false,
        win_x: 200.0,
        win_y: 200.0,
        dir: Dir::E,
        frame: false,
        action: PetAction::Sitting,
        action_ticks: 0,
        mouse_down: false,
        target_pos: None,
        ledge_timer: 8 * 60,
    };

    while !cat.exit {
        if cat.outputs.iter().any(|entry| entry.configured) {
            cat.tick();
        }
        event_queue.roundtrip(&mut cat)?;
        thread::sleep(Duration::from_millis(125));
    }

    Ok(())
}
