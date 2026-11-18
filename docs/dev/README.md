# Developer documentation

How bytefield is put together, for people who want to read or change the code. For players, see `docs/user/`; for the
rules on where knowledge may come from and how to send changes, `CONTRIBUTING.md`.

- [architecture.md](architecture.md): the crates, what depends on what, and what one simulation step does.
- [formats.md](formats.md): the files of a Refractor 1 game that bytefield reads, and where each reader lives.
- [con-dialect.md](con-dialect.md): the `.con` script language as the games actually use it.
- [headless-bench.md](headless-bench.md): running the simulation without a window, from Rust, Python and the command line.
- [tests.md](tests.md): the tests by content layer (base game, expansions, mods), and how to run them.
