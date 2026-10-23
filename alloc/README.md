# mimicway-alloc

The global allocator of Mimicway: the system allocator, counting the bytes each thread holds. A rule script run reads the counter of its thread between operations and stops past its memory budget ([`src/engine/script_budget.rs`](../src/engine/script_budget.rs)).

## Why it exists

Rhai, the language of rule scripts, bounds each value (text length, array and map sizes) but not the sum of what a run holds: the arguments of one call, the keys of a map, what `map` gathers, curried function pointers and values held by nested calls each escaped it, up to 1.7 GB for a script of a few lines that any signed-in user can send to the rule tester. Counting from the language would mean finding every way a script can hold a value; counting what the thread allocates finds them all. A run is synchronous on its thread, so what that thread holds more than when the run started is what the run holds.

## The only `unsafe` code of the project

Implementing `GlobalAlloc` is `unsafe` by definition. The crate keeps it to one `impl` of four methods: each hands its arguments unchanged to `std::alloc::System` and returns its result, and around the call adds to or subtracts from a counter of the calling thread. That counter is a `thread_local!` initialized by a `const`, of a type without a destructor: reading it never allocates (the allocator would call itself) and stays possible while a thread ends. Each `unsafe` block states why it is sound (`// SAFETY:`), and the crate refuses an `unsafe` block without that comment (`clippy::undocumented_unsafe_blocks`).

An allocation never fails because of the counter: the budget is enforced between the operations of a script, where a run can be stopped with an error, while a failed allocation would abort the whole process.

## Checking it

```bash
cargo test -p mimicway-alloc
# Miri checks every unsafe operation the tests reach against the rules of the language
rustup toolchain install nightly-2026-09-01 --component miri rust-src
cargo +nightly-2026-09-01 miri test -p mimicway-alloc
```

The CI runs both whenever a file of `alloc/` changes (job "Allocator under Miri").
