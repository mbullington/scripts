'use strict';

const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const { test } = require('node:test');

function fixture(t, platform = `${process.platform}-${process.arch}`) {
  const directory = mkdtempSync(join(tmpdir(), 'scripts-launcher-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const file of ['scripts.cjs', 'targets.json']) copyFileSync(join(__dirname, file), join(directory, file));
  writeFileSync(join(directory, 'package.json'), JSON.stringify({ version: '1.2.3' }));
  const dependency = join(directory, 'node_modules/@mbullington', `scripts-${platform}`);
  function install(script, version = '1.2.3') {
    mkdirSync(join(dependency, 'bin'), { recursive: true });
    writeFileSync(join(dependency, 'package.json'), JSON.stringify({ version }));
    writeFileSync(join(dependency, 'bin/scripts'), `#!/bin/sh\n${script}\n`, { mode: 0o755 });
  }
  function run(args = [], options = {}) {
    return spawnSync(process.execPath, [join(directory, 'scripts.cjs'), ...args], {
      encoding: 'utf8', timeout: 5_000, ...options,
    });
  }
  return { directory, install, run };
}

test('missing optional package gives reinstall instructions and Cargo fallback', (t) => {
  const result = fixture(t).run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /optional dependencies enabled/);
  assert.match(result.stderr, /cargo install scripts_runner/);
});

test('mismatched platform package cannot silently run another version', (t) => {
  const { install, run } = fixture(t);
  install('exit 0', '1.2.2');
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Expected 1.2.3, found 1.2.2/);
});

test('unsupported platform lists supported targets and Cargo fallback', (t) => {
  const { directory } = fixture(t);
  const result = spawnSync(process.execPath, ['-e',
    `Object.defineProperty(process, 'platform', { value: 'win32' }); require(${JSON.stringify(join(directory, 'scripts.cjs'))})`,
  ], { encoding: 'utf8', timeout: 5_000 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Unsupported platform win32/);
  assert.match(result.stderr, /darwin-arm64, darwin-x64, linux-x64, linux-arm64/);
  assert.match(result.stderr, /cargo install scripts_runner/);
});

test('launcher preserves arguments, stdin, environment, cwd, output, and exit status', (t) => {
  const { directory, install, run } = fixture(t);
  install('read -r input; printf "%s\\n" "$input" "$VALUE" "$PWD" "$@"; echo stderr >&2; exit 37');
  const result = run(['space in argument', '$not-expanded', ''], {
    cwd: directory,
    input: 'from stdin\n',
    env: { ...process.env, VALUE: 'from environment' },
  });
  assert.equal(result.status, 37);
  // macOS resolves /var to /private/var when the shell sets PWD.
  const { realpathSync } = require('node:fs');
  assert.equal(result.stdout, `from stdin\nfrom environment\n${realpathSync(directory)}\nspace in argument\n$not-expanded\n\n`);
  assert.equal(result.stderr, 'stderr\n');
});

test('Linux ARM64 selects its own platform package', (t) => {
  const { directory, install } = fixture(t, 'linux-arm64');
  install('echo arm64-package; exit 17');
  const result = spawnSync(process.execPath, ['-e',
    `Object.defineProperty(process, 'platform', { value: 'linux' });
     Object.defineProperty(process, 'arch', { value: 'arm64' });
     require(${JSON.stringify(join(directory, 'scripts.cjs'))})`,
  ], { encoding: 'utf8', timeout: 5_000 });
  assert.equal(result.status, 17, result.stderr);
  assert.equal(result.stdout, 'arm64-package\n');
});

test('native signal termination is preserved', (t) => {
  const { install, run } = fixture(t);
  install('kill -TERM $$');
  const result = run();
  assert.equal(result.signal, 'SIGTERM');
  assert.equal(result.status, null);
});
