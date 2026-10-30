// Multi-Agent Relay Accounting & Project Filtering (#128, #129, #131, #132, #133)

export const DEFAULT_MANAGED_PROJECTS = [
  "TeslaHub",
  "koe",
  "sonori",
  "pallas-macos",
  "writings-projects"
];

/**
 * Filter requirement cards by managed project name or external_ref prefix.
 */
export function filterRequirementsByProject(requirements = [], project = null) {
  if (!project) return requirements;
  const target = String(project).trim().toLowerCase();
  return requirements.filter((req) => {
    const p = String(req.project || "").trim().toLowerCase();
    const ref = String(req.external_ref || req.externalRef || "").toLowerCase();
    return p === target || ref.includes(`/${target}#`) || ref.startsWith(`${target}:`);
  });
}

/**
 * Perform an atomic handoff step:
 * 1. Update card status (e.g. column: doing/done)
 * 2. Toggle completed checklist items
 * 3. Append progress/handoff comment
 */
export async function relayHandoff(client, {
  cardId,
  actor,
  column = null,
  toggledChecklistIds = [],
  comment = null,
} = {}) {
  const actions = [];

  if (column) {
    actions.push(client.updateRequirement(cardId, { column }));
  }

  for (const itemId of toggledChecklistIds) {
    actions.push(client.toggleChecklist(cardId, itemId, true));
  }

  if (comment) {
    const formattedComment = actor ? `[${actor}] ${comment}` : comment;
    actions.push(client.addComment(cardId, formattedComment));
  }

  return Promise.all(actions);
}

/**
 * Read the full accounting timeline for a requirement card:
 * Returns the card state, checklist state, and comment events newest-first or chronological.
 */
export async function readRelayAccounting(client, cardId) {
  const [card, eventsResult] = await Promise.all([
    client.getRequirement(cardId),
    client.getEvents({ requirementId: cardId, limit: 100 }),
  ]);

  const rawEvents = eventsResult.events || eventsResult || [];
  const comments = rawEvents
    .filter((e) => e.kind === "comment")
    .map((e) => ({
      id: e.id,
      text: e.new?.text || e.text,
      author: e.actor || e.user_id || e.node_id || "unknown",
      createdAt: e.created_at || e.timestamp,
    }));

  return {
    card: card.requirement || card,
    comments,
    events: rawEvents,
  };
}
