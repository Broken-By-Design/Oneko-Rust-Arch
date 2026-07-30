#[cfg(target_os = "macos")]
mod app {
    use std::{thread, time::Duration};

    use core_graphics::display::CGDisplay;
    use device_query::{DeviceQuery, DeviceState};
    use minifb::{Key, Window, WindowOptions};

    mod pet;
    mod sprites;

    use pet::{dir_from_delta, select_sprite, CatState, Dir, PetAction};
    use sprites::SIZE;

    struct MacCat {
        win_x: f32,
        win_y: f32,
        dir: Dir,
        frame: bool,
        action: PetAction,
        action_ticks: u32,
        mouse_down: bool,
        target_pos: Option<(f32, f32)>,
        screen_w: f32,
        screen_h: f32,
    }

    impl MacCat {
        fn tick(&mut self) {
            self.frame = !self.frame;

            let min_x = 0.0;
            let min_y = 0.0;
            let max_x = (self.screen_w - SIZE as f32).max(min_x);
            let max_y = (self.screen_h - SIZE as f32).max(min_y);

            let mut chasing_pointer = false;

            if let Some((mut tx, mut ty)) = self.target_pos {
                tx = tx.clamp(min_x + SIZE as f32 * 0.5, max_x + SIZE as f32 * 0.5);
                ty = ty.clamp(min_y + SIZE as f32 * 0.5, max_y + SIZE as f32 * 0.5);

                let dx = tx - (self.win_x + SIZE as f32 * 0.5);
                let dy = ty - (self.win_y + SIZE as f32 * 0.5);
                let dist = (dx * dx + dy * dy).sqrt();

                if dist < 4.0 && !self.mouse_down {
                    self.target_pos = None;
                    self.action = PetAction::Sitting;
                    self.action_ticks = 8 * 20;
                } else {
                    chasing_pointer = true;
                    if dx.abs() > 2.0 || dy.abs() > 2.0 {
                        self.dir = dir_from_delta(dx, dy);
                    }

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

            if !chasing_pointer {
                if self.action_ticks == 0 {
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
                                PetAction::Washing
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
                                PetAction::Sleeping
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
                                PetAction::Sitting
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

                if let PetAction::Wandering(d) = self.action {
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

                    if self.win_x <= min_x || self.win_x >= max_x || self.win_y <= min_y || self.win_y >= max_y {
                        self.action_ticks = 0;
                    }
                }

                self.win_x = self.win_x.clamp(min_x, max_x);
                self.win_y = self.win_y.clamp(min_y, max_y);
            }
        }

        fn state(&self) -> CatState {
            if self.target_pos.is_some() {
                CatState::Chasing
            } else {
                match self.action {
                    PetAction::Sitting => CatState::Sitting,
                    PetAction::Washing => CatState::Washing,
                    PetAction::Sleeping => CatState::Sleeping,
                    PetAction::Wandering(_) => CatState::Chasing,
                }
            }
        }
    }

    fn render_sprite(sprite: &[u8; 128], mask: &[u8; 128]) -> Vec<u32> {
        let mut frame = vec![0u32; (SIZE * SIZE) as usize];

        for y in 0..SIZE as usize {
            for x in 0..SIZE as usize {
                let byte = y * 4 + x / 8;
                let bit = 1u8 << (x % 8);
                frame[y * SIZE as usize + x] = if mask[byte] & bit != 0 {
                    if sprite[byte] & bit != 0 {
                        0xFF00_0000
                    } else {
                        0xFFFF_FFFF
                    }
                } else {
                    0x0000_0000
                };
            }
        }

        frame
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let display = unsafe { CGDisplay::new(CGDisplay::main().id) };
        let screen_w = display.pixels_wide() as f32;
        let screen_h = display.pixels_high() as f32;

        let mut window = Window::new(
            "oneko-rust-macos",
            SIZE as usize,
            SIZE as usize,
            WindowOptions {
                borderless: true,
                topmost: true,
                transparency: true,
                ..WindowOptions::default()
            },
        )?;

        let device_state = DeviceState::new();

        let mut cat = MacCat {
            win_x: 200.0,
            win_y: 200.0,
            dir: Dir::E,
            frame: false,
            action: PetAction::Sitting,
            action_ticks: 0,
            mouse_down: false,
            target_pos: None,
            screen_w,
            screen_h,
        };

        let mut last_mouse = device_state.get_mouse().coords;

        while window.is_open() && !window.is_key_down(Key::Escape) {
            let mouse = device_state.get_mouse();
            let moved = mouse.coords != last_mouse;
            last_mouse = mouse.coords;

            if moved || mouse.button_pressed.iter().any(|pressed| *pressed) {
                cat.target_pos = Some((mouse.coords.0 as f32, mouse.coords.1 as f32));
            }
            cat.mouse_down = mouse.button_pressed.iter().any(|pressed| *pressed);

            cat.tick();

            let state = cat.state();
            let (sprite, mask) = select_sprite(state, cat.dir, cat.frame, cat.action_ticks);
            let frame = render_sprite(sprite, mask);

            window.set_position(cat.win_x as isize, cat.win_y as isize);
            window.update_with_buffer(&frame, SIZE as usize, SIZE as usize)?;

            thread::sleep(Duration::from_millis(125));
        }

        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("oneko-rust-macos is only supported on macOS.");
}
