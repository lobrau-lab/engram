function isLoopbackHost(host) {
  return host === 'localhost' || host === '127.0.0.1' || host === '::1' || host === '[::1]';
}

function isNativeAppEndpoint(url) {
  const host = url.hostname.toLowerCase();
  return url.protocol === 'engram:' && host === 'app' && !url.port;
}

function isPrivateIpv4(host) {
  const parts = host.split('.');
  if (parts.length !== 4 || parts.some(part => !/^\d{1,3}$/.test(part))) return false;
  const octets = parts.map(Number);
  if (octets.some(octet => octet > 255)) return false;
  if (octets[0] === 10) return true;
  if (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31) return true;
  if (octets[0] === 192 && octets[1] === 168) return true;
  if (octets[0] === 169 && octets[1] === 254) return true;
  return false;
}

function isPrivateNetworkHost(host) {
  const normalized = host.replace(/^\[|\]$/g, '').toLowerCase();
  if (isLoopbackHost(normalized)) return false;
  if (isPrivateIpv4(normalized)) return true;
  if (/^(fc|fd)[0-9a-f]{2}:/i.test(normalized) || /^fe[89ab][0-9a-f]:/i.test(normalized)) return true;
  return normalized.endsWith('.local') || normalized.endsWith('.lan') || normalized.endsWith('.home.arpa');
}

export function classifyEndpoint(value) {
  let url;
  try { url = new URL(value); } catch { return { allowed: false, kind: 'invalid', reason: 'The endpoint URL is malformed.' }; }
  if (url.protocol === 'engram:') {
    if (!isNativeAppEndpoint(url)) return { allowed: false, kind: 'invalid', reason: 'Only engram://app is permitted for native in-process transport.' };
    if (url.username || url.password) return { allowed: false, kind: 'invalid', reason: 'Endpoint userinfo is not permitted.' };
    return { allowed: true, kind: 'local', origin: 'engram://app', url };
  }
  const host = url.hostname.toLowerCase();
  const loopback = isLoopbackHost(host);
  const privateNetwork = isPrivateNetworkHost(host);
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return { allowed: false, kind: 'invalid', reason: 'Only engram, HTTP, or HTTPS endpoints are supported.' };
  if (url.username || url.password) return { allowed: false, kind: 'invalid', reason: 'Endpoint userinfo is not permitted.' };
  if (!loopback && !privateNetwork && url.protocol !== 'https:') return { allowed: false, kind: 'invalid', reason: 'Remote inference requires HTTPS.' };
  return { allowed: true, kind: loopback ? 'local' : 'remote', origin: url.origin, url };
}

export function canSendVaultContext(endpointValue, enabled) {
  const endpoint = classifyEndpoint(endpointValue);
  return Boolean(enabled && endpoint.allowed && (endpoint.kind === 'local' || isPrivateNetworkHost(endpoint.url.hostname)));
}

export function supportsFunctionTools({ model, endpointOrigin, appOrigin }) {
  return !(model === 'apple-on-device' && endpointOrigin === appOrigin);
}

export function requestCategories({ messages = [], tools = [] } = {}) {
  const categories = [];
  if (messages.some(item => item.role === 'user')) categories.push('current message');
  if (messages.some(item => item.role === 'system')) categories.push('system/custom instructions');
  if (messages.some(item => item.role === 'user' && item.content !== messages.at(-1)?.content)) categories.push('recent turns');
  if (messages.some(item => typeof item.content === 'string' && /memory|assertion|decision/i.test(item.content))) categories.push('derived memory');
  if (messages.some(item => item.role === 'tool')) categories.push('local project files or tool results');
  if (messages.some(item => typeof item.content === 'string' && /\[.*?\.md\]/.test(item.content))) categories.push('retrieved history or vault snippets');
  if (tools.length) categories.push('registered tool definitions and possible tool results');
  return [...new Set(categories)];
}

export function disclosureText({ endpoint, apiKey, categories = [], chars = 0, discovery = false }) {
  return [`Destination: ${endpoint}`, `API key: ${apiKey ? 'sent in Authorization header' : 'not sent'}`, discovery ? 'Request content: endpoint metadata and model-discovery parameters only.' : `Data categories: ${categories.length ? categories.join(', ') : 'none declared'}`, chars ? `Estimated request characters: ${chars.toLocaleString()}` : 'Estimated request characters: unavailable'].join('\n');
}

export function confirmDisclosure(details) {
  const dialog = document.querySelector('#remote-consent');
  if (!dialog) return Promise.resolve(false);
  document.querySelector('#remote-consent-body').textContent = disclosureText(details);
  return new Promise(resolve => { dialog.onclose = () => resolve(dialog.returnValue === 'allow'); dialog.showModal(); });
}
