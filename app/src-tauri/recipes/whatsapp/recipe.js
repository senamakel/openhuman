// WhatsApp Web recipe.
// Runs inside web.whatsapp.com after the runtime injects the API.
//
// v1 strategy: every poll, walk the chat list pane (`#pane-side`) and
// snapshot the visible conversation rows — name + last-message preview +
// unread badge. This is intentionally minimal — we just want to prove the
// end-to-end pipe (DOM scrape → Tauri IPC → React UI → core memory).
(function (api) {
  if (!api) return;
  api.log('info', '[whatsapp-recipe] starting');

  let lastSnapshot = '';

  function textOf(el) {
    return (el && el.textContent ? el.textContent : '').trim();
  }

  function scrapeChatList() {
    const pane = document.querySelector('#pane-side');
    if (!pane) return null;

    // WhatsApp Web renders rows with role="listitem" inside the pane.
    const rows = pane.querySelectorAll('div[role="listitem"]');
    const messages = [];
    let unread = 0;

    rows.forEach((row, idx) => {
      // Title (chat name) — typically the first heavy <span> with a title attr
      const titleEl =
        row.querySelector('span[title]') ||
        row.querySelector('span[dir="auto"][aria-label]') ||
        row.querySelector('span');
      const name = textOf(titleEl);

      // Last-message preview line
      const previewEl =
        row.querySelector('span[dir="ltr"]') ||
        row.querySelector('div[role="gridcell"] span[dir="auto"]');
      const preview = textOf(previewEl);

      // Unread badge — span with aria-label like "3 unread messages"
      const badgeEl = row.querySelector('span[aria-label*="unread"]');
      const badgeText = textOf(badgeEl);
      const badgeNum = parseInt(badgeText, 10);
      if (!Number.isNaN(badgeNum)) unread += badgeNum;

      if (name || preview) {
        messages.push({
          id: name ? 'wa:' + name : 'wa:row:' + idx,
          from: name || null,
          body: preview || null,
          unread: !Number.isNaN(badgeNum) ? badgeNum : 0,
        });
      }
    });

    return { messages, unread };
  }

  // ─── Composer attach (ghost-text autocomplete) ──────────────────────
  // WhatsApp Web's input is a contenteditable div carrying
  // `data-tab="10"` and an `aria-label` of "Type a message" (locale-
  // dependent). We try a couple selectors and reattach if the user
  // navigates to a new chat (the composer node is re-mounted on switch).
  let attachedComposerEl = null;
  let attachedHandle = null;

  function findComposer() {
    return document.querySelector('div[contenteditable="true"][data-tab="10"]')
      || document.querySelector('footer div[contenteditable="true"][role="textbox"]')
      || document.querySelector('div[contenteditable="true"][data-lexical-editor="true"]');
  }

  function ensureComposerAttached() {
    const el = findComposer();
    if (!el) return;
    if (el === attachedComposerEl) return;
    if (attachedHandle) {
      try { attachedHandle.detach(); } catch (_) {}
    }
    attachedComposerEl = el;
    attachedHandle = api.attachComposer(el, {
      id: 'whatsapp:composer',
      providerHint: 'whatsapp',
      debounceMs: 250,
      suggestionKey: 'Tab',
    });
    api.log('info', '[whatsapp-recipe] composer attached');
  }

  // ─── WebSocket observation ──────────────────────────────────────────
  // WhatsApp's WS frames are Noise-encrypted protobuf — useless raw —
  // but we still emit `ws_open` / `ws_close` so the core can correlate
  // session lifecycle. Recipes that care can tighten the filter later.
  api.observeWebSocket({
    filter: function (frame) {
      // Only forward textual frames (rare on WhatsApp); drop binary noise.
      return frame.kind === 'text';
    },
  });

  api.loop(function () {
    ensureComposerAttached();

    const snap = scrapeChatList();
    if (!snap) {
      // Likely still on the QR-login screen
      return;
    }

    // Cheap dedup: only ingest when the snapshot changes between polls.
    const key = JSON.stringify({
      n: snap.messages.length,
      u: snap.unread,
      first: snap.messages.slice(0, 5).map(function (m) { return m.from + '|' + m.body + '|' + m.unread; }),
    });
    if (key === lastSnapshot) return;
    lastSnapshot = key;

    api.ingest({
      messages: snap.messages,
      unread: snap.unread,
      snapshotKey: key,
    });
  });

  api.onNotify(function (n) {
    api.log('info', '[whatsapp-recipe] notify: ' + (n && n.title ? n.title : ''));
  });
})(window.__openhumanRecipe);
