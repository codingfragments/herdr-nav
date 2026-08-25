//! Directory navigation mode (DirNav) — Phase 17 (Feature B, part 1).
//!
//! A toggled third mode: a filesystem directory walker starting at the
//! focused pane's cwd. `←` ascends to the parent directory; `→` descends
//! into the cursor entry (directories + dir-symlinks only); `↑↓` move
//! the cursor (wraps). The in-level fuzzy search + find navigation
//! lands in Phase 18; the commit verb (`Enter`/`^t`/`^p`) in Phase 19.
//!
//! **Spec departure (§1 non-goal):** DirNav is path-by-path directory
//! navigation, which §1 lists as a non-goal ("not a file browser").
//! Accepted as a v0.2 scope expansion — see `PLANNING.md` §17 "v0.2
//! phases" and `doc/navigation.md`.
//!
//! The switcher's `Tree` (+ `SearchView`) are preserved off-screen while
//! DirNav is active so `Esc` restores them exactly (expansion intact).

use std::path::{Path, PathBuf};

/// One entry in a DirNav listing: a directory, or a symlink that resolves
/// to a directory (Phase 17 decision B4). Files and links-to-files are
/// not listed — this is a directory walker, not a file browser.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// The entry's base name (e.g. `src`).
    pub name: String,
    /// The resolved absolute path. For a symlink this is the symlink's
    /// own path (not the canonical target) so the breadcrumb shows the
    /// link the user clicked; `→` follows it via `metadata`.
    pub path: PathBuf,
    /// True if this entry is a symlink (drives the glyph + `→ target` meta).
    pub is_symlink: bool,
}

/// The DirNav view's mutable state. Held as `Option<DirNavView>` in the
/// event loop alongside `search_view`; `None` = not in DirNav mode.
#[derive(Debug, Clone)]
pub struct DirNavView {
    /// The directory currently being listed.
    pub cwd: PathBuf,
    /// Sorted directory entries (dirs + dir-symlinks only).
    pub entries: Vec<DirEntry>,
    /// Cursor into `entries`.
    pub cursor: usize,
    /// Vertical scroll (kept for Phase 19 large-dir handling).
    pub scroll: usize,
    /// The entry we ascended from — `←` lands the cursor here on the
    /// parent level so you can re-descend. Cleared on `→`.
    pub came_from: Option<PathBuf>,
}

impl DirNavView {
    /// Build a DirNav view at `cwd`, listing directories + dir-symlinks
    /// (sorted by name). Returns `None` if `cwd` can't be read — the
    /// caller falls back to `$HOME` (Phase 17).
    pub fn at(cwd: PathBuf) -> Option<Self> {
        let entries = read_dir_entries(&cwd)?;
        Some(Self {
            cwd,
            entries,
            cursor: 0,
            scroll: 0,
            came_from: None,
        })
    }

    /// Move the cursor down one entry, wrapping.
    pub fn move_down(&mut self) {
        let n = self.entries.len();
        if n > 0 {
            self.cursor = (self.cursor + 1) % n;
        }
    }

    /// Move the cursor up one entry, wrapping.
    pub fn move_up(&mut self) {
        let n = self.entries.len();
        if n > 0 {
            self.cursor = (self.cursor + n - 1) % n;
        }
    }

    /// `←`: the parent directory to ascend to. `None` at the filesystem
    /// root (no parent) — `←` is inert there. The caller rebuilds the
    /// view at the returned path with `came_from` set to the old cwd.
    pub fn parent(&self) -> Option<PathBuf> {
        self.cwd
            .parent()
            .map(Path::to_path_buf)
            .filter(|p| p != &self.cwd)
    }

    /// `→`: descend into the cursor entry if it's a directory (or a
    /// symlink resolving to one). Returns the path to rebuild the view
    /// at; `None` if the cursor is out of range or the entry isn't a
    /// readable directory (inert). Re-checks the filesystem in case it
    /// changed since the listing was built.
    pub fn child(&self) -> Option<PathBuf> {
        let entry = self.entries.get(self.cursor)?;
        let meta = std::fs::metadata(&entry.path).ok()?;
        if meta.is_dir() {
            Some(entry.path.clone())
        } else {
            None
        }
    }

    /// The cursor entry, if any.
    pub fn cursor_entry(&self) -> Option<&DirEntry> {
        self.entries.get(self.cursor)
    }
}

/// Read the directory entries of `dir`: directories + symlinks that
/// resolve to a directory only (Phase 17 decision B4), sorted by name.
/// Hidden entries (dotfiles) are skipped — Phase 19 adds a `.` toggle.
/// Returns `None` if `dir` can't be read.
pub fn read_dir_entries(dir: &Path) -> Option<Vec<DirEntry>> {
    let read = std::fs::read_dir(dir).ok()?;
    let mut entries: Vec<DirEntry> = read
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None; // hidden — Phase 19 toggle
            }
            let path = e.path();
            let is_symlink = std::fs::symlink_metadata(&path)
                .ok()
                .is_some_and(|m| m.file_type().is_symlink());
            // `metadata` follows symlinks: a symlink counts only if it
            // resolves to a directory.
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_dir() {
                return None;
            }
            Some(DirEntry {
                name,
                path,
                is_symlink,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkdir(p: &Path) {
        std::fs::create_dir_all(p).unwrap();
    }
    fn touch(p: &Path) {
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn read_dir_entries_lists_only_dirs_and_dir_symlinks() {
        let tmp = std::env::temp_dir().join(format!("dirnav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("subdir"));
        touch(&tmp.join("file.txt"));
        // a symlink to a directory
        std::os::unix::fs::symlink(tmp.join("subdir"), tmp.join("link")).ok();
        // a symlink to a file (should be excluded)
        std::os::unix::fs::symlink(tmp.join("file.txt"), tmp.join("linkfile")).ok();

        let entries = read_dir_entries(&tmp).expect("read dir");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"subdir"));
        assert!(names.contains(&"link"));
        assert!(!names.contains(&"file.txt"));
        assert!(!names.contains(&"linkfile"));

        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert!(link.is_symlink);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn read_dir_entries_hides_dotfiles() {
        let tmp = std::env::temp_dir().join(format!("dirnav-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join(".hidden"));
        mkdir(&tmp.join("visible"));
        let entries = read_dir_entries(&tmp).expect("read dir");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"visible"));
        assert!(!names.contains(&".hidden"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn read_dir_entries_sorted_by_name() {
        let tmp = std::env::temp_dir().join(format!("dirnav-sort-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("zeta"));
        mkdir(&tmp.join("alpha"));
        mkdir(&tmp.join("mid"));
        let entries = read_dir_entries(&tmp).expect("read dir");
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn read_dir_entries_none_on_unreadable() {
        let p = Path::new("/this/does/not/exist/definitely/not");
        assert!(read_dir_entries(p).is_none());
    }

    #[test]
    fn cursor_wraps_down_and_up() {
        let tmp = std::env::temp_dir().join(format!("dirnav-wrap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("a"));
        mkdir(&tmp.join("b"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        assert_eq!(v.cursor, 0);
        v.move_down();
        assert_eq!(v.cursor, 1);
        v.move_down(); // wraps to 0
        assert_eq!(v.cursor, 0);
        v.move_up(); // wraps to last
        assert_eq!(v.cursor, 1);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn parent_returns_parent_path() {
        let v = DirNavView {
            cwd: PathBuf::from("/a/b/c"),
            entries: vec![],
            cursor: 0,
            scroll: 0,
            came_from: None,
        };
        assert_eq!(v.parent(), Some(PathBuf::from("/a/b")));
    }

    #[test]
    fn parent_none_at_root() {
        let v = DirNavView {
            cwd: PathBuf::from("/"),
            entries: vec![],
            cursor: 0,
            scroll: 0,
            came_from: None,
        };
        assert_eq!(v.parent(), None);
    }

    #[test]
    fn child_descends_into_cursor_dir() {
        let tmp = std::env::temp_dir().join(format!("dirnav-child-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("sub"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        v.cursor = 0; // "sub"
        assert_eq!(v.child(), Some(tmp.join("sub")));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn child_none_when_cursor_out_of_range() {
        let v = DirNavView {
            cwd: PathBuf::from("/"),
            entries: vec![],
            cursor: 5,
            scroll: 0,
            came_from: None,
        };
        assert_eq!(v.child(), None);
    }

    #[test]
    fn at_returns_none_for_unreadable_cwd() {
        let p = PathBuf::from("/this/does/not/exist");
        assert!(DirNavView::at(p).is_none());
    }
}
