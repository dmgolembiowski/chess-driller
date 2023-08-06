use crate::prelude::*;
use anyhow::{anyhow, bail};
use chess::{Board, ChessMove};
use sdl2::image::InitFlag;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

pub mod clients;
pub mod config;
pub mod db;
pub mod events;
pub mod game;
pub mod render;

pub mod prelude {
    pub use crate::clients::chess_com::*;
    pub use crate::config::*;
    pub use crate::db::*;
    pub use crate::events::*;
    pub use crate::render::*;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct App {
    chess_com_usernames: Vec<String>,
    last_db: Option<PathBuf>,
}

pub fn run() -> anyhow::Result<()> {
    let config = Config::load()?;
    let chess_dot_com = ChessComClient::new();
    let _user_games = chess_dot_com.download_all_games(&config);
    let database = OpeningDatabase::load_default()?;
    let ctx = sdl2::init().map_err(|e| anyhow!(e))?;
    let width = 600;
    let video = ctx.video().map_err(|e| anyhow!(e))?;

    let _image_context = sdl2::image::init(InitFlag::PNG).map_err(|e| anyhow!(e))?;

    let window = match video
        .window("Chess-driller", width, width)
        .position_centered()
        .opengl()
        .build()
    {
        Ok(window) => window,
        Err(err) => bail!("failed to create window: {}", err),
    };

    let mut canvas = window.into_canvas().software().build()?;
    let texture_creator = canvas.texture_creator();

    let mut window = RenderSystem::new(false, width, &mut canvas, &texture_creator)?;
    let mut events = EventSystem::new(ctx)?;
    let mut running = true;

    let mut board = Board::default();
    // Just putting here to decide if we want to store the openings as a graph of `Board` because
    // that might be fast and simple :thinking:
    info!("Board is: {} bytes in memory", std::mem::size_of::<Board>());

    // Without changing the graph structure we need to start tracking the moves from the very
    // beginning for both white and black - so we'll have a node-index into both.

    let mut selected_square = None;
    let mut san_moves = vec![];
    let mut game_state: Option<GameState> = None;
    let mut drag_context = None;
    let mut pending_promotion_square = None;
    while running {
        window.render(&board, selected_square, drag_context);

        if let Some(square) = pending_promotion_square {
            window.render_promotion_picker(square);
        }

        let pending_events = events.handle_events();

        for event in &pending_events {
            match event.kind {
                EventKind::Close => {
                    info!("Closing");
                    running = false;
                }
                EventKind::FlipBoard => {
                    window.flip();
                }
                EventKind::Reset => {
                    san_moves.clear();
                    game_state = None;
                    board = Board::default();
                }
                EventKind::MouseClick { x, y } => {
                    if let Some(square) = window.get_square(x, y) {
                        if let Some(s) = selected_square {
                            let candidate_move = ChessMove::new(s, square, None);
                            if board.legal(candidate_move) {
                                if let Some(san) = game::get_san(candidate_move, &board) {
                                    info!("{}", san);
                                    board = board.make_move_new(candidate_move);
                                    if let Some(state) = game_state.as_mut() {
                                        let prep_status = state.apply_move(&san);
                                        if prep_status == MoveAssessment::InPrep {
                                            if let Some(mv) = state.make_move() {
                                                let text = mv.to_string();
                                                info!("{}", text);
                                                board = board.make_move_new(
                                                    ChessMove::from_san(&board, &text).unwrap(),
                                                );
                                            }
                                        } else {
                                            info!("You've hit the end: {:?}", prep_status);
                                        }
                                    } else {
                                        san_moves.push(san);
                                    }
                                } else {
                                    info!("Something went wrong didn't record this move");
                                }
                                selected_square = None;
                            } else {
                                selected_square = Some(square);
                            }
                        } else if board.piece_on(square).is_some() {
                            selected_square = Some(square);
                        }
                    }
                }
                EventKind::StartPractising => {
                    if let Some(state) = game_state.as_ref() {
                        if state.still_running() {
                            continue;
                        }
                        board = Board::default();
                    }
                    game_state = None;
                    info!("Lets start playing!");
                    game_state = database.start_drill(window.player(), &san_moves);
                    if let Some(state) = game_state.as_mut() {
                        if !state.is_player_turn() {
                            info!("Not the players turn, lets make a move");
                            if let Some(mv) = state.make_move() {
                                info!("I made a move?");
                                let text = mv.to_string();
                                info!("{}", text);
                                board = board
                                    .make_move_new(ChessMove::from_san(&board, &text).unwrap());
                            }
                        }
                    }
                }
                EventKind::MouseDragBegin { x, y } => {
                    drag_context = Some(DragContext {
                        current_x: x,
                        current_y: y,
                    });
                    if let Some(square) = window.get_square(x, y) {
                        if board.piece_on(square).is_some() {
                            selected_square = Some(square);
                        }
                    }
                }
                EventKind::MouseDragMove { x, y } => {
                    drag_context = Some(DragContext {
                        current_x: x,
                        current_y: y,
                    });
                }
                EventKind::MouseDragEnd { x, y } => {
                    if let Some(dst_square) = window.get_square(x, y) {
                        if let Some(src_square) = selected_square {
                            let rank = dst_square.get_rank().to_index();
                            let promotion = match board.piece_on(src_square) {
                                Some(chess::Piece::Pawn) if rank == 0 || rank == 7 => {
                                    Some(chess::Piece::Queen)
                                }
                                _ => None,
                            };
                            let candidate_move = ChessMove::new(src_square, dst_square, promotion);
                            if board.legal(candidate_move) {
                                board = board.make_move_new(candidate_move);
                                selected_square = None;
                            } else {
                                selected_square = None;
                            }
                        }
                    }

                    drag_context = None;
                }
                // TODO: long click mouse up mouse down?
                _ => {}
            }
        }
    }

    std::mem::drop(window);

    Ok(())
}
