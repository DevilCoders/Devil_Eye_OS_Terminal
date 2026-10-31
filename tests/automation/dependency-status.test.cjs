"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  extractDependencyNumbers,
  chooseTaskStatus,
  reconcileStatusLabels,
} = require("../../.github/scripts/dependency-status.cjs");

test("extracts unique issue references from the Dependencies section", () => {
  const body = [
    "### Description",
    "See #999 for background.",
    "### Dependencies",
    "DE-001-02 (#26); DE-001-03 (#27); duplicate #26",
    "### Acceptance criteria",
    "Do not treat #888 as a dependency.",
  ].join("\n");

  assert.deepEqual(extractDependencyNumbers(body), [26, 27]);
});

test("returns an empty dependency list when the section is absent", () => {
  assert.deepEqual(extractDependencyNumbers("No dependency heading here: #42"), []);
});

test("marks a task blocked when any dependency remains open", () => {
  assert.equal(
    chooseTaskStatus({
      body: "### Dependencies\n#26; #27",
      currentLabels: ["type:task", "status:todo"],
      openIssueNumbers: new Set([27]),
    }),
    "status:blocked",
  );
});

test("marks a task ready when all dependencies are closed", () => {
  assert.equal(
    chooseTaskStatus({
      body: "### Dependencies\n#26; #27",
      currentLabels: ["type:task", "status:blocked"],
      openIssueNumbers: new Set(),
    }),
    "status:todo",
  );
});

test("preserves in-progress, review, and done states despite open dependencies", () => {
  for (const protectedStatus of [
    "status:in-progress",
    "status:review",
    "status:done",
  ]) {
    assert.equal(
      chooseTaskStatus({
        body: "### Dependencies\n#26",
        currentLabels: [protectedStatus],
        openIssueNumbers: new Set([26]),
      }),
      protectedStatus,
    );
  }
});

test("normalizes conflicting protected statuses using deterministic precedence", () => {
  assert.equal(
    chooseTaskStatus({
      body: "### Dependencies\n#26",
      currentLabels: ["status:in-progress", "status:review", "status:done"],
      openIssueNumbers: new Set([26]),
    }),
    "status:done",
  );
});

test("adds the desired status and removes every conflicting status label", () => {
  assert.deepEqual(
    reconcileStatusLabels(
      ["type:task", "status:todo", "status:blocked", "component:build"],
      "status:review",
    ),
    {
      add: ["status:review"],
      remove: ["status:todo", "status:blocked"],
    },
  );
});

test("does not write labels when the current status is already correct", () => {
  assert.deepEqual(
    reconcileStatusLabels(["type:task", "status:blocked"], "status:blocked"),
    { add: [], remove: [] },
  );
});

test("rejects a non-status target label", () => {
  assert.throws(
    () => reconcileStatusLabels(["status:todo"], "priority:P0"),
    TypeError,
  );
});
