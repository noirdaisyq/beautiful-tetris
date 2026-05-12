use std::{
    f32::consts::TAU,
    time::{SystemTime, UNIX_EPOCH},
};

use beautiful_tetris::{shape_cells, AiAction, Game, Kind, Piece, COLS, PREVIEW, ROWS};
use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;

struct Layout {
    cell: f32,
    board_x: f32,
    board_y: f32,
    board_w: f32,
    board_h: f32,
    left_x: f32,
    right_x: f32,
    panel_w: f32,
    ui_scale: f32,
}

#[derive(Default)]
struct Repeat {
    left: f64,
    right: f64,
    down: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AiMode {
    #[default]
    Off,
    Fast,
    Natural,
}

impl AiMode {
    fn label(self) -> &'static str {
        match self {
            AiMode::Off => "OFF",
            AiMode::Fast => "FAST",
            AiMode::Natural => "NATURAL",
        }
    }
}

#[derive(Default)]
struct AiController {
    mode: AiMode,
    next_step_at: f64,
}

#[derive(Clone, Copy)]
struct TrailEntry {
    piece: Piece,
    age: f32,
}

struct PieceTrail {
    entries: Vec<TrailEntry>,
    last_piece: Piece,
    last_piece_count: u32,
}

#[derive(Clone, Copy)]
struct Snapshot {
    piece: Piece,
    hold: Option<Kind>,
    lines: u32,
    pieces: u32,
    game_over: bool,
}

struct Sfx {
    move_blip: Option<Sound>,
    rotate: Option<Sound>,
    lock: Option<Sound>,
    clear: Option<Sound>,
    hold: Option<Sound>,
    game_over: Option<Sound>,
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Beautiful Tetris".to_owned(),
        window_width: 1040,
        window_height: 780,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut game = Game::new(seed_now());
    let mut repeat = Repeat::default();
    let mut ai = AiController::default();
    let mut trail = PieceTrail::new(game.current(), game.pieces());
    let mut drop_timer = 0.0f32;
    let sfx = Sfx::load().await;

    loop {
        let dt = get_frame_time().min(0.05);
        handle_input(&mut game, &mut repeat, &sfx, &mut ai);

        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        drive_ai(&mut game, &mut ai, &sfx);

        if !game.is_paused() && !game.is_game_over() {
            drop_timer += dt;
            if drop_timer >= game.drop_interval_seconds() {
                let before = snapshot(&game);
                game.gravity_step();
                play_outcome(before, &game, &sfx, None);
                drop_timer = 0.0;
            }
        }

        trail.update(&game, dt);
        draw_game(&game, &trail, ai.mode);
        next_frame().await;
    }
}

impl Sfx {
    async fn load() -> Self {
        Self {
            move_blip: load_tone(&[(520.0, 0.045)], Wave::Triangle).await,
            rotate: load_tone(&[(680.0, 0.035), (920.0, 0.055)], Wave::Triangle).await,
            lock: load_tone(&[(190.0, 0.055), (120.0, 0.075)], Wave::Square).await,
            clear: load_tone(
                &[(620.0, 0.055), (780.0, 0.055), (1040.0, 0.12)],
                Wave::Sine,
            )
            .await,
            hold: load_tone(&[(340.0, 0.045), (510.0, 0.075)], Wave::Triangle).await,
            game_over: load_tone(&[(320.0, 0.12), (230.0, 0.14), (140.0, 0.18)], Wave::Square)
                .await,
        }
    }

    fn play_move(&self) {
        play_optional(&self.move_blip, 0.35);
    }

    fn play_rotate(&self) {
        play_optional(&self.rotate, 0.42);
    }

    fn play_lock(&self) {
        play_optional(&self.lock, 0.46);
    }

    fn play_clear(&self) {
        play_optional(&self.clear, 0.62);
    }

    fn play_hold(&self) {
        play_optional(&self.hold, 0.46);
    }

    fn play_game_over(&self) {
        play_optional(&self.game_over, 0.58);
    }
}

impl PieceTrail {
    fn new(piece: Piece, piece_count: u32) -> Self {
        Self {
            entries: Vec::new(),
            last_piece: piece,
            last_piece_count: piece_count,
        }
    }

    fn reset(&mut self, game: &Game) {
        self.entries.clear();
        self.last_piece = game.current();
        self.last_piece_count = game.pieces();
    }

    fn update(&mut self, game: &Game, dt: f32) {
        for entry in &mut self.entries {
            entry.age += dt;
        }
        self.entries.retain(|entry| entry.age < 0.42);

        if game.is_game_over()
            || game.pieces() != self.last_piece_count
            || game.current().kind != self.last_piece.kind
            || game.current().y < self.last_piece.y - 3
        {
            self.reset(game);
            return;
        }

        if game.current() != self.last_piece {
            self.entries.insert(
                0,
                TrailEntry {
                    piece: self.last_piece,
                    age: 0.0,
                },
            );
            self.entries.truncate(10);
            self.last_piece = game.current();
        }
    }
}

fn handle_input(game: &mut Game, repeat: &mut Repeat, sfx: &Sfx, ai: &mut AiController) {
    let now = get_time();

    if repeating(&[KeyCode::Left, KeyCode::A], &mut repeat.left, now) {
        apply_action(game, AiAction::MoveLeft, sfx);
    }
    if repeating(&[KeyCode::Right, KeyCode::D], &mut repeat.right, now) {
        apply_action(game, AiAction::MoveRight, sfx);
    }
    if repeating(&[KeyCode::Down, KeyCode::S], &mut repeat.down, now) {
        apply_action(game, AiAction::SoftDrop, sfx);
    }

    if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W) || is_key_pressed(KeyCode::X) {
        apply_action(game, AiAction::RotateCw, sfx);
    }
    if is_key_pressed(KeyCode::Z) {
        apply_action(game, AiAction::RotateCcw, sfx);
    }
    if is_key_pressed(KeyCode::Space) {
        apply_action(game, AiAction::HardDrop, sfx);
    }
    if is_key_pressed(KeyCode::C) || is_key_pressed(KeyCode::LeftShift) {
        apply_action(game, AiAction::Hold, sfx);
    }
    if is_key_pressed(KeyCode::P) {
        game.toggle_pause();
    }
    if is_key_pressed(KeyCode::B) {
        ai.mode = if ai.mode == AiMode::Fast {
            AiMode::Off
        } else {
            AiMode::Fast
        };
        ai.next_step_at = 0.0;
        sfx.play_hold();
    }
    if is_key_pressed(KeyCode::N) {
        ai.mode = if ai.mode == AiMode::Natural {
            AiMode::Off
        } else {
            AiMode::Natural
        };
        ai.next_step_at = 0.0;
        sfx.play_hold();
    }
    if is_key_pressed(KeyCode::R) {
        game.reset(seed_now());
        ai.next_step_at = 0.0;
        sfx.play_hold();
    }
}

fn repeating(keys: &[KeyCode], next_at: &mut f64, now: f64) -> bool {
    let pressed = keys.iter().any(|&key| is_key_pressed(key));
    let held = keys.iter().any(|&key| is_key_down(key));

    if pressed {
        *next_at = now + 0.16;
        true
    } else if held && now >= *next_at {
        *next_at = now + 0.055;
        true
    } else {
        if !held {
            *next_at = 0.0;
        }
        false
    }
}

fn drive_ai(game: &mut Game, ai: &mut AiController, sfx: &Sfx) {
    if ai.mode == AiMode::Off || game.is_paused() || game.is_game_over() {
        return;
    }

    let now = get_time();
    if now < ai.next_step_at {
        return;
    }

    let Some(action) = game.ai_action() else {
        return;
    };

    if ai.mode == AiMode::Natural && matches!(action, AiAction::HardDrop) {
        ai.next_step_at = now + 0.075;
        return;
    }

    apply_action(game, action, sfx);
    ai.next_step_at = now
        + match action {
            AiAction::HardDrop | AiAction::Hold => 0.085,
            AiAction::RotateCw | AiAction::RotateCcw => 0.04,
            AiAction::MoveLeft | AiAction::MoveRight | AiAction::SoftDrop => 0.028,
        };
}

fn apply_action(game: &mut Game, action: AiAction, sfx: &Sfx) {
    let before = snapshot(game);

    match action {
        AiAction::MoveLeft => game.move_left(),
        AiAction::MoveRight => game.move_right(),
        AiAction::SoftDrop => game.soft_drop(),
        AiAction::RotateCw => game.rotate_cw(),
        AiAction::RotateCcw => game.rotate_ccw(),
        AiAction::HardDrop => game.hard_drop(),
        AiAction::Hold => game.hold_piece(),
    }

    play_outcome(before, game, sfx, input_sound_for(action));
}

fn input_sound_for(action: AiAction) -> Option<InputSound> {
    match action {
        AiAction::MoveLeft | AiAction::MoveRight | AiAction::SoftDrop => Some(InputSound::Move),
        AiAction::RotateCw | AiAction::RotateCcw => Some(InputSound::Rotate),
        AiAction::HardDrop => Some(InputSound::Lock),
        AiAction::Hold => Some(InputSound::Hold),
    }
}

#[derive(Clone, Copy)]
enum InputSound {
    Move,
    Rotate,
    Lock,
    Hold,
}

fn snapshot(game: &Game) -> Snapshot {
    Snapshot {
        piece: game.current(),
        hold: game.hold(),
        lines: game.lines(),
        pieces: game.pieces(),
        game_over: game.is_game_over(),
    }
}

fn play_outcome(before: Snapshot, game: &Game, sfx: &Sfx, fallback: Option<InputSound>) {
    if !before.game_over && game.is_game_over() {
        sfx.play_game_over();
        return;
    }

    if game.lines() > before.lines {
        sfx.play_clear();
        return;
    }

    if game.pieces() > before.pieces {
        sfx.play_lock();
        return;
    }

    let moved = game.current().x != before.piece.x || game.current().y != before.piece.y;
    let rotated = game.current().rotation != before.piece.rotation;
    let held = game.hold() != before.hold;

    match fallback {
        Some(InputSound::Move) if moved => sfx.play_move(),
        Some(InputSound::Rotate) if rotated || moved => sfx.play_rotate(),
        Some(InputSound::Hold) if held => sfx.play_hold(),
        Some(InputSound::Lock) => {}
        _ => {}
    }
}

#[derive(Clone, Copy)]
enum Wave {
    Sine,
    Square,
    Triangle,
}

async fn load_tone(notes: &[(f32, f32)], wave: Wave) -> Option<Sound> {
    let bytes = synth_wav(notes, wave);
    load_sound_from_bytes(&bytes).await.ok()
}

fn play_optional(sound: &Option<Sound>, volume: f32) {
    if let Some(sound) = sound {
        play_sound(
            sound,
            PlaySoundParams {
                looped: false,
                volume,
            },
        );
    }
}

fn synth_wav(notes: &[(f32, f32)], wave: Wave) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 44_100;
    let mut samples = Vec::new();

    for &(freq, seconds) in notes {
        let count = (seconds * SAMPLE_RATE as f32) as usize;
        for i in 0..count {
            let t = i as f32 / SAMPLE_RATE as f32;
            let progress = i as f32 / count.max(1) as f32;
            let envelope = attack_release(progress);
            let phase = (freq * t).fract();
            let raw = match wave {
                Wave::Sine => (TAU * phase).sin(),
                Wave::Square => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
            };

            let sample = (raw * envelope * 0.32 * i16::MAX as f32) as i16;
            samples.push(sample);
        }
    }

    wav_from_i16(&samples, SAMPLE_RATE)
}

fn attack_release(progress: f32) -> f32 {
    let attack = (progress / 0.08).clamp(0.0, 1.0);
    let release = ((1.0 - progress) / 0.22).clamp(0.0, 1.0);
    attack.min(release)
}

fn wav_from_i16(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);

    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());

    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }

    bytes
}

fn draw_game(game: &Game, trail: &PieceTrail, ai_mode: AiMode) {
    draw_background();

    let layout = layout();
    draw_title(&layout);
    draw_board(game, trail, &layout);
    draw_side_panels(game, &layout, ai_mode);

    if game.is_paused() {
        draw_center_message(&layout, "PAUSED", "press P to resume");
    } else if game.is_game_over() {
        draw_center_message(&layout, "GAME OVER", "press R to restart");
    }
}

fn layout() -> Layout {
    let sw = screen_width().max(520.0);
    let sh = screen_height().max(520.0);
    let cell = ((sw - 390.0) / COLS as f32)
        .min((sh - 132.0) / ROWS as f32)
        .clamp(14.0, 34.0)
        .floor();
    let board_w = cell * COLS as f32;
    let board_h = cell * ROWS as f32;
    let board_x = (sw - board_w) * 0.5;
    let board_y = (sh - board_h) * 0.5 + 22.0;
    let side_gap = 24.0;
    let available_side = ((sw - board_w) * 0.5 - side_gap * 1.5).max(116.0);
    let panel_w = available_side.min(190.0);
    let left_x = board_x - side_gap - panel_w;
    let right_x = board_x + board_w + side_gap;
    let ui_scale = (cell / 28.0).clamp(0.72, 1.18);

    Layout {
        cell,
        board_x,
        board_y,
        board_w,
        board_h,
        left_x,
        right_x,
        panel_w,
        ui_scale,
    }
}

fn draw_background() {
    let sw = screen_width();
    let sh = screen_height();
    let stripes = 64;

    for index in 0..stripes {
        let t = index as f32 / (stripes - 1) as f32;
        let color = Color::new(0.018 + t * 0.052, 0.02 + t * 0.022, 0.038 + t * 0.05, 1.0);
        draw_rectangle(0.0, sh * t, sw, sh / stripes as f32 + 1.0, color);
    }

    let drift = get_time() as f32;
    for i in 0..42 {
        let x = i as f32 * 48.0 + (drift * 10.0) % 48.0;
        let alpha = if i % 3 == 0 { 0.18 } else { 0.08 };
        draw_line(
            x,
            0.0,
            x - 240.0,
            sh,
            1.0,
            Color::new(0.32, 0.25, 0.48, alpha),
        );
    }

    for i in 0..18 {
        let t = i as f32;
        let x = (t * 173.0 + drift * 11.0).rem_euclid(sw + 80.0) - 40.0;
        let y = (t * 97.0 + (drift * 0.7).sin() * 16.0).rem_euclid(sh + 70.0) - 35.0;
        let radius = 1.4 + (i % 4) as f32 * 0.65;
        draw_circle(x, y, radius, Color::new(0.8, 0.72, 0.42, 0.16));
    }
}

fn draw_title(layout: &Layout) {
    let title = "BEAUTIFUL TETRIS";
    let font_size = 32.0 * layout.ui_scale;
    let dims = measure_text(title, None, font_size as u16, 1.0);
    let x = screen_width() * 0.5 - dims.width * 0.5;
    let y = layout.board_y - 28.0;

    for offset in [6.0, 3.0] {
        draw_text_ex(
            title,
            x,
            y,
            TextParams {
                font_size: (font_size + offset) as u16,
                color: Color::new(0.95, 0.65, 0.22, 0.08),
                ..Default::default()
            },
        );
    }
    draw_text_ex(
        title,
        x,
        y,
        TextParams {
            font_size: font_size as u16,
            color: Color::new(1.0, 0.86, 0.5, 1.0),
            ..Default::default()
        },
    );
}

fn draw_board(game: &Game, trail: &PieceTrail, layout: &Layout) {
    glow_rect(
        layout.board_x - 16.0,
        layout.board_y - 16.0,
        layout.board_w + 32.0,
        layout.board_h + 32.0,
        kind_color(game.current().kind),
        0.11,
    );
    panel(
        layout.board_x - 10.0,
        layout.board_y - 10.0,
        layout.board_w + 20.0,
        layout.board_h + 20.0,
    );

    draw_rectangle(
        layout.board_x,
        layout.board_y,
        layout.board_w,
        layout.board_h,
        Color::new(0.02, 0.021, 0.032, 0.92),
    );

    draw_rectangle(
        layout.board_x,
        layout.board_y,
        layout.board_w,
        layout.board_h,
        Color::new(0.07, 0.08, 0.12, 0.2),
    );

    for y in 0..ROWS {
        for x in 0..COLS {
            let px = layout.board_x + x as f32 * layout.cell;
            let py = layout.board_y + y as f32 * layout.cell;
            if (x + y) % 2 == 0 {
                draw_rectangle(
                    px,
                    py,
                    layout.cell,
                    layout.cell,
                    Color::new(0.12, 0.15, 0.2, 0.08),
                );
            }
            draw_rectangle_lines(
                px,
                py,
                layout.cell,
                layout.cell,
                1.0,
                Color::new(0.18, 0.2, 0.25, 0.32),
            );

            if let Some(block) = game.block_at(x, y) {
                draw_block(px, py, layout.cell, kind_color(block.kind), 1.0);
            }
        }
    }

    draw_trail(game, trail, layout);

    let ghost = game.ghost_piece();
    if ghost != game.current() && !game.is_game_over() {
        for (x, y) in ghost.cells() {
            if y >= 0 {
                let px = layout.board_x + x as f32 * layout.cell;
                let py = layout.board_y + y as f32 * layout.cell;
                draw_rectangle_lines(
                    px + 4.0,
                    py + 4.0,
                    layout.cell - 8.0,
                    layout.cell - 8.0,
                    2.0,
                    fade(kind_color(ghost.kind), 0.42),
                );
            }
        }
    }

    if !game.is_game_over() {
        for (x, y) in game.current().cells() {
            if y >= 0 {
                let px = layout.board_x + x as f32 * layout.cell;
                let py = layout.board_y + y as f32 * layout.cell;
                draw_block(px, py, layout.cell, kind_color(game.current().kind), 1.0);
            }
        }
    }
}

fn draw_side_panels(game: &Game, layout: &Layout, ai_mode: AiMode) {
    let panel_h = layout.board_h;
    panel(layout.left_x, layout.board_y, layout.panel_w, panel_h);
    panel(layout.right_x, layout.board_y, layout.panel_w, panel_h);

    draw_label(
        "HOLD",
        layout.left_x + 18.0,
        layout.board_y + 34.0,
        layout.ui_scale,
    );
    if let Some(kind) = game.hold() {
        draw_piece_preview(
            kind,
            layout.left_x + layout.panel_w * 0.5,
            layout.board_y + 86.0,
            layout.cell * 0.55,
            if game.can_hold() { 1.0 } else { 0.38 },
        );
    }

    draw_badge(
        "BOT MODE",
        ai_mode.label(),
        layout.left_x + 16.0,
        layout.board_y + layout.board_h - 286.0,
        layout.panel_w - 32.0,
        ai_mode,
        layout.ui_scale,
    );
    draw_metric(
        "SCORE",
        &game.score().to_string(),
        layout.left_x + 18.0,
        layout.board_y + layout.board_h - 190.0,
        layout.ui_scale,
    );
    draw_metric(
        "LINES",
        &game.lines().to_string(),
        layout.left_x + 18.0,
        layout.board_y + layout.board_h - 122.0,
        layout.ui_scale,
    );
    draw_metric(
        "LEVEL",
        &game.level().to_string(),
        layout.left_x + 18.0,
        layout.board_y + layout.board_h - 54.0,
        layout.ui_scale,
    );

    draw_label(
        "NEXT",
        layout.right_x + 18.0,
        layout.board_y + 34.0,
        layout.ui_scale,
    );
    for index in 0..PREVIEW {
        draw_piece_preview(
            game.next()[index],
            layout.right_x + layout.panel_w * 0.5,
            layout.board_y + 84.0 + index as f32 * layout.cell * 2.25,
            layout.cell * 0.44,
            1.0 - index as f32 * 0.08,
        );
    }
}

fn panel(x: f32, y: f32, w: f32, h: f32) {
    draw_rectangle(x + 8.0, y + 10.0, w, h, Color::new(0.0, 0.0, 0.0, 0.25));
    draw_rectangle(x, y, w, h, Color::new(0.055, 0.052, 0.078, 0.86));
    draw_rectangle(x, y, w, 3.0, Color::new(0.98, 0.75, 0.32, 0.5));
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.78, 0.62, 0.34, 0.28));
    draw_rectangle_lines(
        x + 4.0,
        y + 4.0,
        w - 8.0,
        h - 8.0,
        1.0,
        Color::new(0.24, 0.32, 0.38, 0.22),
    );
}

fn glow_rect(x: f32, y: f32, w: f32, h: f32, color: Color, alpha: f32) {
    for layer in 0..5 {
        let spread = 7.0 + layer as f32 * 8.0;
        let layer_alpha = alpha / (layer as f32 + 1.4);
        draw_rectangle(
            x - spread,
            y - spread,
            w + spread * 2.0,
            h + spread * 2.0,
            fade(color, layer_alpha),
        );
    }
}

fn draw_trail(game: &Game, trail: &PieceTrail, layout: &Layout) {
    for entry in trail.entries.iter().rev() {
        let life = (1.0 - entry.age / 0.42).clamp(0.0, 1.0);
        let color = kind_color(entry.piece.kind);
        for (x, y) in entry.piece.cells() {
            if x < 0 || x >= COLS as i32 || y < 0 || y >= ROWS as i32 {
                continue;
            }

            let ux = x as usize;
            let uy = y as usize;
            if game.block_at(ux, uy).is_some() || game.active_at(ux, uy) {
                continue;
            }

            let px = layout.board_x + x as f32 * layout.cell;
            let py = layout.board_y + y as f32 * layout.cell;
            draw_block(px, py, layout.cell, color, 0.08 + life * 0.24);
        }
    }
}

fn draw_badge(label: &str, value: &str, x: f32, y: f32, w: f32, ai_mode: AiMode, scale: f32) {
    let h = 54.0 * scale;
    let accent = match ai_mode {
        AiMode::Off => Color::new(0.38, 0.42, 0.5, 1.0),
        AiMode::Fast => Color::new(1.0, 0.58, 0.24, 1.0),
        AiMode::Natural => Color::new(0.35, 0.9, 1.0, 1.0),
    };

    glow_rect(x + 4.0, y + 4.0, w - 8.0, h - 8.0, accent, 0.055);
    draw_rectangle(x, y, w, h, Color::new(0.025, 0.03, 0.045, 0.7));
    draw_rectangle(x, y, 4.0 * scale, h, fade(accent, 0.86));
    draw_rectangle_lines(x, y, w, h, 1.0, fade(accent, 0.35));
    draw_text_ex(
        label,
        x + 14.0 * scale,
        y + 20.0 * scale,
        TextParams {
            font_size: (12.0 * scale) as u16,
            color: Color::new(0.62, 0.66, 0.72, 1.0),
            ..Default::default()
        },
    );
    draw_text_ex(
        value,
        x + 14.0 * scale,
        y + 43.0 * scale,
        TextParams {
            font_size: (20.0 * scale) as u16,
            color: fade(lighten(accent, 0.22), 1.0),
            ..Default::default()
        },
    );
}

fn draw_label(label: &str, x: f32, y: f32, scale: f32) {
    draw_text_ex(
        label,
        x,
        y,
        TextParams {
            font_size: (18.0 * scale) as u16,
            color: Color::new(0.72, 0.9, 0.98, 0.92),
            ..Default::default()
        },
    );
}

fn draw_metric(label: &str, value: &str, x: f32, y: f32, scale: f32) {
    draw_text_ex(
        label,
        x,
        y,
        TextParams {
            font_size: (14.0 * scale) as u16,
            color: Color::new(0.62, 0.66, 0.72, 1.0),
            ..Default::default()
        },
    );
    draw_text_ex(
        value,
        x,
        y + 30.0 * scale,
        TextParams {
            font_size: (27.0 * scale) as u16,
            color: Color::new(0.96, 0.9, 0.72, 1.0),
            ..Default::default()
        },
    );
}

fn draw_piece_preview(kind: Kind, center_x: f32, center_y: f32, cell: f32, alpha: f32) {
    let cells = shape_cells(kind, 0);
    let min_x = cells.iter().map(|(x, _)| *x).min().unwrap_or(0);
    let max_x = cells.iter().map(|(x, _)| *x).max().unwrap_or(0);
    let min_y = cells.iter().map(|(_, y)| *y).min().unwrap_or(0);
    let max_y = cells.iter().map(|(_, y)| *y).max().unwrap_or(0);
    let w = (max_x - min_x + 1) as f32 * cell;
    let h = (max_y - min_y + 1) as f32 * cell;
    let start_x = center_x - w * 0.5;
    let start_y = center_y - h * 0.5;

    for (x, y) in cells {
        draw_block(
            start_x + (x - min_x) as f32 * cell,
            start_y + (y - min_y) as f32 * cell,
            cell,
            kind_color(kind),
            alpha,
        );
    }
}

fn draw_block(x: f32, y: f32, size: f32, color: Color, alpha: f32) {
    let pad = (size * 0.09).max(2.0);
    let x = x + pad;
    let y = y + pad;
    let size = size - pad * 2.0;
    let base = fade(color, alpha);

    draw_rectangle(
        x - pad * 0.8,
        y - pad * 0.8,
        size + pad * 1.6,
        size + pad * 1.6,
        fade(color, alpha * 0.12),
    );
    draw_rectangle(x, y, size, size, base);
    draw_rectangle(x, y, size, size * 0.28, fade(lighten(color, 0.28), alpha));
    draw_rectangle_lines(x, y, size, size, 2.0, fade(lighten(color, 0.45), alpha));
    draw_rectangle(
        x + size * 0.68,
        y + size * 0.14,
        size * 0.16,
        size * 0.16,
        fade(WHITE, alpha * 0.35),
    );
}

fn draw_center_message(layout: &Layout, title: &str, subtitle: &str) {
    let w = layout.board_w * 0.86;
    let h = layout.cell * 4.2;
    let x = layout.board_x + (layout.board_w - w) * 0.5;
    let y = layout.board_y + (layout.board_h - h) * 0.5;

    draw_rectangle(x, y, w, h, Color::new(0.03, 0.028, 0.042, 0.88));
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.96, 0.76, 0.38, 0.55));

    let title_size = (34.0 * layout.ui_scale) as u16;
    let subtitle_size = (16.0 * layout.ui_scale) as u16;
    let title_dims = measure_text(title, None, title_size, 1.0);
    let subtitle_dims = measure_text(subtitle, None, subtitle_size, 1.0);

    draw_text_ex(
        title,
        x + w * 0.5 - title_dims.width * 0.5,
        y + h * 0.48,
        TextParams {
            font_size: title_size,
            color: Color::new(0.98, 0.83, 0.5, 1.0),
            ..Default::default()
        },
    );
    draw_text_ex(
        subtitle,
        x + w * 0.5 - subtitle_dims.width * 0.5,
        y + h * 0.68,
        TextParams {
            font_size: subtitle_size,
            color: Color::new(0.72, 0.9, 0.98, 0.92),
            ..Default::default()
        },
    );
}

fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::I => Color::new(0.35, 0.9, 1.0, 1.0),
        Kind::O => Color::new(1.0, 0.84, 0.25, 1.0),
        Kind::T => Color::new(0.74, 0.45, 1.0, 1.0),
        Kind::S => Color::new(0.38, 0.95, 0.46, 1.0),
        Kind::Z => Color::new(1.0, 0.28, 0.36, 1.0),
        Kind::J => Color::new(0.32, 0.5, 1.0, 1.0),
        Kind::L => Color::new(1.0, 0.58, 0.24, 1.0),
    }
}

fn lighten(color: Color, amount: f32) -> Color {
    Color::new(
        (color.r + amount).min(1.0),
        (color.g + amount).min(1.0),
        (color.b + amount).min(1.0),
        color.a,
    )
}

fn fade(color: Color, alpha: f32) -> Color {
    Color::new(color.r, color.g, color.b, color.a * alpha)
}

fn seed_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0xB7_E7_15)
}
