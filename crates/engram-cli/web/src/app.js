import { getWorkspace, activeConversation, appendTurn, createConversation, setActiveConversation, updateActive, updateMemory, updateSettings, subscribe, clearWorkspace, removeAssertion, getSessionApiKey, setSessionApiKey } from './core/store.js';
import { recordAndUpdate, retrieveEvidence, evaluateRules } from './core/memory-manager.js';
import { filterRemoteHistory, packContext } from './core/context-packer.js';
import { listModels, setModelLoaded, streamChat } from './providers/openai-compatible.js';
import { searchEngramVault } from './providers/engram-vault.js';
import { applicationTools, executeApplicationAction, executeProjectAction } from './core/app-actions.js';
import { classifyEndpoint, canSendVaultContext, requestCategories, confirmDisclosure, supportsFunctionTools } from './core/privacy-policy.js';
import { toolCallRejection } from './core/permission-gate.js';
import { runMemoryBench } from './core/evaluation-bench.js';

const $ = selector => document.querySelector(selector);
const esc = value => String(value ?? '').replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]));
let currentView = 'chat'; let inspectorTab = 'assembly'; let sending = false; let aborter = null; let embeddedModel = null; let appDigests = []; let appResult = ''; let appsLoaded = false; let workshopProposals = []; let workshopResult = ''; let workshopLoaded = false; let takeoverStatus = null; let takeoverSession = null; let takeoverResult = ''; let takeoverTree = ''; let takeoverShot = ''; let takeoverBusy = false; let takeoverTimer = null; let takeoverRequest = 0; let takeoverMinutes = 15;

const accentOptions = {
  green: { label: 'Green', color: '#c8f169', image: 'engram-mark-green.png' },
  purple: { label: 'Purple', color: '#c7a3ff', image: 'engram-icon-purple.png' },
  pink: { label: 'Pink', color: '#f0a5bc', image: 'engram-icon-pink.png' },
  blue: { label: 'Blue', color: '#87bde8', image: 'engram-icon-blue.png' },
  fire: { label: 'Ember', color: '#ff9d4d', image: 'engram-icon-fire.png' },
  forest: { label: 'Forest', color: '#9acb83', image: 'engram-icon-forest.png' },
  ice: { label: 'Ice', color: '#a5d9ed', image: 'engram-icon-ice.png' }
};

function applyAccent(name = 'green') {
  const accent = accentOptions[name] || accentOptions.green;
  document.documentElement.style.setProperty('--green', accent.color);
  document.documentElement.style.setProperty('--green-2', accent.color);
}

function toast(message) { const node = $('#toast'); node.textContent = message; node.classList.add('show'); setTimeout(() => node.classList.remove('show'), 2800); }
function current() { return activeConversation(); }
function memoryCount() { return current().memory?.assertions?.length || 0; }
function shortDate(date) { return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' }).format(new Date(date)); }

function renderThreads() {
  const workspace = getWorkspace();
  $('#thread-list').innerHTML = workspace.conversations.slice(0, 10).map(convo => `<button class="thread-item ${convo.id === workspace.activeId ? 'selected' : ''}" data-conversation="${convo.id}"><span class="thread-glyph">◌</span><span class="thread-name">${esc(convo.title)}</span><span class="thread-date">${shortDate(convo.createdAt)}</span></button>`).join('') || '<div class="empty-threads">Your conversations will appear here.</div>';
  document.querySelectorAll('[data-conversation]').forEach(button => button.addEventListener('click', () => { setActiveConversation(button.dataset.conversation); renderAll(); }));
}

function renderMessages() {
  const convo = current(); const msgs = $('#messages');
  $('#welcome').classList.toggle('hidden', convo.turns.length > 0);
  $('#top-title').textContent = convo.title;
  msgs.innerHTML = convo.turns.map(turn => {
    const turnSources = turn.sourceTurnIds || []; const noteSources = turn.sourceNoteIds || [];
    const sourceCount = turnSources.length + noteSources.length;
    const sourceDetails = [...turnSources.map(id => `turn ${id.slice(0, 8)}`), ...noteSources.map(id => `<a href="/note?path=${encodeURIComponent(id)}">${esc(id)}</a>`)];
    return `<article class="message-row ${turn.role === 'assistant' ? 'assistant-row' : 'user-row'}"><div class="message-avatar ${turn.role}">${turn.role === 'assistant' ? '<img src="./assets/engram-mark.png" alt="" />' : 'T'}</div><div class="message-main"><div class="message-meta"><strong>${turn.role === 'assistant' ? 'Threadline' : 'You'}</strong><time>${new Date(turn.timestamp).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</time>${turn.role === 'assistant' && sourceCount ? `<span class="source-badge">⌘ ${sourceCount} sources</span>` : ''}</div><div class="message-text">${esc(turn.content).replace(/\n/g, '<br>')}</div>${turn.role === 'assistant' && sourceCount ? `<details class="message-sources"><summary>Sources used</summary><span>${sourceDetails.join(' · ')}</span></details>` : ''}</div></article>`;
  }).join('');
  msgs.scrollTop = msgs.scrollHeight;
}

function renderStatus() {
  const settings = getWorkspace().settings;
  $('#connection-label').textContent = settings.model ? `${settings.model} · ${settings.connectionStatus || 'not tested'}` : 'Connect a model';
  $('#connection-pill').classList.toggle('configured', Boolean(settings.model));
  $('#budget-label').textContent = `Context · ${Math.round(settings.contextLimit / 1024)}k`;
  $('#vault-toggle').classList.toggle('active', Boolean(settings.vaultSearchEnabled));
  $('#vault-toggle').setAttribute('aria-pressed', String(Boolean(settings.vaultSearchEnabled)));
  $('#memory-count').textContent = memoryCount(); $('#inspector-memory-count').textContent = memoryCount();
  updateTakeoverPill();
}

function variableSection(title, items, icon = '·') {
  if (!items?.length) return `<div class="variable-row empty"><span>${icon}</span><div><b>${title}</b><small>Nothing recorded yet</small></div></div>`;
  return items.slice(0, 4).map(item => `<div class="variable-row"><span>${icon}</span><div><b>${esc(item.text)}</b><small>source ${item.sourceTurnIds?.map(id => id.slice(0, 8)).join(', ') || 'not linked'}</small></div></div>`).join('');
}

function renderInspector() {
  const convo = current(); const memory = convo.memory; const last = convo.lastAssembly; const content = $('#inspector-content');
  $('#inspector-memory-count').textContent = memory.assertions.length;
  if (inspectorTab === 'assembly') {
    const audit = last?.audit;
    content.innerHTML = `<div class="budget-card"><div class="budget-card-head"><span>INPUT BUDGET</span><span class="estimate-pill">${audit?.exactCount ? 'EXACT' : 'ESTIMATE'}</span></div><div class="budget-total">${(audit?.estimatedInputTokens || 0).toLocaleString()} <small>/ ${(audit?.usableInput || Math.max(0, Number(getWorkspace().settings.contextLimit) - Number(getWorkspace().settings.outputReserve) - Number(getWorkspace().settings.safetyMargin))).toLocaleString()} tokens</small></div><div class="budget-bar"><span style="width:${Math.min(100, audit ? audit.estimatedInputTokens / Math.max(audit.usableInput, 1) * 100 : 0)}%"></span></div><div class="budget-legend"><span><i class="legend-state"></i>State</span><span><i class="legend-source"></i>Evidence</span><span><i class="legend-recent"></i>Recent</span></div></div>
      <div class="panel-section"><div class="section-title"><span>WHAT ENTERED THE PROMPT</span><span>${audit?.selected?.length || 0} items</span></div>${audit?.selected?.length ? audit.selected.map(item => `<div class="assembly-item"><span class="assembly-type ${item.kind.startsWith('source') || item.kind.startsWith('vault') ? 'source' : item.kind.startsWith('recent') ? 'recent' : 'state'}">${esc(item.kind)}</span><p>${esc(item.text.replace(/^\[.*?\] /, ''))}</p><small>~${item.cost} tokens${item.sourceNoteIds?.length ? ` · <a href="/note?path=${encodeURIComponent(item.sourceNoteIds[0])}">open note</a>` : ''}</small></div>`).join('') : '<div class="hint-empty">Send a message to preview compact state, conversation evidence, and Engram vault notes selected for its prompt.</div>'}</div>
      <div class="panel-section compact"><div class="section-title">ACTIVE DECISIONS</div><div class="rule-mini"><span class="rule-check">✓</span><span>Current request is always preserved</span></div><div class="rule-mini"><span class="rule-check">✓</span><span>Historical details need source evidence</span></div><div class="rule-mini"><span class="rule-check">✓</span><span>Unresolved assertions remain visible</span></div></div>`;
  } else if (inspectorTab === 'memory') {
    const groups = [['Current goals', memory.currentGoal, '◎'], ['Active topics', memory.activeTopics, '◌'], ['Decisions', memory.decisions, '↳'], ['Constraints', memory.constraints, '⊣'], ['Preferences', memory.preferences, '♡'], ['Open questions', memory.openQuestions, '?']];
    content.innerHTML = `<div class="memory-intro"><span class="memory-chip">LOCAL · THIS CONVERSATION</span><p>Derived context variables are an index into the transcript. The original turns remain the evidence.</p></div>${groups.map(([title, items, icon]) => `<div class="panel-section"><div class="section-title">${title.toUpperCase()}</div>${variableSection(title, items, icon)}</div>`).join('')}<div class="panel-section"><div class="section-title">ASSERTION LOG <span>${memory.assertions.length}</span></div>${memory.assertions.slice(-8).reverse().map(assertion => `<div class="assertion-card"><div class="assertion-head"><span class="assertion-type">${esc(assertion.type)}</span><span class="assertion-status ${assertion.status}">${esc(assertion.status.replace('_', ' '))}</span><button class="forget-assertion" data-forget="${assertion.id}" title="Remove from derived memory">×</button></div><p>${esc(assertion.content)}</p><small>Source · ${assertion.sourceTurnIds.map(id => id.slice(0, 8)).join(', ')}</small></div>`).join('') || '<div class="hint-empty">Assertions appear as new user turns are recorded.</div>'}</div>${memory.conflicts.length ? `<div class="panel-section"><div class="section-title">CONFLICTS TO REVIEW</div>${memory.conflicts.map(conflict => `<div class="conflict-card"><span>◈ UNRESOLVED</span><p>Two related statements are retained with their source turns. Review their scope and timing before choosing a current value.</p></div>`).join('')}</div>` : ''}`;
    content.querySelectorAll('[data-forget]').forEach(button => button.addEventListener('click', () => { removeAssertion(button.dataset.forget); renderAll(); toast('Removed from the derived assertion list. Source turn remains in history.'); }));
  } else {
    const trace = last?.trace || [
      { rule: 'Memory enabled', condition: 'settings.memoryEnabled', result: getWorkspace().settings.memoryEnabled ? 'on: derive and retrieve state' : 'off: current turn only' },
      { rule: 'History needed?', condition: 'message references prior turns or active topic', result: 'evaluated on the next request' },
      { rule: 'Evidence available?', condition: 'retrieval returned source turns', result: 'waiting for a request' },
      { rule: 'Unresolved conflict?', condition: 'matching assertions conflict', result: memory.conflicts.some(c => c.status === 'unresolved') ? 'preserve both; clarify when relevant' : 'no active conflict detected' },
      { rule: 'Context budget', condition: 'usable input tokens > 0', result: 'ranked prompt packing' }
    ];
    content.innerHTML = `<div class="rules-intro"><div class="rules-symbol">⎇</div><div><strong>Readable rules. Replaceable policy.</strong><p>Each conditional is logged as a decision result. The rule engine is separate from providers, storage, and the UI.</p></div></div><div class="decision-flow">${trace.map((item, i) => `<div class="decision-node"><span class="node-number">${String(i + 1).padStart(2, '0')}</span><div><b>${esc(item.rule)}</b><small>IF ${esc(item.condition)}</small><p>${esc(item.result)}</p></div><span class="node-check">✓</span></div>`).join('')}</div><div class="panel-section"><div class="section-title">CURRENT POLICY</div><div class="policy-row"><span>Memory strategy</span><b>${esc(getWorkspace().settings.strategy)}</b></div><div class="policy-row"><span>Cross-chat recall</span><b>Off</b></div><div class="policy-row"><span>Source records</span><b>Immutable</b></div><div class="policy-row"><span>Token counts</span><b>Estimated</b></div></div>`;
  }
}

function renderAll() { renderThreads(); renderMessages(); renderStatus(); renderInspector(); if (currentView !== 'chat') renderWorkspaceView(); }

function switchView(view) {
  currentView = view;
  document.querySelectorAll('.nav-item').forEach(item => item.classList.toggle('active', item.dataset.view === view));
  $('#chat-view').classList.toggle('hidden', view !== 'chat');
  $('#workspace-view').classList.toggle('hidden', view === 'chat');
  const labels = { chat: current().title, memory: 'Memory ledger', bench: 'Memory bench', rules: 'Decision rules', apps: 'Application digests', takeover: 'Takeover', workshop: 'Workshop', settings: 'Connections & settings' };
  $('#top-title').textContent = labels[view]; if (view !== 'chat') renderWorkspaceView();
  if (view === 'takeover') loadTakeover(true);
}

function renderWorkspaceView() {
  const host = $('#workspace-view'); const memory = current().memory;
  if (currentView === 'bench') {
    const report = runMemoryBench();
    host.innerHTML = `<div class="page-heading"><div class="eyebrow">OFFLINE REPLAY</div><h1>Memory bench</h1><p>Synthetic conversations only. This measures retrieval and packing behavior; it does not prove general memory accuracy.</p><div class="ledger-grid"><section class="ledger-card"><h2>Baseline results</h2><p>Evidence recall: ${(report.evidenceRecall * 100).toFixed(0)}%</p><p>Irrelevant selection rate: ${(report.irrelevantSelectionRate * 100).toFixed(0)}%</p><p>Budget compliance: ${(report.budgetCompliance * 100).toFixed(0)}%</p><p>Conflict cases: ${report.conflictCases.join(', ') || 'none detected'}</p></section>${report.results.map(result => `<section class="ledger-card"><div class="ledger-head"><span>${result.expectedFound ? '✓' : '!'}</span><h2>${result.id}</h2></div><p><b>Query:</b> ${esc(result.query)}</p><p><b>Expected evidence:</b> ${result.expectedEvidence.join(', ') || 'none'}</p><p><b>Selected:</b> ${result.selectedEvidence.join(', ') || 'none'}</p><p><b>Excluded-but-selected:</b> ${result.irrelevantSelected.join(', ') || 'none'}</p><p><b>Budget:</b> ~${result.estimatedInputTokens} / ${result.usableInput} estimated tokens</p></section>`).join('')}</div></div>`;
  } else if (currentView === 'memory') {
    host.innerHTML = `<div class="page-heading"><div class="eyebrow">CONVERSATION STATE</div><h1>Memory ledger</h1><p>A compact set of variables that points back to the original conversation. Nothing here replaces the transcript.</p></div><div class="ledger-grid">${[['Current goal', memory.currentGoal, '01'], ['Active topics', memory.activeTopics, '02'], ['Decisions', memory.decisions, '03'], ['Constraints', memory.constraints, '04'], ['Preferences', memory.preferences, '05'], ['Open questions', memory.openQuestions, '06']].map(([title, items, num]) => `<section class="ledger-card"><div class="ledger-head"><span>${num}</span><h2>${title}</h2><small>${items.length} variables</small></div>${items.length ? items.map(item => `<div class="ledger-item"><p>${esc(item.text)}</p><div><span class="direct-label">DIRECTLY DERIVED</span><span>Source ${item.sourceTurnIds?.map(id => id.slice(0, 8)).join(', ')}</span></div></div>`).join('') : '<div class="ledger-empty">New user messages can add a source-linked variable here.</div>'}</section>`).join('')}</div><div class="transcript-strip"><div><span class="eyebrow">SOURCE OF RECORD</span><h2>Conversation transcript</h2><p>${current().turns.length} immutable turns in this conversation.</p></div><button class="secondary-button" id="back-to-chat">Return to chat ↗</button></div>`;
  } else if (currentView === 'apps') {
    renderAppsView(host);
  } else if (currentView === 'takeover') {
    renderTakeoverView(host);
  } else if (currentView === 'workshop') {
    renderWorkshopView(host);
  } else if (currentView === 'rules') {
    host.innerHTML = `<div class="page-heading"><div class="eyebrow">POLICY ENGINE</div><h1>Decision rules</h1><p>The “growing if statement” is an inspectable set of small policies. Each module can be swapped without rewriting the chat flow.</p></div><div class="rule-catalog">${[['01','Memory gate','IF memory is disabled, skip extraction and retrieval.','Memory policy'],['02','Evidence gate','IF a response recalls the conversation, retrieve and attach source turns.','Evidence policy'],['03','Correction handling','IF a correction is explicit, supersede the derived assertion and retain both source turns.','Conflict policy'],['04','Ambiguity handling','IF incompatible statements lack a clear correction, preserve both and surface uncertainty.','Conflict policy'],['05','Budget gate','IF candidate context exceeds the usable budget, rank and omit optional items first.','Packing policy'],['06','Provider fallback','IF the endpoint fails, preserve the draft and show a retryable connection error.','Provider policy']].map(([n,title,rule,module]) => `<article class="rule-card"><span class="rule-index">${n}</span><div><span class="module-label">${module}</span><h2>${title}</h2><p>${rule}</p></div><span class="rule-enabled">ENABLED <i></i></span></article>`).join('')}</div><div class="rule-note"><span>i</span><p>These starter rules are deterministic and transparent. Language extraction is a lightweight heuristic in this prototype; it is not a truth detector.</p></div>`;
  } else {
    const settings = getWorkspace().settings;
    host.innerHTML = `<div class="page-heading"><div class="eyebrow">PROVIDER & POLICIES</div><h1>Connections & settings</h1><p>Connect a compatible inference endpoint and tune how each prompt is assembled. Provider API keys stay in memory for this page session and are not saved with workspace settings.</p></div><form id="settings-form" class="settings-layout"><section class="settings-card wide-card"><div class="settings-card-heading"><span class="settings-icon">◈</span><div><h2>Threadline appearance &amp; accent</h2><p>Choose the app accent and matching icon treatment</p></div><span class="module-label">APPEARANCE</span></div><p class="brand-appearance-copy">The chosen accent is saved with this browser’s settings. Threadline is the chat and agent workspace; Engram remains a separate local project.</p><div class="accent-picker" role="group" aria-label="App accent and icon"><button class="accent-option" type="button" data-accent="green" aria-pressed="${(settings.accent || 'green') === 'green'}"><img src="./assets/engram-mark-green.png" alt="" /><span>Green</span></button><button class="accent-option" type="button" data-accent="purple" aria-pressed="${settings.accent === 'purple'}"><img src="./assets/engram-icon-purple.png" alt="" /><span>Purple</span></button><button class="accent-option" type="button" data-accent="pink" aria-pressed="${settings.accent === 'pink'}"><img src="./assets/engram-icon-pink.png" alt="" /><span>Pink</span></button><button class="accent-option" type="button" data-accent="blue" aria-pressed="${settings.accent === 'blue'}"><img src="./assets/engram-icon-blue.png" alt="" /><span>Blue</span></button><button class="accent-option" type="button" data-accent="fire" aria-pressed="${settings.accent === 'fire'}"><img src="./assets/engram-icon-fire.png" alt="" /><span>Ember</span></button><button class="accent-option" type="button" data-accent="forest" aria-pressed="${settings.accent === 'forest'}"><img src="./assets/engram-icon-forest.png" alt="" /><span>Forest</span></button><button class="accent-option" type="button" data-accent="ice" aria-pressed="${settings.accent === 'ice'}"><img src="./assets/engram-icon-ice.png" alt="" /><span>Ice</span></button></div></section><section class="settings-card"><div class="settings-card-heading"><span class="settings-icon">↔</span><div><h2>Inference server</h2><p>OpenAI-compatible chat completions API</p></div><span class="module-label">PROVIDER MODULE</span></div><label class="field-label" for="base-url">API base URL</label><input id="base-url" name="baseUrl" type="url" value="${esc(settings.baseUrl)}" placeholder="http://localhost:8000/v1" required /><div class="field-row"><div><label class="field-label" for="model-id">Model ID</label><input id="model-id" name="model" value="${esc(settings.model)}" placeholder="Select or type a model ID" /></div><button type="button" class="secondary-button discover-button" id="discover-models">Discover models</button></div><label class="field-label" for="api-key">API key <span>optional for local servers</span></label><input id="api-key" name="apiKey" type="password" value="${esc(settings.apiKey)}" autocomplete="off" placeholder="Re-enter for each page session" /><div class="settings-actions"><button class="primary-button" type="submit">Save connection</button><button class="secondary-button" id="test-connection" type="button">Test connection</button><span id="connection-feedback" role="status"></span></div><div class="endpoint-note"><span class="status-light"></span><span>Chat endpoint: <code id="endpoint-preview">${esc(settings.baseUrl.replace(/\/$/, ''))}/chat/completions</code></span></div></section><section class="settings-card" id="embedded-model-card"><div class="settings-card-heading"><span class="settings-icon warm">◉</span><div><h2>Built-in on-device model</h2><p>Runs inside Engram with no model server</p></div><span class="module-label">EMBEDDED MODULE</span></div><div class="omlx-status-row"><span class="status-light" id="embedded-status-light"></span><span id="embedded-status-text" role="status">Checking the built-in model…</span></div><div class="settings-actions"><button class="primary-button" type="button" id="use-embedded">Use built-in model</button><button class="secondary-button" type="button" id="refresh-embedded">Refresh</button></div><p class="local-service-note">Apple Intelligence runs the model inside this process on macOS 26 or later. Nothing is sent to a model server or the network. Conversation context stays on this Mac.</p></section><section class="settings-card wide-card" id="omlx-manager-card"><div class="settings-card-heading"><span class="settings-icon warm">◉</span><div><h2>Local oMLX service</h2><p>Manage the on-device inference server from Threadline</p></div><span class="module-label">LOCAL SERVICE MODULE</span></div><p class="brand-appearance-copy">Controls only the installed oMLX service on this Mac. Threadline runs fixed <code>omlx start</code>, <code>stop</code>, or <code>restart</code> actions without a shell. It does not install software or download models.</p><div class="omlx-status-row"><span class="status-light" id="omlx-status-light"></span><span id="omlx-status-text" role="status">Checking local service…</span></div><div class="settings-actions omlx-actions"><button class="primary-button" type="button" data-omlx-action="start">Start oMLX</button><button class="secondary-button" type="button" data-omlx-action="restart">Restart</button><button class="secondary-button" type="button" data-omlx-action="stop">Stop</button><button class="text-button" type="button" id="refresh-omlx">Refresh status</button></div><div class="settings-actions omlx-actions"><button class="secondary-button" type="button" id="load-model">Load selected model</button><button class="secondary-button" type="button" id="unload-model">Unload selected model</button><span id="model-load-feedback" role="status"></span></div><p class="local-service-note">Loading warms the selected local model into memory for immediate requests. oMLX’s idle policy may unload it later; use oMLX model settings to pin it permanently. Threadline leaves model residency and cache policy to oMLX. This manager is available only for the local oMLX service on port 8000.</p></section>
      <section class="settings-card"><div class="settings-card-heading"><span class="settings-icon warm">⌘</span><div><h2>Context algorithm</h2><p>Budget, recency, and evidence packing</p></div><span class="module-label">PACKER MODULE</span></div><div class="field-row setting-numbers"><div><label class="field-label" for="context-limit">Model context limit</label><input id="context-limit" name="contextLimit" type="number" min="1024" step="512" value="${settings.contextLimit}" /></div><div><label class="field-label" for="output-reserve">Output reserve</label><input id="output-reserve" name="outputReserve" type="number" min="128" step="1" value="${settings.outputReserve}" /></div><div><label class="field-label" for="safety-margin">Safety margin</label><input id="safety-margin" name="safetyMargin" type="number" min="0" step="100" value="${settings.safetyMargin}" /></div></div><label class="field-label" for="recent-turns">Recent turns to retain <span id="recent-value">${settings.recentTurns}</span></label><input id="recent-turns" name="recentTurns" type="range" min="2" max="16" value="${settings.recentTurns}" /><label class="field-label" for="strategy">Memory strategy</label><select id="strategy" name="strategy"><option value="hybrid" ${settings.strategy === 'hybrid' ? 'selected' : ''}>Structured state + retrieved turns</option><option value="recent" ${settings.strategy === 'recent' ? 'selected' : ''}>Recent turns only</option><option value="state" ${settings.strategy === 'state' ? 'selected' : ''}>Structured state + recent turns</option></select><label class="check-row"><input type="checkbox" name="memoryEnabled" ${settings.memoryEnabled ? 'checked' : ''} /><span><b>Use conversation memory</b><small>Derive local variables and retrieve relevant turns.</small></span><i></i></label><label class="check-row"><input type="checkbox" name="showEvidence" ${settings.showEvidence ? 'checked' : ''} /><span><b>Show source references</b><small>Display turn IDs when history evidence was packed.</small></span><i></i></label></section>
      <section class="settings-card wide-card"><div class="settings-card-heading"><span class="settings-icon blue-icon">✳</span><div><h2>Assistant operating instructions</h2><p>Baseline prompt used for every request</p></div><span class="module-label">SYSTEM MODULE</span></div><textarea id="system-prompt" name="systemPrompt" rows="4">${esc(settings.systemPrompt)}</textarea><div class="settings-actions"><button class="primary-button" type="submit">Save all settings</button><button class="text-button" type="button" id="reset-data">Delete all local data</button></div></section><section class="settings-card wide-card"><div class="settings-card-heading"><span class="settings-icon blue-icon">⌘</span><div><h2>Threadline project tools</h2><p>Local file access and fixed offline checks</p></div><span class="module-label">CAPABILITY MODULE</span></div><p class="brand-appearance-copy" id="project-access-status">Checking whether a project folder was granted…</p><div class="project-tool-list"><span>Browse and read approved source files</span><span>Create directories and write files after review</span><span>Run git status and allowlisted Cargo checks (Cargo forced offline)</span></div><p class="brand-appearance-copy">Hidden credentials, the <code>.engram</code> vault data, build output, and dependency folders are excluded. Project source stays on device: project tools are withheld from remote models and require a local inference endpoint.</p></section></form>`;
    fetch('/api/v1/project/status').then(response => response.json()).then(status => {
      const label = $('#project-access-status');
      if (status.enabled) label.textContent = `Project tools enabled · ${status.root} · list, read, write, and fixed offline checks. Each action requires approval.`;
      else label.textContent = 'Project tools are off. Launch the local server with --project-root <directory> to grant a project folder.';
    }).catch(() => { $('#project-access-status').textContent = 'Project access status could not be checked.'; });
    refreshOmlxStatus();
    refreshEmbeddedStatus();
    $('#refresh-embedded').addEventListener('click', refreshEmbeddedStatus);
    $('#use-embedded').addEventListener('click', useEmbeddedModel);
    $('#refresh-omlx').addEventListener('click', refreshOmlxStatus);
    document.querySelectorAll('[data-omlx-action]').forEach(button => button.addEventListener('click', () => manageOmlx(button.dataset.omlxAction)));
    $('#load-model').addEventListener('click', () => manageModelResidency(true));
    $('#unload-model').addEventListener('click', () => manageModelResidency(false));
    document.querySelectorAll('[data-accent]').forEach(button => button.addEventListener('click', () => {
      const accent = button.dataset.accent;
      updateSettings({ accent });
      applyAccent(accent);
      renderWorkspaceView();
      toast(`${accentOptions[accent].label} accent saved.`);
    }));
    $('#base-url').addEventListener('input', e => { $('#endpoint-preview').textContent = `${e.target.value.replace(/\/$/, '')}/chat/completions`; });
    $('#recent-turns').addEventListener('input', e => { $('#recent-value').textContent = e.target.value; });
    $('#settings-form').addEventListener('submit', e => { e.preventDefault(); const form = new FormData(e.currentTarget); setSessionApiKey(form.get('apiKey')); updateSettings({ baseUrl: form.get('baseUrl').trim().replace(/\/$/, ''), model: form.get('model').trim(), contextLimit: Number(form.get('contextLimit')), outputReserve: Number(form.get('outputReserve')), safetyMargin: Number(form.get('safetyMargin')), recentTurns: Number(form.get('recentTurns')), strategy: form.get('strategy'), memoryEnabled: form.has('memoryEnabled'), showEvidence: form.has('showEvidence'), systemPrompt: form.get('systemPrompt') }); renderAll(); $('#connection-feedback').textContent = 'Settings saved for this page session; reload requires re-entering the key.'; toast('Settings saved locally. API key is session-only.'); });
    $('#discover-models').addEventListener('click', discoverModels);
    $('#test-connection').addEventListener('click', async () => { const feedback = $('#connection-feedback'); feedback.textContent = 'Checking endpoint…'; try { const baseUrl = $('#base-url').value.trim().replace(/\/$/, ''); const target = classifyEndpoint(baseUrl); if (!target.allowed) throw new Error(target.reason); const apiKey = $('#api-key').value; if (target.kind === 'remote' && !(await confirmDisclosure({ endpoint: target.origin, apiKey, discovery: true }))) return; const models = await listModels({ baseUrl, apiKey }); updateSettings({ connectionStatus: 'reachable', connectionTestedAt: new Date().toISOString() }); feedback.textContent = `Connected · ${models.length} model(s) found`; toast('Inference server responded.'); } catch (error) { updateSettings({ connectionStatus: 'unreachable', connectionTestedAt: new Date().toISOString() }); feedback.textContent = error.message; } });
    $('#reset-data').addEventListener('click', () => { if (confirm('Delete all conversations, memory, and settings stored by this prototype in this browser?')) { clearWorkspace(); location.reload(); } });
  }
  $('#back-to-chat')?.addEventListener('click', () => switchView('chat'));
}

async function refreshEmbeddedStatus() {
  const label = $('#embedded-status-text'); const light = $('#embedded-status-light'); const button = $('#use-embedded');
  if (!label || !light || !button) return;
  label.textContent = 'Checking the built-in model…'; light.classList.remove('is-on', 'is-error'); button.disabled = true;
  try {
    const response = await fetch('/api/v1/inference/embedded');
    const status = await response.json();
    embeddedModel = status.available ? status.model : null;
    label.textContent = status.available ? `Ready · ${status.model}` : (status.reason || 'The built-in model is unavailable on this Mac.');
    light.classList.toggle('is-on', Boolean(status.available));
    light.classList.toggle('is-error', !status.available);
    button.disabled = !status.available;
  } catch {
    embeddedModel = null;
    label.textContent = 'Could not read the built-in model status.'; light.classList.add('is-error'); button.disabled = true;
  }
}

function useEmbeddedModel() {
  if (!embeddedModel) { toast('The built-in model is not available on this Mac.'); return; }
  updateSettings({ baseUrl: `${location.origin}/v1`, model: embeddedModel, connectionStatus: 'on-device', connectionTestedAt: new Date().toISOString() });
  renderAll();
  toast('Threadline is using the built-in on-device model.');
}

async function configureEmbeddedDefault() {
  try {
    const response = await fetch('/api/v1/inference/embedded');
    const status = await response.json();
    const settings = getWorkspace().settings;
    const untouched = !settings.model && (!settings.baseUrl || settings.baseUrl === 'http://localhost:8000/v1');
    if (status.available && untouched) {
      updateSettings({ baseUrl: `${location.origin}/v1`, model: status.model, connectionStatus: 'on-device', connectionTestedAt: new Date().toISOString() });
      renderAll();
      toast('Built-in on-device model is ready.');
    }
  } catch {}
}

async function inspectApplicationForTool(path) {
  const response = await fetch('/api/v1/apps/inspect', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path }) });
  const data = await response.json();
  if (!response.ok) throw new Error(data.error?.message || 'Inspection failed.');
  appsLoaded = false;
  await loadApps();
  return data;
}

async function listApplicationsForTool() {
  const response = await fetch('/api/v1/apps');
  const data = await response.json();
  if (!response.ok) throw new Error(data.error?.message || 'Could not load application digests.');
  return data.apps || [];
}

async function runApplicationForTool(id, action) {
  const response = await fetch('/api/v1/apps/run', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id, action }) });
  const data = await response.json();
  if (!response.ok) throw new Error(data.error?.message || 'The application could not run.');
  return data;
}

function renderWorkshopView(host) {
  const proposals = workshopProposals.length ? workshopProposals.map(proposal => {
    const when = proposal.created ? new Date(proposal.created * 1000).toLocaleString() : 'unknown time';
    return `<section class="ledger-card"><div class="ledger-head"><span>✦</span><h2>${esc(proposal.title)}</h2><small>${esc(when)}</small></div><details><summary>Read the proposal</summary><pre class="app-result">${esc(proposal.content)}</pre></details></section>`;
  }).join('') : '<section class="ledger-card"><div class="ledger-empty">No workshop proposals yet. Draft one to see a reviewable plan for changing Threadline itself.</div></section>';

  host.innerHTML = `<div class="page-heading"><div class="eyebrow">GATED SELF-IMPROVEMENT</div><h1>Workshop</h1><p>Draft a change to Threadline’s own source. The built-in model proposes; nothing is applied or executed. A human reviews the diff, runs the tests, and decides.</p></div><form id="workshop-form" class="settings-card wide-card"><div class="settings-card-heading"><span class="settings-icon warm">✦</span><div><h2>Draft a proposal</h2><p>Choose up to three source files for context</p></div><span class="module-label">REVIEW REQUIRED</span></div><label class="field-label" for="workshop-goal">Goal</label><textarea id="workshop-goal" rows="3" required placeholder="Add a compact capability card to the Applications view…"></textarea><label class="field-label" for="workshop-files">Source files (relative to the project root, comma-separated)</label><input id="workshop-files" value="crates/engram-core/src/capability.rs" autocomplete="off" required /><div class="settings-actions"><button class="primary-button" type="submit">Draft proposal</button><span id="workshop-feedback" role="status">Proposals are never applied automatically.</span></div><p class="local-service-note">The workshop needs the server started with <code>--project-root .</code>. It reads only the files you name, refuses hidden or sensitive paths, and stores the proposal under <code>.engram/workshop/</code>. It does not weaken tests or consent checks.</p></form><div class="ledger-grid">${proposals}</div>${workshopResult ? `<pre class="app-result" aria-live="polite">${esc(workshopResult)}</pre>` : ''}`;

  $('#workshop-form').addEventListener('submit', event => {
    event.preventDefault();
    draftWorkshopProposal($('#workshop-goal').value.trim(), $('#workshop-files').value);
  });
  if (!workshopLoaded) { workshopLoaded = true; loadWorkshop(); }
}

async function loadWorkshop() {
  try {
    const response = await fetch('/api/v1/workshop');
    const data = await response.json();
    if (!response.ok) throw new Error(data.error?.message || 'Could not load workshop proposals.');
    workshopProposals = data.proposals || [];
    renderWorkspaceView();
  } catch (error) {
    workshopLoaded = false;
    workshopResult = `Could not load workshop proposals: ${error.message}`;
    renderWorkspaceView();
  }
}

async function draftWorkshopProposal(goal, filesInput) {
  const files = filesInput.split(',').map(item => item.trim()).filter(Boolean);
  if (!goal || !files.length) return;
  workshopResult = `Drafting a proposal for “${goal}”…\nThis runs the built-in model over the selected source files. Nothing is applied.`;
  renderWorkspaceView();
  try {
    const response = await fetch('/api/v1/workshop/propose', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ goal, files }) });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error?.message || 'The workshop could not draft a proposal.');
    workshopResult = data.proposal;
    await loadWorkshop();
    toast('Workshop proposal saved for review. Nothing was applied.');
  } catch (error) {
    workshopResult = `Workshop failed: ${error.message}`;
    renderWorkspaceView();
  }
}

function renderAppsView(host) {
  const cards = appDigests.length ? appDigests.map(app => {
    const actionLabels = { help: 'Run --help', version: 'Run --version', open: 'Open application' };
    const actions = app.actions.map(action => `<button class="secondary-button" type="button" data-app-run="${esc(app.id)}" data-app-action="${esc(action)}">${esc(actionLabels[action] || action)}</button>`).join('');
    const details = [
      `Path: ${app.path}`,
      app.version ? `Version: ${app.version}` : null,
      app.bundle_id ? `Bundle: ${app.bundle_id}` : null,
      app.usage ? `Usage: ${app.usage}` : null,
      app.flags.length ? `Flags: ${app.flags.slice(0, 12).join(' ')}${app.flags.length > 12 ? ` … +${app.flags.length - 12}` : ''}` : null
    ].filter(Boolean).map(line => `<p>${esc(line)}</p>`).join('');
    return `<section class="ledger-card"><div class="ledger-head"><span>${app.kind === 'app' ? '▣' : '›_'}</span><h2>${esc(app.name)}</h2><small>${esc(app.kind)} · ${esc(app.risk)} risk</small></div>${details}<div class="settings-actions">${actions}</div></section>`;
  }).join('') : '<section class="ledger-card"><div class="ledger-empty">No applications digested yet. Inspect a local program to record its version, usage, and flags.</div></section>';

  host.innerHTML = `<div class="page-heading"><div class="eyebrow">CAPABILITY DIGEST</div><h1>Applications</h1><p>Threadline can read a local program’s help and version output, record its capabilities, and later run only the reviewed actions. It runs the exact path without a shell and refuses if the program changed.</p></div><form id="inspect-app-form" class="settings-card wide-card"><div class="settings-card-heading"><span class="settings-icon warm">▣</span><div><h2>Teach Threadline an application</h2><p>Absolute path to an executable or a macOS .app bundle</p></div><span class="module-label">DIGEST MODULE</span></div><label class="field-label" for="app-path">Application path</label><input id="app-path" name="path" placeholder="/usr/bin/git or /Applications/Safari.app" autocomplete="off" required /><div class="settings-actions"><button class="primary-button" type="submit">Inspect application</button><span id="app-inspect-feedback" role="status"></span></div><p class="local-service-note">Inspection runs the program with only <code>--help</code> and <code>--version</code> (or reads the bundle metadata for an app). Each later action asks for confirmation. The digest is stored under <code>.engram/capabilities/</code> in this vault.</p></form><div class="ledger-grid">${cards}</div>${appResult ? `<pre class="app-result" aria-live="polite">${esc(appResult)}</pre>` : ''}`;

  $('#inspect-app-form').addEventListener('submit', event => { event.preventDefault(); inspectApplication($('#app-path').value.trim()); });
  host.querySelectorAll('[data-app-run]').forEach(button => button.addEventListener('click', () => runApplication(button.dataset.appRun, button.dataset.appAction)));
  if (!appsLoaded) { appsLoaded = true; loadApps(); }
}

async function loadApps() {
  try {
    const response = await fetch('/api/v1/apps');
    const data = await response.json();
    if (!response.ok) throw new Error(data.error?.message || 'Could not load application digests.');
    appDigests = data.apps || [];
    renderWorkspaceView();
  } catch (error) {
    appResult = `Could not load application digests: ${error.message}`;
    renderWorkspaceView();
  }
}

async function inspectApplication(path) {
  if (!path) return;
  appResult = `Inspecting ${path}…\nThis runs --help and --version with a timeout.`;
  renderWorkspaceView();
  try {
    const response = await fetch('/api/v1/apps/inspect', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path }) });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error?.message || 'Inspection failed.');
    appResult = `Digested ${data.name} (${data.kind}).\nPath: ${data.path}\nVersion: ${data.version || 'not reported'}\nActions: ${data.actions.join(', ')}\nFlags found: ${data.flags.length}\n\n${data.help_excerpt || ''}`;
    await loadApps();
    toast(`${data.name} was added to Threadline’s capability digest.`);
  } catch (error) {
    appResult = `Inspection failed: ${error.message}`;
    renderWorkspaceView();
  }
}

async function runApplication(id, action) {
  const app = appDigests.find(item => item.id === id);
  if (!app) return;
  const approved = await confirmToolAction(`Run ${app.name}?\n\nAction: ${action}\nPath: ${app.path}\n\nThis runs the exact program without a shell. The digest refuses to run if the program changed since inspection.`);
  if (!approved) return;
  appResult = `Running ${app.name} · ${action}…`;
  renderWorkspaceView();
  try {
    const response = await fetch('/api/v1/apps/run', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id, action }) });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error?.message || 'The application could not run.');
    appResult = `$ ${app.path} · ${action}\n\n${data.stdout || '(no standard output)'}${data.stderr ? `\n\n[stderr]\n${data.stderr}` : ''}\n\nExit: ${data.status ?? 'unknown'}`;
    renderWorkspaceView();
  } catch (error) {
    appResult = `Run failed: ${error.message}`;
    renderWorkspaceView();
  }
}

function takeoverHeaders() { return { 'Content-Type': 'application/json', 'X-Threadline-Local-Control': '1' }; }
function takeoverRemainingSeconds() { return takeoverSession ? Math.max(0, takeoverSession.expiresAt - Math.floor(Date.now() / 1000)) : 0; }
function isTakeoverArmed() { return Boolean(takeoverSession && takeoverRemainingSeconds() > 0); }
function formatTakeoverTime(seconds) { const safe = Math.max(0, Number(seconds) || 0); return `${Math.floor(safe / 60)}:${String(safe % 60).padStart(2, '0')}`; }

function updateTakeoverPill() {
  const pill = $('#takeover-pill'); if (!pill) return;
  const local = isTakeoverArmed();
  const serverArmed = Boolean(takeoverStatus?.session?.armed);
  pill.classList.toggle('hidden', !(local || serverArmed));
  const label = $('#takeover-pill-label');
  if (label) label.textContent = local ? `Takeover · ${formatTakeoverTime(takeoverRemainingSeconds())}` : 'Takeover armed elsewhere';
}

function clearTakeoverSession() {
  takeoverSession = null;
  if (takeoverTimer) { clearInterval(takeoverTimer); takeoverTimer = null; }
  updateTakeoverPill();
}

function startTakeoverTimer() {
  if (takeoverTimer) clearInterval(takeoverTimer);
  takeoverTimer = setInterval(() => {
    if (!takeoverSession) { clearInterval(takeoverTimer); takeoverTimer = null; updateTakeoverPill(); return; }
    if (!isTakeoverArmed()) { stopTakeover(true); toast('The takeover session expired and was stopped.'); return; }
    updateTakeoverPill();
    const countdown = $('#takeover-countdown');
    if (countdown) countdown.textContent = formatTakeoverTime(takeoverRemainingSeconds());
  }, 1000);
}

async function loadTakeover(render = true) {
  const request = ++takeoverRequest;
  try {
    const response = await fetch('/api/v1/takeover/status');
    const data = await response.json();
    if (request !== takeoverRequest) return;
    if (!response.ok) throw new Error(data.error || 'Could not read takeover status.');
    takeoverStatus = data;
    if (takeoverSession && !data.session?.armed) clearTakeoverSession();
    if (takeoverSession) startTakeoverTimer(); else updateTakeoverPill();
  } catch (error) {
    if (request !== takeoverRequest) return;
    takeoverStatus = { supported: false, helper: { available: false, error: error.message }, accessibility: false, frontmost: '', apps: [], screens: [], session: null, audit: [] };
    updateTakeoverPill();
  }
  if (render && currentView === 'takeover') renderTakeoverView($('#workspace-view'));
}

async function armTakeover(minutes) {
  if (takeoverBusy) return;
  takeoverRequest += 1;
  takeoverBusy = true;
  takeoverResult = `Arming a ${minutes}-minute takeover session…`;
  renderTakeoverView($('#workspace-view'));
  try {
    const response = await fetch('/api/v1/takeover/arm', { method: 'POST', headers: takeoverHeaders(), body: JSON.stringify({ minutes }) });
    const data = await response.json();
    if (!response.ok || !data.ok) throw new Error(data.error || 'Could not arm takeover.');
    takeoverSession = { token: data.token, expiresAt: data.expiresAt, minutes: data.minutes };
    startTakeoverTimer();
    updateTakeoverPill();
    await loadTakeover(false);
    toast(`Takeover armed for ${minutes} minutes. Press Escape or Stop to end it immediately.`);
  } catch (error) {
    takeoverResult = `Could not arm takeover: ${error.message}`;
  } finally {
    takeoverBusy = false;
    if (currentView === 'takeover') renderTakeoverView($('#workspace-view'));
  }
}

async function stopTakeover(quiet = false) {
  takeoverRequest += 1;
  clearTakeoverSession();
  try {
    await fetch('/api/v1/takeover/disarm', { method: 'POST', headers: takeoverHeaders(), body: '{}' });
  } catch {}
  await loadTakeover(false);
  if (currentView === 'takeover') renderTakeoverView($('#workspace-view'));
  if (!quiet) toast('Takeover stopped. Threadline can no longer control this Mac.');
}

async function requestTakeoverPermission() {
  try {
    const response = await fetch('/api/v1/takeover/permission', { method: 'POST', headers: takeoverHeaders(), body: '{}' });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error || 'Could not request Accessibility permission.');
    toast('Accessibility request opened. Enable Engram in System Settings, then refresh this view.');
    setTimeout(() => loadTakeover(true), 1800);
  } catch (error) {
    toast(`Accessibility request failed: ${error.message}`);
  }
}

async function takeoverAction(payload) {
  if (!isTakeoverArmed()) throw new Error('Arm a takeover session first.');
  const response = await fetch('/api/v1/takeover/action', { method: 'POST', headers: takeoverHeaders(), body: JSON.stringify({ ...payload, token: takeoverSession.token }) });
  const data = await response.json();
  if (!response.ok || !data.ok) throw new Error(data.error || 'The takeover action failed.');
  return data;
}

async function performTakeoverAction(payload, label) {
  if (takeoverBusy) return;
  if (!isTakeoverArmed()) { toast('Arm a takeover session first.'); return; }
  takeoverBusy = true;
  takeoverResult = `${label}…`;
  renderTakeoverView($('#workspace-view'));
  try {
    const data = await takeoverAction(payload);
    takeoverResult = JSON.stringify(data.result, null, 2);
    if (payload.action === 'tree') takeoverTree = JSON.stringify(data.result.tree || [], null, 2);
    if (payload.action === 'windows') takeoverTree = JSON.stringify(data.result.windows || [], null, 2);
    if (payload.action === 'screenshot' && data.result?.url) takeoverShot = data.result.url;
    await loadTakeover(false);
  } catch (error) {
    takeoverResult = `${label} failed: ${error.message}`;
  } finally {
    takeoverBusy = false;
    if (currentView === 'takeover') renderTakeoverView($('#workspace-view'));
  }
}

async function computerUseForTool(args) {
  try {
    const data = await takeoverAction(args);
    loadTakeover(false);
    return { ok: true, action: data.action, result: data.result };
  } catch (error) {
    return { ok: false, code: 'takeover_failed', error: error.message };
  }
}

async function exportTakeoverAudit() {
  try {
    const response = await fetch('/api/v1/takeover/audit/export', { headers: { 'X-Threadline-Local-Control': '1' } });
    if (!response.ok) throw new Error('Could not read the takeover audit.');
    const blob = await response.blob();
    const link = document.createElement('a');
    link.href = URL.createObjectURL(blob);
    link.download = `engram-takeover-audit-${new Date().toISOString().slice(0, 10)}.jsonl`;
    link.click();
    URL.revokeObjectURL(link.href);
    toast('Takeover audit exported.');
  } catch (error) {
    toast(`Export failed: ${error.message}`);
  }
}

async function purgeTakeoverData() {
  if (!confirm('Delete the local takeover audit and every screenshot? This cannot be undone.')) return;
  try {
    const response = await fetch('/api/v1/takeover/purge', { method: 'POST', headers: takeoverHeaders(), body: '{}' });
    const data = await response.json();
    if (!response.ok || !data.ok) throw new Error(data.error || 'Could not delete takeover data.');
    takeoverResult = ''; takeoverTree = ''; takeoverShot = '';
    await loadTakeover(true);
    toast(`Takeover data deleted (${data.screenshotsDeleted} screenshot(s)).`);
  } catch (error) {
    toast(`Delete failed: ${error.message}`);
  }
}

function renderTakeoverView(host) {
  if (!takeoverStatus) {
    host.innerHTML = '<div class="page-heading"><div class="eyebrow">GUI CONTROL</div><h1>Takeover</h1><p>Reading accessibility and screen state…</p></div>';
    return;
  }
  const helper = takeoverStatus.helper || {};
  const supported = takeoverStatus.supported !== false;
  const ready = supported && helper.available !== false;
  const activeLocal = isTakeoverArmed();
  const serverArmed = Boolean(takeoverStatus.session?.armed);
  const remaining = activeLocal ? takeoverRemainingSeconds() : (takeoverStatus.session?.remainingSeconds || 0);
  const accessibility = Boolean(takeoverStatus.accessibility);
  const apps = takeoverStatus.apps || [];
  const audit = takeoverStatus.audit || [];
  const screen = (takeoverStatus.screens || [])[0];
  const target = takeoverStatus.frontmost || apps[0]?.name || 'Safari';
  const appOptions = apps.map(app => `<option value="${esc(app.name)}"></option>`).join('');
  const appRows = apps.length ? apps.map(app => `<div class="takeover-app ${app.active ? 'is-frontmost' : ''}"><div><b>${esc(app.name)}</b><small>${esc(app.bundleId || 'no bundle id')}</small></div><div><button class="mini-button" type="button" data-takeover-focus="${esc(app.name)}">Focus</button><button class="mini-button" type="button" data-takeover-tree="${esc(app.name)}">Inspect UI</button></div></div>`).join('') : '<div class="ledger-empty">No regular applications are running.</div>';
  const auditRows = audit.length ? audit.slice().reverse().map(item => `<div class="takeover-audit-row ${item.ok ? 'ok' : 'fail'}"><time>${new Date(item.at * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', second: '2-digit' })}</time><b>${esc(item.action)}</b><span>${esc(item.summary)}</span></div>`).join('') : '<div class="ledger-empty">No takeover actions yet.</div>';

  host.innerHTML = `<div class="page-heading"><div class="eyebrow">ARMED GUI CONTROL · macOS</div><h1>Takeover</h1><p>See what is on screen, focus an application, press keys, type, click controls, run AppleScript, and capture the screen. Arm a timed session to let the local model act without a prompt for every step. Stop is always immediate.</p></div>
    <section class="takeover-hero ${activeLocal ? 'armed' : ''}">
      <div class="takeover-hero-copy"><span class="eyebrow">${ready ? (activeLocal ? 'SESSION ARMED' : serverArmed ? 'ARMED IN ANOTHER TAB' : 'SESSION DISARMED') : 'UNAVAILABLE'}</span><h2>${ready ? (activeLocal ? `Threadline can control this Mac for ${formatTakeoverTime(remaining)}` : serverArmed ? 'A takeover session is active, but this tab does not hold its token' : 'No application is currently under Threadline control') : 'GUI takeover is not available in this build'}</h2><p>${ready ? (activeLocal ? 'Every action is written to a local audit log. Press Escape or Stop to release control immediately.' : 'Arm a bounded session to enable the model and the controls below. Nothing runs while disarmed.') : 'Threadline still runs locally. On macOS, install Xcode Command Line Tools and rebuild Engram to enable the control helper.'}</p></div>
      <div class="takeover-hero-actions">${ready ? (serverArmed ? '<button class="danger-button" type="button" id="takeover-stop">Stop takeover</button>' : `<form id="takeover-arm-form" class="takeover-arm"><label for="takeover-minutes">Session</label><select id="takeover-minutes"><option value="5">5 min</option><option value="15" selected>15 min</option><option value="30">30 min</option><option value="60">60 min</option><option value="120">120 min</option></select><button class="danger-button" type="submit" ${takeoverBusy ? 'disabled' : ''}>Arm takeover</button></form>`) : ''}<button class="mini-button" type="button" id="takeover-refresh">Refresh state</button></div>
    </section>
    <div class="takeover-stats">
      <section class="takeover-stat ${supported ? (accessibility ? 'ok' : 'warn') : 'warn'}"><span>ACCESSIBILITY</span><b>${supported ? (accessibility ? 'Granted' : 'Not granted') : 'Unavailable'}</b><p>${supported ? (accessibility ? 'Engram can inspect and operate other applications.' : 'macOS blocks UI control until Engram is enabled in Privacy & Security → Accessibility.') : 'Accessibility control is only available on macOS.'}</p>${supported && helper.available && !accessibility ? '<button class="mini-button" type="button" id="takeover-permission">Grant accessibility</button>' : ''}</section>
      <section class="takeover-stat ${helper.available ? 'ok' : 'warn'}"><span>CONTROL HELPER</span><b>${helper.available ? 'Ready' : 'Unavailable'}</b><p>${helper.error ? esc(helper.error) : 'The local Accessibility and CoreGraphics bridge is installed.'}</p></section>
      <section class="takeover-stat"><span>FRONTMOST APP</span><b>${esc(takeoverStatus.frontmost || 'Unknown')}</b><p>${esc(takeoverStatus.frontmostBundleId || 'No active application reported.')}</p></section>
      <section class="takeover-stat"><span>SCREEN</span><b>${screen ? `${screen.width} × ${screen.height}` : 'Unknown'}</b><p>${screen ? `Origin ${screen.x}, ${screen.y}. Click coordinates are global screen points.` : 'Screen dimensions are unavailable.'}</p></section>
    </div>
    ${!ready ? '<div class="takeover-locked">GUI takeover is not available in this build. On macOS, install Xcode Command Line Tools and rebuild Engram.</div>' : !activeLocal ? '<div class="takeover-locked">Arm a takeover session to enable the app controls below. Arming and permissions are always available.</div>' : ''}
    <div class="takeover-grid">
      <section class="settings-card takeover-card"><div class="settings-card-heading"><span class="settings-icon">▣</span><div><h2>Running applications</h2><p>Focus a window or inspect its accessibility tree</p></div><span class="module-label">LIVE</span></div><div class="takeover-app-list">${appRows}</div></section>
      <section class="settings-card takeover-card"><div class="settings-card-heading"><span class="settings-icon warm">⌘</span><div><h2>Direct controls</h2><p>These run immediately while armed</p></div><span class="module-label">TRUSTED SESSION</span></div>
        <label class="field-label" for="takeover-app">Target application</label><input id="takeover-app" list="takeover-app-list" value="${esc(target)}" placeholder="Safari or com.apple.Safari" /><datalist id="takeover-app-list">${appOptions}</datalist>
        <div class="takeover-inline"><button class="secondary-button" type="button" data-takeover-focus-target>Focus</button><button class="secondary-button" type="button" data-takeover-windows>Windows</button><button class="secondary-button" type="button" data-takeover-tree-target>Inspect UI</button></div>
        <label class="field-label" for="takeover-key">Key or shortcut</label><div class="takeover-inline"><input id="takeover-key" placeholder="cmd+s, return, cmd+shift+4" /><button class="secondary-button" type="button" data-takeover-key>Press</button></div>
        <label class="field-label" for="takeover-text">Type into the frontmost application (focus a target above first)</label><textarea id="takeover-text" rows="3" placeholder="Text to type. Newlines press Return."></textarea><div class="takeover-inline"><button class="secondary-button" type="button" data-takeover-type>Type text</button></div>
        <label class="field-label" for="takeover-x">Click at global screen coordinates</label><div class="takeover-inline"><input id="takeover-x" type="number" value="500" aria-label="X coordinate" /><input id="takeover-y" type="number" value="500" aria-label="Y coordinate" /><button class="secondary-button" type="button" data-takeover-click>Click</button></div>
        <label class="field-label" for="takeover-dy">Scroll the frontmost application</label><div class="takeover-inline"><input id="takeover-dy" type="number" value="3" aria-label="Scroll amount" /><button class="secondary-button" type="button" data-takeover-scroll>Scroll</button><span class="local-service-note">Positive scrolls down.</span></div>
        <label class="field-label" for="takeover-ui-title">UI element by role and title</label><div class="takeover-inline"><input id="takeover-ui-role" value="AXButton" aria-label="UI role" /><input id="takeover-ui-title" placeholder="Button title or description" /><button class="secondary-button" type="button" data-takeover-ui-click>Click element</button></div><div class="takeover-inline"><input id="takeover-ui-value" placeholder="Value for a text field" aria-label="New value for the UI element" /><button class="secondary-button" type="button" data-takeover-ui-set>Set value</button></div>
        <label class="field-label" for="takeover-open-app">Open an application</label><div class="takeover-inline"><input id="takeover-open-app" placeholder="Calculator or /Applications/Safari.app" /><button class="secondary-button" type="button" data-takeover-open>Open</button></div>
        <div class="takeover-inline"><button class="secondary-button" type="button" data-takeover-screenshot>Capture screen</button><button class="secondary-button" type="button" data-takeover-pause>Stop takeover</button></div>
      </section>
      <section class="settings-card takeover-card wide-card"><div class="settings-card-heading"><span class="settings-icon blue-icon">✳</span><div><h2>AppleScript</h2><p>Arbitrary app automation through osascript</p></div><span class="module-label">FULL POWER</span></div><textarea id="takeover-script" rows="5" placeholder='tell application "Safari" to activate' aria-label="AppleScript source"></textarea><div class="takeover-inline"><button class="secondary-button" type="button" data-takeover-script>Run AppleScript</button><span class="local-service-note">AppleScript can ask macOS for Automation permission per app. It can also run shell scripts. Only run code you intend.</span></div></section>
      <section class="settings-card takeover-card wide-card"><div class="settings-card-heading"><span class="settings-icon">↗</span><div><h2>Ask Threadline to act</h2><p>Place an armed-session instruction in the chat composer</p></div><span class="module-label">AGENT</span></div><form id="takeover-ask-form" class="takeover-inline"><input id="takeover-ask" placeholder="e.g. Open Calculator and compute 7 + 3" aria-label="Takeover goal" /><button class="secondary-button" type="submit">Put in chat</button></form><p class="local-service-note">This fills the composer; it never sends automatically. Review the instruction before pressing Enter.</p></section>
    </div>
    ${takeoverResult ? `<pre class="app-result takeover-result" aria-live="polite">${esc(takeoverResult)}</pre>` : ''}
    ${takeoverTree ? `<details class="takeover-tree"><summary>Accessibility / window output</summary><pre class="app-result">${esc(takeoverTree)}</pre></details>` : ''}
    ${takeoverShot ? `<section class="takeover-shot"><div class="section-title">SCREEN CAPTURE <a href="${esc(takeoverShot)}" target="_blank" rel="noreferrer">open full size ↗</a></div><img src="${esc(takeoverShot)}" alt="Most recent screen capture" /></section>` : ''}
    <section class="settings-card takeover-card wide-card takeover-audit-card"><div class="settings-card-heading"><span class="settings-icon warm">◷</span><div><h2>Action audit</h2><p>Stored locally under <code>.engram/takeover/audit.jsonl</code> · newest ${takeoverStatus.retention?.auditKeepEntries ?? 2000} entries and ${takeoverStatus.retention?.screenshots ?? 50} screenshots are kept</p></div><div class="takeover-inline"><button class="mini-button" type="button" id="takeover-export">Export JSONL</button><button class="mini-button" type="button" id="takeover-purge">Delete data</button></div><span class="module-label">${audit.length} EVENTS</span></div><div class="takeover-audit">${auditRows}</div></section>`;

  if (!activeLocal) {
    host.querySelectorAll('.takeover-grid button, .takeover-grid input, .takeover-grid textarea, .takeover-grid select').forEach(element => { element.disabled = true; });
  }
  $('#takeover-arm-form')?.addEventListener('submit', event => { event.preventDefault(); takeoverMinutes = Number($('#takeover-minutes').value) || 15; armTakeover(takeoverMinutes); });
  const minutesSelect = $('#takeover-minutes');
  if (minutesSelect) { minutesSelect.value = String(takeoverMinutes); minutesSelect.addEventListener('change', () => { takeoverMinutes = Number(minutesSelect.value) || 15; }); }
  $('#takeover-stop')?.addEventListener('click', () => stopTakeover());
  $('[data-takeover-pause]')?.addEventListener('click', () => stopTakeover());
  $('#takeover-refresh')?.addEventListener('click', () => loadTakeover(true));
  $('#takeover-permission')?.addEventListener('click', requestTakeoverPermission);
  $('#takeover-export')?.addEventListener('click', exportTakeoverAudit);
  $('#takeover-purge')?.addEventListener('click', purgeTakeoverData);
  host.querySelectorAll('[data-takeover-focus]').forEach(button => button.addEventListener('click', () => performTakeoverAction({ action: 'focus', app: button.dataset.takeoverFocus }, `Focusing ${button.dataset.takeoverFocus}`)));
  host.querySelectorAll('[data-takeover-tree]').forEach(button => button.addEventListener('click', () => performTakeoverAction({ action: 'tree', app: button.dataset.takeoverTree, depth: 6 }, `Reading the UI tree for ${button.dataset.takeoverTree}`)));
  $('[data-takeover-focus-target]')?.addEventListener('click', () => performTakeoverAction({ action: 'focus', app: $('#takeover-app').value.trim() }, 'Focusing application'));
  $('[data-takeover-windows]')?.addEventListener('click', () => performTakeoverAction({ action: 'windows', app: $('#takeover-app').value.trim() }, 'Reading windows'));
  $('[data-takeover-tree-target]')?.addEventListener('click', () => performTakeoverAction({ action: 'tree', app: $('#takeover-app').value.trim(), depth: 6 }, 'Reading the UI tree'));
  $('[data-takeover-key]')?.addEventListener('click', () => performTakeoverAction({ action: 'key', key: $('#takeover-key').value.trim() }, 'Pressing key'));
  $('[data-takeover-type]')?.addEventListener('click', () => performTakeoverAction({ action: 'type', text: $('#takeover-text').value }, 'Typing text'));
  $('[data-takeover-click]')?.addEventListener('click', () => performTakeoverAction({ action: 'click', x: Number($('#takeover-x').value), y: Number($('#takeover-y').value), button: 'left' }, 'Clicking'));
  $('[data-takeover-scroll]')?.addEventListener('click', () => performTakeoverAction({ action: 'scroll', dy: Number($('#takeover-dy').value), dx: 0 }, 'Scrolling'));
  $('[data-takeover-ui-click]')?.addEventListener('click', () => performTakeoverAction({ action: 'click_ui', app: $('#takeover-app').value.trim(), role: $('#takeover-ui-role').value.trim(), title: $('#takeover-ui-title').value.trim() }, 'Clicking UI element'));
  $('[data-takeover-ui-set]')?.addEventListener('click', () => performTakeoverAction({ action: 'set_value', app: $('#takeover-app').value.trim(), role: $('#takeover-ui-role').value.trim(), title: $('#takeover-ui-title').value.trim(), value: $('#takeover-ui-value').value }, 'Setting UI value'));
  $('[data-takeover-open]')?.addEventListener('click', () => performTakeoverAction({ action: 'open', app: $('#takeover-open-app').value.trim() }, 'Opening application'));
  $('[data-takeover-screenshot]')?.addEventListener('click', () => performTakeoverAction({ action: 'screenshot' }, 'Capturing the screen'));
  $('[data-takeover-script]')?.addEventListener('click', () => performTakeoverAction({ action: 'applescript', source: $('#takeover-script').value }, 'Running AppleScript'));
  $('#takeover-ask-form')?.addEventListener('submit', event => {
    event.preventDefault();
    const goal = $('#takeover-ask').value.trim();
    if (!goal) { toast('Describe what you want Threadline to do.'); return; }
    const instruction = `Use the armed takeover session to ${goal}. Inspect the current screen or accessibility tree first, work step by step, and stop if the UI does not match.`;
    $('#message-input').value = instruction;
    switchView('chat');
    $('#message-input').focus();
    $('#message-input').dispatchEvent(new Event('input'));
    toast('Instruction placed in the chat composer. Review it, then send.');
  });
}

async function refreshOmlxStatus() {
  const label = $('#omlx-status-text'); const light = $('#omlx-status-light');
  if (!label || !light) return;
  label.textContent = 'Checking local service…'; light.classList.remove('is-on', 'is-error');
  try {
    const response = await fetch('/api/v1/inference/omlx');
    const status = await response.json();
    label.textContent = status.message;
    light.classList.toggle('is-on', Boolean(status.endpointListening));
    light.classList.toggle('is-error', Boolean(status.supported && !status.cliAvailable));
    document.querySelectorAll('[data-omlx-action]').forEach(button => { button.disabled = !status.supported || !status.cliAvailable; });
    const modelControlsAllowed = status.supported && status.cliAvailable && classifyEndpoint($('#base-url')?.value || '').kind === 'local';
    [$('#load-model'), $('#unload-model')].filter(Boolean).forEach(button => { button.disabled = !modelControlsAllowed; });
  } catch {
    label.textContent = 'Could not read the local service status.'; light.classList.add('is-error');
    [...document.querySelectorAll('[data-omlx-action], #load-model, #unload-model')].forEach(button => { button.disabled = true; });
  }
}

async function manageOmlx(action) {
  const label = $('#omlx-status-text');
  const buttons = [...document.querySelectorAll('[data-omlx-action]')];
  buttons.forEach(button => { button.disabled = true; });
  if (label) label.textContent = `Running oMLX ${action}…`;
  try {
    const response = await fetch(`/api/v1/inference/omlx/${action}`, { method: 'POST', headers: { 'X-Threadline-Local-Control': '1' } });
    const result = await response.json();
    if (!response.ok || !result.ok) throw new Error(result.error || `oMLX ${action} failed.`);
    if (label) label.textContent = result.message;
    if (action === 'start' || action === 'restart') {
      for (let attempt = 0; attempt < 10; attempt++) {
        await new Promise(resolve => setTimeout(resolve, 500));
        const statusResponse = await fetch('/api/v1/inference/omlx');
        const status = await statusResponse.json();
        if (status.endpointListening) break;
      }
    }
    await refreshOmlxStatus();
  } catch (error) {
    if (label) label.textContent = error.message;
    $('#omlx-status-light')?.classList.add('is-error');
    buttons.forEach(button => { button.disabled = false; });
  }
}

async function manageModelResidency(loaded) {
  const feedback = $('#model-load-feedback');
  const buttons = [$('#load-model'), $('#unload-model')];
  buttons.forEach(button => { button.disabled = true; });
  if (feedback) feedback.textContent = loaded ? 'Loading selected model…' : 'Unloading selected model…';
  try {
    await setModelLoaded({ baseUrl: $('#base-url').value.trim(), apiKey: getSessionApiKey(), model: $('#model-id').value, loaded });
    if (feedback) feedback.textContent = loaded ? 'Model load requested. It may take time to warm up.' : 'Model unload requested.';
  } catch (error) {
    if (feedback) feedback.textContent = error.message;
  } finally {
    buttons.forEach(button => { button.disabled = false; });
  }
}

async function discoverModels() {
  const button = $('#discover-models'); button.disabled = true; button.textContent = 'Loading…';
  const feedback = $('#connection-feedback');
  try {
    const baseUrl = $('#base-url').value.trim().replace(/\/$/, ''); const apiKey = $('#api-key').value; const target = classifyEndpoint(baseUrl); if (!target.allowed) throw new Error(target.reason); if (target.kind === 'remote' && !(await confirmDisclosure({ endpoint: target.origin, apiKey, discovery: true }))) return;
    const models = await listModels({ baseUrl, apiKey });
    if (!models.length) feedback.textContent = 'Connected, but the server returned no model IDs.';
    else {
      const old = $('#model-id').value; const listId = 'model-options'; let dl = document.getElementById(listId);
      if (!dl) { dl = document.createElement('datalist'); dl.id = listId; document.body.append(dl); $('#model-id').setAttribute('list', listId); }
      dl.innerHTML = models.map(item => `<option value="${esc(item.id)}">`).join('');
      if (!old && models[0]) $('#model-id').value = models[0].id;
      feedback.textContent = `${models.length} model(s) available. Select or type a model ID.`;
    }
  } catch (error) { feedback.textContent = error.message; }
  finally { button.disabled = false; button.textContent = 'Discover models'; }
}

async function submitMessage(message) {
  if (sending || !message.trim()) return;
  const settings = getWorkspace().settings;
  if (!settings.model) { switchView('settings'); toast('Connect a model before sending.'); return; }
  const endpoint = classifyEndpoint(settings.baseUrl);
  if (!endpoint.allowed) { toast(`Request blocked: ${endpoint.reason}`); return; }
  const toolsSupported = supportsFunctionTools({ model: settings.model, endpointOrigin: endpoint.origin, appOrigin: location.origin });
  const modelTools = !toolsSupported ? [] : endpoint.kind === 'local'
    ? applicationTools
    : applicationTools.filter(tool => !tool.function.name.startsWith('project_') && tool.function.name !== 'run_project_check' && tool.function.name !== 'computer_use');
  const offeredToolNames = new Set(modelTools.map(tool => tool.function.name));
  const allowVaultContext = canSendVaultContext(settings.baseUrl, settings.vaultSearchEnabled);
  if (settings.vaultSearchEnabled && !allowVaultContext) toast('Vault context is disabled for remote inference.');
  sending = true; $('#send-button').textContent = '■'; $('#send-button').setAttribute('aria-label', 'Stop generation');
  aborter = new AbortController();
  const userTurn = appendTurn({ role: 'user', content: message.trim() });
  let convo = current();
  if (settings.memoryEnabled) { updateMemory(recordAndUpdate(convo.memory, userTurn)); convo = current(); }
  const historyForPrompt = endpoint.kind === 'remote' ? filterRemoteHistory(convo.turns) : convo.turns;
  const evidence = settings.memoryEnabled && settings.strategy !== 'recent' ? retrieveEvidence(historyForPrompt, message, convo.memory, 5) : [];
  let vaultResult = { indexed: false, hits: [] }; let vaultSearchError = null;
  if (allowVaultContext) {
    try {
      vaultResult = await searchEngramVault(message, { signal: aborter.signal });
      if (!vaultResult.indexed && !vaultResult.hits?.length) toast('Engram has no indexed vault yet. Open Vault → Rebuild index, then retry.');
    } catch (error) {
      vaultSearchError = error;
      if (error.name !== 'AbortError') toast(`Vault search unavailable: ${error.message}`);
    }
  }
  const decision = evaluateRules({ message, memory: convo.memory, evidence, settings, vaultResult, vaultSearchError });
  const pack = packContext({ message, history: historyForPrompt, memory: settings.memoryEnabled && settings.strategy !== 'recent' ? convo.memory : { ...convo.memory, currentGoal: [], activeTopics: [], decisions: [], constraints: [], preferences: [], openQuestions: [] }, evidence: settings.strategy === 'state' ? [] : evidence, vaultNotes: vaultResult.hits || [], vaultContextAllowed: allowVaultContext, settings });
  if (endpoint.kind === 'remote') {
    const approved = await confirmDisclosure({ endpoint: endpoint.origin, apiKey: getSessionApiKey(), categories: requestCategories({ messages: pack.messages, tools: modelTools }), chars: JSON.stringify(pack.messages).length });
    if (!approved) { toast('Remote request cancelled. Your message remains in local history.'); sending = false; $('#send-button').textContent = '↑'; renderAll(); return; }
  }
  updateActive({ lastAssembly: { audit: pack.audit, trace: decision.trace, evidence: evidence.map(item => ({ id: item.turn.id, score: item.score, label: item.evidence })), vault: { indexed: vaultResult.indexed, hits: vaultResult.hits || [], error: vaultSearchError?.message || null } } });
  if (pack.audit.estimatedInputTokens > pack.audit.usableInput) {
    appendTurn({ role: 'assistant', content: `This request and required instructions need an estimated ${pack.audit.estimatedInputTokens.toLocaleString()} input tokens, above the configured ${pack.audit.usableInput.toLocaleString()} token input budget. Increase the model context setting or shorten the request. The user message was kept in local history and was not sent.` });
    sending = false; $('#send-button').textContent = '↑'; $('#send-button').setAttribute('aria-label', 'Send message'); renderAll(); return;
  }
  renderAll(); showThinking();
  const placeholder = $('#thinking-indicator');
  try {
     let answer = '';
     let requestMessages = pack.messages; let actionRounds = 0;
     while (actionRounds++ < 3) {
       if (endpoint.kind === 'remote' && actionRounds > 1) {
         const approvedFollowUp = await confirmDisclosure({ endpoint: endpoint.origin, apiKey: getSessionApiKey(), categories: requestCategories({ messages: requestMessages, tools: [] }), chars: JSON.stringify(requestMessages).length });
         if (!approvedFollowUp) { placeholder.remove(); toast('Remote follow-up cancelled. Local tool results were not sent.'); return; }
       }
       let toolCalls = [];
      let toolSupport = true;
      try {
        for await (const event of streamChat({ ...settings, apiKey: getSessionApiKey(), messages: requestMessages, tools: modelTools, signal: aborter.signal })) {
          if (event.type === 'token') { answer += event.content; placeholder.querySelector('.thinking-text').textContent = answer; $('#messages').scrollTop = $('#messages').scrollHeight; }
          if (event.type === 'tool_calls') toolCalls = event.calls;
        }
      } catch (error) {
        if (![400, 404, 422].includes(error.status)) throw error;
        toolSupport = false;
        answer = '';
        placeholder.querySelector('.thinking-text').innerHTML = '<span class="stream-state">Endpoint did not accept function tools · retrying chat without app actions</span>';
        for await (const event of streamChat({ ...settings, apiKey: getSessionApiKey(), messages: requestMessages, signal: aborter.signal })) {
          if (event.type === 'token') { answer += event.content; placeholder.querySelector('.thinking-text').textContent = answer; }
        }
      }
      if (!toolSupport) break;
      if (!toolCalls.length) break;
      answer = '';
      placeholder.querySelector('.thinking-text').innerHTML = '<span class="stream-state"><i></i> applying requested workspace action</span>';
      const toolMessages = [];
      for (const call of toolCalls) {
          const actionCallbacks = {
           actionId: call.id,
          openView: switchView,
          updateSettings: patch => { updateSettings(patch); renderAll(); },
          getMemory: () => current().memory,
          newConversation: () => { createConversation(); switchView('chat'); renderAll(); },
          clearWorkspace: () => { clearWorkspace(); createConversation(); switchView('chat'); renderAll(); },
          inspectApplication: inspectApplicationForTool,
          listApplications: listApplicationsForTool,
          runApplication: runApplicationForTool,
          computerUse: computerUseForTool,
          trustedTakeover: isTakeoverArmed(),
          confirm: message => confirmToolAction(message)
        };
        const rejection = toolCallRejection(call.function.name, offeredToolNames, endpoint.kind);
        if (rejection) {
          toolMessages.push({ role: 'tool', tool_call_id: call.id, content: JSON.stringify({ ok: false, ...rejection }) });
          continue;
        }
        if (call.function.name === 'computer_use' && !isTakeoverArmed()) {
          toolMessages.push({ role: 'tool', tool_call_id: call.id, content: JSON.stringify({ ok: false, code: 'takeover_not_armed', error: 'Arm a takeover session in the Takeover view before using computer control.' }) });
          continue;
        }
        const isProjectTool = call.function.name.startsWith('project_') || call.function.name === 'run_project_check';
        const result = isProjectTool && endpoint.kind !== 'local'
          ? { ok: false, code: 'local_inference_required', error: 'Project tools are disabled for remote inference. Connect a loopback on-device model to keep Engram source offline.' }
          : isProjectTool
            ? await executeProjectAction(call.function.name, call.function.arguments, actionCallbacks)
             : await executeApplicationAction(call.function.name, call.function.arguments, actionCallbacks);
        toolMessages.push({ role: 'tool', tool_call_id: call.id, content: JSON.stringify(result) });
      }
      requestMessages = [...requestMessages, { role: 'assistant', content: null, tool_calls: toolCalls }, ...toolMessages];
      renderStatus(); renderInspector();
    }
    placeholder.remove();
    const vaultSourceNoteIds = [...new Set(pack.audit.selected.flatMap(item => item.sourceNoteIds || []))];
    appendTurn({ role: 'assistant', content: answer || 'The model returned an empty response.', sourceTurnIds: settings.showEvidence ? pack.audit.selected.filter(item => item.kind.startsWith('source')).flatMap(item => item.sourceTurnIds) : [], sourceNoteIds: settings.showEvidence ? vaultSourceNoteIds : [], vaultSourceNoteIds, vaultProvenanceKnown: true, estimatedTokens: pack.audit.estimatedInputTokens });
  } catch (error) {
    placeholder?.remove();
    if (error.name === 'AbortError') appendTurn({ role: 'assistant', content: 'Generation stopped.' });
    else { appendTurn({ role: 'assistant', content: `I couldn’t reach the inference server. ${error.message}\n\nYour message and conversation are saved locally. Check Connections & settings, the endpoint, and whether the server allows browser requests (CORS).` }); }
  } finally { sending = false; aborter = null; $('#send-button').textContent = '↑'; $('#send-button').setAttribute('aria-label', 'Send message'); renderAll(); }
}

function showThinking() {
  $('#messages').insertAdjacentHTML('beforeend', `<article class="message-row assistant-row thinking-row" id="thinking-indicator"><div class="message-avatar assistant"><img src="./assets/engram-mark.png" alt="" /></div><div class="message-main"><div class="message-meta"><strong>Threadline</strong><span class="stream-state"><i></i> assembling and generating</span></div><div class="message-text thinking-text"><span class="loading-dots"><i></i><i></i><i></i></span></div></div></article>`);
  $('#messages').scrollTop = $('#messages').scrollHeight;
}

async function confirmToolAction(disclosure) {
  const dialog = $('#tool-consent');
  $('#tool-consent-content').textContent = disclosure;
  return new Promise(resolve => {
    dialog.onclose = () => resolve(dialog.returnValue === 'allow');
    dialog.showModal();
  });
}

$('#composer-form').addEventListener('submit', e => { e.preventDefault(); if (sending) { aborter?.abort(); return; } const input = $('#message-input'); const message = input.value; input.value = ''; input.style.height = 'auto'; submitMessage(message); });
$('#message-input').addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); $('#composer-form').requestSubmit(); } });
$('#message-input').addEventListener('input', e => { e.target.style.height = 'auto'; e.target.style.height = `${Math.min(e.target.scrollHeight, 180)}px`; });
$('#new-chat').addEventListener('click', () => { createConversation(); switchView('chat'); renderAll(); $('#message-input').focus(); });
$('#clear-history').addEventListener('click', () => { if (confirm('Delete all locally stored conversations and memory?')) { clearWorkspace(); createConversation(); renderAll(); toast('Local conversations cleared.'); } });
document.querySelectorAll('.nav-item').forEach(item => item.addEventListener('click', () => switchView(item.dataset.view)));
document.querySelectorAll('.inspector-tab').forEach(item => item.addEventListener('click', () => { inspectorTab = item.dataset.tab; document.querySelectorAll('.inspector-tab').forEach(tab => tab.classList.toggle('active', tab === item)); renderInspector(); }));
$('#open-inspector').addEventListener('click', () => document.body.classList.add('inspector-open'));
$('#close-inspector').addEventListener('click', () => document.body.classList.remove('inspector-open'));
$('#refresh-inspector').addEventListener('click', renderInspector);
$('#memory-toggle').addEventListener('click', () => { updateSettings({ memoryEnabled: !getWorkspace().settings.memoryEnabled }); renderAll(); toast(`Conversation memory ${getWorkspace().settings.memoryEnabled ? 'enabled' : 'disabled'}.`); });
$('#vault-toggle').addEventListener('click', () => { updateSettings({ vaultSearchEnabled: !getWorkspace().settings.vaultSearchEnabled }); renderAll(); toast(`Engram vault retrieval ${getWorkspace().settings.vaultSearchEnabled ? 'enabled' : 'disabled'}.`); });
$('#context-budget').addEventListener('click', () => { switchView('settings'); });
$('#connection-pill').addEventListener('click', () => switchView('settings'));
$('#open-inspector').addEventListener('dblclick', () => { inspectorTab = 'memory'; renderInspector(); });
document.querySelectorAll('[data-prompt]').forEach(card => card.addEventListener('click', () => { $('#message-input').value = card.dataset.prompt; $('#message-input').focus(); $('#message-input').dispatchEvent(new Event('input')); }));
$('#profile-button').addEventListener('click', () => { switchView('settings'); });
$('#takeover-pill')?.addEventListener('click', () => stopTakeover());
window.addEventListener('pagehide', () => {
  if (!takeoverSession) return;
  try {
    fetch('/api/v1/takeover/disarm', { method: 'POST', headers: takeoverHeaders(), body: '{}', keepalive: true });
  } catch {}
});
document.addEventListener('keydown', event => {
  if (event.key === 'Escape' && isTakeoverArmed() && !document.querySelector('dialog[open]')) {
    event.preventDefault();
    stopTakeover();
  }
});
subscribe(() => { renderThreads(); renderStatus(); renderInspector(); });
applyAccent(getWorkspace().settings.accent || 'green');
if (!getWorkspace().conversations.length) createConversation();
renderAll();
configureEmbeddedDefault();
// Keep first paint and embedded-model discovery off the takeover probe's path.
// The bounded idle callback still surfaces an armed-session Stop pill within ~1s.
const scheduleTakeoverProbe = window.requestIdleCallback
  ? callback => window.requestIdleCallback(callback, { timeout: 1000 })
  : callback => setTimeout(callback, 500);
scheduleTakeoverProbe(() => loadTakeover(false));
