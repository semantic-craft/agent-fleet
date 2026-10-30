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

/**
 * Standard accounting and handoff rules for multi-agent desktop and CLI nodes (#131, #133).
 */
export const UNIFIED_ACCOUNTING_NODE_RULES = `# CommHub Multi-Agent Unified Accounting Rules (#128, #131, #133)
1. 接单时必须调用 requirements_get / requirements_events 读取前任执行上下文、Checklist 状态与进展留言。
2. 任务执行期间在各自桌面/Herdr 可见会话中执行，不静默降级为无头模式。
3. 交接、卡住或完成时，必须调用 requirements_comment 记录分支名、改动文件、验证情况及下一棒交接对象，并调用 requirements_checklist_toggle 勾选对应项。
4. 任务终结时调用 send_reply 回复 CommHub。
`;

/**
 * Push the unified accounting rules to a specific node or session via write_node_rules_file.
 */
export async function pushAccountingRulesToNode(client, {
  nodeId = null,
  alias = null,
  networkId = null,
  content = UNIFIED_ACCOUNTING_NODE_RULES,
} = {}) {
  return client.writeNodeRulesFile({ nodeId, alias, networkId, content });
}
