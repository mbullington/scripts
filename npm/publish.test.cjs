'use strict';

const assert = require('node:assert/strict');
const { execFileSync, spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const { test } = require('node:test');

function fixture(t) {
  const directory = mkdtempSync(join(tmpdir(), 'scripts-publish-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const names = [
    ...require('./targets.json').map(({ platform }) => `@mbullington/scripts-${platform}`),
    '@mbullington/scripts',
  ];
  const version = '1.2.3-beta.1';
  const contents = join(directory, 'package');
  mkdirSync(contents);
  const integrities = {};
  for (const name of names) {
    writeFileSync(join(contents, 'package.json'), JSON.stringify({
      name, version,
      ...(name === '@mbullington/scripts' ? {
        optionalDependencies: Object.fromEntries(names.slice(0, -1).map((dependency) => [dependency, version])),
      } : {}),
    }));
    const archive = join(directory, `${name.replace('@', '').replace('/', '-')}-${version}.tgz`);
    execFileSync('tar', ['-czf', archive, '-C', directory, 'package']);
    integrities[`${name}@${version}`] = `sha512-${createHash('sha512').update(readFileSync(archive)).digest('base64')}`;
  }
  const log = join(directory, 'calls.jsonl');
  const state = join(directory, 'state.json');
  writeFileSync(log, '');
  writeFileSync(join(directory, 'npm'), `#!/usr/bin/env node
const fs = require('node:fs');
const args = process.argv.slice(2);
const state = JSON.parse(fs.readFileSync(process.env.MOCK_NPM_STATE));
fs.appendFileSync(process.env.MOCK_NPM_LOG, JSON.stringify(args) + '\\n');
if (args[0] === 'view') {
  const integrity = state.existing?.[args[1]];
  console.log(JSON.stringify(integrity ?? { error: { code: 'E404' } }));
  process.exit(integrity ? 0 : 1);
}
if (args[0] !== 'publish') process.exit(99);
process.exit(state.failPublish ? 1 : 0);
`, { mode: 0o755 });
  function run(scenario) {
    writeFileSync(state, JSON.stringify(scenario));
    const result = spawnSync(process.execPath, [join(__dirname, 'publish.mjs'), version, directory], {
      encoding: 'utf8', timeout: 10_000,
      env: { ...process.env, PATH: `${directory}:${process.env.PATH}`, MOCK_NPM_LOG: log, MOCK_NPM_STATE: state },
    });
    const calls = readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map((line) => JSON.parse(line));
    return { result, calls };
  }
  return { run, integrities, names };
}

test('publishes dependencies before launcher and sends prereleases to next', (t) => {
  const { run, names } = fixture(t);
  const { result, calls } = run({});
  assert.equal(result.status, 0, result.stderr);
  const publishes = calls.filter(([command]) => command === 'publish');
  assert.equal(publishes.length, names.length);
  for (const [index, args] of publishes.entries()) {
    assert.ok(args[1].endsWith(`${names[index].replace('@', '').replace('/', '-')}-1.2.3-beta.1.tgz`));
    assert.equal(args[args.indexOf('--tag') + 1], 'next');
    assert.ok(args.includes('--provenance'));
  }
});

test('retry skips only byte-identical packages', (t) => {
  const { run, integrities } = fixture(t);
  const { result, calls } = run({ existing: integrities });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(calls.filter(([command]) => command === 'publish').length, 0);
});

test('conflicting existing tarball stops publication', (t) => {
  const { run, integrities } = fixture(t);
  const first = Object.keys(integrities)[0];
  const { result, calls } = run({ existing: { [first]: 'sha512-different' } });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /already exists with different contents/);
  assert.equal(calls.filter(([command]) => command === 'publish').length, 0);
});

test('failed platform publication never publishes the launcher', (t) => {
  const { run } = fixture(t);
  const { result, calls } = run({ failPublish: true });
  assert.equal(result.status, 1);
  assert.equal(calls.filter(([command]) => command === 'publish').length, 1);
});
