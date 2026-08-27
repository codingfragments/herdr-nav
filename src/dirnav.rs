//! Directory navigation mode (DirNav) — Phase 17 + Phase 18.
//!
//! A toggled third mode: a filesystem directory walker starting at the
//! focused pane's cwd. `←` ascends to the parent directory; `→` descends
//! into the cursor entry (directories + dir-symlinks only); `↑↓` move
//! the cursor (wraps). Phase 18 adds an in-level fuzzy search: typing
//! filters the current level's entry names and lands on the first
//! match; `↑↓` then jump between matches (find). The commit verb
//! (`Enter`/`^p`) lands in Phase 19.
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

/// One ranked in-level search match (Phase 18): an index into `entries`
/// plus the character positions in the entry name that matched the
/// query (for highlight rendering).
#[derive(Debug, Clone)]
pub struct DirNavMatch {
    /// Index into `DirNavView::entries`.
    pub entry_idx: usize,
    /// Matched character positions in the entry's `name`.
    pub indices: Vec<u32>,
}

/// The DirNav view's mutable state. Held as `Option<DirNavView>` in the
/// event loop alongside `search_view`; `None` = not in DirNav mode.
#[derive(Debug, Clone)]
pub struct DirNavView {
    /// The directory currently being listed.
    pub cwd: PathBuf,
    /// Sorted directory entries (dirs + dir-symlinks only).
    pub entries: Vec<DirEntry>,
    /// Cursor into the **visible** rows: when `query` is empty this
    /// indexes `entries`; when non-empty it indexes `matches`.
    pub cursor: usize,
    /// Vertical scroll (kept for Phase 19 large-dir handling).
    pub scroll: usize,
    /// The entry we ascended from — `←` lands the cursor here on the
    /// parent level so you can re-descend. Cleared on `→`.
    pub came_from: Option<PathBuf>,
    /// Phase 18 in-level fuzzy query. Empty → full level shown.
    pub query: String,
    /// Phase 18 ranked matches (indices into `entries`). Empty when
    /// `query` is empty.
    pub matches: Vec<DirNavMatch>,
    /// Phase 19: whether hidden entries (dotfiles) are shown. Toggled
    /// by `.`. Default false.
    pub show_hidden: bool,
}

impl DirNavView {
    /// Build a DirNav view at `cwd`, listing directories + dir-symlinks
    /// (sorted by name). Returns `None` if `cwd` can't be read — the
    /// caller falls back to `$HOME` (Phase 17).
    pub fn at(cwd: PathBuf) -> Option<Self> {
        let entries = read_dir_entries(&cwd, false)?;
        Some(Self {
            cwd,
            entries,
            cursor: 0,
            scroll: 0,
            came_from: None,
            query: String::new(),
            matches: Vec::new(),
            show_hidden: false,
        })
    }

    /// Number of visible rows: all entries when the query is empty,
    /// else the match count (Phase 18).
    pub fn visible_len(&self) -> usize {
        if self.query.is_empty() {
            self.entries.len()
        } else {
            self.matches.len()
        }
    }

    /// The `entries` index of the visible row at `cursor`, or `None` if
    /// out of range. When the query is empty, the cursor *is* the entry
    /// index; when searching, it indexes `matches`.
    pub fn visible_entry_idx(&self, cursor: usize) -> Option<usize> {
        if self.query.is_empty() {
            Some(cursor)
        } else {
            self.matches.get(cursor).map(|m| m.entry_idx)
        }
    }

    /// Move the cursor down one visible row, wrapping (Phase 18:
    /// wraps within the match set when searching).
    pub fn move_down(&mut self) {
        let n = self.visible_len();
        if n > 0 {
            self.cursor = (self.cursor + 1) % n;
        }
    }

    /// Move the cursor up one visible row, wrapping.
    pub fn move_up(&mut self) {
        let n = self.visible_len();
        if n > 0 {
            self.cursor = (self.cursor + n - 1) % n;
        }
    }

    /// Phase 18: re-run the in-level fuzzy search against the current
    /// entries' names and reset the cursor to the first match (spec:
    /// "select the first entry that fuzzy-matches"). No provider bias —
    /// pure name match. An empty query clears the matches and shows the
    /// full level (cursor stays where it was, clamped by callers).
    pub fn requery(&mut self) {
        if self.query.is_empty() {
            self.matches.clear();
            return;
        }
        let items: Vec<String> = self.entries.iter().map(|e| e.name.clone()).collect();
        let mut engine = crate::search::FuzzyEngine::new();
        let scored = engine.filter_with_bonus(&self.query, &items, |_| 0);
        self.matches = scored
            .into_iter()
            .map(|m| DirNavMatch {
                entry_idx: m.index,
                indices: m.indices,
            })
            .collect();
        self.cursor = 0;
    }

    /// Phase 19: re-read the current cwd with the current `show_hidden`
    /// setting, preserving the cursor (clamped) and `came_from`. Used
    /// after the `.` toggle so the listing refreshes in place.
    pub fn refresh_entries(&mut self) {
        if let Some(entries) = read_dir_entries(&self.cwd, self.show_hidden) {
            self.entries = entries;
            if self.cursor >= self.entries.len() {
                self.cursor = self.entries.len().saturating_sub(1);
            }
            // Re-run the in-level search against the new entries so the
            // filtered view stays consistent.
            self.requery();
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
        let entry = self.cursor_entry()?;
        let meta = std::fs::metadata(&entry.path).ok()?;
        if meta.is_dir() {
            Some(entry.path.clone())
        } else {
            None
        }
    }

    /// The cursor entry, if any (Phase 18: resolves through `matches`
    /// when the query is active).
    pub fn cursor_entry(&self) -> Option<&DirEntry> {
        let idx = self.visible_entry_idx(self.cursor)?;
        self.entries.get(idx)
    }
}

/// Read the directory entries of `dir`: directories + symlinks that
/// resolve to a directory only (Phase 17 decision B4), sorted by name.
/// Hidden entries (dotfiles) are skipped — Phase 19 adds a `.` toggle.
/// Returns `None` if `dir` can't be read.
pub fn read_dir_entries(dir: &Path, show_hidden: bool) -> Option<Vec<DirEntry>> {
    let read = std::fs::read_dir(dir).ok()?;
    let mut entries: Vec<DirEntry> = read
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
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

// ── Path display (Phase 18) ───────────────────────────────────────────────────
//
// The DirNav search bar shows the cwd as a breadcrumb path so the user
// always knows where they are in the filesystem. Long paths are shortened
// to fit the bar, but the **direct parent** (the last segment = the
// directory currently listed) is never shortened — it's the most
// important context. Earlier segments are reduced to their first
// character, then dropped from the front with a `…/` prefix, before the
// direct parent is ever touched.

/// Convert an absolute path to a display form: `$HOME` → `~` prefix,
/// then shortened to fit `max_chars` with the direct parent kept full.
pub fn display_path(path: &Path, max_chars: usize) -> String {
    let display = home_tilde(path);
    shorten_path(&display, max_chars)
}

/// Replace a leading `$HOME` with `~` for display.
fn home_tilde(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        if let Ok(rest) = path.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

/// Shorten `path` to fit within `max_chars` (character count), keeping
/// the **last segment** (direct parent) full at all times. Stages:
///   1. If the full path fits, return it as-is.
///   2. Reduce every earlier segment to its first character (`/U/s/p/last`).
///   3. Drop leading early segments from the front, prefixing `…/`,
///      keeping trailing first-char segments + the full last segment.
///   4. Last resort: `…/last` (last segment still full; the terminal
///      clips on overflow — the direct parent is never shortened).
///
/// `max_chars == 0` returns the full path (caller guards).
pub fn shorten_path(path: &str, max_chars: usize) -> String {
    let n = path.chars().count();
    if max_chars == 0 || n <= max_chars {
        return path.to_string();
    }
    // Split into non-empty segments, preserving the leading slash.
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return path.to_string(); // just "/"
    }
    let last = parts.last().unwrap().to_string();
    let early = &parts[..parts.len() - 1];

    // Stage 2: first char of each early segment.
    let first_chars: Vec<String> = early
        .iter()
        .map(|s| s.chars().next().unwrap_or(' ').to_string())
        .collect();
    let stage2 = format!("/{}/{}", first_chars.join("/"), last);
    if stage2.chars().count() <= max_chars {
        return stage2;
    }

    // Stage 3: drop leading early segments, keep trailing first-char
    // segments + full last, with a `…/` prefix.
    for k in (0..early.len()).rev() {
        let kept: String = first_chars[early.len() - k..].to_vec().join("/");
        let prefix = if kept.is_empty() {
            String::new()
        } else {
            format!("/{kept}")
        };
        let cand = format!("/…{prefix}/{last}");
        if cand.chars().count() <= max_chars {
            return cand;
        }
    }

    // Stage 4: only the direct parent, with a `…/` prefix. The last
    // segment stays full; if it still overflows, the terminal clips it
    // (we never shorten the direct parent).
    format!("…/{last}")
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

        let entries = read_dir_entries(&tmp, false).expect("read dir");
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
        let entries = read_dir_entries(&tmp, false).expect("read dir");
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
        let entries = read_dir_entries(&tmp, false).expect("read dir");
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn read_dir_entries_none_on_unreadable() {
        let p = Path::new("/this/does/not/exist/definitely/not");
        assert!(read_dir_entries(p, false).is_none());
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
            query: String::new(),
            matches: Vec::new(),
            show_hidden: false,
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
            query: String::new(),
            matches: Vec::new(),
            show_hidden: false,
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
            query: String::new(),
            matches: Vec::new(),
            show_hidden: false,
        };
        assert_eq!(v.child(), None);
    }

    #[test]
    fn at_returns_none_for_unreadable_cwd() {
        let p = PathBuf::from("/this/does/not/exist");
        assert!(DirNavView::at(p).is_none());
    }

    // ── Phase 18: in-level search ───────────────────────────────────────────

    #[test]
    fn requery_narrows_to_matches_and_resets_cursor() {
        let tmp = std::env::temp_dir().join(format!("dirnav-rq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("alpha"));
        mkdir(&tmp.join("beta"));
        mkdir(&tmp.join("gamma"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        v.query = "al".to_string();
        v.requery();
        assert_eq!(v.matches.len(), 1);
        assert_eq!(v.entries[v.matches[0].entry_idx].name, "alpha");
        assert_eq!(v.cursor, 0);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn empty_query_shows_full_level() {
        let tmp = std::env::temp_dir().join(format!("dirnav-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("alpha"));
        mkdir(&tmp.join("beta"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        v.query = "al".to_string();
        v.requery();
        assert_eq!(v.visible_len(), 1);
        v.query.clear();
        v.requery();
        assert_eq!(v.visible_len(), 2); // full level
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn move_down_wraps_within_matches_when_searching() {
        let tmp = std::env::temp_dir().join(format!("dirnav-wrapm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("src"));
        mkdir(&tmp.join("test-src"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        v.query = "src".to_string();
        v.requery();
        assert_eq!(v.matches.len(), 2);
        v.move_down();
        assert_eq!(v.cursor, 1);
        v.move_down(); // wraps to 0
        assert_eq!(v.cursor, 0);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn cursor_entry_resolves_through_matches_when_searching() {
        let tmp = std::env::temp_dir().join(format!("dirnav-cem-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("alpha"));
        mkdir(&tmp.join("zeta"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        v.cursor = 1; // on "zeta"
        v.query = "al".to_string();
        v.requery(); // cursor → 0, first match "alpha"
        let entry = v.cursor_entry().unwrap();
        assert_eq!(entry.name, "alpha");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn child_descends_into_matched_entry_when_searching() {
        let tmp = std::env::temp_dir().join(format!("dirnav-childm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join("alpha"));
        mkdir(&tmp.join("zeta"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        v.query = "al".to_string();
        v.requery();
        assert_eq!(v.child(), Some(tmp.join("alpha")));
        std::fs::remove_dir_all(&tmp).ok();
    }

    // ── Phase 18: path shortening ────────────────────────────────────────────

    #[test]
    fn shorten_path_returns_full_when_it_fits() {
        assert_eq!(shorten_path("/a/b/c", 10), "/a/b/c");
    }

    #[test]
    fn shorten_path_first_char_of_early_segments() {
        // /Users/stefan/projekte/herdr-nav, max 20 → /U/s/p/herdr-nav
        assert_eq!(
            shorten_path("/Users/stefan/projekte/herdr-nav", 20),
            "/U/s/p/herdr-nav"
        );
    }

    #[test]
    fn shorten_path_drops_leading_segments_with_ellipsis() {
        // Force stage 3: keep only the last early first-char segment.
        let s = shorten_path("/Users/stefan/projekte/herdr-nav", 14);
        assert!(s.ends_with("/herdr-nav"));
        assert!(s.starts_with("/…"));
        assert!(s.chars().count() <= 14);
    }

    #[test]
    fn shorten_path_keeps_direct_parent_full_at_all_stages() {
        // Even at tiny budgets the last segment is never shortened.
        let s = shorten_path("/Users/stefan/projekte/herdr-nav", 12);
        assert!(s.contains("herdr-nav"));
        // never a truncated last segment like "herdr…" or "herdr-n"
        assert!(!s.ends_with("…") || s == "…/herdr-nav");
    }

    #[test]
    fn shorten_path_last_resort_ellipsis_slash_last() {
        let s = shorten_path("/a/b/c/very-long-direct-parent-name", 10);
        assert_eq!(s, "…/very-long-direct-parent-name");
    }

    #[test]
    fn shorten_path_root_only() {
        assert_eq!(shorten_path("/", 5), "/");
    }

    #[test]
    fn shorten_path_zero_max_returns_full() {
        assert_eq!(shorten_path("/a/b/c", 0), "/a/b/c");
    }

    // ── Phase 19: hidden toggle + refresh ────────────────────────────────────

    #[test]
    fn read_dir_entries_show_hidden_includes_dotfiles() {
        let tmp = std::env::temp_dir().join(format!("dirnav-sh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join(".hidden"));
        mkdir(&tmp.join("visible"));
        let shown = read_dir_entries(&tmp, false).expect("read");
        assert!(!shown.iter().any(|e| e.name == ".hidden"));
        let all = read_dir_entries(&tmp, true).expect("read");
        assert!(all.iter().any(|e| e.name == ".hidden"));
        assert!(all.iter().any(|e| e.name == "visible"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn refresh_entries_picks_up_hidden_toggle() {
        let tmp = std::env::temp_dir().join(format!("dirnav-ref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        mkdir(&tmp);
        mkdir(&tmp.join(".secret"));
        mkdir(&tmp.join("open"));
        let mut v = DirNavView::at(tmp.clone()).unwrap();
        assert_eq!(v.entries.len(), 1); // only "open"
        v.show_hidden = true;
        v.refresh_entries();
        assert_eq!(v.entries.len(), 2); // ".secret" + "open"
                                        // cursor clamped to range
        v.cursor = 99;
        v.refresh_entries();
        assert!(v.cursor < v.entries.len());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
