# The `.con` dialect

Refractor 1 describes almost everything in plain-text scripts: what a vehicle is made of, how a weapon fires, where
objects stand in a level, how a game mode scores. A mod is mostly scripts. bytefield runs them with its own interpreter
(`crates/con-lang`); what the interpreter must cover was measured over the scripts of the base game, Desert Combat and
Galactic Conquest, not guessed. `python/tools/inventory.py` lists every command, keyword and condition those mods use, over your own copy.

## The language

It is small:

- One command per line: `Namespace.command arg1 arg2 …`, e.g. `ObjectTemplate.create PlayerControlObject Jeep` or
  `ObjectTemplate.setPosition 0/0.5/1.2`. Vectors are written `x/y/z`. `rem` starts a comment line, and
  `beginrem`/`endrem` a block.
- `run file [args]` executes another script; `include file [args]` executes it in the caller's scope. Arguments arrive
  as `v_arg1` … `v_arg8`.
- `var v_name` and `const c_name = value`; `command -> v_name` stores a command's result.
- `if`/`elseif`/`else`/`endif` with `==` comparisons. In practice nearly every condition is `v_arg1 == host`.
- Names starting with `c_` that are never declared are the engine's own constants: input axes (`c_PIThrottle`),
  engine types (`c_ETCar`), grip flags, seat flags, collision groups.

## State is global

The interpreter is a state machine more than a language. `ObjectTemplate.create X` makes `X` the *active* template, and
every `ObjectTemplate.*` line after it changes `X` until another `create` or `ObjectTemplate.active Y`. A vehicle's
`Objects.con` creates its parts and its `Physics.con`, run later, comes back to them with `active`. Files run with
`include` (the soldiers' common data, for instance) set properties on whatever template the includer left active.
`addTemplate` puts a child under the active template, and the `setPosition`/`setRotation` that follow place that child.

## Quirks the data relies on

Fidelity means reproducing what the original does with imperfect scripts, not fixing them:

- `run` paths are relative to the running script, but many only resolve from a parent directory (a game mode's script
  runs its siblings as if from the level's directory). The interpreter searches the same way.
- Stray lines (separators made of asterisks, a bare path, binary garbage) are ignored, as the original does.
- A line such as `ren ObjectTemplate.create …` is not a command: the template it meant to create never exists, and
  everything that refers to it later is silently skipped.
- An `if` on an undeclared variable does not open a block in the original: its body runs and its `endif` closes the
  enclosing one. One shipped level depends on this for its ticket counts.
- A variable lives in the file that declared it; an `include` with arguments binds its own `v_argN` in the shared
  scope and gives the includer's back when it ends.
- Arguments may be quoted (icon paths); case never matters in names or paths.

## Where it is tested

`cargo test -p bytefield-con` runs the interpreter over synthetic scripts (`crates/con-lang/tests` and
`python/tests/fixtures`). With a real install present, `con-check` (`target/release/con-check levels <game> <mod>`)
lists and loads the levels of a mod (`con-check load <game> <mod> --level L` runs one level's scripts and summarizes what they made).
