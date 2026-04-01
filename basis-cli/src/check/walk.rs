use std::path::Path;

/// Walk source files under `root`, skipping hidden dirs and common non-source dirs.
pub fn walk_source_files(root: &Path, callback: &mut dyn FnMut(&Path)) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden directories and common non-source dirs
        if name_str.starts_with('.')
            || name_str == "target"
            || name_str == "node_modules"
            || name_str == "__pycache__"
            || name_str == ".git"
            || name_str == "venv"
            || name_str == ".venv"
        {
            continue;
        }

        if path.is_dir() {
            walk_source_files(&path, callback);
        } else {
            callback(&path);
        }
    }
}
