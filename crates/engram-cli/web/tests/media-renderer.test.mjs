import assert from 'node:assert/strict';
import { renderTurnContent } from '../src/core/media-renderer.js';

const esc = value => String(value ?? '').replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]));

// 1. Valid image markdown
const imgRender = renderTurnContent('Here is the image: ![Cyber Sunrise](/media/render-123.png)', esc);
assert.ok(imgRender.includes('<img src="/media/render-123.png" alt="Cyber Sunrise"'), `imgRender: ${imgRender}`);

// 2. Valid video bracketed syntax
const videoRender = renderTurnContent('Generated clip: [/media/shot-456.mp4]', esc);
assert.ok(videoRender.includes('<video src="/media/shot-456.mp4" controls'), `videoRender: ${videoRender}`);

// 3. Valid audio bracketed syntax
const audioRender = renderTurnContent('Voiceover: [/media/voice-789.wav]', esc);
assert.ok(audioRender.includes('<audio src="/media/voice-789.wav" controls'), `audioRender: ${audioRender}`);

// 4. Traversal attempt [/media/../../etc/passwd]
const traversalRender = renderTurnContent('Check this: [/media/../../etc/passwd]', esc);
assert.ok(!traversalRender.includes('<video') && !traversalRender.includes('<img') && !traversalRender.includes('<audio'));
assert.ok(traversalRender.includes('[/media/../../etc/passwd]'));

// 5. XSS attempt [/media/x" onerror=alert(1)]
const xssRender = renderTurnContent('Check this: [/media/x" onerror=alert(1).png]', esc);
assert.ok(!xssRender.includes('<img') && !xssRender.includes('<video') && !xssRender.includes('<audio'));
assert.ok(xssRender.includes('&quot; onerror=alert(1).png]'));

// 6. JavaScript protocol attempt ![evil](javascript:alert(1))
const jsProtocolRender = renderTurnContent('Click: ![evil](javascript:alert(1))', esc);
assert.ok(!jsProtocolRender.includes('<img') && !jsProtocolRender.includes('<video'));

// 7. Unwhitelisted extension
const unknownExtRender = renderTurnContent('Document: [/media/secret.exe]', esc);
assert.ok(!unknownExtRender.includes('<img') && !unknownExtRender.includes('<video') && !unknownExtRender.includes('<audio'));
assert.ok(unknownExtRender.includes('[/media/secret.exe]'));

console.log('media-renderer sanitization tests passed');
