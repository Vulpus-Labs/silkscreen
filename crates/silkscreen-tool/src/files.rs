//! File operations with errors that name the path.

use std::path::Path;

/// Copy a file or a directory's tree to `dest`, replacing what was there.
pub fn replace(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    remove(dest)?;
    copy_tree(src, dest, &|_| true)
}

/// Copy a file, or a directory's tree, to `dest`: only the files `keep`
/// accepts.
pub fn copy_tree(src: &Path, dest: &Path, keep: &dyn Fn(&Path) -> bool) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        for entry in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &dest.join(entry.file_name()), keep)?;
        }
        Ok(())
    } else if keep(src) {
        copy(src, dest)
    } else {
        Ok(())
    }
}

pub fn remove(path: &Path) -> Result<(), String> {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else if path.exists() {
        std::fs::remove_file(path)
    } else {
        return Ok(());
    };
    result.map_err(|e| format!("removing {}: {e}", path.display()))
}

pub fn copy(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::copy(src, dest)
        .map(|_| ())
        .map_err(|e| format!("copying {} to {}: {e}", src.display(), dest.display()))
}

pub fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}
