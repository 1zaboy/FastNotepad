# Fast Win32 Notepad — Design Spec

## Summary

Windows-only Rust text editor optimized for cold start and editing multi-GB files.

## Architecture

- **UI:** Native Win32 window, menus, common dialogs, GDI text rendering
- **Buffer:** Piece table over mmap (original) + add-buffer (edits), logical UTF-8 offsets
- **Line index:** Incremental line-start offsets for virtualization and gutter
- **Undo:** Inverse insert/delete operations, not full snapshots
- **Encodings:** UTF-8 (with/without BOM), UTF-16 LE/BE, Windows-1251

## Phase 1 scope

New/Open/Save/Save As, edit with undo/redo, clipboard, line numbers, virtual scroll, Find, encoding handling.

## Phase 2 (Notepad+)

1. **Tabs** — multi-document tabs, Ctrl+T / Ctrl+W / Ctrl+Tab, dirty markers
2. **Status bar** — line, column, encoding, line ending, language
3. **Goto Line** — Ctrl+G
4. **Find/Replace** — modeless dialog, chunked replace all
5. **Recent files** — persisted in `%APPDATA%\FastNotepad\settings.json`
6. **Font picker** — ChooseFontW
7. **Word wrap** — soft wrap for viewport
8. **External change watcher** — prompt on disk change
9. **Dark/Light theme** — persisted colors
10. **Print** — PrintDlgW + GDI pagination
11. **Language modes** — built-in plugin-ready `LanguageMode` API; detect by extension (+ JSON sniff); viewport syntax highlight; Format Document (`Ctrl+Shift+F`). Modes: Plain Text (Unknown), Markdown, JSON. Manual override via Language menu (`language_locked`). Format refuses docs larger than 5 MB.

Speed invariants from Phase 1 remain: mmap UTF-8, piece table, virtualized paint, incremental line index. Highlighting is viewport-scoped with a 64-line lexer seed; no tree-sitter / external plugins in v1.
