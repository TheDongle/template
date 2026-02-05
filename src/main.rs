// Games made using `agb` are no_std which means you don't have access to the standard
// rust library. This is because the game boy advance doesn't have an operating
// system, so most of the content of the standard library doesn't apply.
#![no_std]
// `agb` defines its own `main` function, so you must declare your game's main function
// using the #[agb::entry] proc macro. Failing to do so will cause failure in linking
// which won't be a particularly clear error message.
#![no_main]
// This is required to allow writing tests
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]
#![cfg_attr(test, test_runner(agb::test_runner::test_runner))]

// By default no_std crates don't get alloc, so you won't be able to use things like Vec
// until you declare the extern crate. `agb` provides an allocator so it will all work
extern crate alloc;

use agb::display::object::Object;
use agb::display::tiled::{RegularBackground, RegularBackgroundSize, TileFormat, VRAM_MANAGER};
use agb::display::{GraphicsFrame, Priority, WIDTH};
use agb::fixnum::{FixedNum, Number, Rect, Vector2D, num, vec2};
use agb::include_wav;
use agb::input::{Button, ButtonController};
use agb::sound::mixer::{Frequency, Mixer, SoundChannel, SoundData};
use agb::{include_aseprite, include_background_gfx};
use agb_tracker::{Track, Tracker, include_xm};

include_background_gfx!(
    mod background,
    PLAY_FIELD => deduplicate "gfx/background.aseprite",
    GAME_OVER => deduplicate "gfx/game_over.aseprite",
);

include_aseprite!(
    mod sprites,
    "gfx/cpu.aseprite",
    "gfx/sprites.aseprite",
    "gfx/health.aseprite",
    "gfx/player.aseprite"
);

static BALL_PADDLE_HIT: SoundData = include_wav!("sfx/ball-paddle-hit.wav");
static BGM: Track = include_xm!("sfx/bgm.xm");
static WALL_HIT: SoundData = include_wav!("sfx/wall-hit.wav");

fn play_sound(mixer: &mut Mixer, sound: SoundData) {
    let hit_sound = SoundChannel::new(sound);
    mixer.play_sound(hit_sound);
}

struct Circle<T: Number> {
    pos: Vector2D<T>,
    radius: T,
}

trait Touches<T> {
    fn touches(&self, rhs: T) -> bool;
}

impl<T: Number> Circle<T> {
    pub fn new(pos: Vector2D<T>, radius: T) -> Self {
        Self { pos, radius }
    }
    pub fn centre(&self) -> Vector2D<T> {
        self.pos + vec2(self.radius, self.radius)
    }
}

impl Touches<Rect<FixedNum<8>>> for Circle<FixedNum<8>> {
    fn touches(&self, rect: Rect<FixedNum<8>>) -> bool {
        // which edge is closest;
        let test_x = match self.centre().x {
            cx if cx < rect.top_left().x => rect.top_left().x,
            cx if cx > rect.bottom_right().x => rect.bottom_right().x,
            cx => cx,
        };
        let test_y = match self.centre().y {
            cy if cy < rect.top_left().y => rect.top_left().y,
            cy if cy > rect.bottom_left().y => rect.bottom_left().y,
            cy => cy,
        };

        let dist_x = self.centre().x - test_x;
        let dist_y = test_y - self.centre().y;
        let dist = ((dist_x * dist_x) + (dist_y * dist_y)).sqrt();

        dist <= self.radius
    }
}

pub struct Ball {
    pos: Vector2D<FixedNum<8>>,
    velocity: Vector2D<FixedNum<8>>,
}

impl Ball {
    pub fn new(pos: Vector2D<FixedNum<8>>, velocity: Vector2D<FixedNum<8>>) -> Self {
        Self { pos, velocity }
    }

    pub fn update(
        &mut self,
        paddle_a: &mut Paddle<P1>,
        paddle_b: &mut Paddle<P2>,
        mixer: &mut Mixer,
    ) {
        // Speculatively move the ball, we'll update the velocity if this causes it to intersect with either the
        // edge of the map or a paddle.
        let potential_ball_pos = self.pos + self.velocity;

        let ball_mask = Circle::new(potential_ball_pos, num!(8));
        if ball_mask.touches(paddle_a.collision_rect()) {
            self.velocity.x = self.velocity.x.abs();
            let y_difference = (ball_mask.centre().y - paddle_a.collision_rect().centre().y) / 32;
            self.velocity.y += y_difference;
            play_sound(mixer, BALL_PADDLE_HIT);
        }

        if ball_mask.touches(paddle_b.collision_rect()) {
            self.velocity.x = -self.velocity.x.abs();
            let y_difference = (ball_mask.centre().y - paddle_b.collision_rect().centre().y) / 32;
            self.velocity.y -= y_difference;
            play_sound(mixer, BALL_PADDLE_HIT);
        }

        // We check if the ball reaches the edge of the screen and reverse it's direction
        if potential_ball_pos.y <= num!(0)
            || potential_ball_pos.y >= num!(agb::display::HEIGHT - 16)
        {
            self.velocity.y *= -1;
            play_sound(mixer, WALL_HIT);
        }

        if potential_ball_pos.x <= num!(0) {
            paddle_a.health -= 1;
            self.reset();
        }
        if potential_ball_pos.x >= num!(agb::display::WIDTH - 16) {
            paddle_b.health -= 1;
            self.reset();
        }

        self.pos += self.velocity;
    }
    pub fn reset(&mut self) {
        self.pos = vec2(num!(50), num!(50));
        self.velocity = vec2(num!(2), num!(0.5));
    }

    pub fn show(&self, frame: &mut GraphicsFrame) {
        let pos = self.pos.round();
        Object::new(sprites::BALL.sprite(0))
            .set_pos(pos)
            .set_priority(Priority::P1)
            .show(frame);
    }
}

const P1: bool = true;
const P2: bool = false;

pub struct Paddle<const PLAYER: bool> {
    pos: Vector2D<FixedNum<8>>,
    pub health: u16,
}

impl<const PLAYER: bool> Paddle<PLAYER> {
    pub fn new(start: Vector2D<FixedNum<8>>, health: u16) -> Self {
        Self { pos: start, health }
    }

    pub fn move_by(&mut self, y: FixedNum<8>) {
        self.pos.y = (self.pos.y + y)
            .max(num!(0))
            .min(num!(agb::display::HEIGHT - 48));
    }

    pub fn set_pos(&mut self, pos: Vector2D<FixedNum<8>>) {
        self.pos = pos;
    }
    pub fn collision_rect(&self) -> Rect<FixedNum<8>> {
        let pos = self.pos + vec2(num!(4), num!(4));
        Rect::new(pos, vec2(num!(10), num!(40)))
    }
    fn _update(&mut self, up_pressed: bool, down_pressed: bool) {
        let y_change = match (up_pressed, down_pressed) {
            (true, false) => num!(-2),
            (false, true) => num!(2),
            (false, false) | (true, true) => num!(0),
        };
        self.move_by(y_change);
    }
    fn _show_health(&self, mut from: Vector2D<i32>, frame: &mut GraphicsFrame) {
        for i in 0..3 {
            let heart_frame = if i < self.health.into() { 0 } else { 1 };

            Object::new(sprites::HEART.sprite(heart_frame))
                .set_pos(from)
                .show(frame);

            from.x += 8;
        }
    }
    fn _show(&self, frame: &mut GraphicsFrame, h_flip: bool) {
        let pos = self.pos.round();

        Object::new(sprites::PADDLE_END.sprite(0))
            .set_pos(pos)
            .set_priority(Priority::P1)
            .set_hflip(h_flip)
            .show(frame);
        Object::new(sprites::PADDLE_MID.sprite(0))
            .set_pos(pos + vec2(0, 16))
            .set_priority(Priority::P1)
            .set_hflip(h_flip)
            .show(frame);
        Object::new(sprites::PADDLE_END.sprite(0))
            .set_pos(pos + vec2(0, 32))
            .set_priority(Priority::P1)
            .set_hflip(h_flip)
            .set_vflip(true)
            .show(frame);
    }
}

impl Paddle<P1> {
    pub fn show(&self, frame: &mut GraphicsFrame) {
        self._show(frame, false);
    }
    pub fn show_health(&self, frame: &mut GraphicsFrame) {
        let mut top_left = vec2(3, 4);

        // Display the text `PLayer:`
        for i in 0..4 {
            Object::new(sprites::PLAYER.sprite(i))
                .set_pos(top_left)
                .show(frame);
            top_left.x += 8;
        }

        self._show_health(top_left + vec2(3, 0), frame);
    }
    pub fn update(&mut self, bc: &mut ButtonController) {
        self._update(bc.is_pressed(Button::UP), bc.is_pressed(Button::DOWN));
    }
}

impl Paddle<P2> {
    pub fn show(&self, frame: &mut GraphicsFrame) {
        self._show(frame, true);
    }
    pub fn show_health(&self, frame: &mut GraphicsFrame) {
        let mut top_left = vec2(WIDTH - (8 * 5 + 3 * 2), 4);

        for i in 0..2 {
            Object::new(sprites::CPU.sprite(i))
                .set_pos(top_left)
                .show(frame);
            top_left.x += 8;
        }

        self._show_health(top_left + vec2(3, 0), frame);
    }
    pub fn update(&mut self, bc: &mut ButtonController) {
        self._update(bc.is_pressed(Button::A), bc.is_pressed(Button::B));
    }
}

pub struct GamePlay {
    bg: RegularBackground,
    ball: Ball,
    paddle_a: Paddle<P1>,
    paddle_b: Paddle<P2>,
}

pub enum Game {
    Playing(GamePlay),
    Over(RegularBackground),
}

impl Game {
    pub fn new() -> Self {
        let ball = Ball::new(vec2(num!(50), num!(50)), vec2(num!(2), num!(0.5)));
        let paddle_a = Paddle::new(vec2(num!(8), num!(8)), 3); // left paddle
        let paddle_b = Paddle::new(vec2(num!(240 - 16 - 8), num!(8)), 3); // right paddle

        let mut bg = RegularBackground::new(
            Priority::P3,
            RegularBackgroundSize::Background32x32,
            TileFormat::FourBpp,
        );
        bg.fill_with(&background::PLAY_FIELD);

        Game::Playing(GamePlay {
            bg,
            ball,
            paddle_a,
            paddle_b,
        })
    }
}

// The main function must take 1 arguments and never returns, and must be marked with
// the #[agb::entry] macro.
#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let mut controller = agb::input::ButtonController::new();

    let mut mixer = gba.mixer.mixer(Frequency::Hz32768);
    let mut tracker = Tracker::new(&BGM);

    let mut gfx = gba.graphics.get();
    VRAM_MANAGER.set_background_palettes(&background::PALETTES);

    let mut game = Game::new();

    loop {
        game = match game {
            Game::Playing(mut gp) => {
                controller.update();

                gp.ball
                    .update(&mut gp.paddle_a, &mut gp.paddle_b, &mut mixer);

                gp.paddle_a.update(&mut controller);
                gp.paddle_b.update(&mut controller);

                let mut frame = gfx.frame();

                gp.paddle_a.show(&mut frame);
                gp.paddle_b.show(&mut frame);
                gp.ball.show(&mut frame);

                gp.bg.show(&mut frame);

                gp.paddle_a.show_health(&mut frame);
                gp.paddle_b.show_health(&mut frame);

                tracker.step(&mut mixer);
                mixer.frame();

                frame.commit();

                if gp.paddle_a.health == 0 || gp.paddle_b.health == 0 {
                    let mut bg = RegularBackground::new(
                        Priority::P0,
                        RegularBackgroundSize::Background32x32,
                        TileFormat::FourBpp,
                    );
                    bg.fill_with(&background::GAME_OVER);

                    Game::Over(bg)
                } else {
                    Game::Playing(gp)
                }
            }
            Game::Over(bg) => {
                controller.update();

                let mut frame = gfx.frame();
                bg.show(&mut frame);

                mixer.frame();
                frame.commit();

                if controller.is_pressed(Button::START) {
                    Game::new()
                } else {
                    Game::Over(bg)
                }
            }
        }
    }
}
