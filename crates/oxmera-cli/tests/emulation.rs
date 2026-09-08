//! What the emulator can and cannot see of `oxmera`, and whether a screen
//! survives being written down — the assertions the rest of this suite
//! rests on.
//!
//! Every other test here reads a grid a VT emulator produced from oxmera's
//! bytes, and five golden files are compared against it. If oxmera emits a
//! sequence the emulator does not implement, that grid is quietly wrong and
//! every screen assertion in this crate is being made against a
//! plausible-looking fiction — including the goldens, which would be
//! blessed from the same fiction. termlens 0.10 made that checkable:
//! `Screen::unsupported` lists what was dropped.
//!
//! These are whole-suite invariants, not feature tests. They are cheap, and
//! when one breaks the right response is to distrust the other files until
//! it is understood. They live in their own test target so `stress.yml`'s
//! `--test doctor --test tui` selection stays a stress-only run.

use std::path::{Path, PathBuf};
use std::time::Duration;

use termlens::{Screen, Terminal};

const TIMEOUT: Duration = Duration::from_secs(20);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The only sequence the dashboard emits that termlens does not model.
///
/// `SGR 59` is "underline colour: default", which ratatui 0.30 writes as
/// part of resetting a style. termlens carries no underline colour, so it
/// records the sequence and moves on — and because the attribute changes no
/// cell, nothing on the grid is wrong as a result. That is the whole reason
/// this list can be pinned exactly: anything joining it is a sequence that
/// *might* change a cell, and would need reading before the goldens are
/// trusted again.
///
/// It is **not** the known termlens false positive (termlens#320), which
/// reports `^[[5m`/`^[[25m`/`^[[9m`/`^[[29m` — blink and strikethrough —
/// as unsupported although the attribute shadow does implement them. Those
/// cannot appear here: the dashboard sets neither attribute on any cell, as
/// `tests/styles.rs` pins by enumerating every distinct style it draws.
const DASHBOARD_UNSUPPORTED: [&str; 1] = ["^[[59m"];

fn unsupported(screen: &Screen) -> Vec<String> {
    screen.unsupported().iter().map(|s| s.to_string()).collect()
}

/// The dashboard, painted and settled, at a given geometry.
fn dashboard(cols: u16, rows: u16) -> termlens::Result<(Terminal, Screen)> {
    let mut t = Terminal::builder()
        .size(cols, rows)
        .env_clear()
        .timeout(TIMEOUT)
        .arg("train")
        .arg("--replay")
        .arg(fixture("train-replay.toml"))
        .spawn(env!("CARGO_BIN_EXE_oxmera"))?;
    let screen = t.snapshot_after(|s| s.contains("training complete — press q"))?;
    Ok((t, screen))
}

/// The report on a fixture, at a given geometry, after the process exited.
fn report(name: &str, cols: u16, rows: u16) -> termlens::Result<Screen> {
    let mut t = Terminal::builder()
        .size(cols, rows)
        .env_clear()
        .timeout(TIMEOUT)
        .arg("doctor")
        .arg("--fixture")
        .arg(fixture(name))
        .spawn(env!("CARGO_BIN_EXE_oxmera"))?;
    let screen = t.snapshot_after(|s| s.contains("doctor: report complete"))?;
    assert!(t.wait_exit()?.success(), "doctor must exit 0");
    Ok(screen)
}

/// The invariant, on both binaries and at both golden geometries.
#[test]
fn the_emulator_drops_nothing_that_could_change_a_cell() -> termlens::Result<()> {
    // The dashboard: ratatui's style resets, and nothing else.
    for (cols, rows) in [(100, 45), (80, 30)] {
        let (mut t, screen) = dashboard(cols, rows)?;
        assert_eq!(
            unsupported(&screen),
            DASHBOARD_UNSUPPORTED,
            "dashboard at {cols}x{rows}: oxmera emitted a sequence termlens \
             does not model. Until it is understood, every screen assertion \
             in this crate — the goldens included — is being made against a \
             grid that may be wrong.\n{screen}"
        );
        assert_eq!(
            screen.unsupported_overflow(),
            0,
            "dashboard at {cols}x{rows}: the record is complete, not truncated"
        );
        t.send(termlens::Key::Char('q'))?;
        t.wait_exit()?;
    }

    // The report is a stronger claim: it is plain text, so the emulator had
    // nothing to drop at all. `doctor` is a program whose whole output is
    // meant to be pasteable into an issue; an escape sequence appearing in
    // it is a bug, and this is the assertion that would say so.
    for name in ["no-gpu.toml", "metal.toml", "cuda.toml"] {
        for (cols, rows) in [(100, 45), (80, 45)] {
            let screen = report(name, cols, rows)?;
            assert!(
                unsupported(&screen).is_empty(),
                "doctor {name} at {cols}x{rows}: the report is supposed to be \
                 plain text, but termlens dropped {:?}",
                unsupported(&screen)
            );
            assert_eq!(screen.unsupported_overflow(), 0);
            let styled = (0..screen.rows())
                .flat_map(|r| (0..screen.cols()).map(move |c| (r, c)))
                .filter(|(r, c)| {
                    screen
                        .cell(*r, *c)
                        .is_some_and(|cell| *cell.style() != termlens::Style::default())
                })
                .count();
            assert_eq!(
                styled, 0,
                "doctor {name} at {cols}x{rows}: {styled} cells carry a style. \
                 The report is documented as plain text — a golden compares \
                 `to_string()` and would not notice colour arriving."
            );
        }
    }
    Ok(())
}

/// Three smaller invariants that would each make the grid a lie, and that
/// nothing else in this crate would notice.
#[test]
fn oxmera_leaves_the_terminal_modes_alone() -> termlens::Result<()> {
    let (mut t, dash) = dashboard(100, 45)?;
    let doctor = report("metal.toml", 100, 45)?;

    for (what, screen) in [("dashboard", &dash), ("doctor", &doctor)] {
        // Insert mode pushes the rest of a row right. An application that
        // left it on would draw a correct-looking frame with every row
        // shifted, and the goldens would have been blessed from it.
        assert!(!screen.insert_mode(), "{what}: never sets IRM");
        // A visual bell is a flash the grid cannot show, and an audible one
        // is a noise in someone's office.
        assert_eq!(screen.visual_bells(), 0, "{what}: rings no visual bell");
        assert_eq!(screen.bells(), 0, "{what}: and no audible one either");
        // Nothing wraps at the geometry the goldens were taken at: oxmera
        // lays out to the width it was given, so a wrapped row means the
        // layout overflowed and a line is silently on two rows.
        assert!(
            !(0..screen.rows()).any(|row| screen.row_wrapped(row)),
            "{what}: a wrapped row means the layout overflowed:\n{screen}"
        );
        // No hyperlinks and no OSC title: oxmera renames nobody's terminal.
        assert_eq!(screen.links().len(), 0, "{what}: writes no OSC 8 links");
        assert_eq!(screen.title(), "", "{what}: never sets the window title");
    }

    // The dashboard specifically never enables mouse reporting. This is a
    // product property, not a detail: while an application holds the mouse,
    // the terminal's own text selection and copy stop working, and a user
    // reading a loss curve wants to be able to select it. It is also why
    // `Terminal::click` is nowhere in this suite.
    assert!(
        dash.mouse_modes().is_empty(),
        "the dashboard enabled mouse reporting ({:?}), which takes text \
         selection away from the user",
        dash.mouse_modes()
    );
    // And it leaves the cursor's shape alone while hiding it, so the shell
    // gets back the block/bar the user had configured. `signals.rs` pins
    // that the cursor comes back *visible*; this pins that it comes back
    // unchanged.
    assert_eq!(dash.cursor_shape(), termlens::CursorShape::Default);
    assert_eq!(dash.cursor_blink(), None);

    t.send(termlens::Key::Char('q'))?;
    t.wait_exit()?;
    Ok(())
}

/// A dashboard frame has to survive being written down and read back,
/// because that is what a bug report is — and, since 0.10, what
/// `TERMLENS_ARTIFACT_DIR` writes when a wait fails in CI.
///
/// The colour is the half a plain text comparison cannot check, and colour
/// is most of what this frame is: two sparklines, two gauges, a bold header
/// and a reversed footer.
#[test]
fn a_dashboard_frame_survives_the_snapshot_format_and_json() -> termlens::Result<()> {
    let (mut t, screen) = dashboard(100, 45)?;

    // The text format: an insta `.snap`, the block a wait error prints, a
    // `TERMLENS_ARTIFACT_DIR` file, and what `termlens diff` reads.
    let saved = screen.with_styles().to_string();
    let parsed = Screen::parse(&saved)?;
    assert!(
        screen.diff(&parsed).is_empty(),
        "the text format lost something:\n{}",
        screen.diff(&parsed)
    );
    assert_eq!(
        parsed.with_styles().to_string(),
        saved,
        "and it round-trips byte for byte"
    );

    // JSON, which is the `.screen.json` the serde feature writes under
    // TERMLENS_ARTIFACT_DIR — the file `ci.yml`'s report action renders.
    let json = serde_json::to_string(&screen).expect("a Screen serializes");
    let back: Screen = serde_json::from_str(&json).expect("and comes back");
    assert!(
        screen.diff(&back).is_empty(),
        "JSON lost something:\n{}",
        screen.diff(&back)
    );

    // The palette specifically. A round trip that dropped every style would
    // still pass both comparisons above if `diff` only looked at text, so
    // name a cell that is coloured and check the colour itself: this is the
    // LightRed loss sparkline, the top-left cell of its block.
    let (row, col) = screen
        .find_by(|cell| cell.style().fg == termlens::Color::Indexed(9))
        .expect("the loss sparkline is drawn in LightRed");
    assert_eq!(
        back.cell(row, col).map(|c| *c.style()),
        screen.cell(row, col).map(|c| *c.style()),
        "the sparkline colour did not survive JSON"
    );
    assert_eq!(
        parsed.cell(row, col).map(|c| *c.style()),
        screen.cell(row, col).map(|c| *c.style()),
        "the sparkline colour did not survive the text format"
    );

    t.send(termlens::Key::Char('q'))?;
    t.wait_exit()?;
    Ok(())
}

/// The committed goldens are in the interchange format, and still parse.
///
/// `assert_golden` compares them as strings, so they would go on passing
/// after a change to `normalize` left them readable by nothing but this
/// crate. They are the artefact a maintainer reaches for when a frame
/// changed — `termlens render --svg tests/golden/…` puts one in a bug
/// report — and that only works while they are the snapshot text format.
#[test]
fn every_committed_golden_is_a_readable_screen() -> termlens::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("tests/golden exists") {
        let path = entry.expect("readable entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable golden");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let screen = Screen::parse(&text)
            .unwrap_or_else(|e| panic!("{name} is no longer a saved screen: {e}"));
        assert_eq!(
            screen.size(),
            if name.contains("80x30") {
                (80, 30)
            } else {
                (100, 45)
            },
            "{name}: the header's geometry is the one the filename claims"
        );
        seen += 1;
    }
    assert_eq!(seen, 5, "the five goldens this crate commits");
    Ok(())
}
