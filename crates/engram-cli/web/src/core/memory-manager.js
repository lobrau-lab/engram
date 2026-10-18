import { emptyMemory } from './store.js';

const STOP = new Set('about above after again against all also and any are because been before being between both but can could did does doing down during each few for from further had has have having here how into its itself just more most nor not now off once only other our out over own same she should some such than that the their them then there these they this those through too under until very was were what when where which while who why will with would your you user assistant'.split(' '));
const CORRECTION = /\b(?:correction\b|actually,?\s+(?:my|i|that|we|it|the)\b|i\s+meant\b|i\s+mean\b|i\s+was\s+(?:wrong|mistaken)\b|that\s+was\s+wrong\b|replace\s+that\b|i\s+no\s+longer\b|now\s+i\s+prefer\b|instead\s+of\b)/i;
const words = text => (text.toLowerCase().match(/[\p{L}\p{N}_'-]{2,}/gu) || []).filter(w => !STOP.has(w));
const compact = text => text.replace(/\s+/g, ' ').trim();
const overlap = (a, b) => { const aw = new Set(words(a)); const bw = new Set(words(b)); if (!aw.size || !bw.size) return 0; let common = 0; for (const w of aw) if (bw.has(w)) common++; return common / Math.sqrt(aw.size * bw.size); };

export function recordAndUpdate(memory, turn) {
  const next = structuredClone(memory || emptyMemory());
  const content = compact(turn.content);
  const assertion = { id: crypto.randomUUID(), content, sourceTurnIds: [turn.id], timestamp: turn.timestamp, type: inferType(content), status: 'active_candidate', inferred: false };
  const earlier = next.assertions.filter(a => a.status !== 'superseded' && overlap(a.content, content) > 0.6);
  const explicitCorrection = CORRECTION.test(content);
  if (explicitCorrection && earlier.length) {
    for (const item of earlier) { item.status = 'superseded'; assertion.supersedes = [...(assertion.supersedes || []), item.id]; }
    next.events ??= [];
    next.events.push({ type: 'explicit_correction', assertionId: assertion.id, supersedes: assertion.supersedes, timestamp: turn.timestamp });
  } else {
    for (const item of earlier) {
      if (overlap(item.content, content) > 0.72 && !sameScope(item.content, content)) {
        item.status = 'unresolved_conflict'; assertion.status = 'unresolved_conflict';
        next.conflicts.push({ id: crypto.randomUUID(), assertionIds: [item.id, assertion.id], status: 'unresolved', detectedAt: turn.timestamp });
      }
    }
  }
  next.assertions.push(assertion);
  const lower = content.toLowerCase();
  if (/\b(goal|want to|i want|trying to|build|create|make|plan)\b/.test(lower)) next.currentGoal = upsert(next.currentGoal, content, turn.id);
  const topic = content.split(/[.!?]/)[0].slice(0, 100);
  if (words(topic).length >= 3) next.activeTopics = upsert(next.activeTopics, topic, turn.id, 8);
  if (/\b(must|need to|should|requirement|constraint|only|without)\b/.test(lower)) next.constraints = upsert(next.constraints, content.slice(0, 180), turn.id, 12);
  if (/\b(decided|decision|we will|i will|chosen|choose|going with)\b/.test(lower)) next.decisions = upsert(next.decisions, content.slice(0, 180), turn.id, 12);
  if (/\b(prefer|i like|i dislike|i hate|i love|always|never)\b/.test(lower)) next.preferences = upsert(next.preferences, content.slice(0, 180), turn.id, 12);
  if (turn.role === 'user' && /\?\s*$/.test(content)) next.openQuestions = upsert(next.openQuestions, content.slice(0, 180), turn.id, 10);
  const candidates = [...new Set(words(content).filter(w => w.length > 3))].slice(0, 12);
  next.entities = upsert(next.entities, candidates.join(', '), turn.id, 12);
  next.updatedAt = turn.timestamp;
  return next;
}

function inferType(text) {
  if (CORRECTION.test(text)) return 'explicit_correction';
  if (/\b(i prefer|i like|i dislike|i hate|i love)\b/i.test(text)) return 'preference';
  if (/\b(i decided|we decided|decision|we will)\b/i.test(text)) return 'decision';
  if (text.endsWith('?')) return 'question';
  return 'statement';
}
function sameScope(a, b) { return /\b(previously|used to|now|currently|today|this time|in this project|for this chat)\b/i.test(a + ' ' + b); }
function upsert(list, text, turnId, max = 8) {
  if (!text) return list;
  const existing = list.find(item => typeof item === 'object' && overlap(item.text, text) > 0.8);
  if (existing) { existing.sourceTurnIds = [...new Set([...existing.sourceTurnIds, turnId])]; existing.text = text; }
  else list.unshift({ id: crypto.randomUUID(), text, sourceTurnIds: [turnId] });
  return list.slice(0, max);
}

export function retrieveEvidence(history, query, memory, limit = 5) {
  const qWords = new Set(words(query));
  return history.map((turn, index) => {
    const lexical = overlap(query, turn.content);
    const topical = memory.activeTopics.reduce((sum, topic) => sum + overlap(topic.text, turn.content), 0);
    const recency = index / Math.max(1, history.length - 1);
    const exact = [...qWords].some(w => w.length > 4 && turn.content.toLowerCase().includes(w)) ? 0.08 : 0;
    const score = lexical * 0.72 + topical * 0.12 + recency * 0.08 + exact;
    return { turn, score, evidence: score >= 0.15 ? 'candidate' : 'weak' };
  }).filter(item => item.score > 0.04).sort((a, b) => b.score - a.score).slice(0, limit);
}

export function evidenceLabel(text, query) {
  const score = overlap(text, query);
  return score > 0.45 ? 'strong lexical match' : score > 0.16 ? 'related candidate' : 'weak candidate';
}

/** Return unresolved conflicts relevant to this query, preserving both assertions. */
export function relevantConflicts(memory, query) {
  const assertions = new Map((memory.assertions || []).map(item => [item.id, item]));
  return (memory.conflicts || []).filter(conflict => conflict.status === 'unresolved' &&
    (conflict.assertionIds || []).some(id => {
      const assertion = assertions.get(id);
      return assertion && overlap(assertion.content, query) > 0.16;
    })
  ).map(conflict => ({
    ...conflict,
    assertions: (conflict.assertionIds || []).map(id => assertions.get(id)).filter(Boolean)
  }));
}

export function evaluateRules({ message, memory, evidence, settings, vaultResult, vaultSearchError }) {
  const trace = [];
  const add = (rule, condition, result) => trace.push({ rule, condition, result });
  add('Memory enabled', 'settings.memoryEnabled', settings.memoryEnabled ? 'on: derive and retrieve state' : 'off: current turn only');
  const asksHistory = /\b(earlier|before|previous|what did i|what was|remember|we discussed|you said|i said|last time|that decision)\b/i.test(message);
  add('History needed?', 'message references prior turns or active topic', asksHistory ? 'yes: retrieve evidence' : 'search by relevance');
  add('Evidence available?', 'retrieval returned source turns', evidence.length ? `${evidence.length} candidate turn(s)` : 'none: historical claims should be qualified');
  add('Engram vault search?', 'settings.vaultSearchEnabled', !settings.vaultSearchEnabled ? 'off: conversation-only retrieval' : vaultSearchError ? `unavailable: ${vaultSearchError.message}` : !vaultResult?.indexed ? 'enabled: vault index is missing or stale' : `${vaultResult.hits?.length || 0} note candidate(s) retrieved`);
  const conflicts = relevantConflicts(memory, message);
  add('Relevant unresolved conflicts?', 'a retained assertion conflicts with this request', conflicts.length ? `${conflicts.length} conflict(s): preserve both assertions and qualify the answer` : 'none relevant to this request');
  add('Context budget', 'usable input tokens > 0', settings.contextLimit - settings.outputReserve - settings.safetyMargin > 0 ? 'pack ranked context' : 'invalid budget: do not send');
  return { asksHistory, trace };
}

export { words, overlap };
