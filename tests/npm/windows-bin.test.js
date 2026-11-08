const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join, relative } = require('node:path');
const { test } = require('node:test');
const optimize = require('../../npm/lib/windows-bin');

test('Windows optimization changes only command shims targeting this installation', () => {
  const root = mkdtempSync(join(tmpdir(), 'ad-windows-bin-'));
  const bin = join(root, 'node_modules/agent-desktop/bin');
  mkdirSync(bin, { recursive: true });
  const binary = join(bin, 'agent-desktop-win32-x64.exe');
  writeFileSync(binary, 'native');
  const command = join(root, 'agent-desktop.cmd');
  const otherCommand = join(root, 'node_modules/.bin/agent-desktop.cmd');
  mkdirSync(join(root, 'node_modules/.bin'), { recursive: true });
  const unrelated = '@echo off\nnode other-package/cli.js %*\n';
  writeFileSync(otherCommand, unrelated);
  const target = relative(root, join(bin, 'agent-desktop'));
  writeFileSync(command, `@echo off\nnode "${target}" %*\n`);
  assert.equal(optimize(bin, 'agent-desktop-win32-x64.exe'), true);
  const installed = readFileSync(command, 'utf8');
  assert.ok(installed.includes(`"${binary}" %*`));
  assert.doesNotMatch(installed, /node(?:\.exe)?[ "\t]/);
  assert.equal(readFileSync(otherCommand, 'utf8'), unrelated);
});

test('Windows optimization points the sh shim used by Git Bash at the native binary', {
  skip: process.platform === 'win32',
}, () => {
  const root = mkdtempSync(join(tmpdir(), 'ad-windows-bin-'));
  const bin = join(root, 'node_modules/agent-desktop/bin');
  mkdirSync(bin, { recursive: true });
  const binary = join(bin, 'agent-desktop-win32-x64.exe');
  writeFileSync(binary, '#!/bin/sh\necho native "$@"\n');
  chmodSync(binary, 0o755);
  const launcher = join(bin, 'agent-desktop');
  const launcherSource = '#!/usr/bin/env node\nrequire("./agent-desktop");\n';
  writeFileSync(launcher, launcherSource);
  writeFileSync(join(root, 'agent-desktop.cmd'),
    '@echo off\nnode "%~dp0\\node_modules\\agent-desktop\\bin\\agent-desktop" %*\n');
  const shim = join(root, 'agent-desktop');
  writeFileSync(shim, [
    '#!/bin/sh',
    'basedir=$(dirname "$(echo "$0" | sed -e \'s,\\\\,/,g\')")',
    'if [ -x "$basedir/node" ]; then',
    '  exec "$basedir/node"  "$basedir/node_modules/agent-desktop/bin/agent-desktop" "$@"',
    'else',
    '  exec node  "$basedir/node_modules/agent-desktop/bin/agent-desktop" "$@"',
    'fi',
    '',
  ].join('\n'));
  assert.equal(optimize(bin, 'agent-desktop-win32-x64.exe'), true);
  assert.doesNotMatch(readFileSync(shim, 'utf8'), /node(?:\.exe)?[ "\t]/);
  assert.equal(execFileSync('sh', [shim, 'a', 'b c'], { encoding: 'utf8' }), 'native a b c\n');
  assert.equal(readFileSync(launcher, 'utf8'), launcherSource);
});
