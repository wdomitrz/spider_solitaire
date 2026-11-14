//! The board both solitaires are drawn on: where every card sits, what a drop
//! lands on, and what a gesture means.
//!
//! No `web-sys`, no `js-sys`, no `wasm-bindgen`: this module is pure arithmetic
//! and state, so `cargo test` runs it natively — including the parts that used
//! to be untestable, like "which pile is under this finger" and "does a drag
//! count as a drag".
//!
//! **This file is carried, byte for byte, by both solitaires in this family** —
//! Solitaire and Spider Solitaire. They are separate repositories, so nothing
//! enforces that; the rule is that a change here belongs in both.
//!
//! Everything game-specific — the rules, the piles, the score, the words —
//! stays in each app's `game.rs`, which is what makes this file possible: it is
//! handed a description of the board and answers questions about it, and knows
//! nothing about cards.
//!
//!
//! **This file is carried, byte for byte, by both solitaires in this family** —
//! Solitaire and Spider Solitaire. They are separate repositories, so nothing
//! enforces that; the rule is that a change here belongs in both.
//!
//! Three decisions shape it:
//!
//! * **A pile is a [`Place`], not a DOM node.** Layout is computed from the
//!   game's own description, so a question like "what is under this finger" is
//!   arithmetic with an answer a test can state — and it cannot be fooled by
//!   the dragged card itself, which is exactly what a DOM hit test under the
//!   cursor finds.
//! * **The drop target is the place whose rectangle overlaps the dragged stack
//!   the most**, not the place whose cards happen to be under the pointer. A
//!   card is bigger than a fingertip and a half-covered pile is still a legal
//!   destination, so overlap is the question a player is actually asking.
//! * **A gesture is a small state machine with named states**, so "the drag
//!   never started", "the drag started and was dropped on nothing" and "the
//!   drag started and was cancelled" are three different answers rather than
//!   three ways to reach the same branch.

/// One place on the board: a slot in the top row, or a tableau column.
///
/// The two games number these differently — Klondike's top row is the stock,
/// the waste and four foundations — so the board is told
/// how many of each it has and never assumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    /// A slot in the top row, counted from the left.
    Top(usize),
    /// A tableau column, counted from the left.
    Column(usize),
}

impl Place {
    /// The stable token the DOM carries, and the page parses back.
    ///
    /// The *game* decides what a place means; this is only how it is spelled in
    /// an attribute, and it is the same spelling in both apps.
    #[must_use]
    pub fn slug(self) -> String {
        match self {
            Self::Top(index) => format!("top-{index}"),
            Self::Column(index) => format!("column-{index}"),
        }
    }

    /// A place from its DOM token, rejecting anything malformed.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        if let Some(rest) = slug.strip_prefix("top-") {
            return rest.parse().ok().map(Self::Top);
        }
        let rest = slug.strip_prefix("column-")?;
        rest.parse().ok().map(Self::Column)
    }

    /// The place's name in running text, for a screen reader.
    ///
    /// Generic on purpose: "column 3" and "the stock" are the game's words, and
    /// the game supplies this via [`Slot::label`]. This is only the fallback for
    /// a place nobody described.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Top(index) => format!("slot {}", index + 1),
            Self::Column(index) => format!("column {}", index + 1),
        }
    }
}

/// How a pile is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fan {
    /// The gap between consecutive face-down cards, as a fraction of the card's
    /// height.
    pub down: f64,
    /// The same for face-up cards, which need enough of themselves to show a
    /// rank.
    pub up: f64,
    /// A packet rather than a fan: every card sits on the slot's own origin.
    pub stacked: bool,
    /// When not stacked, only the topmost `fan_tail` cards are fanned and the
    /// rest sit at the origin. Zero means every card is fanned.
    pub fan_tail: usize,
}

impl Fan {
    /// A tableau column: a long fan, face-down cards closer together.
    pub const COLUMN: Self = Self {
        down: 0.13,
        up: 0.30,
        stacked: false,
        fan_tail: 0,
    };

    /// A packet: every card in one place, like a stock.
    pub const PACKET: Self = Self {
        down: 0.0,
        up: 0.0,
        stacked: true,
        fan_tail: 0,
    };

    /// A small fan, for a pile where only the last few cards must be
    /// distinguishable and the rest are history behind them.
    #[must_use]
    pub const fn tail(cards: usize) -> Self {
        Self {
            down: 0.0,
            up: 0.30,
            stacked: false,
            fan_tail: cards,
        }
    }
}

/// One card, as the board needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardSlot {
    /// The deck index. Several cards can be identical — one-suit Spider holds
    /// eight of every rank — so this is the only identity there is.
    pub id: u16,
    /// Printed face down.
    pub face_down: bool,
}

/// One pile, as the board draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// Where it is.
    pub place: Place,
    /// Its cards, bottom first.
    pub cards: Vec<CardSlot>,
    /// How the pile is drawn.
    pub fan: Fan,
    /// The faint mark an empty pile shows: `"A"`, `"K"`, `"↻"`, or nothing.
    pub mark: &'static str,
    /// What a screen reader calls this pile, including its top card and its
    /// size. Written by the game, because it is the game's words.
    pub label: String,
    /// True for a pile the player can press with nothing in hand: the stock.
    pub pressable: bool,
    /// How many column cells this pile occupies, at least one.
    ///
    /// A pile whose cards fan sideways needs room to fan into. Three cards drawn
    /// to one side of a board is the case this exists for: one cell is not
    /// enough, and a fan that spills into the next pile's cell is a fan that
    /// paints a card under that pile. Two cells give it the room the real game
    /// gives it.
    pub span: usize,
}

impl Slot {
    /// An empty slot with no mark and nothing to say.
    #[must_use]
    pub fn new(place: Place, fan: Fan, label: impl Into<String>) -> Self {
        Self {
            place,
            cards: Vec::new(),
            fan,
            mark: "",
            label: label.into(),
            pressable: false,
            span: 1,
        }
    }

    /// The same slot, with a mark for its empty state.
    #[must_use]
    pub fn with_mark(mut self, mark: &'static str) -> Self {
        self.mark = mark;
        self
    }

    /// The same slot, pressable when empty.
    #[must_use]
    pub fn pressable(mut self) -> Self {
        self.pressable = true;
        self
    }

    /// The same slot, occupying `cells` column cells rather than one.
    ///
    /// A pile that fans sideways must be given the room it fans into; see
    /// [`Slot::span`].
    #[must_use]
    pub fn spanning(mut self, cells: usize) -> Self {
        self.span = cells.max(1);
        self
    }

    /// How many column cells this pile occupies.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.span.max(1)
    }

    /// The same slot, holding these cards.
    #[must_use]
    pub fn with_cards(mut self, cards: &[(u16, bool)]) -> Self {
        self.cards = cards
            .iter()
            .map(|(id, face_down)| CardSlot {
                id: *id,
                face_down: *face_down,
            })
            .collect();
        self
    }

    /// How many cards the pile holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// True when the pile is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// The card at `index`, if the pile is that long.
    #[must_use]
    pub fn card(&self, index: usize) -> Option<CardSlot> {
        self.cards.get(index).copied()
    }

    /// The top card, if any.
    #[must_use]
    pub fn top(&self) -> Option<CardSlot> {
        self.cards.last().copied()
    }
}

/// Everything the board needs to draw one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Every pile, in draw order.
    pub slots: Vec<Slot>,
    /// How many slots the top row holds; the rest are columns, left to right.
    pub top_slots: usize,
    /// The held run: where it starts, and how long it is.
    pub held: Option<(Place, usize)>,
    /// The run a hint lights: where it starts, and how long it is.
    pub hint: Option<(Place, usize, usize)>,
    /// The pile and position the keyboard is on.
    pub focus: Option<(Place, Option<usize>)>,
}

impl Scene {
    /// A scene with no held run, no hint and no focus.
    #[must_use]
    pub fn new(slots: Vec<Slot>, top_slots: usize) -> Self {
        Self {
            slots,
            top_slots,
            held: None,
            hint: None,
            focus: None,
        }
    }

    /// The slot for a place.
    #[must_use]
    pub fn slot(&self, place: Place) -> Option<&Slot> {
        self.slots.iter().find(|slot| slot.place == place)
    }

    /// How many columns the board has.
    #[must_use]
    pub fn columns(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| matches!(slot.place, Place::Column(_)))
            .count()
    }

    /// Every place, in draw order.
    #[must_use]
    pub fn places(&self) -> Vec<Place> {
        self.slots.iter().map(|slot| slot.place).collect()
    }

    /// The cards in each place, as `(deck id, face down)`, bottom first. This is
    /// the shape a save format wants, which is why it is here rather than in
    /// either app.
    #[must_use]
    pub fn cards(&self, place: Place) -> Vec<CardSlot> {
        self.slot(place)
            .map(|slot| slot.cards.clone())
            .unwrap_or_default()
    }
}

/// How big the cards are and how much room there is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The board's own width.
    pub width: f64,
    /// The board's own height.
    pub height: f64,
    /// The gap between columns, and between the top row and the columns.
    pub gap: f64,
    /// The margin inside the board's edge.
    pub margin: f64,
}

/// The card's aspect ratio: height over width. Both games draw the same card, so
/// both use this one number, and every measurement inside a face is a multiple
/// of the width.
pub const ASPECT: f64 = 1.45;

/// Where every card and every pile is, for one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The card's width and height in pixels.
    pub card_w: f64,
    pub card_h: f64,
    /// The top-left of every pile, by place.
    pub slots: Vec<(Place, f64, f64)>,
    /// Every card's top-left, by deck index. A card that is in no pile — one
    /// that has left the board — is absent.
    pub cards: Vec<(u16, f64, f64)>,
}

impl Layout {
    /// The top-left of a place.
    #[must_use]
    pub fn place(&self, place: Place) -> Option<(f64, f64)> {
        self.slots
            .iter()
            .find(|(known, _, _)| *known == place)
            .map(|(_, x, y)| (*x, *y))
    }

    /// The top-left of a card.
    #[must_use]
    pub fn card(&self, id: u16) -> Option<(f64, f64)> {
        self.cards
            .iter()
            .find(|(known, _, _)| *known == id)
            .map(|(_, x, y)| (*x, *y))
    }

    /// A pile's rectangle: where its first card sits, one card wide and as tall
    /// as the pile is drawn. An empty pile is one card.
    #[must_use]
    pub fn pile_rect(&self, scene: &Scene, place: Place) -> Option<Rect> {
        let (x, y) = self.place(place)?;
        let slot = scene.slot(place)?;
        let height = self.pile_height(slot);
        Some(Rect {
            x,
            y,
            width: self.card_w,
            height,
        })
    }

    /// How tall a pile is drawn.
    #[must_use]
    pub fn pile_height(&self, slot: &Slot) -> f64 {
        let count = slot.cards.len();
        if count == 0 || slot.fan.stacked {
            return self.card_h;
        }
        let first_fanned = count.saturating_sub(slot.fan.fan_tail.max(count).min(count));
        let fanned = if slot.fan.fan_tail == 0 {
            0
        } else {
            first_fanned
        };
        let mut offset = 0.0;
        for index in 1..count {
            if index <= fanned {
                continue;
            }
            let below = slot.cards[index - 1];
            offset += if below.face_down {
                self.card_h * slot.fan.down
            } else {
                self.card_h * slot.fan.up
            };
        }
        self.card_h + offset
    }

    /// The card or pile under `point`, ignoring nothing.
    ///
    /// Used for a plain press, where the answer is the topmost card at that
    /// point. The topmost card in a pile is the one drawn last, so the search
    /// runs each pile's cards from the top down.
    #[must_use]
    pub fn card_at(&self, scene: &Scene, point: (f64, f64)) -> Option<(Place, usize)> {
        let mut best: Option<(Place, usize, f64)> = None;
        for slot in &scene.slots {
            let Some(rect) = self.pile_rect(scene, slot.place) else {
                continue;
            };
            if !rect.contains(point) {
                continue;
            }
            // Which card of this pile: the last one whose own rectangle holds
            // the point, which for a fanned pile is the one covering it.
            let mut hit = None;
            for index in (0..slot.cards.len()).rev() {
                let Some((x, y)) = self.card(slot.cards[index].id) else {
                    continue;
                };
                let card = Rect {
                    x,
                    y,
                    width: self.card_w,
                    height: self.card_h,
                };
                if card.contains(point) {
                    hit = Some(index);
                    break;
                }
            }
            // A pile whose cards are all above the point still answers: the
            // bottom of a short pile is the pile itself.
            let index = hit.unwrap_or(0);
            let depth = f64::from(u32::try_from(index).unwrap_or(0));
            if best.is_none_or(|(_, _, best_depth)| depth >= best_depth) {
                best = Some((slot.place, index, depth));
            }
        }
        best.map(|(place, index, _)| (place, index))
    }

    /// The place a drag should land on, given where the pointer is and where
    /// the held stack is drawn.
    ///
    /// Two questions, in order, because they are the two a player is asking:
    ///
    /// 1. **Is the pointer inside a pile?** That is the deliberate case — the
    ///    finger is on the column the card is going to — and it wins outright,
    ///    even when the dragged card also covers a neighbour.
    /// 2. **Otherwise, which pile does the held stack mostly cover?** A card
    ///    dropped short of the pile it is aimed at is still aimed at it, so the
    ///    pile under at least [`DROP_OVERLAP`] of the card wins.
    ///
    /// The answer is the pile it came from when the player put it back, and the
    /// caller treats that as "no move" rather than as a drop: a drop on the
    /// source pile is a cancel, not a move onto a pile the game would refuse.
    ///
    /// Neither question is a DOM hit test, and that is the point. The element
    /// under the pointer is the dragged card itself — it is drawn under the
    /// finger by definition — so a hit test answers "the thing you are holding"
    /// for every drop, which is the bug this replaced.
    #[must_use]
    pub fn drop_target(
        &self,
        scene: &Scene,
        point: (f64, f64),
        stack: Rect,
        from: Place,
    ) -> Option<Place> {
        let mut under_pointer = None;
        let mut best: Option<(Place, f64)> = None;
        for slot in &scene.slots {
            let Some(rect) = self.pile_rect(scene, slot.place) else {
                continue;
            };
            if under_pointer.is_none() && rect.contains(point) {
                under_pointer = Some(slot.place);
            }
            let overlap = rect.intersection_area(stack) / (self.card_w * self.card_h);
            if overlap >= DROP_OVERLAP && best.is_none_or(|(_, best)| overlap > best) {
                best = Some((slot.place, overlap));
            }
        }
        let target = under_pointer.or_else(|| best.map(|(place, _)| place));
        target.filter(|place| *place != from)
    }

    /// The place the keyboard should move to from `from`, `delta` places along
    /// the board's reading order.
    ///
    /// Reading order is the top row left to right, then the columns; a board
    /// read as a grid is the one place a keyboard user is not lost.
    #[must_use]
    pub fn next_place(&self, scene: &Scene, from: Place, delta: i32) -> Place {
        let places = scene.places();
        let Some(current) = places.iter().position(|place| *place == from) else {
            return places.first().copied().unwrap_or(Place::Top(0));
        };
        if places.is_empty() {
            return from;
        }
        let count = i32::try_from(places.len()).unwrap_or(1);
        let next = (i32::try_from(current).unwrap_or(0) + delta).rem_euclid(count);
        places[usize::try_from(next).unwrap_or(0)]
    }

    /// The position within a column the keyboard should move to, clamped to the
    /// face-up cards.
    #[must_use]
    pub fn next_index(
        &self,
        scene: &Scene,
        place: Place,
        from: Option<usize>,
        delta: i32,
    ) -> Option<usize> {
        let slot = scene.slot(place)?;
        let len = slot.cards.len();
        let first_up = (0..len)
            .find(|&index| !slot.cards[index].face_down)
            .unwrap_or(len);
        if first_up >= len {
            return None;
        }
        let low = i32::try_from(first_up).unwrap_or(0);
        let high = i32::try_from(len - 1).unwrap_or(low);
        let current = from.unwrap_or(len - 1);
        let current = i32::try_from(current.clamp(first_up, len - 1)).unwrap_or(low);
        Some(usize::try_from((current + delta).clamp(low, high)).unwrap_or(0))
    }
}

/// How much of a card must overlap a pile for a drop to land there.
///
/// A third of the card. Lower and a drag across the board lands on something the
/// player never aimed at; higher and a drop that looks placed snaps back.
pub const DROP_OVERLAP: f64 = 0.33;

/// How far a finger may drift before a press becomes a drag, in CSS pixels.
///
/// A thumb resting on a card moves a little; a finger that is dragging moves a
/// lot. Six pixels is below the jitter of a deliberate press and above the
/// noise of a still thumb.
pub const DRAG_SLOP: f64 = 6.0;

/// A rectangle, in board-local pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left.
    pub x: f64,
    /// Top.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl Rect {
    /// Whether the point is inside.
    #[must_use]
    pub fn contains(self, point: (f64, f64)) -> bool {
        point.0 >= self.x
            && point.0 <= self.x + self.width
            && point.1 >= self.y
            && point.1 <= self.y + self.height
    }

    /// The area two rectangles share.
    #[must_use]
    pub fn intersection_area(self, other: Self) -> f64 {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let width = (right - left).max(0.0);
        let height = (bottom - top).max(0.0);
        width * height
    }

    /// The same rectangle moved by a delta.
    #[must_use]
    pub fn moved(self, dx: f64, dy: f64) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// What a press has become so far.
///
/// Four states rather than two, because "not yet a drag" and "a drag that has
/// been abandoned" are different answers: the first still becomes a tap, the
/// second must not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// A finger is down, and has not moved far enough to mean anything yet.
    Down,
    /// The finger has moved past [`DRAG_SLOP`] and holds a run.
    Dragging,
    /// The gesture was cancelled — a second finger, a scroll, the browser
    /// taking the pointer away. Nothing is held and nothing is tapped.
    Cancelled,
}

/// One gesture, from the press to the release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gesture {
    /// The pointer's identity, so a second finger is recognised as somebody
    /// else's.
    pub pointer: i32,
    /// Where the press began.
    pub origin: (f64, f64),
    /// Where the pointer is now.
    pub point: (f64, f64),
    /// The pile the press landed on.
    pub place: Place,
    /// The card in that pile the press landed on.
    pub index: usize,
    /// What the press has become.
    pub state: Press,
}

impl Gesture {
    /// A press that has only just begun.
    #[must_use]
    pub fn new(pointer: i32, place: Place, index: usize, point: (f64, f64)) -> Self {
        Self {
            pointer,
            origin: point,
            point,
            place,
            index,
            state: Press::Down,
        }
    }

    /// How far the pointer has travelled.
    #[must_use]
    pub fn travel(&self) -> f64 {
        (self.point.0 - self.origin.0).hypot(self.point.1 - self.origin.1)
    }

    /// What moved: the pointer is now at `point`.
    ///
    /// Returns the state the gesture is in after the move. A gesture that was
    /// cancelled stays cancelled however far the finger travels.
    #[must_use]
    pub fn moved_to(&mut self, point: (f64, f64)) -> Press {
        self.point = point;
        if self.state == Press::Down && self.travel() >= DRAG_SLOP {
            self.state = Press::Dragging;
        }
        self.state
    }

    /// The gesture is over, normally.
    ///
    /// Returns what it was — a tap or a drag — so the caller can tell "released
    /// without dragging" from "released after dragging". A cancelled gesture
    /// returns `None`: there is nothing to finish.
    #[must_use]
    pub fn release(&self) -> Option<Press> {
        match self.state {
            Press::Down => Some(Press::Down),
            Press::Dragging => Some(Press::Dragging),
            Press::Cancelled => None,
        }
    }

    /// Abandon the gesture.
    pub fn cancel(&mut self) {
        self.state = Press::Cancelled;
    }

    /// The offset the held cards should be drawn at, as a delta from where the
    /// board put them.
    #[must_use]
    pub fn offset(&self) -> (f64, f64) {
        (self.point.0 - self.origin.0, self.point.1 - self.origin.1)
    }
}

/// Lay a scene out in a frame.
///
/// The card is as big as both the width and the height allow. Width decides it
/// when the board is wide — ten columns in Spider, seven plus a top row in
/// Klondike — and height decides it when the columns are long, because a card
/// the player cannot read is worse than a smaller board. The size is then solved
/// from the *tallest pile in this scene*, with a floor: a long game shrinks the
/// whole board rather than hiding every rank behind the next card, and only past
/// the floor does the fan itself compress.
#[must_use]
pub fn layout(scene: &Scene, frame: &Frame) -> Layout {
    let columns = scene.columns().max(1) as f64;
    let tops = scene.top_slots;
    let usable_w = (frame.width - frame.margin * 2.0).max(1.0);
    let gap = frame.gap;
    let by_width = (usable_w - gap * (columns - 1.0)) / columns;

    // The tallest pile, in card heights, as a fraction of the card's height.
    let mut tallest = 0.0_f64;
    for slot in &scene.slots {
        if matches!(slot.place, Place::Top(_)) || slot.fan.stacked {
            continue;
        }
        let count = slot.cards.len();
        if count < 2 {
            continue;
        }
        let fanned = if slot.fan.fan_tail == 0 {
            0
        } else {
            count.saturating_sub(slot.fan.fan_tail)
        };
        let mut fan = 0.0;
        for index in 1..count {
            if index <= fanned {
                continue;
            }
            fan += if slot.cards[index - 1].face_down {
                slot.fan.down
            } else {
                slot.fan.up
            };
        }
        tallest = tallest.max(fan);
    }

    // The top row, a gap, and the tallest column below it.
    //
    // Three sizes, and the middle one is what keeps a long game readable:
    //
    // * `actual` is the card that lets this scene's tallest fan fit unshrunk.
    // * `zero_fan` is the largest card for which the top row and one column
    //   card still fit at all. It is a hard ceiling: past it the board is
    //   taller than the screen whatever the fan does.
    // * `floor` is a fraction of `zero_fan`, below which the cards stop
    //   shrinking and the fan compresses instead. A small card with a legible
    //   rank corner beats a board of specks, and a fan of slivers beats both
    //   only because it is the only thing left.
    let vertical = (frame.height - frame.margin * 2.0).max(1.0);
    let zero_fan = vertical / (ASPECT * (2.0 + TOP_ROW_GAP));
    let actual = vertical / (ASPECT * (2.0 + TOP_ROW_GAP + tallest));
    let floor = zero_fan * MIN_SCALE;
    let card_w = by_width
        .min(actual)
        .max(floor.min(by_width))
        .min(zero_fan)
        .max(6.0);
    let card_h = card_w * ASPECT;

    let total_w = card_w * columns + gap * (columns - 1.0);
    let x0 = ((frame.width - total_w) / 2.0).max(frame.margin);
    let column_x = |index: usize| x0 + index as f64 * (card_w + gap);
    let top_y = frame.margin;
    let column_y = top_y + card_h + (card_h * TOP_ROW_GAP).max(4.0);

    // The top row is laid out by accumulated spans, not by index: a pile that
    // occupies two cells pushes everything after it along, which is what gives
    // a fanned pile its room and stops its fan reaching the next one.
    let mut top_cells: Vec<(Place, f64, f64)> = Vec::new();
    let mut cursor = 0usize;
    for slot in scene.slots.iter().take(tops) {
        let cell_x = column_x(cursor);
        let (_, cell_w) = cell_width(card_w, gap, slot.cells());
        top_cells.push((slot.place, cell_x, cell_w));
        cursor += slot.cells();
    }

    let mut slots = Vec::with_capacity(scene.slots.len());
    let mut cards = Vec::new();
    for slot in &scene.slots {
        let (x, y) = match slot.place {
            Place::Top(_) => top_cells
                .iter()
                .find(|(place, _, _)| *place == slot.place)
                .map_or((column_x(0), top_y), |(_, x, _)| (*x, top_y)),
            Place::Column(index) => (column_x(index), column_y),
        };
        let cell_w = match slot.place {
            Place::Top(_) => top_cells
                .iter()
                .find(|(place, _, _)| *place == slot.place)
                .map_or(card_w, |(_, _, width)| *width),
            Place::Column(_) => card_w,
        };
        slots.push((slot.place, x, y));
        let offsets = fan_offsets(slot, card_w, card_h, cell_w, vertical, column_y, frame);
        for (index, card) in slot.cards.iter().enumerate() {
            let (dx, dy) = offsets.get(index).copied().unwrap_or((0.0, 0.0));
            cards.push((card.id, x + dx, y + dy));
        }
    }

    Layout {
        card_w,
        card_h,
        slots,
        cards,
    }
}

/// How far apart a tail fan's cards are.
///
/// The natural spacing is a fraction of the card's *height*, which is wide
/// enough to show a rank corner. It is then clamped to the pile's cell: a fan
/// that leaves its cell overlaps the pile beside it, and the pile beside it
/// paints over the fan — which is how three drawn cards came to hide under a
/// pile. Only the last card's right edge matters, so the limit is
/// `cell_w - card_w` of total spread, shared between the gaps.
///
/// The floor keeps the cards distinguishable when the cell really is tight: a
/// sliver of rank is more use than one card pretending to be three.
fn tail_step(card_w: f64, card_h: f64, cell_w: f64, fan: f64, shown: usize) -> f64 {
    let natural = card_h * fan;
    if shown < 2 {
        return 0.0;
    }
    let room = (cell_w - card_w).max(0.0) / (shown - 1) as f64;
    let floor = card_w * 0.08;
    if room <= 0.0 {
        return 0.0;
    }
    natural.min(room).max(floor.min(room))
}

/// The gap between the top row and the columns, in card widths.
const TOP_ROW_GAP: f64 = 0.13;

/// The width of a pile's cell: `cells` cards and the gaps between them.
#[must_use]
fn cell_width(card_w: f64, gap: f64, cells: usize) -> (f64, f64) {
    let cells = cells.max(1);
    let width = card_w * cells as f64 + gap * (cells.saturating_sub(1)) as f64;
    (0.0, width)
}

/// The smallest the cards may shrink to before the fan compresses instead.
const MIN_SCALE: f64 = 0.56;

/// How far each card in a pile is offset from the pile's origin.
///
/// Face-down and face-up gaps are honoured, then two floors keep a squeezed
/// column readable — a face-up card below `.09h` apart is one pile of rank
/// glyphs, and a face-down one below `.055h` is a smear — and finally the whole
/// fan is scaled into the room left below the top row.
fn fan_offsets(
    slot: &Slot,
    card_w: f64,
    card_h: f64,
    cell_w: f64,
    vertical: f64,
    column_y: f64,
    frame: &Frame,
) -> Vec<(f64, f64)> {
    let count = slot.cards.len();
    let mut offsets = vec![(0.0, 0.0); count];
    if count == 0 {
        return offsets;
    }
    if slot.fan.stacked {
        return offsets;
    }
    // A tail fan goes sideways for the last few cards.
    if slot.fan.fan_tail > 0 {
        let first = count.saturating_sub(slot.fan.fan_tail);
        let shown = count - first;
        let step = tail_step(card_w, card_h, cell_w, slot.fan.up, shown);
        for (index, offset) in offsets.iter_mut().enumerate().skip(first) {
            let nth = f64::from(u32::try_from(index - first).unwrap_or(0));
            *offset = (nth * step, 0.0);
        }
        return offsets;
    }

    let room = (vertical - (column_y - frame.margin) - card_h).max(0.0);
    let mut gaps = Vec::with_capacity(count.saturating_sub(1));
    for index in 1..count {
        let below = slot.cards[index - 1];
        let gap = if below.face_down {
            card_h * slot.fan.down
        } else {
            card_h * slot.fan.up
        };
        gaps.push(gap);
    }
    let natural: f64 = gaps.iter().sum();
    if natural > room && natural > 0.0 {
        let scale = room / natural;
        for gap in &mut gaps {
            *gap *= scale;
        }
    }
    let mut offset = 0.0;
    for (index, gap) in gaps.into_iter().enumerate() {
        offset += gap;
        offsets[index + 1] = (0.0, offset);
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: f64, height: f64) -> Frame {
        Frame {
            width,
            height,
            gap: 6.0,
            margin: 8.0,
        }
    }

    fn column(place: usize, cards: &[(u16, bool)]) -> Slot {
        Slot {
            place: Place::Column(place),
            cards: cards
                .iter()
                .map(|(id, face_down)| CardSlot {
                    id: *id,
                    face_down: *face_down,
                })
                .collect(),
            fan: Fan::COLUMN,
            mark: "",
            label: format!("column {}", place + 1),
            pressable: false,
            span: 1,
        }
    }

    fn top(place: usize, cards: &[(u16, bool)], fan: Fan) -> Slot {
        Slot {
            place: Place::Top(place),
            cards: cards
                .iter()
                .map(|(id, face_down)| CardSlot {
                    id: *id,
                    face_down: *face_down,
                })
                .collect(),
            fan,
            mark: "",
            label: format!("slot {}", place + 1),
            pressable: true,
            span: 1,
        }
    }

    /// A scene shaped like a real Klondike board: seven columns, and a top row
    /// of six piles where the fanned one takes two cells — which is the whole
    /// point of `span`.
    fn simple_scene() -> Scene {
        let mut slots = vec![
            top(0, &[(0, true), (1, true), (2, true)], Fan::PACKET),
            Slot {
                span: 2,
                ..top(1, &[(3, false), (4, false), (5, false)], Fan::tail(3))
            },
            top(2, &[], Fan::PACKET),
            top(3, &[], Fan::PACKET),
            top(4, &[], Fan::PACKET),
            top(5, &[], Fan::PACKET),
            column(0, &[(10, true), (11, false), (12, false)]),
            column(1, &[(20, false)]),
            column(2, &[]),
            column(3, &[]),
            column(4, &[]),
            column(5, &[]),
            column(6, &[]),
        ];
        slots.shrink_to_fit();
        Scene::new(slots, 6)
    }

    #[test]
    fn a_place_round_trips_through_its_dom_token() {
        for place in [
            Place::Top(0),
            Place::Top(3),
            Place::Column(0),
            Place::Column(9),
        ] {
            assert_eq!(Place::from_slug(&place.slug()), Some(place));
        }
        assert_eq!(Place::from_slug("column-"), None);
        assert_eq!(Place::from_slug("column-x"), None);
        assert_eq!(Place::from_slug("pile-0"), None);
        assert_eq!(Place::from_slug(""), None);
    }

    #[test]
    fn the_layout_puts_every_column_on_one_row_and_every_top_slot_above_it() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let mut top_y = None;
        let mut column_y = None;
        for slot in &scene.slots {
            let (_, y) = layout.place(slot.place).expect("every place is placed");
            match slot.place {
                Place::Top(_) => {
                    assert_eq!(*top_y.get_or_insert(y), y, "the top row is one row");
                }
                Place::Column(_) => {
                    assert_eq!(*column_y.get_or_insert(y), y, "the columns are one row");
                }
            }
        }
        assert!(column_y.expect("columns") > top_y.expect("top row"));
        // And each column is to the right of the one before it.
        let mut last = f64::MIN;
        for index in 0..3 {
            let (x, _) = layout.place(Place::Column(index)).expect("placed");
            assert!(x > last, "columns are laid out left to right");
            last = x;
        }
    }

    #[test]
    fn a_spanning_pile_pushes_the_piles_after_it_along() {
        // The top row is a packet (one cell), a fanned pile (two cells), then
        // three single cells. So the pile after the fanned one sits above the
        // *fourth* column, not the third — which is what gives the fan room.
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let x = |place| layout.place(place).expect("placed").0;
        assert_eq!(x(Place::Top(0)), x(Place::Column(0)), "the first slot");
        assert_eq!(x(Place::Top(1)), x(Place::Column(1)), "the fanned slot");
        assert_eq!(
            x(Place::Top(2)),
            x(Place::Column(3)),
            "the pile after a two-cell pile starts two cells along"
        );
        assert_eq!(x(Place::Top(5)), x(Place::Column(6)), "the last top slot");
    }

    #[test]
    fn a_long_column_shrinks_the_cards_rather_than_hiding_the_ranks() {
        let short = Scene::new(
            vec![column(0, &[(0, false), (1, false)]), column(1, &[])],
            0,
        );
        let long = Scene::new(
            vec![
                column(0, &(0..20).map(|id| (id, false)).collect::<Vec<_>>()),
                column(1, &[]),
            ],
            0,
        );
        let short_layout = layout(&short, &frame(1200.0, 900.0));
        let long_layout = layout(&long, &frame(1200.0, 900.0));
        assert!(
            long_layout.card_w < short_layout.card_w,
            "twenty cards must make the cards smaller: {} vs {}",
            long_layout.card_w,
            short_layout.card_w
        );
        assert!(
            long_layout.card_w >= short_layout.card_w * MIN_SCALE - 0.01,
            "but never below the floor: {} vs {}",
            long_layout.card_w,
            short_layout.card_w * MIN_SCALE
        );
    }

    #[test]
    fn the_top_row_and_one_column_card_always_fit_on_the_board() {
        // The invariant that keeps a board usable however long a column gets:
        // whatever the fan does, the first card of a column is on screen.
        for count in [1usize, 5, 20, 100] {
            let scene = Scene::new(
                vec![
                    top(0, &[(999, true)], Fan::PACKET),
                    column(
                        0,
                        &(0..count as u16).map(|id| (id, false)).collect::<Vec<_>>(),
                    ),
                    column(1, &[]),
                ],
                1,
            );
            for (width, height) in [(360.0, 640.0), (800.0, 700.0), (1200.0, 900.0)] {
                let layout = layout(&scene, &frame(width, height));
                let (_, top_y) = layout.place(Place::Top(0)).expect("the stock");
                let (_, column_y) = layout.place(Place::Column(0)).expect("the column");
                assert!(
                    column_y > top_y,
                    "{count} cards at {width}x{height}: the rows must not overlap"
                );
                assert!(
                    column_y - top_y >= layout.card_h * 0.99,
                    "{count} cards at {width}x{height}: the top row must clear the column"
                );
                let first = layout.card(0).expect("the first card is placed");
                assert!(
                    first.1 >= column_y - 0.5,
                    "{count} cards at {width}x{height}: the column starts below the top row"
                );
            }
        }
    }

    #[test]
    fn a_face_down_gap_is_closer_than_a_face_up_one() {
        let scene = Scene::new(
            vec![column(0, &[(0, true), (1, true), (2, false), (3, false)])],
            0,
        );
        let layout = layout(&scene, &frame(900.0, 1200.0));
        let gap = |a: u16, b: u16| {
            let (_, ya) = layout.card(a).expect("placed");
            let (_, yb) = layout.card(b).expect("placed");
            yb - ya
        };
        assert!(
            gap(0, 1) < gap(2, 3),
            "face-down cards show less of themselves"
        );
    }

    #[test]
    fn a_stacked_pile_puts_every_card_in_one_place() {
        let scene = Scene::new(
            vec![top(0, &[(0, true), (1, true), (2, true)], Fan::PACKET)],
            1,
        );
        let layout = layout(&scene, &frame(600.0, 400.0));
        let first = layout.card(0).expect("placed");
        assert_eq!(layout.card(1), Some(first));
        assert_eq!(layout.card(2), Some(first));
        assert_eq!(
            layout.pile_height(scene.slot(Place::Top(0)).expect("slot")),
            layout.card_h
        );
    }

    #[test]
    fn a_tail_fan_spreads_only_its_last_few_cards() {
        let scene = Scene::new(
            vec![Slot {
                span: 2,
                ..top(
                    0,
                    &[(0, false), (1, false), (2, false), (3, false), (4, false)],
                    Fan::tail(3),
                )
            }],
            1,
        );
        let layout = layout(&scene, &frame(900.0, 600.0));
        let x = |id: u16| layout.card(id).expect("placed").0;
        assert_eq!(x(0), x(1), "the first cards are history");
        assert!(x(2) < x(3) && x(3) < x(4), "the last three fan out");
        assert!(
            x(4) + layout.card_w
                <= layout.place(Place::Top(0)).expect("placed").0 + 2.0 * layout.card_w,
            "and they stay inside the pile's two cells"
        );
    }

    /// The reported defect: three fanned cards hid partially under the pile
    /// beside them.
    ///
    /// A fan spreads by a fraction of the card's height, which is wider than a
    /// card's cell — so the third card's right edge landed inside the next
    /// pile's cell, and that pile painted over it. The fan now stays inside its
    /// own cell, and the cell is two wide.
    #[test]
    fn a_three_card_fan_never_reaches_the_next_pile() {
        let scene = simple_scene();
        for (width, height) in [(1280.0, 800.0), (1000.0, 700.0), (390.0, 844.0)] {
            let layout = layout(&scene, &frame(width, height));
            let last = layout.card(5).expect("the third fanned card is placed");
            let (next_x, _) = layout.place(Place::Top(2)).expect("the next pile");
            assert!(
                last.0 + layout.card_w <= next_x + 0.01,
                "at {width}x{height} the fan's last card reaches {:.1}, past the next \
                 pile at {next_x:.1}",
                last.0 + layout.card_w
            );
        }
    }

    /// A pile with no room to fan does not fan into its neighbour.
    ///
    /// The alternative — spreading anyway because that is what the fraction
    /// says — is exactly the bug above. A pile that wants a fan asks for the
    /// cells; one that does not gets a packet.
    #[test]
    fn a_fan_with_no_room_holds_its_cards_together_rather_than_spilling() {
        let scene = Scene::new(
            vec![
                top(0, &[(0, false), (1, false), (2, false)], Fan::tail(3)),
                top(1, &[], Fan::PACKET),
            ],
            2,
        );
        let layout = layout(&scene, &frame(900.0, 600.0));
        let x = |id: u16| layout.card(id).expect("placed").0;
        assert_eq!(x(0), x(1));
        assert_eq!(x(1), x(2));
        let (next, _) = layout.place(Place::Top(1)).expect("the next pile");
        assert!(x(2) + layout.card_w <= next + 0.01);
    }

    #[test]
    fn a_card_is_found_under_the_point_that_covers_it() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let (x, y) = layout.place(Place::Column(0)).expect("placed");
        // Near the top of the column: the stack's top card sits lowest, so the
        // answer depends on where in the pile the point is.
        let hit = layout
            .card_at(&scene, (x + layout.card_w / 2.0, y + 4.0))
            .expect("a card is there");
        assert_eq!(hit.0, Place::Column(0));
    }

    #[test]
    fn a_point_below_every_pile_finds_nothing() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        assert_eq!(layout.card_at(&scene, (500.0, 799.0)), None);
    }

    #[test]
    fn an_empty_pile_is_still_a_target() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let (x, y) = layout.place(Place::Column(2)).expect("placed");
        let hit = layout
            .card_at(&scene, (x + 5.0, y + 5.0))
            .expect("the empty place itself");
        assert_eq!(hit, (Place::Column(2), 0));
    }

    #[test]
    fn a_drop_puts_the_card_on_the_pile_under_the_finger() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let (x, y) = layout.place(Place::Column(1)).expect("placed");
        let point = (x + layout.card_w / 2.0, y + layout.card_h / 2.0);
        // The stack is drawn offset from the finger; the pile under the finger
        // wins regardless of what the stack covers.
        let stack = Rect {
            x: x - layout.card_w,
            y,
            width: layout.card_w,
            height: layout.card_h,
        };
        assert_eq!(
            layout.drop_target(&scene, point, stack, Place::Column(0)),
            Some(Place::Column(1))
        );
    }

    #[test]
    fn a_drop_short_of_the_pile_still_lands_on_it() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let (x, y) = layout.place(Place::Column(1)).expect("placed");
        // The finger is past the bottom of the pile, on the empty table; the
        // held card still covers most of the pile it was aimed at.
        let point = (x + layout.card_w / 2.0, y + layout.card_h * 2.0);
        let stack = Rect {
            x,
            y,
            width: layout.card_w,
            height: layout.card_h,
        };
        assert_eq!(
            layout.drop_target(&scene, point, stack, Place::Column(0)),
            Some(Place::Column(1)),
            "a card dropped roughly on a pile is dropped on it"
        );
    }

    #[test]
    fn a_drop_in_empty_space_lands_nowhere() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let stack = Rect {
            x: 500.0,
            y: 780.0,
            width: layout.card_w,
            height: layout.card_h,
        };
        assert_eq!(
            layout.drop_target(&scene, (520.0, 790.0), stack, Place::Column(0)),
            None,
            "the empty table below the board is not a pile"
        );
    }

    #[test]
    fn dropping_a_card_back_where_it_came_from_is_not_a_drop() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let (x, y) = layout.place(Place::Column(0)).expect("placed");
        let point = (x + layout.card_w / 2.0, y + layout.card_h / 2.0);
        let stack = Rect {
            x,
            y,
            width: layout.card_w,
            height: layout.card_h,
        };
        assert_eq!(
            layout.drop_target(&scene, point, stack, Place::Column(0)),
            None
        );
    }

    #[test]
    fn a_drop_targets_an_empty_column_as_readily_as_a_full_one() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        let (x, y) = layout.place(Place::Column(2)).expect("placed");
        let point = (x + 4.0, y + 4.0);
        let stack = Rect {
            x,
            y,
            width: layout.card_w,
            height: layout.card_h,
        };
        assert_eq!(
            layout.drop_target(&scene, point, stack, Place::Column(0)),
            Some(Place::Column(2))
        );
    }

    #[test]
    fn the_keyboard_moves_along_the_reading_order_and_wraps() {
        let scene = simple_scene();
        let layout = layout(&scene, &frame(1000.0, 800.0));
        // The last top slot; the next stop is the first column, and stepping
        // back from it returns to that slot.
        assert_eq!(
            layout.next_place(&scene, Place::Top(5), 1),
            Place::Column(0)
        );
        assert_eq!(
            layout.next_place(&scene, Place::Column(0), -1),
            Place::Top(5)
        );
        assert_eq!(
            layout.next_place(&scene, Place::Column(6), 1),
            Place::Top(0),
            "the board wraps rather than stopping"
        );
    }

    #[test]
    fn the_keyboard_stays_on_the_face_up_cards_of_a_column() {
        let scene = Scene::new(
            vec![column(0, &[(0, true), (1, true), (2, false), (3, false)])],
            0,
        );
        let layout = layout(&scene, &frame(900.0, 700.0));
        // From the top card, up twice reaches the lowest face-up card and stops.
        assert_eq!(
            layout.next_index(&scene, Place::Column(0), Some(3), -1),
            Some(2)
        );
        assert_eq!(
            layout.next_index(&scene, Place::Column(0), Some(2), -1),
            Some(2),
            "a face-down card is not a keyboard stop"
        );
        assert_eq!(
            layout.next_index(&scene, Place::Column(0), Some(3), 1),
            Some(3)
        );
    }

    #[test]
    fn a_press_becomes_a_drag_only_after_it_has_travelled() {
        let mut gesture = Gesture::new(1, Place::Column(0), 2, (100.0, 100.0));
        assert_eq!(gesture.moved_to((102.0, 101.0)), Press::Down);
        assert_eq!(gesture.release(), Some(Press::Down), "that was a tap");
        assert_eq!(gesture.moved_to((120.0, 100.0)), Press::Dragging);
        assert_eq!(
            gesture.offset(),
            (20.0, 0.0),
            "the held cards follow the finger exactly"
        );
        assert_eq!(gesture.release(), Some(Press::Dragging));
    }

    #[test]
    fn a_cancelled_gesture_is_neither_a_tap_nor_a_drop() {
        let mut gesture = Gesture::new(7, Place::Column(1), 0, (10.0, 10.0));
        gesture.cancel();
        assert_eq!(gesture.moved_to((90.0, 90.0)), Press::Cancelled);
        assert_eq!(gesture.release(), None);
    }

    #[test]
    fn a_second_pointer_is_not_part_of_the_first_gesture() {
        // The gesture carries the pointer that started it, so the page can tell
        // a second finger — a pinch, a stray palm — from more of the same drag.
        let gesture = Gesture::new(3, Place::Column(0), 0, (0.0, 0.0));
        assert_eq!(gesture.pointer, 3);
    }

    #[test]
    fn rectangles_intersect_by_area_and_never_negatively() {
        let a = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let b = Rect {
            x: 5.0,
            y: 5.0,
            width: 10.0,
            height: 10.0,
        };
        assert_eq!(a.intersection_area(b), 25.0);
        let far = Rect {
            x: 100.0,
            y: 100.0,
            width: 10.0,
            height: 10.0,
        };
        assert_eq!(a.intersection_area(far), 0.0);
        assert!(a.contains((0.0, 0.0)));
        assert!(a.contains((10.0, 10.0)));
        assert!(!a.contains((10.1, 0.0)));
        assert_eq!(b.moved(-5.0, -5.0), a);
    }

    #[test]
    fn an_empty_scene_lays_out_without_panicking() {
        let scene = Scene::new(Vec::new(), 0);
        let layout = layout(&scene, &frame(0.0, 0.0));
        assert!(layout.card_w > 0.0);
        assert!(layout.cards.is_empty());
        assert_eq!(layout.next_place(&scene, Place::Top(0), 1), Place::Top(0));
    }
}
