'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const manifestPath = path.resolve(
  __dirname,
  '../../docs/engineering/platform-ci-matrix.json',
);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const { renderSummary, validateMatrix } = require('../../.github/scripts/platform-ci-status.cjs');

test('Linux x86_64 remains the required native CI target', () => {
  const linux = manifest.targets.find((target) => target.id === 'linux-x86_64');

  assert.ok(linux);
  assert.equal(linux.status, 'required');
  assert.equal(linux.runner, 'ubuntu-24.04');
  assert.equal(linux.rustTarget, 'x86_64-unknown-linux-gnu');
  assert.deepEqual(linux.cToolchains, ['GCC', 'Clang']);
  assert.ok(linux.requiredWorkflows.includes('Rust CI'));
  assert.ok(linux.requiredWorkflows.includes('C CI'));
  assert.deepEqual(validateMatrix(manifest), []);
});

test('future Windows and macOS targets are explicitly not enabled', () => {
  const futureTargets = manifest.targets.filter((target) => target.id !== 'linux-x86_64');

  assert.equal(futureTargets.length, 3);
  for (const target of futureTargets) {
    assert.equal(target.status, 'not-enabled', target.id);
    assert.ok(target.runnerRequirement, `${target.id} runner requirement`);
    assert.ok(target.rustTarget, `${target.id} Rust target`);
    assert.ok(target.cToolchain, `${target.id} C toolchain`);
    assert.ok(target.blockers.length > 0, `${target.id} blockers`);
    assert.ok(target.enablementCriteria.length > 0, `${target.id} enablement criteria`);
    assert.equal(target.requiredWorkflows, undefined, `${target.id} must not claim required jobs`);
  }
});

test('the readiness summary reports future targets as not enabled, never passed', () => {
  const summary = renderSummary(manifest);

  assert.match(summary, /linux-x86_64 \\| REQUIRED — active native CI/);
  assert.equal((summary.match(/NOT ENABLED — no native jobs scheduled/g) || []).length, 3);
  assert.match(summary, /not mark a not-enabled target as built, tested, supported, or passed/);
  assert.doesNotMatch(summary, /\\| (windows|macos)[^\\n]*\\| PASS\\b/i);
});

test('the policy rejects promoting a future target without changing the gate', () => {
  const invalid = structuredClone(manifest);
  invalid.targets.find((target) => target.id === 'windows-x86_64').status = 'required';

  assert.ok(validateMatrix(invalid).some((error) => error.includes('windows-x86_64 must remain not-enabled')));
});

test('the policy rejects duplicate target identifiers', () => {
  const invalid = structuredClone(manifest);
  invalid.targets.push(structuredClone(invalid.targets[0]));

  assert.ok(validateMatrix(invalid).includes('target IDs must be unique'));
});
