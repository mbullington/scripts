import assert from 'node:assert/strict';
import { execFile, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { promisify } from 'node:util';

const execute = promisify(execFile);
const manager = process.argv[2];
assert.ok(['npm', 'pnpm', 'yarn'].includes(manager), 'Usage: node npm/smoke.mjs <npm|pnpm|yarn> <tarball-directory>');
assert.ok(process.argv[3], 'Missing tarball directory');
const dist = resolve(process.argv[3]);
const targets = JSON.parse(readFileSync(new URL('./targets.json', import.meta.url)));
const target = targets.find(({ os, cpu }) => os === process.platform && cpu === process.arch);
assert.ok(target, 'The smoke test must run on a supported target');
const packages = new Map(readdirSync(dist).filter((file) => file.endsWith('.tgz')).map((file) => {
  const manifest = JSON.parse(execFileSync('tar', ['-xOf', join(dist, file), 'package/package.json']));
  return [manifest.name, { file, manifest, bytes: readFileSync(join(dist, file)) }];
}));
const { version } = packages.get('@mbullington/scripts').manifest;
assert.equal(packages.size, targets.length + 1, 'Provide the launcher and all platform tarballs from one build');

async function run(command, args, cwd, extraEnv = {}) {
  try {
    const { stdout } = await execute(command, args, {
      cwd, encoding: 'utf8', timeout: 120_000,
      env: { ...process.env, CI: '1', npm_config_update_notifier: 'false', ...extraEnv },
    });
    return stdout;
  } catch (error) {
    error.message += `\n${error.stdout ?? ''}\n${error.stderr ?? ''}`;
    throw error;
  }
}

await test(`${manager}: registry install selects the native package and runs without lifecycle scripts`, async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'scripts-npm-'));
  let registry;
  const server = createServer((request, response) => {
    const path = decodeURIComponent(request.url);
    const archive = [...packages.values()].find(({ file }) => path === `/tarballs/${file}`);
    if (archive) {
      response.writeHead(200, { 'Content-Type': 'application/octet-stream' });
      response.end(archive.bytes);
      return;
    }
    const entry = packages.get(path.slice(1));
    if (!entry) {
      response.writeHead(404, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify({ error: `Not in test registry: ${path}` }));
      return;
    }
    const { manifest, file, bytes } = entry;
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({
      name: manifest.name,
      'dist-tags': { latest: version },
      time: { [version]: '2020-01-01T00:00:00.000Z' },
      versions: { [version]: { ...manifest, dist: {
        tarball: `${registry}/tarballs/${file}`,
        integrity: `sha512-${createHash('sha512').update(bytes).digest('base64')}`,
      } } },
    }));
  });
  try {
    await new Promise((resolveListen, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolveListen);
    });
    registry = `http://127.0.0.1:${server.address().port}`;
    writeFileSync(join(cwd, 'package.json'), JSON.stringify({ name: 'scripts-install-smoke', private: true }));
    writeFileSync(join(cwd, '.npmrc'),
      `registry=${registry}\n@mbullington:registry=${registry}\ncache=${join(cwd, 'npm-cache')}\nstore-dir=${join(cwd, 'pnpm-store')}\n`);
    if (manager === 'yarn') {
      writeFileSync(join(cwd, '.yarnrc.yml'),
        `nodeLinker: pnp\nenableScripts: false\nenableGlobalCache: false\nenableTelemetry: false\n` +
        `globalFolder: ${JSON.stringify(join(cwd, 'yarn-global'))}\n` +
        `enableImmutableInstalls: false\nnpmRegistryServer: "${registry}"\nunsafeHttpWhitelist: ["127.0.0.1"]\n`);
    }
    const spec = `@mbullington/scripts@${version}`;
    const installArgs = {
      npm: ['install', '--save-dev', '--save-exact', '--ignore-scripts', '--no-audit', '--no-fund', spec],
      pnpm: ['add', '-D', '-E', '--ignore-scripts', spec],
      yarn: ['add', '--dev', '--exact', spec],
    }[manager];
    await run(manager, installArgs, cwd);
    const installed = JSON.parse(readFileSync(join(cwd, 'package.json')));
    assert.equal(installed.devDependencies['@mbullington/scripts'], version);
    const ciArgs = {
      npm: ['ci', '--ignore-scripts', '--no-audit', '--no-fund'],
      pnpm: ['install', '--frozen-lockfile', '--ignore-scripts'],
      yarn: ['install', '--immutable'],
    }[manager];
    await run(manager, ciArgs, cwd);
    const command = manager === 'npm' ? ['exec', '--', 'scripts'] : ['exec', 'scripts'];
    assert.equal((await run(manager, [...command, '--version'], cwd)).trim(), `scripts ${version}`);

    // Resolve from the launcher so this also checks Yarn's PnP dependency map.
    const inspect = `
      const assert = require('node:assert/strict');
      const { createRequire } = require('node:module');
      const fromLauncher = createRequire(require.resolve('@mbullington/scripts/package.json'));
      for (const platform of ${JSON.stringify(targets.map(({ platform }) => platform))}) {
        const name = '@mbullington/scripts-' + platform;
        if (platform === ${JSON.stringify(target.platform)}) {
          assert.equal(fromLauncher(name + '/package.json').version, ${JSON.stringify(version)});
        } else {
          assert.throws(() => fromLauncher.resolve(name + '/bin/scripts'));
        }
      }
    `;
    await run(manager === 'yarn' ? 'yarn' : process.execPath,
      manager === 'yarn' ? ['node', '-e', inspect] : ['-e', inspect], cwd);

    await run('git', ['init', '--quiet'], cwd);
    writeFileSync(join(cwd, 'SCRIPTS'), `[echo]\ncommand = 'printf "%s\\n" "$SCRIPTS_NPM_SMOKE"'\n\n[fail]\ncommand = 'exit 7'\n`);
    assert.equal(await run(manager, [...command, 'run', ':echo', '--quiet', '--', 'forwarded-argument'], cwd,
      { SCRIPTS_NPM_SMOKE: 'npm launcher environment' }), 'npm launcher environment\nforwarded-argument\n');
    await assert.rejects(run(manager, [...command, 'run', ':fail', '--quiet'], cwd),
      (error) => Number.isInteger(error.code) && error.code !== 0);
  } finally {
    server.closeAllConnections();
    await new Promise((resolveClose) => server.close(resolveClose));
    rmSync(cwd, { recursive: true, force: true });
  }
});
