import { classifyEndpoint } from '../core/privacy-policy.js';

function endpoint(baseUrl) {
  const result = classifyEndpoint(baseUrl);
  if (!result.allowed) throw new Error(result.reason);
  return result;
}

export async function listModels({ baseUrl, apiKey }) {
  const target = endpoint(baseUrl);
  const response = await fetch(`${target.url.toString().replace(/\/$/, '')}/models`, { headers: apiKey ? { Authorization: `Bearer ${apiKey}` } : {} });
  if (!response.ok) throw new Error(`Model discovery failed (${response.status}). Check the endpoint, key, and server CORS settings.`);
  const data = await response.json(); return data.data || [];
}

export async function setModelLoaded({ baseUrl, apiKey, model, loaded }) {
  const target = endpoint(baseUrl);
  if (target.kind !== 'local') throw new Error('Model load controls are allowed only for a loopback inference server.');
  const modelId = String(model || '').trim();
  if (!modelId || !/^[\w.:-]+$/.test(modelId)) throw new Error('Enter a model ID containing only letters, numbers, dots, underscores, colons, or hyphens.');
  const response = await fetch(`${target.url.toString().replace(/\/$/, '')}/models/${encodeURIComponent(modelId)}/${loaded ? 'load' : 'unload'}`, {
    method: 'POST', headers: apiKey ? { Authorization: `Bearer ${apiKey}` } : {}
  });
  if (!response.ok) {
    let detail = ''; try { detail = (await response.json()).detail || ''; } catch {}
    throw new Error(detail || `The local server could not ${loaded ? 'load' : 'unload'} this model (${response.status}).`);
  }
  return response.json().catch(() => ({}));
}

export async function* streamChat({ baseUrl, apiKey, model, messages, tools, signal }) {
  const target = endpoint(baseUrl);
  const response = await fetch(`${target.url.toString().replace(/\/$/, '')}/chat/completions`, {
    method: 'POST', signal,
    headers: { 'Content-Type': 'application/json', ...(apiKey ? { Authorization: `Bearer ${apiKey}` } : {}) },
    body: JSON.stringify({ model, messages, ...(tools?.length ? { tools, tool_choice: 'auto' } : {}), stream: true, temperature: 0.7 })
  });
  if (!response.ok) {
    let detail = ''; try { detail = (await response.json()).error?.message || ''; } catch {}
    const error = new Error(detail || `Inference request failed (${response.status}). Verify the model ID and compatible endpoint.`);
    error.status = response.status;
    throw error;
  }
  if (!response.body) throw new Error('Streaming is unavailable for this endpoint.');
  const reader = response.body.getReader(); const decoder = new TextDecoder(); let buffer = ''; const toolCalls = [];
  while (true) {
    const { value, done } = await reader.read(); buffer += decoder.decode(value || new Uint8Array(), { stream: !done });
    const chunks = buffer.split('\n'); buffer = chunks.pop() || '';
    for (const line of chunks) {
      if (!line.startsWith('data:')) continue;
      const payload = line.slice(5).trim(); if (payload === '[DONE]') { if (toolCalls.length) yield { type: 'tool_calls', calls: toolCalls }; return; }
      try {
        const item = JSON.parse(payload); const delta = item.choices?.[0]?.delta; const token = delta?.content;
        if (token) yield { type: 'token', content: token };
        for (const part of delta?.tool_calls || []) {
          const call = toolCalls[part.index] ||= { id: '', type: 'function', function: { name: '', arguments: '' } };
          if (part.id) call.id = part.id;
          if (part.function?.name) call.function.name += part.function.name;
          if (part.function?.arguments) call.function.arguments += part.function.arguments;
        }
      } catch {}
    }
    if (done) break;
  }
  if (toolCalls.length) yield { type: 'tool_calls', calls: toolCalls };
}
