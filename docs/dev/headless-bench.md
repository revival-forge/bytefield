# The headless bench

The simulation runs without a window, deterministically, at the original's fixed step (1/30 s). That makes it a test
bench: the tests, the bot tournaments, the measurement tools and the bug-report replays all drive the same `Sim` the
game draws. Nothing below needs a GPU or a display.

All of it needs an installed game (`<game>` below: a directory with `BF1942.exe` and `Mods/`). Without one, the tests
that need it skip; the rest run on synthetic data.

## From Rust

```rust
let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest")?;
let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
let jeep = s.spawn_vehicle("Willy", [1000.0, 30.0, 1200.0], [0.0, 0.0, 0.0]).unwrap();
for _ in 0..300 {
    s.step();
}
println!("{:?}", s.vehicles[jeep].craft.body.pos);
```

`crates/sim/tests/` has many small examples of this shape: a vehicle left alone stands still, a spring turned in its
parent keeps its parent's axes, a round ends when the tickets run out.

## From Python

`crates/py` builds `import bytefield`, the same bench for scripts and notebooks:

```sh
cd python && uv run maturin develop --release -m ../crates/py/Cargo.toml
```
```python
import bytefield
w = bytefield.World("<game>", "bf1942", "El_Alamein", "conquest")
w.world_size, w.height_at(1000, 1000)
s = w.sim(1 / 30)
v = s.add_vehicle("Willy", (1000, 30, 1200), (0, 0, 0))
s.step(300, False)
```
`python/tests/base/bench/test_bench.py` shows the rest: terrain queries, segment tests against obstacles, damage formulas, firing
weapons and following projectiles.

## From the command line

The examples in `crates/sim/examples` are small tools over the bench (`cargo build --release -p bytefield-sim --examples`,
then `target/release/examples/<name>`):

- `arena <game> <mod> <level> <players> <minutes> [--mind bots|none] [--seed N]`: one round of bots, with who won,
  tickets, flags and kills.
- `tournament3 <game> --a bots --b bots --maps bf1942:Berlin,… --seeds 4 --minutes 8 --players 32 --out <dir>`: many
  rounds with the sides swapped, one JSON per round with the quality meter's cards; `python3 -I
  python/tools/bots_table.py <dir>` makes the table.
- `drive_all`, `fire_all`, `fire_kits`: every vehicle of a level driven, every weapon fired, every kit's weapons, with
  what moved, hit and broke.

## Determinism

Same build, same inputs, same seed: the same result, bit for bit. Every random draw goes through the sim's seeded
generator; nothing reads the clock or iterates a hash map in an order that matters. A change that should not alter
behaviour (a rename, a refactor) is checked by running a tournament before and after and comparing the outputs value by
value. A debug build with `--profile checked` also asserts after every step that nothing became NaN or infinite.
