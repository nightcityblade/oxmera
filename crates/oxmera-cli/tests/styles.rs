//! The dashboard's colour — the half of the frame the goldens cannot see.
//!
//! `tests/tui.rs` compares `screen.to_string()` against two golden files.
//! That is the *text* of the frame and nothing else, so until this file
//! existed you could swap the loss and accuracy sparkline colours, drop the
//! header's bold, lose the gauge cyan entirely, or paint the whole
//! dashboard red, and every test in this crate stayed green. Colour is most
//! of what a training dashboard communicates at a glance, so that was the
//! largest hole in the suite.
//!
//! Two kinds of assertion here, deliberately:
//!
//! 1. **The vocabulary.** Every distinct style the frame draws, enumerated.
//!    This is the one that catches a colour nobody anticipated — a new
//!    widget, an accidental `Modifier::BLINK`, a theme — because anything
//!    not on the list fails it.
//! 2. **The assignments.** Which colour is on which widget, anchored to the
//!    text that names the widget rather than to a row number, so a failure
//!    reads as "the loss sparkline is not LightRed any more".
//!
//! Its own test target, so `stress.yml`'s `--test doctor --test tui`
//! selection stays a stress-only run.

use std::path::{Path, PathBuf};
use std::time::Duration;

use termlens::{Color, Key, Screen, Style, Terminal};

const TIMEOUT: Duration = Duration::from_secs(20);

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/train-replay.toml")
}

/// `Style` is `#[non_exhaustive]` from 0.10, so a struct literal will not
/// compile out here. Build from the default and set what matters.
fn style_with(set: impl FnOnce(&mut Style)) -> Style {
    let mut style = Style::default();
    set(&mut style);
    style
}

/// A short, readable rendering of a style, for set comparison and for the
/// failure message. `{:?}` on `Style` is ten fields wide and unreadable in
/// a diff of seven of them.
fn describe(style: &Style) -> String {
    let mut parts: Vec<String> = Vec::new();
    if style.fg != Color::Default {
        parts.push(format!("fg={:?}", style.fg));
    }
    if style.bg != Color::Default {
        parts.push(format!("bg={:?}", style.bg));
    }
    for (flag, name) in [
        (style.bold, "bold"),
        (style.dim, "dim"),
        (style.italic, "italic"),
        (style.underline, "underline"),
        (style.reverse, "reverse"),
        (style.blink, "blink"),
        (style.conceal, "conceal"),
        (style.strikethrough, "strikethrough"),
    ] {
        if flag {
            parts.push(name.to_string());
        }
    }
    if parts.is_empty() {
        "plain".to_string()
    } else {
        parts.join(" ")
    }
}

/// Every distinct style on the grid, sorted, deduplicated.
fn vocabulary(screen: &Screen) -> Vec<String> {
    let mut seen: Vec<String> = (0..screen.rows())
        .flat_map(|row| (0..screen.cols()).map(move |col| (row, col)))
        .filter_map(|(row, col)| screen.cell(row, col).map(|cell| describe(cell.style())))
        .collect();
    seen.sort();
    seen.dedup();
    seen
}

fn dashboard(cols: u16, rows: u16) -> termlens::Result<(Terminal, Screen)> {
    let mut t = Terminal::builder()
        .size(cols, rows)
        .env_clear()
        .timeout(TIMEOUT)
        .arg("train")
        .arg("--replay")
        .arg(fixture())
        .spawn(env!("CARGO_BIN_EXE_oxmera"))?;
    let screen = t.snapshot_after(|s| s.contains("training complete — press q"))?;
    Ok((t, screen))
}

/// The whole palette, at both golden geometries.
///
/// `src/tui.rs` draws exactly seven styles: the plain body, the bold header
/// span, LightRed (`SGR 38;5;9`) for loss, LightGreen (`10`) for accuracy,
/// Cyan (`6`) for the gauges, that same cyan as a *background* where the
/// filled bar runs behind the gauge label, and a reversed footer. An eighth
/// entry means something started painting that nothing here describes.
///
/// Blink and strikethrough are absent from this list, which is what lets
/// `tests/emulation.rs` pin `unsupported()` exactly: the known termlens
/// false positive (termlens#320) reports `^[[5m`/`^[[25m`/`^[[9m`/`^[[29m`
/// for applications that use those attributes, and this dashboard uses
/// neither.
#[test]
fn the_dashboard_paints_seven_styles_and_no_others() -> termlens::Result<()> {
    let expected: Vec<String> = {
        let mut v: Vec<String> = [
            style_with(|_| {}),
            style_with(|s| s.bold = true),
            style_with(|s| s.reverse = true),
            style_with(|s| s.fg = Color::Indexed(9)),
            style_with(|s| s.fg = Color::Indexed(10)),
            style_with(|s| s.fg = Color::Indexed(6)),
            style_with(|s| s.bg = Color::Indexed(6)),
        ]
        .iter()
        .map(describe)
        .collect();
        v.sort();
        v
    };

    for (cols, rows) in [(100, 45), (80, 30)] {
        let (mut t, screen) = dashboard(cols, rows)?;
        assert_eq!(
            vocabulary(&screen),
            expected,
            "at {cols}x{rows} the dashboard's palette changed. The goldens \
             compare text only, so nothing else in this crate would notice."
        );
        t.send(Key::Char('q'))?;
        assert!(t.wait_exit()?.success());
    }
    Ok(())
}

/// Which widget wears which colour.
///
/// Anchored to the text that names each widget, so the failure says what
/// broke rather than naming a row. The two sparkline colours being
/// *different* is asserted on its own: swapping them keeps the vocabulary
/// test green, and a red accuracy curve beside a green loss curve reads
/// exactly backwards to anyone glancing at it.
#[test]
fn each_widget_keeps_the_colour_it_is_read_by() -> termlens::Result<()> {
    let (mut t, screen) = dashboard(100, 45)?;

    let style_at = |row: u16, col: u16| screen.cell(row, col).map(|c| *c.style());

    // The header span ` oxmera train ` is bold; the model and device that
    // follow it on the same line are not, which is what makes it a heading
    // rather than a bold line.
    let (row, col) = screen.find("oxmera train").expect("the header");
    for offset in 0.."oxmera train".len() as u16 {
        assert_eq!(
            style_at(row, col + offset).map(|s| s.bold),
            Some(true),
            "the header span lost its bold at column {}",
            col + offset
        );
    }
    let device = screen.find("device metal").expect("the device label");
    assert_eq!(
        style_at(device.0, device.1).map(|s| s.bold),
        Some(false),
        "only the ` oxmera train ` span is bold, not the whole header line"
    );

    // The sparklines. The block glyphs sit in the first column inside each
    // border, on every row of the block, so the colour is read there.
    let loss_title = screen.find("loss — 0.2005").expect("the loss block").0;
    let acc_title = screen
        .find("accuracy — 95.3%")
        .expect("the accuracy block")
        .0;
    for row in loss_title + 1..acc_title - 1 {
        assert_eq!(
            style_at(row, 1).map(|s| s.fg),
            Some(Color::Indexed(9)),
            "row {row} of the loss sparkline is not LightRed"
        );
    }
    for row in acc_title + 1..acc_title + 14 {
        assert_eq!(
            style_at(row, 1).map(|s| s.fg),
            Some(Color::Indexed(10)),
            "row {row} of the accuracy sparkline is not LightGreen"
        );
    }
    // Deliberately no `assert_ne!` that the two series differ. The loops
    // above pin each one to a specific colour — LightRed for loss, LightGreen
    // for accuracy — so "they are the same colour" and "they are swapped" both
    // already fail there, by name and on the offending row. A comparison
    // between two cells those loops have just pinned to different constants
    // could not fail, whatever it claimed to be guarding.

    // The gauges: cyan bar, and the filled portion inverts behind the
    // label so the digits stay legible on top of it. That inversion is the
    // only reason `6/6` is readable at all, and nothing else pins it.
    for label in ["6/6", "8/8"] {
        let (row, col) = screen
            .find(label)
            .unwrap_or_else(|| panic!("{label} gauge"));
        assert_eq!(
            style_at(row, col).map(|s| s.bg),
            Some(Color::Indexed(6)),
            "{label}: the filled bar no longer runs behind the label"
        );
        assert_eq!(
            style_at(row, col).map(|s| s.fg),
            Some(Color::Default),
            "{label}: the label is meant to be knocked out of the bar"
        );
        assert_eq!(
            style_at(row, 1).map(|s| s.fg),
            Some(Color::Indexed(6)),
            "{label}: the bar itself is no longer cyan"
        );
    }

    // The footer hint is reversed across the full width — it is a status
    // bar, not a sentence, and a partly reversed one looks like a bug.
    let footer = screen
        .find("training complete — press q")
        .expect("the footer")
        .0;
    for col in 0..screen.cols() {
        assert_eq!(
            style_at(footer, col).map(|s| s.reverse),
            Some(true),
            "the footer bar stops being reversed at column {col}"
        );
    }

    t.send(Key::Char('q'))?;
    assert!(t.wait_exit()?.success());
    Ok(())
}
