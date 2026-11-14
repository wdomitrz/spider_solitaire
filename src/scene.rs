//! The game as the board draws it: the scene and where the keyboard starts.
//!
//! No `web-sys` here, on purpose. Building a scene is pure — a `Game` in, a
//! [`Scene`] out — and it is where the board's *arrangement* is decided: which
//! pile owns how many cells, how its cards fan, what it says to a screen reader.
//! Compiled for the host, so `cargo test` checks the arrangement rather than
//! only the arithmetic behind it.
//!
//! The app's `ui.rs` is the browser half; this is the half that can be checked.

use crate::board::{Fan, Place, Scene, Slot};
use crate::game::{Game, Hint, Pile, COLUMNS};

/// How many column cells the top row holds.
///
/// One: the stock. Nothing in this game fans in the top row, so every other
/// cell of that row is free for the strip of completed runs, which this game
/// draws there — see `decorate` in `ui.rs`.
pub const TOP_CELLS: usize = 1;

/// The game as the board draws it.
#[must_use]
pub fn scene(game: &Game, hint: Option<&Hint>) -> Scene {
    let mut slots = Vec::with_capacity(Pile::ALL.len());
    for pile in Pile::ALL {
        let cards: Vec<(u16, bool)> = game
            .pile(pile)
            .iter()
            .enumerate()
            .map(|(index, &id)| (id, game.is_face_down(pile, index)))
            .collect();
        let mut slot = Slot::new(pile.place(), fan_for(pile), game.pile_label(pile))
            .with_mark(game.slot_mark(pile))
            .with_cards(&cards);
        if pile == Pile::Stock {
            slot = slot.pressable();
        }
        slots.push(slot);
    }
    let mut scene = Scene::new(slots, TOP_CELLS);
    scene.held = game.selection().map(|held| (held.pile.place(), held.index));
    scene.hint = hint.and_then(|hint| match hint {
        Hint::Move {
            from, index, count, ..
        } => Some((from.place(), *index, *count)),
        Hint::Deal { .. } => Some((
            Pile::Stock.place(),
            game.pile_len(Pile::Stock).saturating_sub(1),
            1,
        )),
        Hint::None { .. } => None,
    });
    scene
}

/// How a pile is drawn.
///
/// A column fans downwards; the stock is a packet, because its cards are never
/// distinguished one from another — the count on it says how many deals are
/// left, and the top card is the only one that matters.
#[must_use]
pub fn fan_for(pile: Pile) -> Fan {
    match pile {
        Pile::Stock => Fan::PACKET,
        Pile::Tableau(_) => Fan::COLUMN,
    }
}

/// Where the keyboard should be: the stock while it has cards, else the first
/// column with a card that can be picked up.
#[must_use]
pub fn default_focus(game: &Game) -> (Place, Option<usize>) {
    if game.pile_len(Pile::Stock) > 0 {
        return (Pile::Stock.place(), Some(game.pile_len(Pile::Stock) - 1));
    }
    for column in 0..COLUMNS {
        let pile = Pile::Tableau(column);
        let len = game.pile_len(pile);
        if let Some(index) = (0..len)
            .rev()
            .find(|&index| !game.is_face_down(pile, index))
        {
            return (pile.place(), Some(index));
        }
    }
    (Pile::Tableau(0).place(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Frame;
    use crate::game::{Difficulty, Scoring};

    #[test]
    fn the_board_is_one_top_slot_and_ten_columns() {
        let game = Game::new(Difficulty::Medium, Scoring::Standard, 2);
        let board = scene(&game, None);
        assert_eq!(board.columns(), COLUMNS);
        assert_eq!(
            board
                .slots
                .iter()
                .filter(|slot| matches!(slot.place, Place::Top(_)))
                .count(),
            1,
            "Spider's top row is the stock alone"
        );
    }

    /// Nothing in this game fans sideways, so every pile takes one cell — and a
    /// column's fan is vertical, which the board clamps to the room below.
    #[test]
    fn no_pile_asks_for_more_than_one_cell() {
        let game = Game::new(Difficulty::Hard, Scoring::Standard, 3);
        let board = scene(&game, None);
        for slot in &board.slots {
            assert_eq!(slot.cells(), 1, "{:?}", slot.place);
        }
    }

    /// Every card the game holds is on the board exactly once, and every card
    /// the board draws is one the scene placed.
    #[test]
    fn the_scene_accounts_for_the_whole_deck() {
        let game = Game::new(Difficulty::Medium, Scoring::Standard, 4);
        let board = scene(&game, None);
        let placed: usize = board.slots.iter().map(Slot::len).sum();
        assert_eq!(placed, game.deck_len(), "every card is in a pile");
        let layout = crate::board::layout(
            &board,
            &Frame {
                width: 1280.0,
                height: 800.0,
                gap: 8.0,
                margin: 12.0,
            },
        );
        assert_eq!(layout.cards.len(), game.deck_len(), "every card is placed");
    }

    #[test]
    fn a_stock_is_a_packet_and_a_column_fans() {
        assert!(fan_for(Pile::Stock).stacked);
        assert!(!fan_for(Pile::Tableau(0)).stacked);
        assert_eq!(fan_for(Pile::Tableau(0)).fan_tail, 0);
    }

    #[test]
    fn the_keyboard_starts_on_the_stock_while_there_is_one() {
        let game = Game::new(Difficulty::Medium, Scoring::Standard, 5);
        assert_eq!(default_focus(&game).0, Pile::Stock.place());
    }
}
