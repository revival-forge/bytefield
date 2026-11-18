# Building Bytefield

You need [Rust](https://rustup.rs) (stable). No game data is needed to build or to run the tests.

## On Linux, for Linux

```
cargo build --release -p bytefield-client -p bytefield-server -p bytefield-content
target/release/bytefield
```

Besides Rust, a C compiler, `pkg-config` and the ALSA headers for sound: on Debian and Ubuntu,
`sudo apt install build-essential pkg-config libasound2-dev`; on Fedora, `sudo dnf install gcc pkg-config alsa-lib-devel`.
The window goes through X11 (XWayland on a Wayland desktop).

### A binary that runs on older distributions

A binary built on a recent distribution asks for that distribution's glibc and does not start on older ones. To link
it against an older glibc (2.31: Debian 11, Ubuntu 20.04 and later), without root:

1. Unpack [zig](https://ziglang.org/download/) (0.15) anywhere and put the folder on the `PATH`.
2. `cargo install cargo-zigbuild`
3. `GLIBC=2.31 scripts/package-linux <out_dir>`: builds, checks that no binary asks for a newer glibc, and leaves the
   tarball.

By hand: `PKG_CONFIG_ALLOW_CROSS=1 RUSTFLAGS="-C link-arg=-Wl,--allow-shlib-undefined" cargo zigbuild --release
--target x86_64-unknown-linux-gnu.2.31 -p bytefield-client -p bytefield-server -p bytefield-content` (the linker flag lets the
system's `libasound`, built against a newer glibc, stay unresolved until it runs on the player's machine, with theirs).

## On Linux, for Windows

With the same zig and cargo-zigbuild:

```
rustup target add x86_64-pc-windows-gnu
CFLAGS_x86_64_pc_windows_gnu="-Wno-error=date-time" \
    cargo zigbuild --release --target x86_64-pc-windows-gnu -p bytefield-client -p bytefield-server -p bytefield-content
```

The `.exe` files are in `target/x86_64-pc-windows-gnu/release/`. They only need the system's DLLs (the Universal C
Runtime, part of Windows 10 and 11). They run in Wine too, which is a quick check. (The `CFLAGS` line is because zig
treats mimalloc's use of `__DATE__` as an error.)

## On Windows, for Windows

1. Install [rustup](https://rustup.rs) with its default choice (the MSVC toolchain); it offers to install the Visual
   Studio Build Tools, which are needed.
2. Get the source (a zip from the project's page, or `git clone`) into a short path such as `C:\src\Bytefield` (Windows
   limits path length).
3. In a terminal there:

   ```
   cargo build --release -p bytefield-client -p bytefield-server -p bytefield-content
   ```

   The programs are in `target\release\`: copy `bytefield.exe`, `bytefield-content.exe` and `bytefield-server.exe` to a folder
   of your own.

## Packages

The release packages come from two scripts, run in a clean copy of the source; the CI runs the same two on every push
and keeps what they leave as the artifacts `bytefield-linux` and `bytefield-windows`, so a release can be made with or
without it.

- `GLIBC=2.31 scripts/package-linux <out_dir>`: `bytefield-<version>-linux-x86_64.tar.gz` (zig and cargo-zigbuild, as
  above; without `GLIBC` it links against the glibc of the machine it runs on).
- `scripts/package-windows <out_dir>`: `bytefield-<version>-windows-x86_64.zip`. On Linux it cross-builds with zig and
  cargo-zigbuild (and `rustup target add x86_64-pc-windows-gnu`); on Windows, in Git Bash, with cargo as it is. Needs
  `zip` or `7z`.

Both hold the three programs and the same texts (README, LICENSE, CREDITS, NOTICE, AUTHORS, CONTRIBUTING, the commands
and the third-party licences); the Windows one also has the Windows guide.

## Tests

```
cargo test
cd python && uv run pytest
```

Tests that need a real install skip when they do not find one.
