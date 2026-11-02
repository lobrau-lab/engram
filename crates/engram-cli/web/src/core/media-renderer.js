const ALLOWED_MEDIA_EXTENSIONS = new Set([
  'png', 'jpg', 'jpeg', 'gif', 'webp', 'mp4', 'webm', 'mov', 'wav', 'mp3', 'ogg'
]);

/**
 * Render Markdown media references (`![alt](/media/...)` and `[/media/...]`)
 * into secure HTML elements with attribute escaping and strict extension allowlisting.
 */
export function renderTurnContent(rawContent, esc) {
  if (!rawContent) return '';
  const escapeFn = typeof esc === 'function'
    ? esc
    : (value => String(value ?? '').replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char])));
  let safe = escapeFn(rawContent);

  // Strict regex: allows only single-component alphanumeric filename with whitelisted extension
  // Reject path traversals, internal slashes, script injections, or unknown extensions
  const markdownImgRegex = /!\[([^\]]*)\]\((\/media\/([a-zA-Z0-9_-]+\.([a-zA-Z0-9]+)))\)/g;
  safe = safe.replace(markdownImgRegex, (_match, alt, _url, filename, ext) => {
    const lowerExt = ext.toLowerCase();
    if (!ALLOWED_MEDIA_EXTENSIONS.has(lowerExt)) return _match;
    const cleanAlt = escapeFn(alt || '');
    const cleanUrl = `/media/${encodeURIComponent(filename)}`;
    if (['mp4', 'webm', 'mov'].includes(lowerExt)) {
      return `<video src="${cleanUrl}" controls style="max-width:100%;border-radius:8px;margin:8px 0;display:block;background:#000;"></video>`;
    }
    if (['wav', 'mp3', 'ogg'].includes(lowerExt)) {
      return `<audio src="${cleanUrl}" controls style="width:100%;margin:8px 0;display:block;"></audio>`;
    }
    return `<img src="${cleanUrl}" alt="${cleanAlt}" style="max-width:100%;border-radius:8px;margin:8px 0;display:block;" />`;
  });

  const bracketedRegex = /\[(\/media\/([a-zA-Z0-9_-]+\.([a-zA-Z0-9]+)))\]/g;
  safe = safe.replace(bracketedRegex, (_match, _url, filename, ext) => {
    const lowerExt = ext.toLowerCase();
    if (!ALLOWED_MEDIA_EXTENSIONS.has(lowerExt)) return _match;
    const cleanUrl = `/media/${encodeURIComponent(filename)}`;
    if (['mp4', 'webm', 'mov'].includes(lowerExt)) {
      return `<video src="${cleanUrl}" controls style="max-width:100%;border-radius:8px;margin:8px 0;display:block;background:#000;"></video>`;
    }
    if (['wav', 'mp3', 'ogg'].includes(lowerExt)) {
      return `<audio src="${cleanUrl}" controls style="width:100%;margin:8px 0;display:block;"></audio>`;
    }
    return `<img src="${cleanUrl}" alt="" style="max-width:100%;border-radius:8px;margin:8px 0;display:block;" />`;
  });

  return safe.replace(/\n/g, '<br>');
}
