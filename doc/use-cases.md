# Use cases

Worked walkthroughs showing `herdr-nav` in real scenarios. See
[`doc/keybinding.md`](keybinding.md) for the keymap,
[`doc/navigation.md`](navigation.md) for the two modes and five groups,
[`doc/config-reference.md`](config-reference.md) for the full config
schema, and press `?` inside the popup for the full keybinding dialog.

---

## Jump to a pane you can't see

You have a dev server, a test runner, and a shell spread across tabs and
can't remember which tab the server is on:

1. `Ctrl k` → switcher opens in Browse, Session pre-expanded to the
   active tab, cursor on row 0.
2. `↓` into the Session tree, expand the right workspace/tab.
3. Preview shows the candidate pane's last scrollback lines — yes, that's
   the server.
4. `Enter` → you're in that pane; the popup closed and toasted
   `jumped to pane editor:nvim`.

## Fuzzy-search across everything

You know you have an `nvim` somewhere and a `notes` zsh, but not where:

1. `Ctrl k` → then type `nvim`. The tree flips to Search: a flat ranked
   list of every leaf whose `crumbs › label` contains `nvim` as a
   subsequence, hits highlighted in peach.
2. `Enter` on the top match → you're in that pane.
3. Or `Backspace` to empty → back to the Browse tree, expansion intact.

## Switch to a blocked agent

An agent is waiting on a confirmation:

1. `Ctrl k` → expand Agents. The agent's meta reads `waiting`.
2. Preview shows the pending question and its options verbatim — decide
   before you jump.
3. `Enter` → land in the agent's pane.

## Open a directory as a fresh workspace

1. `Ctrl k` → expand Pinned dirs (or zoxide). Preview shows the dir
   listing + git branch/dirty.
2. `Enter` → a **new** workspace opens at that path. When templates are configured, Enter first opens a template picker (default preselected via match-glob → `default: true` → first); confirm → name prompt → build + open. With no templates, Enter skips the picker and opens the name prompt directly with the hardcoded 1-tab/1-pane default. The current workspace is never reused.

## Run a plugin action

1. `Ctrl k` → expand Plugins. A failed plugin shows its error in red;
   its default action becomes "view error".
2. `Enter` on a healthy plugin → the list pane becomes a selector of
   that plugin's declared actions, default preselected.
3. `↑↓` to the action you want, `Enter` → it runs and the popup closes;
   `Esc` → back to the switcher with the plugin still selected.

## Capture the current workspace as a template

1. `prefix+ctrl+t` → the capture wizard opens with a live summary of
   the current workspace (tabs, panes, per-tab breakdown).
2. Walk the steps: name → match globs → command policy → cwd policy →
   tab names. A live YAML preview on the right shows the template
   evolving as you choose. `←` goes back, `Esc` aborts.
3. On Review, `Enter` writes the template to
   `~/.config/herdr/templates/<name>.yaml`. If the name clashes, choose
   overwrite / rename / cancel.
4. After the write, `y` opens `$EDITOR` on the file for fine-tuning
   (or `n` to close). Verify the `# best-effort:` command guesses.
5. The new template is immediately available in the `Enter` template
   picker the next time you open a directory or zoxide entry.
