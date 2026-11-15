// Tests of the seeds of the Rust property tests (proptest). Every Rust file of src/ and tests/, found by walking them,
// is read: a property test draws its cases from the seed written beside it, through src/test_seed.rs, so that the CI
// and the mutation gate judge known cases and a red test stays red when run again (the reasons are in that file).
// Run with: node --test scripts/*.test.mjs
//
// Not seen: a runner built by a function of another file, a `proptest!` invoked through a macro of our own,
// proptest's attribute macro (`#[property_test]`, a feature the crate does not enable), and a test that draws through
// the server's own random generator (fastrand) without seeding it by `crate::test_seed::draw_from`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const HELPER = 'src/test_seed.rs';
const SEEDED = 'crate::test_seed::config(';

function rustFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return rustFiles(path);
    return entry.name.endsWith('.rs') ? [path] : [];
  });
}

// Returns one line per property test, or piece of proptest configuration, that does not take its seed from the helper.
export function unseeded(source) {
  const problems = [];
  const lines = source.split('\n');
  lines.forEach((line, i) => {
    const code = line.replace(/\/\/.*$/, '');
    // The closure and bracket forms take no `#![proptest_config]`: only the block form is allowed.
    if (/\bproptest!\s*[([]/.test(code)) problems.push(`${i + 1}: proptest! written otherwise than as a block`);
    if (/\bproptest!\s*\{/.test(code)) {
      const first = lines.slice(i + 1).find((l) => l.trim() !== '');
      if (!first || !first.trim().startsWith(`#![proptest_config(${SEEDED}`)) {
        problems.push(`${i + 1}: proptest! without #![proptest_config(${SEEDED}…)] as its first line`);
      }
    }
    const runner = code.match(/\bTestRunner::new\(\s*([^)]*)/);
    if (runner) {
      const argument = runner[1].trim();
      const name = argument.match(/^(\w+)$/)?.[1];
      // The last binding of that name before the runner.
      const binding = name
        ? lines
            .slice(0, i)
            .reverse()
            .find((l) => new RegExp(`\\blet\\s+(?:mut\\s+)?${name}\\s*=`).test(l))
        : undefined;
      const seeded = argument.startsWith(SEEDED) || (binding !== undefined && binding.includes(`= ${SEEDED}`));
      if (!seeded) problems.push(`${i + 1}: TestRunner::new without ${SEEDED}…)`);
    }
    for (const [pattern, what] of [
      [/\bRngSeed\b/, 'a seed chosen outside the helper'],
      [/\bProptestConfig\b/, 'a configuration built outside the helper'],
      [/\btest_runner::(?:Config\b|\{[^}]*\bConfig\b)/, 'a configuration built outside the helper'],
      [/\bTestRunner::(?:default|deterministic|new_with_rng)\b/, 'a runner whose seed the helper does not choose'],
      // Any other line that names a runner and assigns or calls (`let r: TestRunner = Default::default()`) builds one
      // the helper does not seed; a name in a list of imports does neither.
      [
        /^(?!\s*use\b)(?!.*\bTestRunner::new\()(?=.*[=(]).*\bTestRunner\b/,
        'a runner whose seed the helper does not choose',
      ],
      [/\bproptest_config\((?!crate::test_seed::config\()/, 'a configuration built outside the helper'],
    ]) {
      if (pattern.test(code)) problems.push(`${i + 1}: ${what}`);
    }
  });
  return problems;
}

test('every property test draws its cases from the seed written beside it', () => {
  const files = [...rustFiles(join(root, 'src')), ...rustFiles(join(root, 'tests'))];
  assert.ok(files.length > 50, `only ${files.length} Rust files found`);
  const problems = files
    .filter((file) => relative(root, file) !== HELPER)
    .flatMap((file) => unseeded(readFileSync(file, 'utf8')).map((p) => `${relative(root, file)}:${p}`));
  assert.deepEqual(problems, []);
  // The check reads real property tests: both forms are there.
  const sources = files.map((file) => readFileSync(file, 'utf8')).join('\n');
  assert.match(sources, /proptest!\s*\{\s*#!\[proptest_config\(crate::test_seed::config\(/);
  assert.match(sources, /TestRunner::new\(config\)/);
});

test('the unseeded shapes are found, the seeded ones are not', () => {
  assert.deepEqual(
    unseeded(`proptest! {
    #![proptest_config(crate::test_seed::config(256, 0x1))]

    #[test]
    fn law(x in 0..3u8) {}
}
let config = crate::test_seed::config(64, 0x2);
let result = TestRunner::new(config).run(&strategy, |x| Ok(()));
TestRunner::new(crate::test_seed::config(8, 0x3)).run(&s, |_| Ok(()));`),
    [],
  );
  assert.equal(unseeded(`proptest! {\n    #[test]\n    fn law(x in 0..3u8) {}\n}`).length, 1);
  assert.equal(unseeded(`proptest::proptest! {\n    #[test]\n    fn law(x in 0..3u8) {}\n}`).length, 1);
  assert.equal(
    unseeded(`proptest! {\n    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]\n}`)
      .length,
    3,
  );
  const drawn = `let config = Config { cases: 8, ..Config::default() };
let result = TestRunner::new(config).run(&s, |_| Ok(()));`;
  assert.equal(unseeded(drawn).length, 1);
  const rebound = `let config = crate::test_seed::config(8, 0x4);
let config = Config { cases: 9, ..config };
TestRunner::new(config).run(&s, |_| Ok(()));`;
  assert.equal(unseeded(rebound).length, 1);
  assert.equal(unseeded('let mut runner = TestRunner::default();').length, 2);
  assert.equal(unseeded('let runner = TestRunner::deterministic();').length, 2);
  assert.equal(unseeded('let mut runner: TestRunner = Default::default();').length, 1);
  assert.equal(unseeded('proptest!(|(x in 0..3u8)| { assert!(x < 3); });').length, 1);
  assert.deepEqual(unseeded('use proptest::test_runner::{\n    TestCaseError,\n    TestRunner,\n};'), []);
  assert.equal(unseeded('proptest![\n    #[test]\n    fn law(x in 0..3u8) {}\n];').length, 1);
  assert.equal(unseeded('use proptest::test_runner::{Config, RngSeed, TestRunner};').length, 2);
  // A comment that names them is not code.
  assert.deepEqual(unseeded('// RngSeed, TestRunner::default() and proptest! { are named here only'), []);
});
