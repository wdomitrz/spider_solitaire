//! Spider, as the shared page sees it.
//!
//! The board is [`crate::board_ui`] and the page around it is [`crate::shell`];
//! both are carried byte for byte by Solitaire. What is left here is the only
//! thing that is Spider's own: how a [`Scene`] is built from this game, what
//! each [`Intent`] means in its rules, and the two decorations this board has —
//! the strip of completed runs and the count on the stock.
//!
//! No decision lives here. Every rule — what a run is, what completes one, what
//! the page says — is in [`crate::game`], which is where the tests are.

use wasm_bindgen::prelude::*;

use crate::board::{Place, Scene};
use crate::board_ui::{BoardUi, Intent};
use crate::game::deck as spider_deck;
use crate::game::{
    Difficulty, Game, Hint, Options, Pile, Report, Scoring, COLUMNS, OPTIONS_KEY, RUNS, STORAGE_KEY,
};
use crate::shell::{fresh_seed, Repeat, Rules};

/// Start the app.
///
/// # Errors
///
/// Fails only when there is no window, no document, or no board in the shell.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let board = document
        .get_element_by_id("board")
        .ok_or_else(|| JsValue::from_str("no #board"))?;
    crate::shell::Shell::start(&window, &document, board, Spider::default())
}

/// The game, plus the two things the page keeps beside it: the next deal's
/// options and the hint being shown.
struct Spider {
    game: Game,
    options: Options,
    hint: Option<Hint>,
    /// What the game last said, waiting to be shown.
    words: Option<String>,
}

impl Default for Spider {
    fn default() -> Self {
        Self {
            game: Game::new(Options::default().difficulty, Options::default().scoring, 0),
            options: Options::default(),
            hint: None,
            words: None,
        }
    }
}

impl Spider {
    /// Answer one gesture with the game's rules.
    fn answer(&mut self, intent: Intent) -> Report {
        match intent {
            Intent::Tap { place, index } => self.tap(place, index),
            Intent::DoubleTap { place, index } => self.double_tap(place, index),
            Intent::Drop { from, index, to } => self.drop(from, index, to),
            Intent::Back => self.game.clear_selection(),
            Intent::Cancelled => Report::quiet(""),
        }
    }

    fn tap(&mut self, place: Place, index: Option<usize>) -> Report {
        let Some(pile) = Pile::from_place(place) else {
            return Report::quiet("");
        };
        if pile == Pile::Stock {
            return self.game.deal();
        }
        match index {
            Some(index) => self.game.tap(pile, index),
            None if self.game.selection().is_some() => self.game.drop(pile),
            None => Report::quiet(""),
        }
    }

    fn double_tap(&mut self, place: Place, index: Option<usize>) -> Report {
        let (Some(pile), Some(index)) = (Pile::from_place(place), index) else {
            return Report::quiet("");
        };
        self.game.double_tap(pile, index)
    }

    /// A drag: take the run up in the rules, then put it down where the player
    /// did.
    ///
    /// The two steps are separate because they are separate questions — "may
    /// this run move" and "does this column take it" — and the game answers each
    /// with the rule a tap uses, so a drag and a tap can never disagree about
    /// what is legal.
    fn drop(&mut self, from: Place, index: usize, to: Place) -> Report {
        let (Some(from), Some(to)) = (Pile::from_place(from), Pile::from_place(to)) else {
            return Report::quiet("");
        };
        self.game.select(from, index);
        self.game.drop(to)
    }

    /// Keep whatever the game said, for the shell to put under the board.
    fn show(&mut self, report: Report) {
        if !report.words.is_empty() {
            self.words = Some(report.words);
        }
    }

    /// Paint the strip of completed runs.
    ///
    /// One label and eight pips: a filled pip is a run already lifted, and it
    /// keeps the suit it was, so a player can see at a glance which ones are
    /// gone. The strip is one labelled image to a screen reader rather than
    /// eight unlabelled boxes.
    fn paint_runs(&self, board: &BoardUi) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(runs) = document.get_element_by_id("runs") else {
            return;
        };
        // The strip sits in the top row, beside the stock: from the second
        // column's x to the last column's right edge. It is anchored to the
        // *columns* rather than to top slots, because this game has only one of
        // those — the stock — and the rest of the row is free.
        // x from the columns, y from the top row: the strip lives in the top
        // row, spanning the columns the board does not use for anything else.
        let (Some((left, _)), Some((last, _)), Some((_, top))) = (
            board.layout().place(Place::Column(1)),
            board.layout().place(Place::Column(COLUMNS - 1)),
            board.layout().place(Place::Top(0)),
        ) else {
            return;
        };
        let width = (last - left) + board.layout().card_w;
        let _ = runs.set_attribute(
            "style",
            &format!(
                "left:{left}px;top:{top}px;width:{width}px;height:{}px",
                board.layout().card_h
            ),
        );
        if let Some(label) = document.get_element_by_id("runs-label") {
            label.set_text_content(Some(&format!("{} of {RUNS}", self.game.runs())));
        }
        let completed = self.game.completed_suits().to_vec();
        for index in 0..RUNS {
            let Some(pip) = document.get_element_by_id(&format!("run-{index}")) else {
                continue;
            };
            let suit = completed.get(index).copied();
            let mut classes = String::from("run-pip");
            if let Some(suit) = suit {
                classes.push_str(" done ");
                classes.push_str(suit.class());
            }
            let _ = pip.set_attribute("class", &classes);
            let text = suit
                .map(|suit| suit.symbol().to_string())
                .unwrap_or_default();
            pip.set_text_content(Some(&text));
        }
        let _ = runs.set_attribute(
            "aria-label",
            &format!("{} of {RUNS} runs completed", self.game.runs()),
        );
    }

    /// Paint the count of deals left, on the stock.
    fn paint_badge(&self, board: &BoardUi) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(badge) = document.get_element_by_id("deals") else {
            return;
        };
        let deals = self.game.deals_left();
        let Some((x, y)) = board.layout().place(Place::Top(0)) else {
            return;
        };
        let offset = board.layout().card_w * 0.62;
        let _ = badge.set_attribute(
            "style",
            &format!(
                "left:{}px;top:{}px;z-index:900",
                x + offset,
                y + offset * 0.35
            ),
        );
        let _ = badge.set_attribute("class", if deals == 0 { "badge gone" } else { "badge" });
        badge.set_text_content(Some(&deals.to_string()));
    }
}

impl Rules for Spider {
    fn deck(&self) -> Vec<crate::card::Card> {
        // The whole deck at the hardest setting: every suit, twice. The board
        // only ever draws cards the scene places, so a deck larger than the
        // deal is harmless — and it means "New game" at another difficulty does
        // not have to rebuild the elements.
        spider_deck(Difficulty::Hard)
    }

    fn scene(&self) -> Scene {
        crate::scene::scene(&self.game, self.hint.as_ref())
    }

    fn act(&mut self, intent: Intent) {
        let report = self.answer(intent);
        self.show(report);
    }

    fn lift(&self, place: Place, index: usize) -> usize {
        match Pile::from_place(place) {
            Some(pile) => self.game.run_len(pile, index).unwrap_or(0),
            None => 0,
        }
    }

    fn default_focus(&self) -> (Place, Option<usize>) {
        crate::scene::default_focus(&self.game)
    }

    fn hint(&mut self) -> Option<String> {
        if self.game.won() {
            return None;
        }
        let hint = self.game.hint();
        let words = hint.words().to_owned();
        self.hint = Some(hint);
        Some(words)
    }

    fn clear_hint(&mut self) {
        self.hint = None;
    }

    fn won(&self) -> bool {
        self.game.won()
    }

    fn result_words(&self) -> String {
        self.game.result_words()
    }

    fn can_undo(&self) -> bool {
        self.game.can_undo()
    }

    fn undo(&mut self) {
        let report = self.game.undo();
        self.show(report);
    }

    fn started(&self) -> bool {
        self.game.started()
    }

    fn elapsed(&self) -> u32 {
        self.game.elapsed()
    }

    fn tick(&mut self, seconds: u32) {
        self.game.tick(seconds);
    }

    fn readout(&self) -> (String, String, Option<String>) {
        let readout = self.game.readout();
        (readout.clock, readout.moves, readout.score)
    }

    fn variant(&self) -> String {
        self.game.variant().to_owned()
    }

    fn summary(&self) -> String {
        self.game.summary()
    }

    fn action_label(&self) -> Option<&'static str> {
        Some("Deal")
    }

    fn action_enabled(&self) -> bool {
        self.game.can_deal()
    }

    fn action(&mut self) -> Repeat {
        let report = self.game.deal();
        self.show(report);
        Repeat::Once
    }

    fn take_words(&mut self) -> Option<String> {
        self.words.take()
    }

    fn decorate(&mut self, board: &BoardUi) {
        self.paint_runs(board);
        self.paint_badge(board);
    }

    fn option_values(&self) -> Vec<(&'static str, String)> {
        vec![
            ("difficulty", self.options.difficulty.slug().to_owned()),
            ("scoring", self.options.scoring.slug().to_owned()),
        ]
    }

    fn option_note(&self, id: &str) -> String {
        match id {
            "difficulty" => self.options.difficulty.sentence().to_owned(),
            "scoring" => self.options.scoring.sentence().to_owned(),
            _ => String::new(),
        }
    }

    fn set_option(&mut self, id: &str, value: &str) {
        match id {
            "difficulty" => {
                self.options.difficulty =
                    Difficulty::from_slug(value).unwrap_or(Options::default().difficulty);
            }
            "scoring" => {
                self.options.scoring = Scoring::from_slug(value).unwrap_or(Scoring::Standard);
            }
            _ => {}
        }
    }

    fn store_options(&self) -> String {
        self.options.to_stored()
    }

    fn load_options(&mut self, text: &str) {
        self.options = Options::from_stored(Some(text));
    }

    fn store_game(&self) -> Option<String> {
        self.game.to_stored()
    }

    fn load_game(&mut self, text: &str) {
        if let Some(game) = Game::from_stored(Some(text)) {
            self.game = game;
        } else {
            // An unreadable save is a fresh deal, not an error: a corrupt entry
            // must never stop the app from starting.
            self.game = Game::new(self.options.difficulty, self.options.scoring, fresh_seed());
        }
    }

    fn keys(&self) -> (&'static str, &'static str) {
        (STORAGE_KEY, OPTIONS_KEY)
    }

    fn new_game(&mut self) {
        self.game = Game::dealt(self.options.difficulty, self.options.scoring, fresh_seed());
        self.words = Some(format!("New deal. {}", self.options.summary()));
    }

    fn restart(&mut self) {
        self.game = self.game.restart();
        self.words = Some("Restarted the same deal.".to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pile_has_a_place_and_every_place_has_a_pile() {
        for pile in Pile::ALL {
            assert_eq!(Pile::from_place(pile.place()), Some(pile));
        }
        assert_eq!(
            Pile::from_place(Place::Top(1)),
            None,
            "Spider has one top slot"
        );
    }

    #[test]
    fn the_deck_covers_every_difficulty() {
        // The board is built once, from this deck, and every difficulty must be
        // drawable from it.
        let built = spider_deck(Difficulty::Hard);
        for difficulty in Difficulty::ALL {
            for card in spider_deck(difficulty) {
                assert!(built.contains(&card), "{difficulty:?} needs {card}");
            }
        }
    }

    #[test]
    fn a_column_fans_and_the_stock_is_a_packet() {
        let game = Game::new(Difficulty::Medium, Scoring::Standard, 1);
        let spider = Spider {
            game,
            options: Options::default(),
            hint: None,
            words: None,
        };
        let scene = spider.scene();
        let stock = scene.slot(Pile::Stock.place()).expect("the stock");
        assert!(stock.fan.stacked);
        let column = scene.slot(Pile::Tableau(0).place()).expect("a column");
        assert!(!column.fan.stacked);
        assert_eq!(scene.columns(), COLUMNS);
    }
}
