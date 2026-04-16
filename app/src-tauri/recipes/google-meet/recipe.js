// Google Meet recipe. Reports whether the user is in a call, the meeting
// code, and — when captions are turned on — a rolling snapshot of live
// caption lines.
//
// Meet's DOM uses heavily obfuscated class names, so we lean on stable
// hooks: the URL (meeting code lives in the path), jsname attributes on
// caption regions, and aria labels on the participant count chip. These
// break less often than CSS classes but are not fully stable either —
// expect periodic maintenance.
(function (api) {
  if (!api) return;
  api.log('info', '[google-meet-recipe] starting');

  // Meet call URLs look like `/xxx-xxxx-xxx`. Landing / lobby pages use
  // other paths (`/landing`, `/new`, `/meeting-diagnostics`, …).
  const MEETING_CODE_RE = /^\/([a-z]{3,4}-[a-z]{3,4}-[a-z]{3,4})(?:$|\/|\?)/i;

  let last = '';

  function textOf(el) {
    return (el && el.textContent ? el.textContent : '').trim();
  }

  function meetingCode() {
    try {
      const m = MEETING_CODE_RE.exec(window.location.pathname || '');
      return m ? m[1] : null;
    } catch (_) {
      return null;
    }
  }

  function participantCount() {
    // The "People" chip in the bottom bar renders as an aria-label like
    // "Show everyone (3)". Fall back to counting video tiles if the chip
    // isn't present (e.g. presenter view).
    try {
      const chips = document.querySelectorAll('[aria-label]');
      for (let i = 0; i < chips.length; i++) {
        const label = chips[i].getAttribute('aria-label') || '';
        const m = /\((\d+)\)/.exec(label);
        if (m && /people|everyone|participant/i.test(label)) {
          const n = parseInt(m[1], 10);
          if (!Number.isNaN(n)) return n;
        }
      }
      const tiles = document.querySelectorAll('[data-participant-id], [data-self-name]');
      if (tiles && tiles.length) return tiles.length;
    } catch (_) {}
    return null;
  }

  function captionLines() {
    // Live captions render inside a region with jsname="tgaKEf" (stable
    // for years). Each speaker's rolling text is one child div.
    const lines = [];
    try {
      const region = document.querySelector('[jsname="tgaKEf"], [aria-label*="aptions" i]');
      if (!region) return lines;
      const entries = region.querySelectorAll('div');
      entries.forEach(function (el) {
        const t = textOf(el);
        if (t && lines.indexOf(t) === -1) lines.push(t);
      });
    } catch (_) {}
    return lines.slice(-20); // keep it bounded — only the last 20 lines
  }

  api.loop(function () {
    const code = meetingCode();
    const inCall = code != null;
    const participants = inCall ? participantCount() : null;
    const captions = inCall ? captionLines() : [];

    const messages = [];
    if (inCall) {
      messages.push({
        id: 'gm-call:' + code,
        from: 'Google Meet',
        body:
          'In call ' +
          code +
          (participants != null ? ' — ' + participants + ' participant(s)' : ''),
        unread: 0,
      });
      captions.forEach(function (line, idx) {
        messages.push({
          id: 'gm-caption:' + code + ':' + idx,
          from: 'caption',
          body: line,
          unread: 0,
        });
      });
    }

    const key = JSON.stringify({
      code: code,
      n: participants,
      c: captions.length,
      tail: captions.slice(-3),
    });
    if (key === last) return;
    last = key;

    api.ingest({
      messages: messages,
      unread: 0,
      snapshotKey: key,
      meetingCode: code,
      inCall: inCall,
      participantCount: participants,
    });
  });
})(window.__openhumanRecipe);
