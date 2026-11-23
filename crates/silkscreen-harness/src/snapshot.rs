//! PNG snapshot comparison.

use std::fmt;
use std::path::{Path, PathBuf};

use silkscreen::core::tiny_skia::{Pixmap, PremultipliedColorU8};

const BLESS_VAR: &str = "SILKSCREEN_BLESS";

#[derive(Debug)]
pub enum SnapshotError {
    Missing {
        path: PathBuf,
    },
    SizeMismatch {
        path: PathBuf,
        expected: (u32, u32),
        actual: (u32, u32),
        actual_path: PathBuf,
    },
    PixelMismatch {
        path: PathBuf,
        differing: usize,
        actual_path: PathBuf,
        diff_path: PathBuf,
    },
    Io {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Missing { path } => write!(
                f,
                "snapshot {} does not exist. Run with {BLESS_VAR}=1 to create it, then inspect it",
                path.display()
            ),
            SnapshotError::SizeMismatch {
                path,
                expected,
                actual,
                actual_path,
            } => write!(
                f,
                "snapshot {} is {}x{} but the frame is {}x{}. Actual frame written to {}. \
                 If the change is intended, re-bless with {BLESS_VAR}=1",
                path.display(),
                expected.0,
                expected.1,
                actual.0,
                actual.1,
                actual_path.display()
            ),
            SnapshotError::PixelMismatch {
                path,
                differing,
                actual_path,
                diff_path,
            } => write!(
                f,
                "snapshot {} differs in {differing} pixels. Actual: {}. Diff (red = changed): {}. \
                 If the change is intended, re-bless with {BLESS_VAR}=1",
                path.display(),
                actual_path.display(),
                diff_path.display()
            ),
            SnapshotError::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Compare `frame` against `{dir}/{name}.png`. With `SILKSCREEN_BLESS=1`,
/// write the frame there instead. On mismatch, writes `{name}.actual.png`
/// and `{name}.diff.png` beside the snapshot.
pub fn check_snapshot(frame: &Pixmap, dir: &Path, name: &str) -> Result<(), SnapshotError> {
    let bless = std::env::var_os(BLESS_VAR).is_some_and(|v| v == "1");
    check_snapshot_with(frame, dir, name, bless)
}

/// [`check_snapshot`] with blessing chosen explicitly rather than from the
/// environment.
pub fn check_snapshot_with(
    frame: &Pixmap,
    dir: &Path,
    name: &str,
    bless: bool,
) -> Result<(), SnapshotError> {
    let path = dir.join(format!("{name}.png"));
    let actual_path = dir.join(format!("{name}.actual.png"));
    let diff_path = dir.join(format!("{name}.diff.png"));
    let io = |path: &Path, e: &dyn fmt::Display| SnapshotError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    };

    if bless {
        std::fs::create_dir_all(dir).map_err(|e| io(dir, &e))?;
        frame.save_png(&path).map_err(|e| io(&path, &e))?;
        let _ = std::fs::remove_file(&actual_path);
        let _ = std::fs::remove_file(&diff_path);
        return Ok(());
    }

    if !path.exists() {
        return Err(SnapshotError::Missing { path });
    }
    let expected = Pixmap::load_png(&path).map_err(|e| io(&path, &e))?;
    let actual_size = (frame.width(), frame.height());
    let expected_size = (expected.width(), expected.height());
    if actual_size != expected_size {
        frame
            .save_png(&actual_path)
            .map_err(|e| io(&actual_path, &e))?;
        return Err(SnapshotError::SizeMismatch {
            path,
            expected: expected_size,
            actual: actual_size,
            actual_path,
        });
    }

    let differing = expected
        .pixels()
        .iter()
        .zip(frame.pixels())
        .filter(|(a, b)| a != b)
        .count();
    if differing == 0 {
        let _ = std::fs::remove_file(&actual_path);
        let _ = std::fs::remove_file(&diff_path);
        return Ok(());
    }

    frame
        .save_png(&actual_path)
        .map_err(|e| io(&actual_path, &e))?;
    diff_image(&expected, frame)
        .save_png(&diff_path)
        .map_err(|e| io(&diff_path, &e))?;
    Err(SnapshotError::PixelMismatch {
        path,
        differing,
        actual_path,
        diff_path,
    })
}

/// The actual frame dimmed to a quarter, with differing pixels in red.
fn diff_image(expected: &Pixmap, actual: &Pixmap) -> Pixmap {
    let mut out = actual.clone();
    let red = PremultipliedColorU8::from_rgba(255, 0, 0, 255).unwrap();
    for (o, e) in out.pixels_mut().iter_mut().zip(expected.pixels()) {
        if o == e {
            *o = PremultipliedColorU8::from_rgba(o.red() / 4, o.green() / 4, o.blue() / 4, 255)
                .unwrap();
        } else {
            *o = red;
        }
    }
    out
}
