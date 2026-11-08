# 2. Performance is judged by how cost grows, counted by Callgrind

Accepted, 2026-11-08.

## Context

Mimicway promises to stay light with many rules and much traffic ([roadmap](../ROADMAP.md), "What Mimicway stays"). Until now no check in CI measured cost: benchmarks were run once, recorded in the commit that needed them, and removed, with no permanent `criterion` suite. What a permanent measure should be was left open until a test could say how cost grows with the input.

A shared CI runner gives wall-clock times that vary by tens of percent from one run to the next; a laptop under the load of other work, more. A permanent check has to give the same verdict for the same code.

## Options

1. **Timed benchmarks (`criterion`) in CI, compared with a stored baseline.** Measures what a user feels, but a runner's noise is larger than most regressions worth catching, and the baseline goes stale with each runner image.
2. **Instruction counts compared with a stored baseline.** Callgrind counts the same instructions on every run, so a threshold of a fraction of a percent works. But every compiler release, dependency update or change of heap layout moves the counts with no change of the work: the baseline would need a new commit each time, and a red verdict would mostly mean "the toolchain changed".
3. **The growth of instruction counts with the input.** Each family of inputs (rules for one request, services, requests, conflict search, configuration load and save, suggestions, script values) runs at n, 2n and 4n under Callgrind; the verdict is the ratio of the counts at 4n and n against the growth written next to the family (linear, n log n), with one tolerance. A compiler or dependency change moves all three counts together and leaves the ratio alone; a step that turns quadratic changes it by four.

## Decision

Option 3 is the permanent measure: `scripts/scale-test.mjs`, the CI job `scale`, with the workloads in `src/scale_probe.rs` (feature `scale-probe`, never in the shipped binary). The growth from n to 4n may exceed its class by a factor of 1.25 at most, and must reach 4 / 1.25: a family whose fixed cost hides its growth is red too, so a test that measures nothing cannot stay green.

A change made for speed is still measured once, by counting the family it touches before and after with the same probe (`valgrind --tool=callgrind --collect-atstart=no --toggle-collect='*scale_probe::measured*' <probe> probe <family> <n>`), and the numbers go in its commit message. No baseline is stored, and no `criterion` suite is kept.

The C library's allocator runs without its per-thread cache and fast bins under the test (`GLIBC_TUNABLES`): with them, the work of `malloc` and `free` jumps with the size of the heap by more than the tolerance, at one size and not the next. Its instructions stay counted, so a step that allocates or copies more is still seen.

## Consequences

- A cost that stays linear but doubles (a constant factor) is not seen by CI; only the one-off count of a change made for speed, or a user, sees it.
- A cost that grows only beyond 4n, with an input shape no family builds, or as time spent waiting (disk, network, locks), is not seen either. A new kind of input that can grow with use gets its family.
- A family's sizes stay on one side of a threshold of a dependency: Rhai's string interner changes its behaviour beyond 256 strings, and Rhai refuses maps of more than 500 keys, so the script family stays under 256 values.
- The first run found two quadratic steps, fixed with it: finding the service of a request compared every service's group with every group, and grouping observed responses for a suggestion compared each response with every group found so far.

## What would make this decision wrong

- A regression of speed that users report and that stays within its growth class: an absolute count per family, kept as a budget, would then be the next step.
- A ratio that changes from one run to the next with the same code by more than a few percent: the tolerance would then be judging noise, and the counting method has to change before the tolerance does.
