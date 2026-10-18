import { evidenceLabel, relevantConflicts } from './memory-manager.js';

export function estimateTokens(text) { return Math.ceil((text || '').length / 3.7); }

export function packContext({ message, history, memory, evidence, vaultNotes = [], settings }) {
  const system = settings.systemPrompt || 'You are a helpful assistant.';
  const instructionCost = estimateTokens(system);
  const messageCost = estimateTokens(message);
  const usable = Math.max(0, Number(settings.contextLimit) - Number(settings.outputReserve) - Number(settings.safetyMargin));
  const budget = Math.max(0, usable - instructionCost - messageCost);
  const selected = [];
  let used = 0;
  const add = (kind, text, sourceTurnIds = [], score = 0, sourceNoteIds = []) => {
    const cost = estimateTokens(text);
    if (!text || used + cost > budget) return false;
    selected.push({ kind, text, sourceTurnIds, sourceNoteIds, cost, score }); used += cost; return true;
  };

  if (settings.memoryEnabled) {
    const stateGroups = [memory.currentGoal, memory.decisions, memory.constraints, memory.preferences, memory.activeTopics, memory.openQuestions];
    const stateItems = stateGroups.flat().filter(item => item?.text).map(item => ({ ...item, kind: 'state', score: evidenceLabel(item.text, message) === 'strong lexical match' ? 0.8 : evidenceLabel(item.text, message) === 'related candidate' ? 0.4 : 0.1 })).filter(item => item.score >= 0.35);
    for (const item of stateItems.sort((a, b) => b.score - a.score)) add('state', item.text, item.sourceTurnIds, item.score);
    for (const conflict of relevantConflicts(memory, message)) {
      const sourceIds = [...new Set(conflict.assertions.flatMap(assertion => assertion.sourceTurnIds || []))];
      const details = conflict.assertions.map(assertion =>
        `- ${assertion.content} (source turn${assertion.sourceTurnIds?.length === 1 ? '' : 's'}: ${(assertion.sourceTurnIds || []).map(id => id.slice(0, 8)).join(', ') || 'unknown'})`
      ).join('\n');
      add('conflict', `UNRESOLVED CONFLICT: These retained assertions may disagree. Do not choose one without evidence or clarification.\n${details}`, sourceIds, 1);
    }
    for (const item of evidence) {
      const label = evidenceLabel(item.turn.content, message);
      if (item.turn.content === message) continue;
      add(`source · ${label}`, `[${item.turn.role} turn ${item.turn.id.slice(0, 8)} · ${new Date(item.turn.timestamp).toLocaleString()}] ${item.turn.content}`, [item.turn.id], item.score);
    }
  }
  if (settings.vaultSearchEnabled) {
    for (const note of vaultNotes) {
      const text = `[Engram note: ${note.title} · ${note.id}]\n${note.snippet}`;
      add('vault source', text, [], note.score || 0, [note.id]);
    }
  }
  const recent = history.slice(-Number(settings.recentTurns));
  for (const turn of [...recent].reverse()) {
    if (turn.content === message) continue;
    add(`recent · ${turn.role}`, `${turn.role}: ${turn.content}`, [turn.id], 0.25);
  }
  const lines = [`COMPACT CONVERSATION STATE\n${selected.filter(x => x.kind === 'state').map(x => `- ${x.text}`).join('\n') || '- No relevant state variables found.'}`, `UNRESOLVED CONFLICTS\n${selected.filter(x => x.kind === 'conflict').map(x => x.text).join('\n') || '- No relevant unresolved conflicts selected.'}`, `CONVERSATION SOURCE TURNS\n${selected.filter(x => x.kind.startsWith('source')).map(x => `- ${x.text}`).join('\n') || '- No matching conversation source turns found.'}`, `ENGRAM VAULT SOURCES\n${selected.filter(x => x.kind.startsWith('vault')).map(x => `- ${x.text}`).join('\n') || '- No Engram vault notes selected.'}\nCite note IDs in square brackets when using these sources. Do not imply a note supports a claim unless its excerpt does.`, `RECENT CONTEXT\n${selected.filter(x => x.kind.startsWith('recent')).map(x => `- ${x.text}`).join('\n') || '- None.'}`];
  const assembledSystem = `${system}\n\n${lines.join('\n\n')}`;
  const items = [{ role: 'system', content: assembledSystem }, { role: 'user', content: message }];
  const counted = estimateTokens(assembledSystem) + messageCost;
  return {
    messages: items,
    audit: { contextLimit: Number(settings.contextLimit), outputReserve: Number(settings.outputReserve), safetyMargin: Number(settings.safetyMargin), usableInput: usable, estimatedInputTokens: counted, exactCount: false, remainingTokens: Math.max(0, usable - counted), selected, retrievalCandidates: evidence.length + vaultNotes.length, budgetStatus: counted <= usable ? 'within budget (estimated)' : 'over budget estimate' }
  };
}
