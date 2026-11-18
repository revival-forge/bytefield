# Tests and content layers

Bytefield reads the player's own install, and installs differ: the base game alone, with one or both expansions,
with Desert Combat or Galactic Conquest, with an HD pack laid over it. The tests are organised the same way, so that
each machine runs what it has and says what it skipped.

## The layers

| Layer | What | Folders under `Mods/` |
|---|---|---|
| synthetic | fixtures in the repository (`python/tests/fixtures`) | none |
| `base` | Battlefield 1942 1.6 | `bf1942` |
| `xpack1`, `xpack2` | The Road to Rome, Secret Weapons of WWII | `XPack1`, `XPack2` |
| `dc`, `dc07` | Desert Combat Final, Desert Combat 0.7 | `DC_Final`, `DesertCombat` |
| `gc` | Galactic Conquest | `GCMOD` |
| `ref` | mods we only compare against, never required | `DC_Extended`, `GC_Redux` |
| `hd` | an HD pack (`BYTEFIELD_HD`) | none |

The install the tests read is `data/game` (or `BYTEFIELD_GAME`). `BYTEFIELD_LAYERS` picks the layers even when more
are installed: a comma list of the names above, `xpack` for both expansions, or `all`. Unset, every installed layer
but `ref` and `hd`. So `BYTEFIELD_LAYERS=base` runs this machine as one with only the base game.

## Where a test goes

- **Rust.** A crate's tests on the real data live in `tests/<layer>/` (one test binary per layer: `tests/base/main.rs`
  declares a module per topic); what several layers share, in `tests/support/`. Each test asks for its layers with the
  `bytefield-testdata` dev-dependency and returns early, saying so, when one is missing:

  ```rust
  #[test]
  fn the_m2a3_of_bragg() {
      let game = bytefield_testdata::require!(Base, Dc);
      let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Bragg", "conquest").unwrap();
      // ...
  }
  ```

  `with_mod("DC_Final")` does the same for a test that takes the mod as a parameter. Unit tests next to the code
  (`#[cfg(test)]`) use the same calls. A test that scans `Mods/` (the menu) takes `install_view()`: the install with
  only the available layers' mods.
- **Python.** The folder under `python/tests/` says the layers (`synthetic/`, `base/…`, `dc/`, `gc/`); a case that needs
  more is marked `@pytest.mark.layer("xpack1")`, and a helper that loads a mod given as a parameter calls
  `require_mod(mod)` (`python/tests/layers.py`).
- A test with no game data at all goes in `synthetic/` (Python) or in the crate's plain tests (Rust): it is what
  continuous integration runs.

## Running them

```
scripts/tests quick     # the base game only, without the client's tests: each change, under 2 minutes
scripts/tests full      # every installed layer, the client's tests too: before a merge
scripts/tests ci        # no game data, as the public CI
```

Each prints how many tests each layer skipped. The client's tests (`cargo test -p bytefield-client` and the Python ones
under `tests/base/client/`, marked `slow` by their folder) drive the release client on a virtual display; the harness
runs at most as many clients at once as the video card has room for (about 1.6 GB each; `BYTEFIELD_CLIENT_SLOTS`
overrides it). Plain `cargo test` and `uv run pytest` work too, with the same
`BYTEFIELD_LAYERS`.
