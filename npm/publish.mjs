import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import assert from 'node:assert/strict';

const [version, directory, mode] = process.argv.slice(2);
assert.match(version ?? '', /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
assert.ok(directory && (!mode || mode === '--dry-run'),
  'Usage: node npm/publish.mjs <version> <tarball-directory> [--dry-run]');
const targets = JSON.parse(readFileSync(new URL('./targets.json', import.meta.url)));
const names = [...targets.map(({ platform }) => `@mbullington/scripts-${platform}`), '@mbullington/scripts'];
const packages = names.map((name) => {
  const archive = resolve(join(directory, `${name.replace('@', '').replace('/', '-')}-${version}.tgz`));
  const manifest = JSON.parse(execFileSync('tar', ['-xOf', archive, 'package/package.json']));
  assert.equal(manifest.name, name);
  assert.equal(manifest.version, version);
  if (name === '@mbullington/scripts') {
    assert.deepEqual(manifest.optionalDependencies,
      Object.fromEntries(names.slice(0, -1).map((dependency) => [dependency, version])));
  }
  return { name, archive, integrity: `sha512-${createHash('sha512').update(readFileSync(archive)).digest('base64')}` };
});

// Publish native dependencies first. A retry may skip only byte-identical tarballs.
for (const { name, archive, integrity } of packages) {
  if (mode !== '--dry-run') {
    const result = spawnSync('npm', ['view', `${name}@${version}`, 'dist.integrity', '--json'], { encoding: 'utf8' });
    if (result.error) throw result.error;
    const existing = JSON.parse(result.stdout);
    if (result.status === 0) {
      assert.equal(existing, integrity, `${name}@${version} already exists with different contents`);
      console.log(`Already published: ${name}@${version}`);
      continue;
    }
    if (existing.error?.code !== 'E404') {
      throw new Error(`Cannot check ${name}@${version}: ${result.stderr}`);
    }
  }
  execFileSync('npm', [
    'publish', archive, '--access', 'public',
    '--tag', version.includes('-') ? 'next' : 'latest',
    ...(mode === '--dry-run' ? ['--dry-run'] : ['--provenance']),
  ], { stdio: 'inherit' });
}
