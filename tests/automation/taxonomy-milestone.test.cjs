"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  resolveMilestoneTitle,
} = require("../../.github/scripts/taxonomy-milestone.cjs");

const milestones = [
  "M0 Engineering Foundation",
  "M9 Release Candidate",
  "M10 Production 1.0",
];
const epicMilestones = {
  "DE-001": "M0 Engineering Foundation",
  "DE-016": "M9 Release Candidate",
};

test("resolves a direct milestone target", () => {
  assert.equal(
    resolveMilestoneTitle("M0 Engineering Foundation", "DE-001", milestones, epicMilestones),
    "M0 Engineering Foundation",
  );
});

test("resolves a continuous release-gate target to its milestone", () => {
  assert.equal(
    resolveMilestoneTitle("Continuous; release gate M9", "DE-016", milestones, epicMilestones),
    "M9 Release Candidate",
  );
});

test("does not assign a single milestone to work spanning all milestones", () => {
  assert.equal(
    resolveMilestoneTitle("Continuous; all milestones", "DE-019", milestones, epicMilestones),
    null,
  );
});

test("uses the Epic milestone mapping when no explicit target exists", () => {
  assert.equal(
    resolveMilestoneTitle(null, "DE-001", milestones, epicMilestones),
    "M0 Engineering Foundation",
  );
});

test("an explicit but unresolved target does not silently fall back to the Epic", () => {
  assert.equal(
    resolveMilestoneTitle("Continuous; all releases", "DE-001", milestones, epicMilestones),
    null,
  );
});

test("returns null when neither explicit target nor Epic mapping exists", () => {
  assert.equal(
    resolveMilestoneTitle(null, "DE-019", milestones, epicMilestones),
    null,
  );
});
