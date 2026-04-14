// OpenHuman webview-accounts recipe runtime.
// Injected via WebviewBuilder.initialization_script BEFORE page JS runs.
// Exposes a small `window.__openhumanRecipe` API that per-provider recipes
// use to scrape DOM state, intercept WebSocket traffic, and drive a
// ghost-text autocomplete overlay on the provider's message composer.
//
// Runs in the loaded service's origin (e.g. https://web.whatsapp.com).
// IPC back to Rust uses Tauri's `window.__TAURI_INTERNALS__.invoke`,
// which Tauri auto-injects into every webview it controls (including
// child webviews on external origins).
//
// Event kinds emitted to Rust via `webview_recipe_event`:
//   log              { level, msg }
//   notify           { title, options }
//   ingest           { messages, unread?, snapshotKey? }      (recipe-driven)
//   ws_message       { direction:'in'|'out', kind, data, url, size, ts }
//   ws_open          { url, ts }
//   ws_close         { url, code, reason, ts }
//   composer_attach  { composerId, providerHint? }
//   composer_input   { composerId, text, cursor, ts }
//   composer_commit  { composerId, text, source:'tab'|'api', ts }
//   composer_dismiss { composerId, reason }
//
// Rust → page (via `webview.eval(...)`) calls these globals:
//   __openhumanRecipe.setSuggestion(composerId, text)
//   __openhumanRecipe.clearSuggestion(composerId)
//   __openhumanRecipe.commitSuggestion(composerId)
//   __openhumanRecipe.runScript(jsString)        // escape hatch
(function () {
  if (window.__openhumanRecipe) return;

  const ctx = window.__OPENHUMAN_RECIPE_CTX__ || { accountId: 'unknown', provider: 'unknown' };
  const POLL_MS = 2000;

  // Cap the size of WS payloads we forward — Telegram / WhatsApp ship
  // large encrypted blobs we can't decode anyway, and dragging huge buffers
  // through Tauri IPC will stall the UI thread.
  const WS_MAX_FORWARD_BYTES = 16 * 1024;

  function rawInvoke(cmd, payload) {
    try {
      const inv = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
      if (typeof inv !== 'function') return Promise.resolve();
      return inv(cmd, payload || {});
    } catch (e) {
      // swallow — never let a bad invoke break the host page
      return Promise.resolve();
    }
  }

  function send(kind, payload) {
    return rawInvoke('webview_recipe_event', {
      accountId: ctx.accountId,
      provider: ctx.provider,
      kind: kind,
      payload: payload || {},
      ts: Date.now(),
    });
  }

  let loopFn = null;
  let pollTimer = null;
  let notifyHandler = null;

  function safeRunLoop() {
    if (!loopFn) return;
    try {
      loopFn(api);
    } catch (e) {
      send('log', { level: 'warn', msg: '[recipe] loop threw: ' + (e && e.message ? e.message : String(e)) });
    }
  }

  // ─── Notification patch ───────────────────────────────────────────────
  // Many web messengers use the Notification API for new-message pings;
  // we forward them so recipes can react without polling.
  try {
    const NativeNotification = window.Notification;
    if (NativeNotification && !NativeNotification.__openhumanPatched) {
      function PatchedNotification(title, options) {
        try {
          if (notifyHandler) {
            notifyHandler({ title: title, options: options || {} });
          }
          send('notify', { title: title, options: options || {} });
        } catch (_) {}
        return new NativeNotification(title, options);
      }
      PatchedNotification.prototype = NativeNotification.prototype;
      PatchedNotification.permission = NativeNotification.permission;
      PatchedNotification.requestPermission = NativeNotification.requestPermission.bind(NativeNotification);
      PatchedNotification.__openhumanPatched = true;
      window.Notification = PatchedNotification;
    }
  } catch (_) {
    // Notification API not available — fine
  }

  // ─── WebSocket interception ──────────────────────────────────────────
  // We patch `window.WebSocket` early (before the page boots) so we capture
  // every socket the provider opens. Emission to Rust is gated behind
  // `api.observeWebSocket()` so recipes can opt in only after they're sure
  // the chat UI is loaded — keeps noise (auth/handshake frames) down.
  let wsObserve = false;
  let wsFilter = null;
  const wsRegistry = new WeakSet();

  function classify(data) {
    if (typeof data === 'string') return 'text';
    if (data instanceof ArrayBuffer) return 'arraybuffer';
    if (typeof Blob !== 'undefined' && data instanceof Blob) return 'blob';
    if (data && typeof data.byteLength === 'number') return 'arraybufferview';
    return 'unknown';
  }

  function sizeOf(data) {
    if (typeof data === 'string') return data.length;
    if (data instanceof ArrayBuffer) return data.byteLength;
    if (typeof Blob !== 'undefined' && data instanceof Blob) return data.size;
    if (data && typeof data.byteLength === 'number') return data.byteLength;
    return 0;
  }

  function serializeForForward(data, kind) {
    if (kind === 'text') {
      const s = String(data);
      return s.length > WS_MAX_FORWARD_BYTES ? s.slice(0, WS_MAX_FORWARD_BYTES) : s;
    }
    // Binary frames — return null; recipes that care can decode in JS first
    // (provider-specific protobuf) and re-emit text via api.emitWebSocket().
    return null;
  }

  function shouldForward(frame) {
    if (!wsObserve) return false;
    if (typeof wsFilter !== 'function') return true;
    try { return !!wsFilter(frame); } catch (_) { return false; }
  }

  function forwardFrame(frame) {
    if (!shouldForward(frame)) return;
    send('ws_message', frame);
  }

  try {
    const NativeWS = window.WebSocket;
    if (NativeWS && !NativeWS.__openhumanPatched) {
      function PatchedWS(url, protocols) {
        const sock = protocols === undefined
          ? new NativeWS(url)
          : new NativeWS(url, protocols);
        wsRegistry.add(sock);
        try { send('ws_open', { url: String(url) }); } catch (_) {}

        const nativeSend = sock.send.bind(sock);
        sock.send = function (data) {
          try {
            const kind = classify(data);
            const size = sizeOf(data);
            forwardFrame({
              direction: 'out',
              kind: kind,
              data: serializeForForward(data, kind),
              url: String(url),
              size: size,
              ts: Date.now(),
            });
          } catch (_) {}
          return nativeSend(data);
        };

        sock.addEventListener('message', function (ev) {
          try {
            const kind = classify(ev.data);
            const size = sizeOf(ev.data);
            forwardFrame({
              direction: 'in',
              kind: kind,
              data: serializeForForward(ev.data, kind),
              url: String(url),
              size: size,
              ts: Date.now(),
            });
          } catch (_) {}
        });
        sock.addEventListener('close', function (ev) {
          try { send('ws_close', { url: String(url), code: ev.code, reason: ev.reason }); } catch (_) {}
        });
        return sock;
      }
      PatchedWS.prototype = NativeWS.prototype;
      PatchedWS.CONNECTING = NativeWS.CONNECTING;
      PatchedWS.OPEN = NativeWS.OPEN;
      PatchedWS.CLOSING = NativeWS.CLOSING;
      PatchedWS.CLOSED = NativeWS.CLOSED;
      PatchedWS.__openhumanPatched = true;
      window.WebSocket = PatchedWS;
    }
  } catch (_) {
    // WebSocket missing — fine, nothing to patch.
  }

  // ─── Composer / ghost-text autocomplete ───────────────────────────────
  // Recipes call `api.attachComposer(element, opts)` to wire the chat
  // input box. We:
  //   - debounce input events and emit `composer_input` to Rust
  //   - render a grey suggestion span at the caret (contenteditable) or
  //     overlay (input/textarea) when Rust calls setSuggestion()
  //   - on Tab (or `opts.suggestionKey`), insert the suggestion as if the
  //     user had typed it, dispatching the platform's expected events so
  //     the host framework's reactivity sees the change
  const composers = Object.create(null);

  const GHOST_STYLE = 'pointer-events:none;user-select:none;color:rgba(120,120,120,0.65);' +
                      'white-space:pre-wrap;font:inherit;letter-spacing:inherit;';

  function getComposerText(el) {
    if (!el) return '';
    if (el.isContentEditable) return el.innerText || '';
    if ('value' in el) return el.value || '';
    return el.textContent || '';
  }

  function getComposerCursor(el) {
    if (!el) return 0;
    if ('selectionStart' in el && el.selectionStart != null) return el.selectionStart;
    try {
      const sel = window.getSelection();
      if (sel && sel.rangeCount) {
        const range = sel.getRangeAt(0);
        const pre = range.cloneRange();
        pre.selectNodeContents(el);
        pre.setEnd(range.endContainer, range.endOffset);
        return pre.toString().length;
      }
    } catch (_) {}
    return getComposerText(el).length;
  }

  function detachGhost(state) {
    if (state.ghostNode && state.ghostNode.parentNode) {
      state.ghostNode.parentNode.removeChild(state.ghostNode);
    }
    state.ghostNode = null;
  }

  function renderGhostInline(state) {
    detachGhost(state);
    if (!state.suggestion) return;
    const el = state.el;
    const ghost = document.createElement('span');
    ghost.setAttribute('data-openhuman-ghost', state.composerId);
    ghost.setAttribute('contenteditable', 'false');
    ghost.style.cssText = GHOST_STYLE;
    ghost.textContent = state.suggestion;
    state.ghostNode = ghost;

    if (el.isContentEditable) {
      // Append at the end of the editable surface. Most chat composers have
      // a single contenteditable line/paragraph, so end-append is a sane
      // default. Recipes can override by passing opts.placeGhost(el, ghost).
      try {
        if (typeof state.placeGhost === 'function') {
          state.placeGhost(el, ghost);
        } else {
          el.appendChild(ghost);
        }
      } catch (_) {}
    } else {
      // For input/textarea we can't inject a child; mount an overlay sibling.
      try {
        if (!state.overlay) {
          const overlay = document.createElement('div');
          overlay.setAttribute('data-openhuman-ghost-overlay', state.composerId);
          overlay.style.cssText = 'position:absolute;pointer-events:none;z-index:9999;';
          state.overlay = overlay;
          // Position over the input.
          const rect = el.getBoundingClientRect();
          overlay.style.left = rect.left + window.scrollX + 'px';
          overlay.style.top = rect.top + window.scrollY + 'px';
          overlay.style.width = rect.width + 'px';
          overlay.style.height = rect.height + 'px';
          overlay.style.padding = window.getComputedStyle(el).padding;
          overlay.style.font = window.getComputedStyle(el).font;
          document.body.appendChild(overlay);
        }
        // Render typed text invisibly + ghost suffix.
        state.overlay.textContent = '';
        const filler = document.createElement('span');
        filler.style.cssText = 'visibility:hidden;white-space:pre-wrap;';
        filler.textContent = getComposerText(el);
        state.overlay.appendChild(filler);
        state.overlay.appendChild(ghost);
      } catch (_) {}
    }
  }

  function clearSuggestionState(state, reason) {
    if (!state.suggestion) return;
    state.suggestion = '';
    detachGhost(state);
    if (state.overlay && state.overlay.parentNode) {
      state.overlay.parentNode.removeChild(state.overlay);
      state.overlay = null;
    }
    if (reason) send('composer_dismiss', { composerId: state.composerId, reason: reason });
  }

  function commitSuggestion(state, source) {
    if (!state.suggestion) return false;
    const text = state.suggestion;
    clearSuggestionState(state, null);
    const el = state.el;
    try {
      el.focus();
      if (el.isContentEditable) {
        // execCommand is deprecated but still the most reliable way to
        // insert text into a contenteditable composer in a way that the
        // host framework (React/Vue/Lit) sees as a real user edit.
        const ok = document.execCommand && document.execCommand('insertText', false, text);
        if (!ok) {
          const sel = window.getSelection();
          if (sel && sel.rangeCount) {
            const r = sel.getRangeAt(0);
            r.deleteContents();
            r.insertNode(document.createTextNode(text));
            r.collapse(false);
          } else {
            el.appendChild(document.createTextNode(text));
          }
          el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
        }
      } else if ('value' in el) {
        const start = el.selectionStart != null ? el.selectionStart : el.value.length;
        const end = el.selectionEnd != null ? el.selectionEnd : start;
        el.value = el.value.slice(0, start) + text + el.value.slice(end);
        const caret = start + text.length;
        try { el.setSelectionRange(caret, caret); } catch (_) {}
        el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
      }
    } catch (e) {
      send('log', { level: 'warn', msg: '[composer] commit failed: ' + (e && e.message ? e.message : String(e)) });
    }
    send('composer_commit', { composerId: state.composerId, text: text, source: source || 'api' });
    return true;
  }

  function attachComposer(el, opts) {
    if (!el) {
      send('log', { level: 'warn', msg: '[composer] attachComposer called with null element' });
      return null;
    }
    opts = opts || {};
    const composerId = opts.id || ('composer-' + Math.random().toString(36).slice(2, 10));
    const debounceMs = typeof opts.debounceMs === 'number' ? opts.debounceMs : 200;
    const suggestionKey = opts.suggestionKey || 'Tab';

    if (composers[composerId]) {
      // Re-attach: tear down old listeners first.
      try { composers[composerId].detach(); } catch (_) {}
    }

    const state = {
      composerId: composerId,
      el: el,
      suggestion: '',
      ghostNode: null,
      overlay: null,
      placeGhost: typeof opts.placeGhost === 'function' ? opts.placeGhost : null,
      inputTimer: null,
    };

    function onInput() {
      // Any user keystroke invalidates the current suggestion.
      clearSuggestionState(state, 'user-input');
      clearTimeout(state.inputTimer);
      state.inputTimer = setTimeout(function () {
        send('composer_input', {
          composerId: composerId,
          text: getComposerText(el),
          cursor: getComposerCursor(el),
        });
      }, debounceMs);
    }

    function onKeyDown(ev) {
      if (!state.suggestion) return;
      if (ev.key === suggestionKey) {
        ev.preventDefault();
        ev.stopPropagation();
        commitSuggestion(state, 'tab');
      } else if (ev.key === 'Escape') {
        clearSuggestionState(state, 'escape');
      }
    }

    function onBlur() { clearSuggestionState(state, 'blur'); }

    el.addEventListener('input', onInput);
    el.addEventListener('keydown', onKeyDown, true);
    el.addEventListener('blur', onBlur);

    const handle = {
      composerId: composerId,
      element: el,
      setSuggestion: function (text) {
        state.suggestion = text == null ? '' : String(text);
        renderGhostInline(state);
      },
      clear: function () { clearSuggestionState(state, 'api'); },
      commit: function () { return commitSuggestion(state, 'api'); },
      detach: function () {
        clearSuggestionState(state, 'detach');
        clearTimeout(state.inputTimer);
        try { el.removeEventListener('input', onInput); } catch (_) {}
        try { el.removeEventListener('keydown', onKeyDown, true); } catch (_) {}
        try { el.removeEventListener('blur', onBlur); } catch (_) {}
        delete composers[composerId];
      },
    };
    composers[composerId] = handle;
    send('composer_attach', { composerId: composerId, providerHint: opts.providerHint || null });
    return handle;
  }

  // ─── Public API ───────────────────────────────────────────────────────
  const api = {
    loop(fn) {
      loopFn = fn;
      if (pollTimer) clearInterval(pollTimer);
      pollTimer = setInterval(safeRunLoop, POLL_MS);
      // also kick once on next tick so we don't wait POLL_MS for the first call
      setTimeout(safeRunLoop, 250);
      send('log', { level: 'info', msg: '[recipe] loop registered, polling every ' + POLL_MS + 'ms' });
    },
    ingest(payload) {
      // payload: { messages: Array<{id?, from?, body, ts?}>, unread?, snapshotKey? }
      send('ingest', payload || {});
    },
    log(level, msg) {
      send('log', { level: level || 'info', msg: String(msg) });
    },
    onNotify(fn) {
      notifyHandler = fn;
    },
    context() {
      return Object.assign({}, ctx);
    },

    // WebSocket
    observeWebSocket(opts) {
      opts = opts || {};
      wsObserve = true;
      wsFilter = typeof opts.filter === 'function' ? opts.filter : null;
      send('log', { level: 'info', msg: '[recipe] websocket observation enabled' });
    },
    stopObservingWebSocket() {
      wsObserve = false;
      wsFilter = null;
    },
    /** Manually emit a normalized ws frame after recipe-side decoding. */
    emitWebSocket(frame) {
      if (!frame) return;
      send('ws_message', Object.assign({ direction: 'in', kind: 'text', ts: Date.now() }, frame));
    },

    // Composer
    attachComposer: attachComposer,
    setSuggestion(composerId, text) {
      const h = composers[composerId];
      if (!h) return false;
      h.setSuggestion(text);
      return true;
    },
    clearSuggestion(composerId) {
      const h = composers[composerId];
      if (!h) return false;
      h.clear();
      return true;
    },
    commitSuggestion(composerId) {
      const h = composers[composerId];
      if (!h) return false;
      return h.commit();
    },
    listComposers() {
      return Object.keys(composers);
    },

    // Escape hatch — used by Rust when it wants to run arbitrary recipe
    // helpers without round-tripping through a typed command.
    runScript(js) {
      try { return (new Function(js))(); } catch (e) {
        send('log', { level: 'error', msg: '[recipe] runScript threw: ' + (e && e.message ? e.message : String(e)) });
        return null;
      }
    },
  };

  window.__openhumanRecipe = api;
  send('log', { level: 'info', msg: '[recipe-runtime] ready provider=' + ctx.provider + ' accountId=' + ctx.accountId });
})();
