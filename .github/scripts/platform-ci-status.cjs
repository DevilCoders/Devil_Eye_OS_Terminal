'use strict';

const fs = require('node:fs');
const path = require('node:path');

const manifestPath = path.resolve(
  __dirname,
  '../../docs/engineering/platform-ci-matrix.json',
);

function validateMatrix(matrix) {
  const errors = [];

  if (matrix === null || typeof matrix !== 'object' || Array.isArray(matrix)) {
    return ['matrix must be a JSON object'];
  }
  if (matrix.schemaVersion !== 1) {
    errors.push('schemaVersion must be 1');
  }
  if (!matrix.policy || typeof matrix.policy !== 'object') {
    errors.push('policy must be an object');
  }
  if (!Array.isArray(matrix.targets) || matrix.targets.length === 0) {
    errors.push('targets must be a non-empty array');
    return errors;
  }

  const ids = matrix.targets.map((target) => target && target.id);
  if (matrix.targets.some((target) =>
    !target || typeof target !== 'object' || typeof target.id !== 'string' || target.id.length === 0)) {
    errors.push('every target must be an object with a non-empty ID');
  }
  if (new Set(ids).size !== ids.length) {
    errors.push('target IDs must be unique');
  }

  const initialTarget = matrix.policy?.initialTarget;
  const linux = matrix.targets.find((target) => target && target.id === initialTarget);
  if (!linux) {
    errors.push(`initial target ${String(initialTarget)} is missing`);
  } else {
    if (linux.status !== 'required') {
      errors.push('the initial Linux target must remain required');
    }
    if (linux.os !== 'Linux' || linux.architecture !== 'x86_64') {
      errors.push('the initial target must be Linux x86_64');
    }
    if (linux.runner !== 'ubuntu-24.04') {
      errors.push('the Linux runner must remain pinned to ubuntu-24.04');
    }
    if (linux.rustTarget !== 'x86_64-unknown-linux-gnu') {
      errors.push('the Linux Rust target triple is incorrect');
    }
    if (linux.cmakeMinimumVersion !== '3.25') {
      errors.push('the Linux CMake minimum version must remain 3.25');
    }
    if (linux.cmakeMinimumVersion !== '3.25') {
      errors.push('the Linux CMake minimum version must remain 3.25');
    }
    if (!Array.isArray(linux.requiredWorkflows)
      || !linux.requiredWorkflows.includes('Rust CI')
      || !linux.requiredWorkflows.includes('C CI')) {
      errors.push('Linux must require both Rust CI and C CI');
    }
    if (!Array.isArray(linux.cToolchains)
      || !linux.cToolchains.includes('GCC')
      || !linux.cToolchains.includes('Clang')) {
      errors.push('Linux C CI must cover both GCC and Clang');
    }
  }

  if (matrix.policy?.nonLinuxStatus !== 'not-enabled') {
    errors.push('nonLinuxStatus must be not-enabled until native jobs exist');
  }
  if (matrix.policy?.nativeIntegrationRequired !== true) {
    errors.push('nativeIntegrationRequired must remain true');
  }
  if (matrix.policy?.crossCompileAloneQualifies !== false) {
    errors.push('cross-compilation alone must not qualify a platform');
  }

  for (const target of matrix.targets) {
    if (!target || target.id === initialTarget) {
      continue;
    }
    if (target.status !== 'not-enabled') {
      errors.push(`${target.id || '<unnamed>'} must remain not-enabled`);
    }
    if (typeof target.runnerRequirement !== 'string' || target.runnerRequirement.length === 0) {
      errors.push(`${target.id || '<unnamed>'} must state its native runner requirement`);
    }
    if (typeof target.rustTarget !== 'string' || target.rustTarget.length === 0) {
      errors.push(`${target.id || '<unnamed>'} must state its Rust target triple`);
    }
    if (typeof target.cToolchain !== 'string' || target.cToolchain.length === 0) {
      errors.push(`${target.id || '<unnamed>'} must state its C toolchain`);
    }
    if (!Array.isArray(target.blockers) || target.blockers.length === 0) {
      errors.push(`${target.id || '<unnamed>'} must document current blockers`);
    }
    if (!Array.isArray(target.enablementCriteria) || target.enablementCriteria.length === 0) {
      errors.push(`${target.id || '<unnamed>'} must document enablement criteria`);
    }
    if (Array.isArray(target.requiredWorkflows) && target.requiredWorkflows.length > 0) {
      errors.push(`${target.id || '<unnamed>'} cannot declare required workflows while not-enabled`);
    }
  }

  return errors;
}

function renderSummary(matrix) {
  const lines = [
    '# Platform CI readiness',
    '',
    '> This is a readiness declaration, not a build result. Only Linux x86_64 has active native build/test workflows.',
    '',
    '| Target | State | Runner / toolchain | Native CI evidence |',
    '| --- | --- | --- | --- |',
  ];

  for (const target of matrix.targets) {
    if (target.status === 'required') {
      const workflows = [
        ...(target.requiredWorkflows || []),
        ...(target.supplementalWorkflows || []),
      ].join('; ');
      lines.push(
        `| ${target.id} | REQUIRED — active native CI | ${target.runner}; Rust ${target.rustToolchain}; ${(target.cToolchains || []).join('/')}`
        + ` | ${workflows} |`,
      );
    } else {
      lines.push(
        `| ${target.id} | NOT ENABLED — no native jobs scheduled | ${target.runnerRequirement} ${target.cToolchain}`
        + ` | No native CI evidence; blockers remain |`,
      );
    }
  }

  lines.push(
    '',
    'A green result from this reporting workflow validates the matrix policy only. It does not mark a not-enabled target as built, tested, supported, or passed.',
    '',
  );
  return lines.join('\n');
}

function main() {
  let matrix;
  try {
    matrix = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  } catch (error) {
    process.stderr.write(`Unable to read platform CI matrix: ${error.message}\n`);
    process.exitCode = 1;
    return;
  }

  const errors = validateMatrix(matrix);
  if (errors.length > 0) {
    process.stderr.write(`Invalid platform CI matrix:\n- ${errors.join('\n- ')}\n`);
    process.exitCode = 1;
    return;
  }

  process.stdout.write(`${renderSummary(matrix)}\n`);
}

if (require.main === module) {
  main();
}

module.exports = { renderSummary, validateMatrix };
