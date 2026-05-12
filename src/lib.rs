pub const COLS: usize = 10;
pub const ROWS: usize = 20;
pub const PREVIEW: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::I,
        Kind::O,
        Kind::T,
        Kind::S,
        Kind::Z,
        Kind::J,
        Kind::L,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub kind: Kind,
    pub rotation: usize,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiAction {
    MoveLeft,
    MoveRight,
    SoftDrop,
    RotateCw,
    RotateCcw,
    HardDrop,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiPlan {
    pub use_hold: bool,
    pub target_x: i32,
    pub target_rotation: usize,
    pub score: f32,
}

impl Piece {
    pub fn spawn(kind: Kind) -> Self {
        Self {
            kind,
            rotation: 0,
            x: 4,
            y: -1,
        }
    }

    pub fn cells(self) -> [(i32, i32); 4] {
        shape_cells(self.kind, self.rotation).map(|(x, y)| (self.x + x, self.y + y))
    }
}

#[derive(Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    fn range(&mut self, max: u32) -> u32 {
        self.next_u32() % max.max(1)
    }
}

#[derive(Clone)]
pub struct Game {
    board: [Option<Block>; COLS * ROWS],
    rng: Rng,
    current: Piece,
    hold: Option<Kind>,
    can_hold: bool,
    next: [Kind; PREVIEW],
    score: u32,
    lines: u32,
    pieces: u32,
    paused: bool,
    game_over: bool,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let current = Piece::spawn(random_kind(&mut rng));
        let next = std::array::from_fn(|_| random_kind(&mut rng));

        Self {
            board: [None; COLS * ROWS],
            rng,
            current,
            hold: None,
            can_hold: true,
            next,
            score: 0,
            lines: 0,
            pieces: 0,
            paused: false,
            game_over: false,
        }
    }

    pub fn reset(&mut self, seed: u64) {
        *self = Self::new(seed);
    }

    pub fn board(&self) -> &[Option<Block>; COLS * ROWS] {
        &self.board
    }

    pub fn current(&self) -> Piece {
        self.current
    }

    pub fn hold(&self) -> Option<Kind> {
        self.hold
    }

    pub fn next(&self) -> &[Kind; PREVIEW] {
        &self.next
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn lines(&self) -> u32 {
        self.lines
    }

    pub fn pieces(&self) -> u32 {
        self.pieces
    }

    pub fn level(&self) -> u32 {
        self.lines / 10 + 1
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn is_game_over(&self) -> bool {
        self.game_over
    }

    pub fn can_hold(&self) -> bool {
        self.can_hold
    }

    pub fn toggle_pause(&mut self) {
        if !self.game_over {
            self.paused = !self.paused;
        }
    }

    pub fn drop_interval_seconds(&self) -> f32 {
        let speed = 0.82 - (self.level().saturating_sub(1) as f32 * 0.055);
        speed.max(0.08)
    }

    pub fn block_at(&self, x: usize, y: usize) -> Option<Block> {
        self.board[y * COLS + x]
    }

    pub fn active_at(&self, x: usize, y: usize) -> bool {
        self.current
            .cells()
            .iter()
            .any(|&(px, py)| px == x as i32 && py == y as i32)
    }

    pub fn ghost_piece(&self) -> Piece {
        let mut ghost = self.current;
        while !self.collides(Piece {
            y: ghost.y + 1,
            ..ghost
        }) {
            ghost.y += 1;
        }
        ghost
    }

    pub fn ai_plan(&self) -> Option<AiPlan> {
        best_ai_plan(self)
    }

    pub fn ai_action(&self) -> Option<AiAction> {
        if self.paused || self.game_over {
            return None;
        }

        let plan = self.ai_plan()?;

        if plan.use_hold && self.can_hold {
            return Some(AiAction::Hold);
        }

        let current_rotation = self.current.rotation % 4;
        let target_rotation = plan.target_rotation % 4;
        if current_rotation != target_rotation {
            let clockwise = (target_rotation + 4 - current_rotation) % 4;
            let counter_clockwise = (current_rotation + 4 - target_rotation) % 4;

            return Some(if clockwise <= counter_clockwise {
                AiAction::RotateCw
            } else {
                AiAction::RotateCcw
            });
        }

        if self.current.x < plan.target_x {
            Some(AiAction::MoveRight)
        } else if self.current.x > plan.target_x {
            Some(AiAction::MoveLeft)
        } else {
            Some(AiAction::HardDrop)
        }
    }

    pub fn move_left(&mut self) {
        self.try_move(-1, 0);
    }

    pub fn move_right(&mut self) {
        self.try_move(1, 0);
    }

    pub fn soft_drop(&mut self) {
        if self.paused || self.game_over {
            return;
        }

        if self.try_move(0, 1) {
            self.score += 1;
        } else {
            self.lock_current();
        }
    }

    pub fn hard_drop(&mut self) {
        if self.paused || self.game_over {
            return;
        }

        let mut distance = 0;
        while self.try_move(0, 1) {
            distance += 1;
        }

        self.score += distance * 2;
        self.lock_current();
    }

    pub fn rotate_cw(&mut self) {
        self.rotate(1);
    }

    pub fn rotate_ccw(&mut self) {
        self.rotate(3);
    }

    pub fn hold_piece(&mut self) {
        if self.paused || self.game_over || !self.can_hold {
            return;
        }

        let current_kind = self.current.kind;
        match self.hold {
            Some(held) => {
                self.current = Piece::spawn(held);
                self.hold = Some(current_kind);
                if self.collides(self.current) {
                    self.game_over = true;
                }
            }
            None => {
                self.hold = Some(current_kind);
                self.spawn_next();
            }
        }

        self.can_hold = false;
    }

    pub fn gravity_step(&mut self) {
        if self.paused || self.game_over {
            return;
        }

        if !self.try_move(0, 1) {
            self.lock_current();
        }
    }

    fn try_move(&mut self, dx: i32, dy: i32) -> bool {
        if self.paused || self.game_over {
            return false;
        }

        let moved = Piece {
            x: self.current.x + dx,
            y: self.current.y + dy,
            ..self.current
        };

        if self.collides(moved) {
            false
        } else {
            self.current = moved;
            true
        }
    }

    fn rotate(&mut self, amount: usize) {
        if self.paused || self.game_over {
            return;
        }

        let rotated = Piece {
            rotation: (self.current.rotation + amount) % 4,
            ..self.current
        };

        for kick in [0, -1, 1, -2, 2] {
            let kicked = Piece {
                x: rotated.x + kick,
                ..rotated
            };
            if !self.collides(kicked) {
                self.current = kicked;
                break;
            }
        }
    }

    fn lock_current(&mut self) {
        for (x, y) in self.current.cells() {
            if y < 0 {
                self.game_over = true;
                return;
            }

            self.board[y as usize * COLS + x as usize] = Some(Block {
                kind: self.current.kind,
            });
        }

        self.pieces += 1;
        self.can_hold = true;
        let cleared = self.clear_lines();
        if cleared > 0 {
            self.lines += cleared;
            self.score += line_score(cleared) * self.level();
        }
        self.spawn_next();
    }

    fn spawn_next(&mut self) {
        self.current = Piece::spawn(self.next[0]);
        self.next.rotate_left(1);
        self.next[PREVIEW - 1] = random_kind(&mut self.rng);

        if self.collides(self.current) {
            self.game_over = true;
        }
    }

    fn clear_lines(&mut self) -> u32 {
        let mut write_y = ROWS as i32 - 1;
        let mut cleared = 0;

        for read_y in (0..ROWS).rev() {
            let full = (0..COLS).all(|x| self.board[read_y * COLS + x].is_some());
            if full {
                cleared += 1;
            } else {
                for x in 0..COLS {
                    self.board[write_y as usize * COLS + x] = self.board[read_y * COLS + x];
                }
                write_y -= 1;
            }
        }

        for y in 0..=write_y {
            for x in 0..COLS {
                self.board[y as usize * COLS + x] = None;
            }
        }

        cleared
    }

    fn collides(&self, piece: Piece) -> bool {
        piece.cells().iter().any(|&(x, y)| {
            x < 0
                || x >= COLS as i32
                || y >= ROWS as i32
                || (y >= 0 && self.board[y as usize * COLS + x as usize].is_some())
        })
    }
}

pub fn shape_cells(kind: Kind, rotation: usize) -> [(i32, i32); 4] {
    match kind {
        Kind::I => match rotation % 2 {
            0 => [(-1, 0), (0, 0), (1, 0), (2, 0)],
            _ => [(0, -1), (0, 0), (0, 1), (0, 2)],
        },
        Kind::O => [(0, 0), (1, 0), (0, 1), (1, 1)],
        Kind::T => match rotation % 4 {
            0 => [(-1, 0), (0, 0), (1, 0), (0, 1)],
            1 => [(0, -1), (0, 0), (0, 1), (1, 0)],
            2 => [(-1, 0), (0, 0), (1, 0), (0, -1)],
            _ => [(0, -1), (0, 0), (0, 1), (-1, 0)],
        },
        Kind::S => match rotation % 2 {
            0 => [(0, 0), (1, 0), (-1, 1), (0, 1)],
            _ => [(0, -1), (0, 0), (1, 0), (1, 1)],
        },
        Kind::Z => match rotation % 2 {
            0 => [(-1, 0), (0, 0), (0, 1), (1, 1)],
            _ => [(1, -1), (0, 0), (1, 0), (0, 1)],
        },
        Kind::J => match rotation % 4 {
            0 => [(-1, 0), (0, 0), (1, 0), (-1, 1)],
            1 => [(0, -1), (0, 0), (0, 1), (1, 1)],
            2 => [(-1, 0), (0, 0), (1, 0), (1, -1)],
            _ => [(0, -1), (0, 0), (0, 1), (-1, -1)],
        },
        Kind::L => match rotation % 4 {
            0 => [(-1, 0), (0, 0), (1, 0), (1, 1)],
            1 => [(0, -1), (0, 0), (0, 1), (1, -1)],
            2 => [(-1, 0), (0, 0), (1, 0), (-1, -1)],
            _ => [(0, -1), (0, 0), (0, 1), (-1, 1)],
        },
    }
}

fn random_kind(rng: &mut Rng) -> Kind {
    Kind::ALL[rng.range(Kind::ALL.len() as u32) as usize]
}

fn line_score(cleared: u32) -> u32 {
    match cleared {
        1 => 100,
        2 => 300,
        3 => 500,
        4 => 800,
        _ => 0,
    }
}

fn best_ai_plan(game: &Game) -> Option<AiPlan> {
    let mut best = best_plan_for_kind(
        &game.board,
        game.current.kind,
        game.current.y,
        game.next[0],
        false,
    );

    if game.can_hold {
        let (kind, next_after) = match game.hold {
            Some(held) => (held, game.next[0]),
            None => (game.next[0], game.next[1]),
        };

        if let Some(plan) = best_plan_for_kind(&game.board, kind, -1, next_after, true) {
            if best.is_none_or(|current| plan.score > current.score + 0.35) {
                best = Some(plan);
            }
        }
    }

    best
}

fn best_plan_for_kind(
    board: &[Option<Block>; COLS * ROWS],
    kind: Kind,
    start_y: i32,
    lookahead: Kind,
    use_hold: bool,
) -> Option<AiPlan> {
    let mut best: Option<AiPlan> = None;

    for rotation in 0..4 {
        for x in -3..(COLS as i32 + 3) {
            let piece = Piece {
                kind,
                rotation,
                x,
                y: start_y,
            };
            let Some(landed) = landing_piece(board, piece) else {
                continue;
            };
            let Some((next_board, cleared)) = lock_piece_on_board(board, landed) else {
                continue;
            };

            let board_score = evaluate_board(&next_board, cleared);
            let lookahead_score = best_score_for_kind(&next_board, lookahead).unwrap_or(-500.0);
            let score = board_score + lookahead_score * 0.46 - if use_hold { 0.08 } else { 0.0 };
            let candidate = AiPlan {
                use_hold,
                target_x: landed.x,
                target_rotation: rotation,
                score,
            };

            if best.is_none_or(|plan| candidate.score > plan.score) {
                best = Some(candidate);
            }
        }
    }

    best
}

fn best_score_for_kind(board: &[Option<Block>; COLS * ROWS], kind: Kind) -> Option<f32> {
    let mut best: Option<f32> = None;

    for rotation in 0..4 {
        for x in -3..(COLS as i32 + 3) {
            let piece = Piece {
                kind,
                rotation,
                x,
                y: -1,
            };
            let Some(landed) = landing_piece(board, piece) else {
                continue;
            };
            let Some((next_board, cleared)) = lock_piece_on_board(board, landed) else {
                continue;
            };

            let score = evaluate_board(&next_board, cleared);
            if best.is_none_or(|current| score > current) {
                best = Some(score);
            }
        }
    }

    best
}

fn landing_piece(board: &[Option<Block>; COLS * ROWS], mut piece: Piece) -> Option<Piece> {
    if collides_on_board(board, piece) {
        return None;
    }

    while !collides_on_board(
        board,
        Piece {
            y: piece.y + 1,
            ..piece
        },
    ) {
        piece.y += 1;
    }

    Some(piece)
}

fn lock_piece_on_board(
    board: &[Option<Block>; COLS * ROWS],
    piece: Piece,
) -> Option<([Option<Block>; COLS * ROWS], u32)> {
    let mut next = *board;

    for (x, y) in piece.cells() {
        if y < 0 || x < 0 || x >= COLS as i32 || y >= ROWS as i32 {
            return None;
        }
        next[y as usize * COLS + x as usize] = Some(Block { kind: piece.kind });
    }

    let cleared = clear_lines_on_board(&mut next);
    Some((next, cleared))
}

fn clear_lines_on_board(board: &mut [Option<Block>; COLS * ROWS]) -> u32 {
    let mut write_y = ROWS as i32 - 1;
    let mut cleared = 0;

    for read_y in (0..ROWS).rev() {
        let full = (0..COLS).all(|x| board[read_y * COLS + x].is_some());
        if full {
            cleared += 1;
        } else {
            for x in 0..COLS {
                board[write_y as usize * COLS + x] = board[read_y * COLS + x];
            }
            write_y -= 1;
        }
    }

    for y in 0..=write_y {
        for x in 0..COLS {
            board[y as usize * COLS + x] = None;
        }
    }

    cleared
}

fn collides_on_board(board: &[Option<Block>; COLS * ROWS], piece: Piece) -> bool {
    piece.cells().iter().any(|&(x, y)| {
        x < 0
            || x >= COLS as i32
            || y >= ROWS as i32
            || (y >= 0 && board[y as usize * COLS + x as usize].is_some())
    })
}

fn evaluate_board(board: &[Option<Block>; COLS * ROWS], cleared: u32) -> f32 {
    let heights = column_heights(board);
    let aggregate_height: i32 = heights.iter().sum();
    let max_height = heights.iter().copied().max().unwrap_or(0);
    let holes = count_holes(board);
    let bumpiness = heights
        .windows(2)
        .map(|pair| (pair[0] - pair[1]).abs())
        .sum::<i32>();
    let wells = count_wells(&heights);
    let danger = heights.iter().filter(|&&height| height >= 16).count() as i32;

    cleared as f32 * 12.5
        - aggregate_height as f32 * 0.48
        - holes as f32 * 7.2
        - bumpiness as f32 * 0.38
        - wells as f32 * 0.18
        - max_height as f32 * 0.92
        - danger as f32 * 3.5
}

fn column_heights(board: &[Option<Block>; COLS * ROWS]) -> [i32; COLS] {
    let mut heights = [0; COLS];

    for x in 0..COLS {
        for y in 0..ROWS {
            if board[y * COLS + x].is_some() {
                heights[x] = (ROWS - y) as i32;
                break;
            }
        }
    }

    heights
}

fn count_holes(board: &[Option<Block>; COLS * ROWS]) -> i32 {
    let mut holes = 0;

    for x in 0..COLS {
        let mut covered = false;
        for y in 0..ROWS {
            if board[y * COLS + x].is_some() {
                covered = true;
            } else if covered {
                holes += 1;
            }
        }
    }

    holes
}

fn count_wells(heights: &[i32; COLS]) -> i32 {
    let mut wells = 0;

    for x in 0..COLS {
        let left = if x == 0 { ROWS as i32 } else { heights[x - 1] };
        let right = if x == COLS - 1 {
            ROWS as i32
        } else {
            heights[x + 1]
        };
        let rim = left.min(right);

        if rim > heights[x] {
            wells += rim - heights[x];
        }
    }

    wells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_has_four_cells() {
        for kind in Kind::ALL {
            for rotation in 0..4 {
                assert_eq!(shape_cells(kind, rotation).len(), 4);
            }
        }
    }

    #[test]
    fn hard_drop_locks_a_piece() {
        let mut game = Game::new(1);
        game.hard_drop();

        assert_eq!(game.pieces(), 1);
        assert!(game.board().iter().filter(|cell| cell.is_some()).count() >= 4);
    }

    #[test]
    fn hold_can_only_be_used_once_per_piece() {
        let mut game = Game::new(2);
        let first = game.current().kind;
        game.hold_piece();
        assert_eq!(game.hold(), Some(first));
        let after_first_hold = game.current().kind;
        game.hold_piece();
        assert_eq!(game.current().kind, after_first_hold);
    }

    #[test]
    fn ghost_is_never_above_current_piece() {
        let game = Game::new(3);
        assert!(game.ghost_piece().y >= game.current().y);
    }

    #[test]
    fn ai_can_autoplay_a_short_game() {
        let mut game = Game::new(42);
        let mut steps = 0;

        while game.pieces() < 40 && !game.is_game_over() && steps < 2_000 {
            if let Some(action) = game.ai_action() {
                apply_ai_action(&mut game, action);
            } else {
                game.gravity_step();
            }
            steps += 1;
        }

        assert!(!game.is_game_over());
        assert!(game.pieces() >= 40);
    }

    fn apply_ai_action(game: &mut Game, action: AiAction) {
        match action {
            AiAction::MoveLeft => game.move_left(),
            AiAction::MoveRight => game.move_right(),
            AiAction::SoftDrop => game.soft_drop(),
            AiAction::RotateCw => game.rotate_cw(),
            AiAction::RotateCcw => game.rotate_ccw(),
            AiAction::HardDrop => game.hard_drop(),
            AiAction::Hold => game.hold_piece(),
        }
    }
}
