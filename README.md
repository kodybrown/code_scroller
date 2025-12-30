# code_scroller

Auto-scroll code files in your terminal with syntax highlighting. Built with ratatui (for TUI) and syntect (for highlighting).

## Features

- Syntax-highlighted view of many languages
- Automatic vertical scrolling with configurable speed and step size
- Navigate between files in a directory tree
- Works on Windows, Linux, and macOS

## Install / Build

Prereqs: Rust toolchain (rustup + stable).

Debug build:

```bash
cargo build
```

Release build:

```bash
cargo build --release
```

The binary will be in `target/release/` (`code_scroller.exe` on Windows).

## Usage

```bash
code_scroller <PATH> [--speed-ms <u64>] [--step <usize>] [--loop=<bool>] [--ext <ext>]... [--max-kb <u64>] [--random-start=<bool>]
```

Arguments

- `PATH` — file or directory to display
- `--speed-ms` — delay per tick in milliseconds (default: 60)
- `--step` — number of lines advanced per tick (default: 1)
- `--loop` — whether to loop across files (default: true). To disable, pass `--loop=false`.
- `--ext` — repeatable flag for allowed extensions (with or without dots). Example: `--ext .cs --ext json --ext html`. If omitted, a broad default of common text/code types is used.
- `--max-kb` — maximum file size to load in KB (default: 512)
- `--random-start` — start at a random file (default: false). Enable via `--random-start=true`.

Key bindings

- `q` quit
- `space` pause/resume
- `n` / `RightArrow` next file
- `p` / `LeftArrow` previous file
- `r` reload current file
- `Home` go to top
- `End` go to bottom

Examples

```bash
# Auto-scroll the current directory with defaults
code_scroller .

# Faster scrolling, two lines per tick
code_scroller ./src --speed-ms 30 --step 2

# Only C#, JSON, and HTML files; allow bigger files
code_scroller . --ext .cs --ext .json --ext .html --max-kb 2048

# Do not loop across files
code_scroller . --loop=false

# Start at a random file
code_scroller . --random-start=true
```

## Publishing release binaries

Below are simple ways to produce distributable binaries for Windows and Linux.

### Local builds (per-OS)

Build on the target OS for the simplest, fully-static-free binaries:

- Windows (MSVC):

```powershell
cargo build --release
# output: target\\release\\code_scroller.exe
# optional: zip for distribution
Compress-Archive -Path target\\release\\code_scroller.exe -DestinationPath code_scroller-x86_64-pc-windows-msvc.zip
```

- Linux (GNU):

```bash
cargo build --release
# output: target/release/code_scroller
# optional: tar.gz for distribution
tar czf code_scroller-x86_64-unknown-linux-gnu.tar.gz -C target/release code_scroller
```

### Cross-compiling with `cross` (recommended for reproducible builds)

[`cross`](https://github.com/cross-rs/cross) uses Docker images to cross-compile without local linker setup.

Install once:

```bash
cargo install cross
```

Build Linux binary (from any OS with Docker available):

```bash
cross build --release --target x86_64-unknown-linux-gnu
# artifact: target/x86_64-unknown-linux-gnu/release/code_scroller
```

Build Windows GNU binary:

```bash
cross build --release --target x86_64-pc-windows-gnu
# artifact: target/x86_64-pc-windows-gnu/release/code_scroller.exe
```

You can then package the produced binaries (zip/tar.gz) as shown above.

### GitHub Actions (automated builds and releases)

If this repo is on GitHub, you can automate multi-OS release binaries.

Create `.github/workflows/release.yml` with the following minimal workflow:

```yaml
name: release
on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            bin: code_scroller.exe
            archive: zip
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            bin: code_scroller
            archive: tar.gz
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      - name: Package
        shell: bash
        run: |
          mkdir dist
          BIN=target/${{ matrix.target }}/release/${{ matrix.bin }}
          if [ "${{ matrix.archive }}" = "zip" ]; then
            7z a dist/code_scroller-${{ matrix.target }}.zip "$BIN"
          else
            tar czf dist/code_scroller-${{ matrix.target }}.tar.gz -C "$(dirname "$BIN")" "${{ matrix.bin }}"
          fi
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: code_scroller-${{ matrix.target }}
          path: dist/*

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist
      - uses: softprops/action-gh-release@v2
        with:
          files: dist/**/*
```

Usage:

1. Commit and push the workflow.
2. Create a tag like `v0.1.0` and push it (`git tag v0.1.0 && git push origin v0.1.0`).
3. GitHub Actions will build and attach archives to a GitHub Release automatically.

## Notes

- Some very large files are skipped based on `--max-kb`.
- Dotfiles are skipped unless you explicitly add them by modifying the code (current behavior avoids hidden files by default).
- Terminal size and font can affect perceived scroll speed.

## License

MIT or your preferred license.
