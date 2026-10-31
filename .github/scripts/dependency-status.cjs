"use strict";

const PROTECTED_STATUSES = [
  "status:done",
  "status:review",
  "status:in-progress",
];

/**
 * Extracts unique issue references from the Dependencies section only.
 *
 * @param {string} body
 * @returns {number[]}
 */
function extractDependencyNumbers(body) {
  const source = typeof body === "string" ? body : "";
  const section = source.match(
    /### Dependencies\s*([\s\S]*?)(?:\r?\n###|$)/i,
  );
  if (!section) return [];

  return [
    ...new Set(
      [...section[1].matchAll(/#(\d+)/g)].map((match) => Number(match[1])),
    ),
  ];
}

/**
 * Computes the one desired status for an open task.
 *
 * Protected workflow states are preserved even if a dependency is open.
 *
 * @param {{body: string, currentLabels: string[], openIssueNumbers: Set<number>}} input
 * @returns {"status:done"|"status:review"|"status:in-progress"|"status:blocked"|"status:todo"}
 */
function chooseTaskStatus({ body, currentLabels, openIssueNumbers }) {
  const labels = new Set(currentLabels);
  const protectedStatus = PROTECTED_STATUSES.find((status) =>
    labels.has(status),
  );
  if (protectedStatus) return protectedStatus;

  const blocked = extractDependencyNumbers(body).some((number) =>
    openIssueNumbers.has(number),
  );
  return blocked ? "status:blocked" : "status:todo";
}

/**
 * Returns the minimal label mutations needed to enforce one status label.
 *
 * @param {string[]} currentLabels
 * @param {string} desiredStatus
 * @returns {{add: string[], remove: string[]}}
 */
function reconcileStatusLabels(currentLabels, desiredStatus) {
  if (!desiredStatus.startsWith("status:")) {
    throw new TypeError("desiredStatus must be a status:* label");
  }

  const labels = new Set(currentLabels);
  return {
    add: labels.has(desiredStatus) ? [] : [desiredStatus],
    remove: [...labels].filter(
      (label) => label.startsWith("status:") && label !== desiredStatus,
    ),
  };
}

module.exports = {
  extractDependencyNumbers,
  chooseTaskStatus,
  reconcileStatusLabels,
};
