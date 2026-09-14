#!/usr/bin/env node
'use strict';

const { accessSync, constants } = require('node:fs');
const targets = require('./targets.json');
const { version } = require('./package.json');

try {
  const target = targets.find(({ os, cpu }) => os === process.platform && cpu === process.arch);
  if (!target) {
    throw new Error(
      `Unsupported platform ${process.platform}-${process.arch}. ` +
      `Supported targets: ${targets.map(({ platform }) => platform).join(', ')}. ` +
      'To build from source on a supported Unix system, run: cargo install scripts_runner',
    );
  }
  if (typeof process.execve !== 'function') {
    throw new Error('The npm launcher requires Node.js 22.15+ or 23.11+. Upgrade Node.js or run: cargo install scripts_runner');
  }

  const name = `@mbullington/scripts-${target.platform}`;
  let binary;
  try {
    const installed = require(`${name}/package.json`);
    if (installed.version !== version) {
      throw new Error(`Expected ${version}, found ${installed.version}`);
    }
    binary = require.resolve(`${name}/bin/scripts`);
    accessSync(binary, constants.X_OK);
  } catch (error) {
    throw new Error(
      `Cannot load ${name}@${version}: ${error.message}. ` +
      'Reinstall @mbullington/scripts with optional dependencies enabled on this machine. ' +
      'Do not use --omit=optional or --no-optional, or copy node_modules between platforms. ' +
      'Alternatively, run: cargo install scripts_runner',
    );
  }

  process.execve(binary, [binary, ...process.argv.slice(2)], process.env);
} catch (error) {
  console.error(`scripts: ${error.message}`);
  process.exitCode = 1;
}
