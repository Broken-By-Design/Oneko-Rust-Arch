use crate::sprites::{
    CAT_DOWN1, CAT_DOWN1_MASK, CAT_DOWN2, CAT_DOWN2_MASK, CAT_LEFT1, CAT_LEFT1_MASK, CAT_LEFT2,
    CAT_LEFT2_MASK, CAT_NE1, CAT_NE1_MASK, CAT_NE2, CAT_NE2_MASK, CAT_NW1, CAT_NW1_MASK,
    CAT_NW2, CAT_NW2_MASK, CAT_RIGHT1, CAT_RIGHT1_MASK, CAT_RIGHT2, CAT_RIGHT2_MASK, CAT_SE1,
    CAT_SE1_MASK, CAT_SE2, CAT_SE2_MASK, CAT_SITTING, CAT_SITTING_MASK, CAT_SLEEPING_1,
    CAT_SLEEPING_1_MASK, CAT_SLEEPING_2, CAT_SLEEPING_2_MASK, CAT_SW1, CAT_SW1_MASK, CAT_SW2,
    CAT_SW2_MASK, CAT_UP1, CAT_UP1_MASK, CAT_UP2, CAT_UP2_MASK, CAT_WASHING_1, CAT_WASHING_1_MASK,
    CAT_WASHING_2, CAT_WASHING_2_MASK,
};

#[derive(Clone, Copy, PartialEq)]
pub enum CatState {
    Chasing,
    Sitting,
    Washing,
    Sleeping,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Dir {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PetAction {
    Wandering(Dir),
    Sitting,
    Washing,
    Sleeping,
}

pub fn dir_from_delta(dx: f32, dy: f32) -> Dir {
    let adx = dx.abs();
    let ady = dy.abs();
    if adx > ady * 2.0 {
        if dx > 0.0 { Dir::E } else { Dir::W }
    } else if ady > adx * 2.0 {
        if dy > 0.0 { Dir::S } else { Dir::N }
    } else {
        match (dx >= 0.0, dy <= 0.0) {
            (true, true) => Dir::NE,
            (false, true) => Dir::NW,
            (true, false) => Dir::SE,
            (false, false) => Dir::SW,
        }
    }
}

pub fn select_sprite(
    state: CatState,
    dir: Dir,
    frame: bool,
    action_ticks: u32,
) -> (&'static [u8; 128], &'static [u8; 128]) {
    match state {
        CatState::Sitting => (&CAT_SITTING, &CAT_SITTING_MASK),
        CatState::Washing => {
            if frame {
                (&CAT_WASHING_1, &CAT_WASHING_1_MASK)
            } else {
                (&CAT_WASHING_2, &CAT_WASHING_2_MASK)
            }
        }
        CatState::Sleeping => {
            if (action_ticks / 4) % 2 == 0 {
                (&CAT_SLEEPING_1, &CAT_SLEEPING_1_MASK)
            } else {
                (&CAT_SLEEPING_2, &CAT_SLEEPING_2_MASK)
            }
        }
        CatState::Chasing => match (dir, frame) {
            (Dir::E, true) => (&CAT_RIGHT1, &CAT_RIGHT1_MASK),
            (Dir::E, false) => (&CAT_RIGHT2, &CAT_RIGHT2_MASK),
            (Dir::W, true) => (&CAT_LEFT1, &CAT_LEFT1_MASK),
            (Dir::W, false) => (&CAT_LEFT2, &CAT_LEFT2_MASK),
            (Dir::N, true) => (&CAT_UP1, &CAT_UP1_MASK),
            (Dir::N, false) => (&CAT_UP2, &CAT_UP2_MASK),
            (Dir::S, true) => (&CAT_DOWN1, &CAT_DOWN1_MASK),
            (Dir::S, false) => (&CAT_DOWN2, &CAT_DOWN2_MASK),
            (Dir::NE, true) => (&CAT_NE1, &CAT_NE1_MASK),
            (Dir::NE, false) => (&CAT_NE2, &CAT_NE2_MASK),
            (Dir::NW, true) => (&CAT_NW1, &CAT_NW1_MASK),
            (Dir::NW, false) => (&CAT_NW2, &CAT_NW2_MASK),
            (Dir::SE, true) => (&CAT_SE1, &CAT_SE1_MASK),
            (Dir::SE, false) => (&CAT_SE2, &CAT_SE2_MASK),
            (Dir::SW, true) => (&CAT_SW1, &CAT_SW1_MASK),
            (Dir::SW, false) => (&CAT_SW2, &CAT_SW2_MASK),
        },
    }
}
