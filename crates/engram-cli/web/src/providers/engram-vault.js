export async function searchEngramVault(query, { signal } = {}) {
  if (window.location.protocol === 'file:') {
    throw new Error('Open Threadline through the Engram local workspace to search its vault.');
  }
  const endpoint = new URL('/api/v1/vault/search', window.location.origin);
  endpoint.searchParams.set('q', query);
  const response = await fetch(endpoint, { headers: { Accept: 'application/json' }, signal });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(payload.error || `Engram vault search failed (${response.status}).`);
  return payload;
}
