//! The page around the board: the header, the buttons, the menu, saved state and
//! the service worker.
//!
//! Every solitaire in this family has the same shell — three facts, four
//! buttons, a folded menu holding the next deal's choices, a clock that measures
//! wall time rather than counting timer callbacks, and a worker that never swaps
//! itself in unasked. That is here, once.
//!
//! **This file is carried, byte for byte, by both solitaires in this family** —
//! Solitaire and Spider Solitaire. They are separate repositories, so nothing
//! enforces that; the rule is that a change here belongs in both.
//!
//! An app supplies its [`Rules`] — what the game is, what its choices mean, how
//! a place is named — and keeps its own `game.rs`. The rules never cross this
//! boundary: the shell asks the game what to do and shows what it says.
//!
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Document, Element, HtmlButtonElement, HtmlInputElement, HtmlSelectElement, KeyboardEvent,
    Storage, Window,
};

use crate::board::{Frame, Place, Scene};
use crate::board_ui::{listen, point_of, BoardUi, Intent};
use crate::card::Card;

/// The scope every app's worker is registered for.
const SCOPE: &str = "./";

/// How often the clock is checked. It is a check, not a tick: the elapsed time
/// comes from the wall clock, so a tab the browser throttled still catches up.
const TICK_MS: i32 = 1_000;

/// How long a hint stays lit.
const HINT_MS: i32 = 2_600;

/// What one game tells the shell about itself.
///
/// Deliberately small: every item is a fact the *page* needs and the game
/// already knows. No rule crosses this boundary — the shell asks the game what
/// to do and shows the answer.
pub trait Rules {
    /// The deck, in deck-index order: every card the board can draw.
    fn deck(&self) -> Vec<Card>;

    /// The board as it should be drawn now.
    fn scene(&self) -> Scene;

    /// Answer a gesture.
    fn act(&mut self, intent: Intent);

    /// How many cards may be lifted from this place and position. Zero means
    /// the press cannot become a drag.
    fn lift(&self, place: Place, index: usize) -> usize;

    /// Where the keyboard should be after an action that replaced the board.
    fn default_focus(&self) -> (Place, Option<usize>);

    /// A hint and the words for it, or `None` when a hint cannot help.
    fn hint(&mut self) -> Option<String>;

    /// Clear a hint.
    fn clear_hint(&mut self);

    /// True when the game is won.
    fn won(&self) -> bool;

    /// The line under the board when the game is won.
    fn result_words(&self) -> String;

    /// True when there is something to take back.
    fn can_undo(&self) -> bool;

    /// Take back the last action.
    fn undo(&mut self);

    /// True once the player has done something, so an untouched app stores
    /// nothing.
    fn started(&self) -> bool;

    /// Seconds on the clock.
    fn elapsed(&self) -> u32;

    /// Advance the clock.
    fn tick(&mut self, seconds: u32);

    /// The header's three facts, already formatted: clock, moves, and a score
    /// (`None` when the deal is unscored).
    fn readout(&self) -> (String, String, Option<String>);

    /// The label beside the title.
    fn variant(&self) -> String;

    /// The rule summary the menu shows.
    fn summary(&self) -> String;

    /// The one extra button: Klondike's automatic finish, Spider's deal.
    fn action_label(&self) -> Option<&'static str>;

    /// Whether that button can be pressed now.
    fn action_enabled(&self) -> bool;

    /// Press it.
    ///
    /// The answer says whether pressing it again would do something: a deal is
    /// `Once` (one press, one deal), while an automatic finish repeats until
    /// the board is done. The interval belongs to the shell, so an app never
    /// holds a timer handle it could leak.
    fn action(&mut self) -> Repeat;

    /// The last thing the game said, taken — `None` when it said nothing worth
    /// showing. The shell puts it under the board.
    fn take_words(&mut self) -> Option<String>;

    /// Draw whatever this game shows beside the cards — a strip of completed
    /// runs, a count on the stock. Called after every paint, with the board's
    /// layout ready.
    ///
    /// The default draws nothing, which is right for a game whose board is only
    /// cards.
    fn decorate(&mut self, board: &BoardUi) {
        let _ = board;
    }

    /// The menu's selects: `(id, current value)`.
    fn option_values(&self) -> Vec<(&'static str, String)>;

    /// The sentence under one select, explaining the value it holds.
    fn option_note(&self, id: &str) -> String;

    /// Apply a menu choice. It is a request for the *next* deal, so this must
    /// not touch the game in progress.
    fn set_option(&mut self, id: &str, value: &str);

    /// The options, stored.
    fn store_options(&self) -> String;

    /// Read the stored options back.
    fn load_options(&mut self, text: &str);

    /// The game, stored — or `None` when nothing is worth keeping.
    fn store_game(&self) -> Option<String>;

    /// Read a stored game back. An unreadable one is ignored, not an error: a
    /// corrupt entry must never stop the app from starting.
    fn load_game(&mut self, text: &str);

    /// The storage keys: the game's and the options'.
    fn keys(&self) -> (&'static str, &'static str);

    /// A fresh deal, applying the current options.
    fn new_game(&mut self);

    /// The same deal again.
    fn restart(&mut self);
}

/// What pressing the extra button again would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    /// Nothing more: the action was complete.
    Once,
    /// Again, after this many milliseconds, until it says `Once`.
    Every(i32),
}

/// The page, as it runs.
pub struct Shell<R: Rules> {
    window: Window,
    document: Document,
    board: BoardUi,
    rules: R,
    /// Fractional seconds measured but not yet handed to the game.
    pending: Cell<f64>,
    /// The elapsed time at which the saved game is written again.
    next_save: Cell<u32>,
    /// Bumped on every hint, so a stale timer cannot clear a newer one.
    hint_token: Cell<u32>,
    /// The repeating action's interval handle, if one is running.
    action_timer: Cell<Option<i32>>,
}

impl<R: Rules + 'static> Shell<R> {
    /// Build the page: the board, the game, and the saved state.
    ///
    /// # Errors
    ///
    /// When the document will not make the elements, which is a document that
    /// is not a page.
    pub fn start(
        window: &Window,
        document: &Document,
        board: Element,
        mut rules: R,
    ) -> Result<(), JsValue> {
        let (game_key, options_key) = rules.keys();
        if let Some(text) = read(window, options_key) {
            rules.load_options(&text);
        }
        if let Some(text) = read(window, game_key) {
            rules.load_game(&text);
        }
        let board_ui = BoardUi::new(document, board, &rules.deck())?;
        let focus = rules.default_focus();
        let shared = Rc::new(RefCell::new(Self {
            window: window.clone(),
            document: document.clone(),
            board: board_ui,
            rules,
            pending: Cell::new(0.0),
            next_save: Cell::new(10),
            hint_token: Cell::new(0),
            action_timer: Cell::new(None),
        }));
        {
            let mut shell = shared.borrow_mut();
            shell
                .next_save
                .set(shell.rules.elapsed().saturating_add(10));
            shell.board.set_focus(focus.0, focus.1);
            shell.paint();
            if shell.rules.won() {
                shell.show_win();
            }
        }
        bind(&shared);
        start_clock(&shared);
        start_resize(&shared);
        release_instant(&shared);

        let offline = window.clone();
        spawn_local(keep_offline(offline));
        // The app lives for the page's lifetime: dropping the `Rc` here would
        // take the game with it.
        std::mem::forget(shared);
        Ok(())
    }

    /// Draw the whole page from the state.
    fn paint(&mut self) {
        let scene = self.rules.scene();
        let element = self.board.element().clone();
        let width = f64::from(element.client_width());
        let height = f64::from(element.client_height());
        let frame = Frame {
            width,
            height,
            gap: (width * 0.008).clamp(3.0, 10.0),
            margin: (width * 0.012).clamp(4.0, 12.0),
        };
        self.board.paint(&scene, &frame);
        self.paint_facts();
        self.paint_tools();
        self.paint_menu();
        self.rules.decorate(&self.board);
        let won = self.rules.won();
        set_hidden(&self.document, "won", !won);
        if won {
            set_text(&self.document, "won-detail", &self.rules.result_words());
        }
    }

    fn paint_facts(&self) {
        let (clock, moves, score) = self.rules.readout();
        set_text(&self.document, "clock", &clock);
        set_text(&self.document, "moves", &moves);
        match score {
            Some(score) => {
                set_text(&self.document, "score", &score);
                set_hidden(&self.document, "score-fact", false);
            }
            None => set_hidden(&self.document, "score-fact", true),
        }
    }

    fn paint_tools(&self) {
        let won = self.rules.won();
        if let Some(undo) = button(&self.document, "undo") {
            undo.set_disabled(won || !self.rules.can_undo());
        }
        if let Some(hint) = button(&self.document, "hint") {
            hint.set_disabled(won);
        }
        let label = self.rules.action_label();
        set_hidden(&self.document, "action", label.is_none());
        if let (Some(action), Some(label)) = (button(&self.document, "action"), label) {
            action.set_disabled(won || !self.rules.action_enabled());
            action.set_text_content(Some(label));
        }
    }

    fn paint_menu(&self) {
        set_text(&self.document, "variant", &self.rules.variant());
        set_text(&self.document, "game-summary", &self.rules.summary());
        for (id, value) in self.rules.option_values() {
            set_select(&self.document, id, &value);
            set_text(
                &self.document,
                &format!("{id}-note"),
                &self.rules.option_note(id),
            );
        }
    }

    /// Answer one gesture.
    fn act(&mut self, intent: Intent) {
        self.rules.clear_hint();
        self.rules.act(intent);
        self.after();
    }

    /// Show whatever the game said, then repaint.
    fn speak(&mut self) {
        if let Some(words) = self.rules.take_words() {
            if self.rules.won() {
                // The dialog carries the result; the line under the board says
                // only that the deal is over, rather than printing the same
                // sentence twice on one screen.
                set_text(&self.document, "status", "Deal complete");
            } else {
                set_text(&self.document, "status", &words);
            }
        }
    }

    /// Repaint after the game changed, and keep the focus and the saved game in
    /// step with it.
    fn after(&mut self) {
        self.speak();
        let focus = self.rules.default_focus();
        self.board.set_focus(focus.0, focus.1);
        self.paint();
        if self.rules.won() {
            self.show_win();
        }
        self.save_game();
    }

    fn show_win(&mut self) {
        set_hidden(&self.document, "won", false);
        set_text(&self.document, "won-detail", &self.rules.result_words());
        if let Some(new) = button(&self.document, "won-new") {
            focus(&new);
        }
    }

    fn save_game(&self) {
        let Some(storage) = storage(&self.window) else {
            return;
        };
        let (game_key, _) = self.rules.keys();
        match self.rules.store_game() {
            Some(value) => {
                if let Err(error) = storage.set_item(game_key, &value) {
                    warn("could not save the game", &error);
                }
            }
            None => {
                let _ = storage.remove_item(game_key);
            }
        }
    }

    fn save_options(&self) {
        let Some(storage) = storage(&self.window) else {
            return;
        };
        let (_, options_key) = self.rules.keys();
        if let Err(error) = storage.set_item(options_key, &self.rules.store_options()) {
            warn("could not save the options", &error);
        }
    }

    fn tick(&mut self, seconds: u32) {
        if self.rules.won() {
            return;
        }
        self.rules.tick(seconds);
        let (clock, _, _) = self.rules.readout();
        set_text(&self.document, "clock", &clock);
        if self.rules.started() && self.rules.elapsed() >= self.next_save.get() {
            self.next_save.set(self.rules.elapsed().saturating_add(10));
            self.save_game();
        }
    }

    fn new_game(&mut self, restart: bool) {
        self.stop_action();
        self.rules.clear_hint();
        if restart {
            self.rules.restart();
        } else {
            self.rules.new_game();
        }
        self.next_save.set(self.rules.elapsed().saturating_add(10));
        close_menu(&self.document);
        self.after();
        self.save_options();
        self.board.focus_element();
    }

    fn undo(&mut self) {
        self.stop_action();
        self.rules.clear_hint();
        self.rules.undo();
        self.after();
    }

    fn hint(&mut self, shared: &Weak<RefCell<Self>>) {
        if self.rules.won() {
            return;
        }
        let Some(words) = self.rules.hint() else {
            return;
        };
        self.say(&words);
        let token = self.hint_token.get().wrapping_add(1);
        self.hint_token.set(token);
        // A hint that has no card to ring still has a pile to point at, and the
        // game is the one that knows which: while a hint is up, `default_focus`
        // answers with the hint's own target. So the keyboard lands on it —
        // which is also how Enter performs a hinted move — and a hint about an
        // empty pile is answered by focus alone.
        let focus = self.rules.default_focus();
        self.board.set_focus(focus.0, focus.1);
        self.paint();
        self.board.focus_element();
        let weak = Weak::clone(shared);
        let closure = Closure::<dyn FnMut()>::new(move || {
            let Some(shared) = weak.upgrade() else {
                return;
            };
            let mut shell = shared.borrow_mut();
            if shell.hint_token.get() == token {
                shell.rules.clear_hint();
                shell.paint();
            }
        });
        let _ = self
            .window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                HINT_MS,
            );
        closure.forget();
    }

    /// Press the extra button, and keep pressing it while it asks to repeat.
    fn action(&mut self, shared: &Weak<RefCell<Self>>) {
        match self.rules.action() {
            Repeat::Once => self.after(),
            Repeat::Every(ms) => {
                self.after();
                self.stop_action();
                let weak = Weak::clone(shared);
                let closure = Closure::<dyn FnMut()>::new(move || {
                    let Some(shared) = weak.upgrade() else {
                        return;
                    };
                    let mut shell = shared.borrow_mut();
                    match shell.rules.action() {
                        Repeat::Once => {
                            shell.stop_action();
                            shell.after();
                        }
                        Repeat::Every(_) => {
                            shell.after();
                        }
                    }
                });
                let window = self.window.clone();
                match window.set_interval_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    ms,
                ) {
                    Ok(handle) => {
                        self.action_timer.set(Some(handle));
                        closure.forget();
                    }
                    Err(error) => warn("could not start the repeating action", &error),
                }
            }
        }
    }

    /// Stop the repeating action, if one is running.
    fn stop_action(&mut self) {
        if let Some(handle) = self.action_timer.take() {
            self.window.clear_interval_with_handle(handle);
        }
    }

    fn say(&self, words: &str) {
        set_text(&self.document, "status", words);
    }

    fn menu_open(&self) -> bool {
        checkbox(&self.document, "menu").is_some_and(|toggle| toggle.checked())
    }
}

fn bind<R: Rules + 'static>(shared: &Rc<RefCell<Shell<R>>>) {
    let (board, document) = {
        let shell = shared.borrow();
        (shell.board.element().clone(), shell.document.clone())
    };
    bind_board(&board, shared);
    bind_tools(&document, shared);
    bind_menu(&document, shared);
    bind_keys(&document, shared);
}

fn bind_board<R: Rules + 'static>(board: &Element, shared: &Rc<RefCell<Shell<R>>>) {
    let down = {
        let app = Rc::clone(shared);
        let board = board.clone();
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            if event.button() != 0 {
                return;
            }
            let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                return;
            };
            event.prevent_default();
            // Capture, so a finger that leaves the board mid-drag keeps sending
            // its moves here.
            let _ = target.set_pointer_capture(event.pointer_id());
            let point = point_of(event.as_ref(), &board);
            let mut shell = app.borrow_mut();
            let Some((place, index)) = shell.board.press(event.pointer_id(), point) else {
                return;
            };
            let lift = shell.rules.lift(place, index);
            shell.board.lift(lift);
        })
    };
    listen(board, "pointerdown", down.as_ref().unchecked_ref());
    down.forget();

    let moved = {
        let app = Rc::clone(shared);
        let board = board.clone();
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            let point = point_of(event.as_ref(), &board);
            app.borrow_mut().board.moved(event.pointer_id(), point);
        })
    };
    listen(board, "pointermove", moved.as_ref().unchecked_ref());
    moved.forget();

    let up = {
        let app = Rc::clone(shared);
        let board = board.clone();
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            let point = point_of(event.as_ref(), &board);
            let intent = app.borrow_mut().board.release(event.pointer_id(), point);
            if let Some(intent) = intent {
                app.borrow_mut().act(intent);
            }
        })
    };
    listen(board, "pointerup", up.as_ref().unchecked_ref());
    up.forget();

    let cancel = {
        let app = Rc::clone(shared);
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            app.borrow_mut().board.cancel(event.pointer_id());
        })
    };
    listen(board, "pointercancel", cancel.as_ref().unchecked_ref());
    cancel.forget();

    // A keyboard activation arrives as a click with no pointer detail; a mouse
    // or touch click is the gesture's own release and is ignored.
    let click = {
        let app = Rc::clone(shared);
        let board = board.clone();
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if event.detail() != 0 {
                return;
            }
            let point = point_of(event.as_ref(), &board);
            let mut shell = app.borrow_mut();
            if shell.board.press(1, point).is_none() {
                return;
            }
            shell.board.lift(0);
            let intent = shell.board.release(1, point);
            if let Some(intent) = intent {
                shell.act(intent);
            }
        })
    };
    listen(board, "click", click.as_ref().unchecked_ref());
    click.forget();
}

fn bind_tools<R: Rules + 'static>(document: &Document, shared: &Rc<RefCell<Shell<R>>>) {
    if let Some(undo) = button(document, "undo") {
        let app = Rc::clone(shared);
        let closure = Closure::<dyn FnMut()>::new(move || {
            app.borrow_mut().undo();
        });
        listen(&undo, "click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(hint) = button(document, "hint") {
        let app = Rc::clone(shared);
        let weak = Rc::downgrade(&app);
        let closure = Closure::<dyn FnMut()>::new(move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            app.borrow_mut().hint(&weak);
        });
        listen(&hint, "click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(action) = button(document, "action") {
        let app = Rc::clone(shared);
        let weak = Rc::downgrade(&app);
        let closure = Closure::<dyn FnMut()>::new(move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            app.borrow_mut().action(&weak);
        });
        listen(&action, "click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(new) = button(document, "won-new") {
        let app = Rc::clone(shared);
        let closure = Closure::<dyn FnMut()>::new(move || {
            app.borrow_mut().new_game(false);
        });
        listen(&new, "click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

fn bind_menu<R: Rules + 'static>(document: &Document, shared: &Rc<RefCell<Shell<R>>>) {
    if let Some(toggle) = checkbox(document, "menu") {
        let app = Rc::clone(shared);
        let boxed = toggle.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let document = app.borrow().document.clone();
            set_hidden(&document, "menu-panel", !boxed.checked());
        });
        listen(&toggle, "change", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    for id in option_ids(document) {
        let Some(select) = select_element(document, &id) else {
            continue;
        };
        let app = Rc::clone(shared);
        let handle = select.clone();
        let name = id.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let value = handle.value();
            let mut shell = app.borrow_mut();
            shell.rules.set_option(&name, &value);
            shell.save_options();
            shell.paint_menu();
            shell.say("Next deal");
        });
        listen(&select, "change", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(new) = button(document, "new") {
        let app = Rc::clone(shared);
        let closure = Closure::<dyn FnMut()>::new(move || {
            app.borrow_mut().new_game(false);
        });
        listen(&new, "click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(restart) = button(document, "restart") {
        let app = Rc::clone(shared);
        let closure = Closure::<dyn FnMut()>::new(move || {
            app.borrow_mut().new_game(true);
        });
        listen(&restart, "click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

/// The menu's selects, found by their markup: a select carrying `data-option`
/// is one the shell binds, so a new choice is one line in the shell and one
/// option list in the app.
fn option_ids(document: &Document) -> Vec<String> {
    let mut ids = Vec::new();
    let Ok(selects) = document.query_selector_all("select[data-option]") else {
        return ids;
    };
    for index in 0..selects.length() {
        let Some(node) = selects.item(index) else {
            continue;
        };
        let Ok(element) = node.dyn_into::<Element>() else {
            continue;
        };
        if let Some(id) = element.get_attribute("id") {
            ids.push(id);
        }
    }
    ids
}

fn bind_keys<R: Rules + 'static>(document: &Document, shared: &Rc<RefCell<Shell<R>>>) {
    let app = Rc::clone(shared);
    let closure = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        if is_form_control(event.target()) {
            return;
        }
        let mut shell = app.borrow_mut();
        if shell.menu_open() {
            if event.key() == "Escape" {
                close_menu(&shell.document);
                event.prevent_default();
            }
            return;
        }
        let handled = match event.key().as_str() {
            "ArrowLeft" => {
                shell.board.move_focus(-1);
                shell.board.focus_element();
                true
            }
            "ArrowRight" => {
                shell.board.move_focus(1);
                shell.board.focus_element();
                true
            }
            "ArrowUp" => {
                shell.board.move_within(-1);
                shell.board.focus_element();
                true
            }
            "ArrowDown" => {
                shell.board.move_within(1);
                shell.board.focus_element();
                true
            }
            "Enter" | " " => {
                let (place, index) = shell.board.focus();
                shell.act(Intent::Tap { place, index });
                true
            }
            "Escape" => {
                shell.act(Intent::Back);
                true
            }
            "u" | "U" => {
                shell.undo();
                true
            }
            "h" | "H" => {
                let weak = Rc::downgrade(&app);
                shell.hint(&weak);
                true
            }
            "a" | "A" => {
                let weak = Rc::downgrade(&app);
                shell.action(&weak);
                true
            }
            "z" | "Z" if event.ctrl_key() || event.meta_key() => {
                shell.undo();
                true
            }
            _ => false,
        };
        if handled {
            event.prevent_default();
        }
    });
    listen(document, "keydown", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn start_clock<R: Rules + 'static>(shared: &Rc<RefCell<Shell<R>>>) {
    let last = Rc::new(Cell::new(js_sys::Date::now()));
    let app = Rc::clone(shared);
    let closure = Closure::<dyn FnMut()>::new(move || {
        let now = js_sys::Date::now();
        let delta = ((now - last.get()) / 1_000.0).max(0.0);
        last.set(now);
        let mut shell = app.borrow_mut();
        let pending = shell.pending.get() + delta;
        let whole = pending.floor();
        shell.pending.set(pending - whole);
        if whole >= 1.0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let seconds = whole.min(f64::from(u32::MAX)) as u32;
            shell.tick(seconds);
        }
    });
    let window = shared.borrow().window.clone();
    match window.set_interval_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        TICK_MS,
    ) {
        Ok(_) => closure.forget(),
        Err(error) => warn("could not start the clock", &error),
    }
}

fn start_resize<R: Rules + 'static>(shared: &Rc<RefCell<Shell<R>>>) {
    let (board, window) = {
        let shell = shared.borrow();
        (shell.board.element().clone(), shell.window.clone())
    };
    let app = Rc::clone(shared);
    let callback = Closure::<dyn FnMut()>::new(move || {
        app.borrow_mut().paint();
    });
    match web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) {
        Ok(observer) => {
            observer.observe(&board);
            // The observer lives exactly as long as the page, and the page is
            // the app: dropping it would stop every future resize.
            std::mem::forget(observer);
        }
        Err(error) => warn("could not watch the board size", &error),
    }
    callback.forget();
    let _ = window;
}

fn release_instant<R: Rules + 'static>(shared: &Rc<RefCell<Shell<R>>>) {
    let app = Rc::clone(shared);
    let closure = Closure::<dyn FnMut()>::new(move || {
        app.borrow_mut().board.settle();
    });
    let window = shared.borrow().window.clone();
    let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
    closure.forget();
}

fn is_form_control(target: Option<web_sys::EventTarget>) -> bool {
    let Some(target) = target.and_then(|target| target.dyn_into::<Element>().ok()) else {
        return false;
    };
    matches!(
        target.tag_name().as_str(),
        "INPUT" | "SELECT" | "TEXTAREA" | "OPTION"
    )
}

fn close_menu(document: &Document) {
    if let Some(toggle) = checkbox(document, "menu") {
        toggle.set_checked(false);
    }
    set_hidden(document, "menu-panel", true);
}

// -- small DOM helpers ---------------------------------------------------

fn element(document: &Document, id: &str) -> Option<Element> {
    document.get_element_by_id(id)
}

fn button(document: &Document, id: &str) -> Option<HtmlButtonElement> {
    element(document, id)?.dyn_into().ok()
}

fn checkbox(document: &Document, id: &str) -> Option<HtmlInputElement> {
    element(document, id)?.dyn_into().ok()
}

fn select_element(document: &Document, id: &str) -> Option<HtmlSelectElement> {
    element(document, id)?.dyn_into().ok()
}

fn set_text(document: &Document, id: &str, text: &str) {
    if let Some(element) = element(document, id) {
        element.set_text_content(Some(text));
    }
}

fn set_hidden(document: &Document, id: &str, hidden: bool) {
    if let Some(element) = element(document, id) {
        if hidden {
            let _ = element.set_attribute("hidden", "");
        } else {
            let _ = element.remove_attribute("hidden");
        }
    }
}

fn set_select(document: &Document, id: &str, value: &str) {
    if let Some(select) = select_element(document, id) {
        if select.value() != value {
            select.set_value(value);
        }
    }
}

fn focus(element: &Element) {
    if let Some(html) = element.dyn_ref::<web_sys::HtmlElement>() {
        let _ = html.focus();
    }
}

fn storage(window: &Window) -> Option<Storage> {
    window.local_storage().ok().flatten()
}

fn read(window: &Window, key: &str) -> Option<String> {
    storage(window)?.get_item(key).ok().flatten()
}

/// A fresh seed for a new deal: the clock and the platform's random number,
/// mixed. `Math.random` alone repeats across a restored tab; the clock alone
/// repeats two deals started in the same millisecond.
#[must_use]
pub fn fresh_seed() -> u64 {
    let now = js_sys::Date::now();
    let random = js_sys::Math::random();
    now.to_bits() ^ random.to_bits().rotate_left(17)
}

fn warn(context: &str, error: &JsValue) {
    web_sys::console::warn_2(&JsValue::from_str(context), error);
}

// -- offline -------------------------------------------------------------

/// Register the service worker, and release any stale registration.
///
/// Deliberately silent: the worker is a background concern, and an app that
/// stays usable without it is the point.
async fn keep_offline(window: Window) {
    let container = window.navigator().service_worker();
    let options = web_sys::RegistrationOptions::new();
    options.set_scope(SCOPE);
    let registered =
        JsFuture::from(container.register_with_options("./service-worker.js", &options)).await;
    let registered = match registered {
        Ok(value) => value,
        Err(error) => {
            warn("service worker registration failed", &error);
            return;
        }
    };
    if let Ok(registered) = registered.dyn_into::<web_sys::ServiceWorkerRegistration>() {
        nudge_worker_update(&registered).await;
    }
    release_stale_registrations(&window).await;
}

/// Ask this app's worker to take over as soon as it can, because the page that
/// asks reloads as the handover happens. The worker never swaps itself in
/// unasked.
async fn nudge_worker_update(registration: &web_sys::ServiceWorkerRegistration) {
    let Some(window) = web_sys::window() else {
        return;
    };
    watch_for_controller_change(&window);
    if let Some(worker) = registration.waiting() {
        if worker.state() == web_sys::ServiceWorkerState::Installed {
            ask_worker_in(&worker);
            return;
        }
    }
    let installing = match registration.installing() {
        Some(worker) => Some(worker),
        None => update_found(registration).await,
    };
    let Some(worker) = installing else {
        return;
    };
    if wait_for_state_change(&worker).await.is_err() {
        warn("could not wait for the worker to install", &JsValue::NULL);
        return;
    }
    if worker.state() == web_sys::ServiceWorkerState::Installed {
        ask_worker_in(&worker);
    }
}

fn ask_worker_in(worker: &web_sys::ServiceWorker) {
    worker.post_message(&JsValue::from_str("SKIP_WAITING")).ok();
}

fn watch_for_controller_change(window: &web_sys::Window) {
    thread_local! {
        static WATCHING: Cell<bool> = const { Cell::new(false) };
    }
    if WATCHING.with(Cell::get) {
        return;
    }
    WATCHING.with(|watching| watching.set(true));

    let container = window.navigator().service_worker();
    // Whether this page was already being served by a worker. On a first visit
    // the worker claims the page as it activates, which fires
    // `controllerchange` — but nothing stale was replaced, so reloading would
    // only cost the reader a second start.
    let replacing = container.controller().is_some();
    let handler = Closure::<dyn FnMut(JsValue)>::new(move |_event: JsValue| {
        if !replacing {
            return;
        }
        if let Some(window) = web_sys::window() {
            window.location().reload().ok();
        }
    });
    let _ = container
        .add_event_listener_with_callback("controllerchange", handler.as_ref().unchecked_ref());
    handler.forget();
}

async fn update_found(
    registration: &web_sys::ServiceWorkerRegistration,
) -> Option<web_sys::ServiceWorker> {
    let event = JsFuture::from(js_sys::Promise::new(&mut |resolve, _reject| {
        let resolved = Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
            resolve.call1(&JsValue::NULL, &value).ok();
        });
        let _ = registration
            .add_event_listener_with_callback("updatefound", resolved.as_ref().unchecked_ref());
        resolved.forget();
    }))
    .await
    .ok()?;
    let event = event.dyn_into::<web_sys::Event>().ok()?;
    let announced = event
        .target()?
        .dyn_into::<web_sys::ServiceWorkerRegistration>()
        .ok()?;
    announced.installing()
}

async fn wait_for_state_change(worker: &web_sys::ServiceWorker) -> Result<(), JsValue> {
    JsFuture::from(js_sys::Promise::new(&mut |resolve, _reject| {
        let resolved = Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
            resolve.call1(&JsValue::NULL, &value).ok();
        });
        let _ = worker
            .add_event_listener_with_callback("statechange", resolved.as_ref().unchecked_ref());
        resolved.forget();
    }))
    .await
    .map(|_| ())
}

async fn release_stale_registrations(window: &Window) {
    let container = window.navigator().service_worker();
    let Ok(registrations) = JsFuture::from(container.get_registrations()).await else {
        return;
    };
    let Ok(array) = registrations.dyn_into::<js_sys::Array>() else {
        return;
    };
    let Ok(home) = window.location().href() else {
        return;
    };
    let Ok(ours) = web_sys::Url::new_with_base(&home, "./") else {
        return;
    };
    let ours = ours.href();
    for entry in array.iter() {
        let Ok(registration) = entry.dyn_into::<web_sys::ServiceWorkerRegistration>() else {
            continue;
        };
        let scope = registration.scope();
        if ours.starts_with(&scope) {
            continue;
        }
        let script = registration
            .active()
            .map(|worker| worker.script_url())
            .unwrap_or_default();
        if script_belongs_to_app(&script, &ours) {
            match registration.unregister() {
                Ok(promise) => {
                    let _ = JsFuture::from(promise).await;
                }
                Err(error) => warn("stale service worker could not be released", &error),
            }
        }
    }
}

fn script_belongs_to_app(script: &str, ours: &str) -> bool {
    match web_sys::Url::new(script) {
        Ok(url) => url.href().strip_suffix("service-worker.js") == Some(ours),
        Err(_) => false,
    }
}
