"use strict";

/**
 * Resolves an issue's milestone target to one canonical repository milestone.
 *
 * Descriptive targets such as "Continuous; release gate M9" resolve to M9.
 * "Continuous; all milestones" intentionally resolves to no single milestone.
 * An explicit target takes precedence over the Epic fallback.
 *
 * @param {string|null} rawTarget
 * @param {string|null} epicId
 * @param {string[]} milestoneTitles
 * @param {Record<string, string>} epicMilestones
 * @returns {string|null}
 */
function resolveMilestoneTitle(rawTarget, epicId, milestoneTitles, epicMilestones) {
  if (typeof rawTarget === "string" && rawTarget.trim() !== "") {
    const milestoneCode = rawTarget.match(/\bM(?:10|[0-9])\b/)?.[0];
    if (!milestoneCode) return null;

    return (
      milestoneTitles.find((title) => title.startsWith(milestoneCode + " ")) ||
      null
    );
  }

  return (epicId && epicMilestones[epicId]) || null;
}

module.exports = { resolveMilestoneTitle };
