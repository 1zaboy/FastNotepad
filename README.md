# Fast Notepad

Native Windows text editor written in Rust. Built for fast cold start and comfortable editing of very large files (mmap + piece table, virtualized painting).

**Platform:** Windows only (Win32 / GDI).

## Features

- Native Win32 UI with tabs, menus, and status bar
- mmap + piece-table buffer with incremental line index
- Virtualized paint (only visible lines are drawn)
- Edit, undo/redo, clipboard, Find / Replace, Goto Line
- Encodings: UTF-8 (±BOM), UTF-16 LE/BE, Windows-1251
- Recent files, font picker, word wrap, dark/light theme
- External file change detection
- Print support
- Language modes: Markdown, JSON, Plain Text — detect on open, viewport syntax highlight, Format Document

## Requirements

- Windows 10/11
- [Rust](https://rustup.rs/) (edition 2021 toolchain)

## Build / Run

```bash
cargo run --release
```

Release binary:

```text
target/release/fast-notepad.exe
```

## Tests

```bash
cargo test
```

## Project layout

| Path | Purpose |
|------|---------|
| `src/` | Application, buffer engine, rendering, language modes |
| `tests/` | Integration tests |
| `docs/` | Design notes |

## License

MIT — see [LICENSE](LICENSE).
