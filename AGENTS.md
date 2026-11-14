# AGENTS.md

Decisions and verification for this app. `INSTRUCTIONS.md` is the Rust PWA
template's guide: this crate was cut from it, and every rule there applies here.
What follows is what is specific to this game.

## Shape

Four of these files are **shared, byte for byte, with Solitaire**:

- `src/card.rs` — the deck: `Rank`, `Suit`, `Card`, the two-character storage
  code, the printed short form.
- `src/board.rs` — the geometry: where every card and pile sits, which card a
  point is over, and where a dragged stack lands. Pure arithmetic, no web-sys.
- `src/board_ui.rs` — the DOM: one element per deck index, a render that moves
  cards rather than rebuilding them, and the gesture state machine.
- `src/shell.rs` — the page: the header, the four buttons, the folded menu, the
  clock, saved state, the service worker.

This game's own files:

- `src/game.rs` — Spider: the deal, the move grammar, completed runs, the score,
  the undo history, the hint heuristic, the save format, and **every word the
  page shows**. No `web-sys`, so `cargo test` runs it.
- `src/scene.rs` — the game as the board draws it: the piles, how each fans,
  where the keyboard starts. Compiled for the host too, so the board's
  arrangement is something `cargo test` can check.
- `src/ui.rs` — a `Rules` implementation: what each intent means, and the two
  decorations this board has. It binds no DOM events and holds no elements.
- `src/ui.html`, `src/style.css`, `src/service-worker.js`, `build.rs`,
  `assets/icon.svg` — the shell, the styles, the worker, the site writer and the
  icon, as the template describes them.

The shared files carry a test that keeps them shareable: none of them may name
either game, nor either game's piles or rules. Editing one is editing both apps,
and the rule is that a change belongs in both.

## Decisions

**Difficulty is the suit count.** That is the axis every established
implementation uses, and it is not a slider for its own sake: with one suit
every descending sequence is also a suit sequence, so any group can be moved and
any complete run can be lifted; with four suits most neighbours are a mixed
pair. Every difficulty deals exactly 104 cards — eight copies of one suit's
thirteen ranks, four of two suits, or the ordinary two decks — which is what
makes the stock's five deals of ten work at all three. The menu gives each one
its rule in a sentence, because "Easy" alone tells a player nothing.

**A run is one suit, descending, ending at the column's top card.** That is the
rule that makes a run a thing worth seeing, and the one every serious
implementation keeps. It is also the difference between this game and Klondike,
so it is asserted directly (`only_an_in_suit_descending_run_lifts_as_a_group`).

**The stock deals only while no column is empty.** The classic rule, and the
reason an empty column is a resource rather than a convenience. It is checked
before the snapshot is taken, so a refused deal leaves nothing to undo.

**Scoring is standard or none, and undo has a rule of its own.** Standard starts
at 500, charges a point per move, pays 100 for each completed run — and an undo
takes back what the action *earned*, keeps what it *cost* and charges itself a
point. That is what stops a player farming points by finishing the same run
twice; a plain snapshot restore would pay out again on the repeat. The move
counter goes back with the board, because it counts the moves that are on it.
"No score" turns all of it off, and there is no arithmetic anywhere in that mode.

**One transaction per mutation.** Every move, deal and completion takes a
snapshot first. It matters more here than in Klondike: one move can complete a
run and take thirteen cards off the board, and undo has to put all of them back.

**Every deal is a seed.** The same seed deals the same game, so "Restart deal"
is honest and a save is a seed plus the piles.

**A saved game is checked, not trusted.** The deck holds several identical cards
at every difficulty, so the check is a multiset one: each code is matched to the
first *unused* copy, and at the end every card must have been claimed exactly
once. Completed runs are stored as their suit, and loading marks the thirteen
cards that run took — which is the case a naive per-card check gets wrong, and it
is covered by its own test. A save that claims a run while all thirteen of its
cards are still on the board is refused.

**A deal adds one face-up card to each column and turns nothing over.** `up` is
the index of the *first* face-up card, so `up = len - 1` after a deal hides the
whole column. That was a real defect, reported from play; it is now pinned by
two tests, one on a hand-built column and one that deals five times and checks
the face-up count never goes down.

**A pile that fans sideways is given the cells it fans into.** Spider's columns
fan downwards, inside their own cell, so this game asks for no extra cells — but
the board that guarantees it is shared, and it is the board that decides. A pile
occupying two cells pushes the next pile along, and a fan is clamped to its own
cell; the test that says no pile here asks for more than one is what keeps this
game's row as one stock plus nine free cells for the runs strip.

**Cards are placed with `left`/`top`, not `transform`.** A transform transition
is compositor-driven: a renderer that does not advance animations draws every
card in the top-left corner — a board that looks empty and works. `left`/`top`
animate on the main thread, so a card is in the right place at frame zero.

**A card that has left the board is hidden.** The deck is built once and never
rebuilt, so when a run is lifted its thirteen elements stay in the document.
They are hidden *and* stripped of their pile and position, so nothing that walks
the board by name can find a ghost.

**The face has a compact form, chosen from the card's width.** Ten columns on a
phone make cards too narrow for the big centre pip to be anything but decoration
competing with the rank in the corner, so below 60px the pip goes and the corner
grows — the same trade a bridge-size printed card makes. The class is set from
Rust, where the card's actual width is known, rather than from a media query on
the viewport: the board's width is what matters, and it is not the window's.

**The board's decorations are the game's.** The strip of completed runs and the
count on the stock are drawn by this app's `decorate`, over a board that knows
nothing about runs. They are anchored to the board's own layout — x from the
columns, y from the top row — so they move with it at any size.

## What is on the board rather than in the header

The completed-runs strip and the stock's deal count are drawn into the board,
beside the things they are about. They are facts about the board, and a reader
watching the board should not have to look away from it. The header keeps the
three facts that belong to the sitting: the clock, the move count and the score.

## Verification

```bash
cargo test --locked                 # the rules, the save format, the shell
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --lib --target wasm32-unknown-unknown -- -D warnings
```

Then both build steps, in order, and check `dist/` — see `README.md`. The built
site was also driven in a headless browser against the real page with real
pointer events: a deal (checking that every column gained exactly one face-up
card and no face-down card changed), a drag with the drop ring visible mid-flight
and the card following the finger to the pixel, a tap-to-move, an undo, a
completed run lifting thirteen cards off the board and filling a pip, and the
service worker taking control. Those checks are not committed tests — the
browser is not part of the toolchain here — and they are written down so the next
reader knows what "verified" covered.

What is *not* covered by a test: the layout's geometry. Card sizes, fan offsets
and the compact-face threshold all depend on the box the board gets, and the
only honest check is to look at it at a few sizes.

## Sharing the card format with Solitaire

`src/card.rs` and the `.card` block of `src/style.css` are also carried by
[solitaire](https://github.com/bot-git-ai/solitaire). The two are separate
repositories, so nothing enforces that they agree; the rule is that a change to
either belongs in both. The deck, the codes and the printed faces are the shared
surface. Everything else — the piles, the runs strip, the scoring, the compact
face — is this game's own.
