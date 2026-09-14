# Publishing npm packages

The release workflow publishes these public packages:

- `@mbullington/scripts`
- `@mbullington/scripts-darwin-arm64`
- `@mbullington/scripts-darwin-x64`
- `@mbullington/scripts-linux-x64`
- `@mbullington/scripts-linux-arm64`

`npm/package.py` generates package manifests from `Cargo.toml` and
`npm/targets.json`. Do not maintain separate npm version numbers. Release tags
must equal `v<Cargo version>`. The workflow rejects mismatches before publishing
to npm, crates.io, or GitHub Releases.

## Configure publishing

1. Confirm that your npm account can publish public packages in the
   `@mbullington` scope.
2. For the first release, create a granular npm access token with write access
   to these packages and permission to bypass 2FA. Save it as the repository's
   `NPM_TOKEN` Actions secret. Do not commit the token.
3. After the packages exist, configure a trusted publisher in each package's
   npm settings. Select GitHub Actions with user `mbullington`, repository
   `scripts`, and workflow filename `release.yml`. Leave the environment name
   blank unless you also add that environment to the publishing job.
4. Remove the repository's `NPM_TOKEN` secret and revoke the bootstrap token
   after trusted publishing works. The workflow grants `id-token: write` and
   installs npm 11 for OIDC publishing.

See npm's [trusted publishing documentation](https://docs.npmjs.com/trusted-publishers/).
No credentials are needed to build or test packages.

## Release

1. Update the crate version and lockfile as usual.
2. Push the matching `v<version>` tag when the change is ready for publication.
3. Check the `release` Actions run. It builds and packages all four targets,
   then installs the tarballs with npm, pnpm, and Yarn PnP on each target.
   Install tests disable lifecycle scripts and use a temporary loopback registry
   serving only the packed artifacts. They check platform filtering and frozen
   lockfile installs without accessing the public npm registry. The test closes
   its registry before exiting.
4. Confirm that all five npm packages have the same version.

The publish job uploads platform packages before the launcher. Stable versions
use the `latest` dist-tag. Prereleases use `next`. Retrying a partially completed
publish skips packages only when their registry integrity matches the local
tarball. If an existing version has different contents, publish a new version
rather than overwriting it.

A manual workflow run from a branch builds and tests without publishing.
A manual run from a `v*` tag can publish. No package is published by pull-request
CI. GitHub archives now include architecture in their filenames, such as
`scripts-v0.1.0-darwin-arm64.tar.gz`.

## Verify locally

With Python 3.11 or later, Node.js 22.15+ or 23.11+, npm, and Rust installed:

```sh
cargo build --locked --release
python3 npm/package.py --tag v0.1.0 pack linux-x64 target/release/scripts
node --test npm/*.test.cjs
```

Use the current Cargo version and the platform matching your machine. Local
packaging copies the supplied binary. The release workflow builds Linux with
`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` for static binaries.
Each target builds and runs its installation checks on a matching native runner.

To repeat the full installation checks, download all five `npm-*` artifacts
from one Actions run into a single directory. With npm, pnpm 10, and Yarn 4 on
`PATH`, run:

```sh
node npm/smoke.mjs npm target/npm/dist
node npm/smoke.mjs pnpm target/npm/dist
node npm/smoke.mjs yarn target/npm/dist
node npm/publish.mjs 0.1.0 target/npm/dist --dry-run
```

The launcher uses Node's `process.execve` to replace itself with the native
binary. This preserves terminal ownership, arguments, environment, and exit
signals without a JavaScript process managing the task runner.
