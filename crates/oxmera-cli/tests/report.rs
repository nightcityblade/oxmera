//! `oxmera doctor` as a report rather than as a golden: what happens to it
//! at a narrow terminal, what happens to it when it is taller than the
//! terminal, and what it looks like when it is probing a *real* machine
//! instead of reading a fixture.
//!
//! `tests/doctor.rs` pins three fixtures at 100x45 against golden files.
//! Everything here is a claim those goldens structurally cannot make:
//!
//! - a golden at 100 columns says nothing about 80, which is the width a
//!   terminal tool should actually respect;
//! - a golden of the visible screen says nothing about the rows that
//!   scrolled off it, which is where most of the report lands on a normal
//!   terminal;
//! - and every doctor test in this crate passes `--fixture`, so
//!   `src/probe.rs` — the 89 lines that read the real machine — had no
//!   coverage at all.
//!
//! Its own test target, so `stress.yml`'s `--test doctor --test tui`
//! selection stays a stress-only run.

use std::path::{Path, PathBuf};
use std::time::Duration;

use termlens::{Screen, Terminal};

const TIMEOUT: Duration = Duration::from_secs(30);

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn doctor(name: &str, cols: u16, rows: u16, history: bool) -> termlens::Result<Screen> {
    let mut t = Terminal::builder()
        .size(cols, rows)
        .env_clear()
        .timeout(TIMEOUT)
        .scrollback_styles(history)
        .arg("doctor")
        .arg("--fixture")
        .arg(fixture(name))
        .spawn(env!("CARGO_BIN_EXE_oxmera"))?;
    let screen = t.snapshot_after(|s| s.contains("doctor: report complete"))?;
    assert!(t.wait_exit()?.success(), "doctor exited non-zero");
    Ok(screen)
}

fn wrapped_rows(screen: &Screen) -> Vec<u16> {
    (0..screen.rows())
        .filter(|row| screen.row_wrapped(*row))
        .collect()
}

/// No row of the report wraps an 80-column terminal.
///
/// `tests/cli.rs::capability_rows_fit_a_narrow_terminal` asserts the same
/// thing with arithmetic — `12 + detail.chars().count() <= 80` — and its own
/// doc comment admits it is modelling a terminal rather than looking at
/// one. It is kept, because it names the offending row and its text, which
/// this cannot. This is the other half: an actual 80-column terminal, with
/// the emulator reporting which rows it soft-wrapped.
///
/// The 60-column leg is the instrument check. A test that only ever asserts
/// "nothing wrapped" would pass just as happily if `row_wrapped` always
/// returned false, so the same report is also run somewhere it *must* wrap.
/// If that leg ever goes green, this whole test has stopped measuring.
///
/// One caveat if the 80-column leg ever trips: a row that fills exactly to
/// the last column sets the terminal's pending-wrap flag, so read the
/// offending row's text before assuming a regression.
#[test]
fn no_report_row_wraps_a_narrow_terminal() -> termlens::Result<()> {
    for name in ["no-gpu.toml", "metal.toml", "cuda.toml"] {
        let screen = doctor(name, 80, 45, false)?;
        assert_eq!(
            wrapped_rows(&screen),
            Vec::<u16>::new(),
            "{name}: these rows wrapped an 80-column terminal, so each costs \
             two rows of the report and reads as a broken line:\n{screen}"
        );

        let narrow = doctor(name, 60, 45, false)?;
        assert!(
            !wrapped_rows(&narrow).is_empty(),
            "{name}: nothing wrapped at 60 columns either, so `row_wrapped` \
             is not reporting and the 80-column assertion above is vacuous"
        );
    }
    Ok(())
}

/// Because `doctor` never takes the alternate screen, a report taller than
/// the terminal has to land in scrollback where the user can scroll back to
/// it. `tests/doctor.rs` asserts the negative — "must never take the
/// alternate screen" — and this is the consequence that makes the rule
/// worth having; without it the negative is a bare one.
///
/// At 100x24 the report does not fit, which is the ordinary case: a default
/// terminal is 24 rows and the report is over thirty.
#[test]
fn a_report_taller_than_the_terminal_lands_in_scrollback() -> termlens::Result<()> {
    for (name, expected) in [("no-gpu.toml", 8usize), ("metal.toml", 9), ("cuda.toml", 9)] {
        let screen = doctor(name, 100, 24, true)?;

        assert!(
            !screen.alternate_screen(),
            "{name}: a report that took the alternate screen would lose \
             every scrolled row the moment it exited"
        );
        assert_eq!(
            screen.scrollback_rows(),
            expected,
            "{name}: the rows that scrolled off a 24-row terminal"
        );

        // The title is off the top of the screen and only history has it.
        // `contains` deliberately looks at the visible grid alone, so this
        // is the distinction `locate` exists to make.
        assert!(
            !screen.contains("oxmera doctor\n"),
            "{name}: the title has scrolled off the visible screen"
        );
        assert!(
            matches!(
                screen.locate("oxmera doctor"),
                Some(termlens::Location::History { row: 0, col: 0 })
            ),
            "{name}: the title is the first row of history, at {:?}",
            screen.locate("oxmera doctor")
        );
        assert!(
            matches!(
                screen.locate("doctor: report complete"),
                Some(termlens::Location::Screen { .. })
            ),
            "{name}: and the sentinel is still on the visible screen"
        );

        // The scrolled rows are *text*, and with `scrollback_styles` they
        // are cells too — so "the report was plain all the way up" is
        // checkable, not just "the visible part was".
        assert!(
            screen.scrollback_text().starts_with("oxmera doctor"),
            "{name}: history begins with the title:\n{}",
            screen.scrollback_text()
        );
        assert!(screen.styled_scrollback(), "history retained its cells");
        let first = screen
            .scrollback_cell(0, 0)
            .expect("the first cell of history");
        assert_eq!(first.contents(), "o");
        assert_eq!(
            *first.style(),
            termlens::Style::default(),
            "{name}: the report is plain text in history too"
        );

        // Nothing was lost between the two halves: the whole report is
        // readable as history plus screen.
        let whole = screen.full_text();
        for needle in ["oxmera doctor", "toolchain", "devices", "capabilities"] {
            assert!(
                whole.contains(needle),
                "{name}: `{needle}` is missing from history + screen"
            );
        }
    }
    Ok(())
}

/// Both device rows say it, and there are exactly two of them.
///
/// `tests/doctor.rs` makes this claim with two separate `contains` calls,
/// which pass just as well if one row were duplicated and the other
/// deleted. `find_all` counts.
#[test]
fn a_machine_with_no_gpu_reports_exactly_two_absent_devices() -> termlens::Result<()> {
    let screen = doctor("no-gpu.toml", 100, 45, false)?;
    let hits = screen.find_all("not available on this host");
    assert_eq!(
        hits.len(),
        2,
        "metal and cuda, one row each, got {hits:?}\n{screen}"
    );
    let rows: Vec<u16> = hits.iter().map(|(row, _)| *row).collect();
    assert_eq!(rows[1], rows[0] + 1, "consecutive rows of the device block");
    assert!(screen.row_text(rows[0]).starts_with("  metal   "));
    assert!(screen.row_text(rows[1]).starts_with("  cuda    "));
    Ok(())
}

/// The report `oxmera doctor` prints when it is reading a real machine.
///
/// This is the first coverage `src/probe.rs` has had. Everything else in
/// this crate passes `--fixture`, so the probe could have started printing
/// an error, losing a section, or crashing on a machine with no `sysctl`,
/// and the whole suite would have stayed green.
///
/// It runs on any machine because the machine-dependent rows are **masked**
/// rather than asserted: `mask_rect` blanks a row's contents and keeps the
/// size, the cursor and every style, so what is left is the report's shape —
/// section order, the fixed labels, the capability block, the sentinel — and
/// that is identical on a Linux runner and a macOS one.
///
/// `PATH` is set to a directory that does not exist rather than merely
/// cleared. That is what makes the shape deterministic across platforms:
/// `probe()` shells out to `rustc`, `cargo` and (on macOS) `sysctl`, and
/// with no reachable PATH all three fail on every platform, so the host
/// block collapses to its two-row form everywhere and the toolchain rows
/// take the `not found` branch. That branch is itself worth pinning: no
/// fixture can reach it, and a doctor that panicked or printed an error
/// when a tool is missing would be useless on exactly the machine someone
/// runs `doctor` on.
#[test]
fn the_real_machine_probe_prints_the_same_report_shape_anywhere() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(100, 45)
        .env_clear()
        .env("PATH", "/nonexistent-oxmera-doctor-probe")
        .timeout(TIMEOUT)
        .arg("doctor")
        .spawn(env!("CARGO_BIN_EXE_oxmera"))?;
    let screen = t.snapshot_after(|s| s.contains("doctor: report complete"))?;
    let status = t.wait_exit()?;

    assert!(status.success(), "probing a real machine exited {status:?}");
    assert!(
        !screen.alternate_screen(),
        "the report must never take the alternate screen"
    );
    assert!(
        screen.cursor().2,
        "and must leave the cursor visible for the shell"
    );
    assert!(
        screen.unsupported().is_empty(),
        "the real-machine report is plain text too, but termlens dropped {:?}",
        screen.unsupported()
    );
    assert_eq!(wrapped_rows(&screen), Vec::<u16>::new());

    // The rows that describe *this* machine. Located by their labels, not
    // by row number, because the host block is a different height on macOS
    // when `sysctl` is reachable.
    let volatile = [
        "host: ",
        "  cpu cores ",
        "  cpu     ",
        "  metal   ",
        "  cuda    ",
    ];
    let mut masked = screen.clone();
    let mut found = Vec::new();
    for prefix in volatile {
        let row = (0..screen.rows())
            .find(|row| screen.row_text(*row).starts_with(prefix))
            .unwrap_or_else(|| panic!("no row starts with {prefix:?}:\n{screen}"));
        masked = masked.mask_rect(.., row..=row);
        found.push(row);
    }
    // A mask replaces contents and nothing else — same geometry, so no
    // column moved and the shape below is comparable row for row.
    assert_eq!(masked.size(), screen.size());
    assert!(found.windows(2).all(|w| w[0] < w[1]), "in reading order");

    let mut expected: Vec<String> = vec![
        "oxmera doctor".into(),
        "=============".into(),
        String::new(),
        String::new(), // host: <os> / <arch>
        String::new(), // cpu cores <n>
        String::new(),
        "toolchain".into(),
        "  rustc        not found".into(),
        "  cargo        not found".into(),
        String::new(),
        "devices".into(),
        String::new(), // cpu available — <n> threads (rayon)
        String::new(), // metal <this machine>
        String::new(), // cuda  <this machine>
        String::new(),
        "capabilities".into(),
    ];
    // Assembled from the crates that implement them, exactly as
    // `src/doctor.rs` assembles them — so adding an op family needs no edit
    // here. What this pins is the *rendering*: the `{area:<9} {detail}`
    // column, and that every row reaches the real-machine report and not
    // only the fixture one. `tests/cli.rs` holds the hand-written second
    // opinion about which families must exist at all.
    expected.extend(
        oxmera::tensor::CAPABILITIES
            .iter()
            .chain(oxmera::autograd::CAPABILITIES)
            .chain(oxmera::nn::CAPABILITIES)
            .chain(oxmera::optim::CAPABILITIES)
            .map(|(area, detail)| format!("  {area:<9} {detail}")),
    );
    expected.extend([
        String::new(),
        "try: `oxmera train --tui` — the live training dashboard".into(),
        String::new(),
        "doctor: report complete".into(),
    ]);

    let actual: Vec<String> = (0..screen.rows())
        .map(|row| masked.row_text(row).trim_end().to_string())
        .collect();
    assert_eq!(
        actual[..expected.len()],
        expected[..],
        "the real-machine report changed shape.\n--- as rendered ---\n{screen}"
    );
    assert!(
        actual[expected.len()..].iter().all(String::is_empty),
        "nothing is printed after the sentinel:\n{screen}"
    );
    Ok(())
}
