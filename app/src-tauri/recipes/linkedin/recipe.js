// LinkedIn Messaging recipe v0.3
// Scrapes the conversation list, active thread, and connection requests.
// Emits per-conversation-per-day memory ingest events following the
// WhatsApp pattern so the recall pipeline gets stable, upsertable docs.
(function (api) {
  if (!api) return;
  api.log('info', '[linkedin-recipe] v0.3 starting');

  // ── helpers ────────────────────────────────────────────────────────────────

  function textOf(el) {
    return (el && el.textContent ? el.textContent : '').trim();
  }

  function isoDay(ms) {
    return new Date(ms).toISOString().slice(0, 10);
  }

  // Convert LinkedIn relative timestamps ("2h", "3d", "1w") to epoch ms.
  function parseRelativeTime(text) {
    if (!text) return null;
    var t = text.trim().toLowerCase();
    var m = t.match(/^(\d+)\s*([smhdw])/);
    if (!m) return null;
    var n = parseInt(m[1], 10);
    var units = { s: 1000, m: 60000, h: 3600000, d: 86400000, w: 604800000 };
    return Date.now() - n * (units[m[2]] || 0);
  }

  // Extract a stable conversation ID from a LinkedIn href.
  // Handles both /messaging/thread/2-xxx/ and /messaging/conversations/2-xxx
  function chatIdFromHref(href) {
    if (!href) return null;
    var m = href.match(/(?:thread|conversation(?:s)?)[/=]([^/?#&]+)/i);
    return m ? m[1] : null;
  }

  // ── conversation list ──────────────────────────────────────────────────────

  var lastListKey = '';
  var prevUnread = {}; // chatId -> last seen unread count

  function scrapeConversationList() {
    var rows = document.querySelectorAll([
      'li.msg-conversation-listitem',
      '.msg-conversations-container__pillar li',
      '.scaffold-layout__list-container li[data-id]',
      '.msg-conversations-container li',
    ].join(', '));
    if (!rows || rows.length === 0) return null;

    var items = [];
    rows.forEach(function (row, idx) {
      // Participant name — multiple fallbacks for selector churn resilience
      var nameEl =
        row.querySelector('.msg-conversation-listitem__participant-names') ||
        row.querySelector('.msg-conversation-card__participant-names') ||
        row.querySelector('.msg-conversation-card__title') ||
        row.querySelector('[data-control-name="overlay.open_conversation"] span') ||
        row.querySelector('h3') ||
        row.querySelector('h4');

      // Message snippet / preview
      var previewEl =
        row.querySelector('.msg-conversation-card__message-snippet') ||
        row.querySelector('.msg-conversation-listitem__message-snippet') ||
        row.querySelector('.msg-conversation-card__message-snippet-body') ||
        row.querySelector('[class*="conversation-card__message"]') ||
        row.querySelector('[class*="message-snippet"]');

      // Unread badge
      var unreadEl = row.querySelector([
        '.notification-badge__count',
        '.msg-conversation-card__unread-count',
        '[class*="unread-count"]',
        '[class*="badge__count"]',
      ].join(', '));

      // Timestamp
      var timeEl = row.querySelector([
        '.msg-conversation-card__time-stamp',
        '.msg-conversation-listitem__time-stamp',
        'time',
        '[class*="time-stamp"]',
      ].join(', '));

      // Conversation link → stable chat ID
      var linkEl = row.querySelector('a[href*="messaging"], a[href*="conversation"]');
      var href = linkEl ? linkEl.getAttribute('href') : null;
      var chatId = chatIdFromHref(href);

      var name = textOf(nameEl);
      var preview = textOf(previewEl);
      var unreadNum = parseInt(textOf(unreadEl), 10);
      var unread = Number.isNaN(unreadNum) ? 0 : unreadNum;
      var timeText = textOf(timeEl);
      var approxTs = parseRelativeTime(timeText);

      if (name || preview) {
        items.push({
          chatId: chatId || ('li:' + (name || idx)),
          name: name || null,
          preview: preview || null,
          unread: unread,
          timeText: timeText || null,
          ts: approxTs,
        });
      }
    });
    return items;
  }

  // ── active thread reading ──────────────────────────────────────────────────

  var lastThreadKey = '';

  function getActiveChatId() {
    var m = location.href.match(/(?:thread|conversation(?:s)?)[/=]([^/?#&]+)/i);
    return m ? m[1] : null;
  }

  function scrapeActiveThread() {
    var chatId = getActiveChatId();
    if (!chatId) return null;

    var events = document.querySelectorAll([
      '.msg-s-event-listitem',
      '.msg-s-message-list__event',
      '[class*="s-event-listitem"]',
    ].join(', '));
    if (!events || events.length === 0) return null;

    var msgs = [];
    events.forEach(function (ev) {
      var bodyEl =
        ev.querySelector('.msg-s-event-listitem__body') ||
        ev.querySelector('.msg-s-event-listitem__message-text') ||
        ev.querySelector('[class*="event-listitem__body"]');
      var senderEl =
        ev.querySelector('.msg-s-event-listitem__sender') ||
        ev.querySelector('.msg-s-event-listitem__author') ||
        ev.querySelector('[class*="event-listitem__sender"]');
      var timeEl =
        ev.querySelector('.msg-s-message-list-content__timestamp') ||
        ev.querySelector('time') ||
        ev.querySelector('[class*="timestamp"]');

      var body = textOf(bodyEl);
      if (!body) return;

      var sender = textOf(senderEl);
      var timeAttr = timeEl ? (timeEl.getAttribute('datetime') || textOf(timeEl)) : null;
      var tsMs = timeAttr ? new Date(timeAttr).getTime() : NaN;
      var tsSec = Number.isNaN(tsMs) ? null : Math.floor(tsMs / 1000);

      // Own messages have a right-aligned or "own-message" CSS marker
      var fromMe =
        ev.classList.contains('msg-s-event-listitem--own-message') ||
        ev.querySelector('[class*="own-message"]') !== null;

      msgs.push({
        from: sender || null,
        body: body,
        timestamp: tsSec,
        fromMe: fromMe,
      });
    });

    if (msgs.length === 0) return null;
    return { chatId: chatId, msgs: msgs };
  }

  // ── connection requests ────────────────────────────────────────────────────

  var lastRequestsKey = '';

  function scrapeConnectionRequests() {
    if (!location.href.includes('invitation') && !location.href.includes('mynetwork')) return null;
    var cards = document.querySelectorAll([
      '.invitation-card',
      '[data-view-name="manage-received-invitation"]',
      '[class*="invitation-card"]',
    ].join(', '));
    if (!cards || cards.length === 0) return null;

    var requests = [];
    cards.forEach(function (card) {
      var nameEl = card.querySelector(
        '.invitation-card__title, h3, [class*="invitation-card__title"]'
      );
      var subtitleEl = card.querySelector(
        '.invitation-card__subtitle, [class*="invitation-card__subtitle"]'
      );
      var name = textOf(nameEl);
      if (!name) return;
      requests.push({ name: name, subtitle: textOf(subtitleEl) || null });
    });
    return requests.length > 0 ? requests : null;
  }

  // ── main loop ──────────────────────────────────────────────────────────────

  api.loop(function () {
    var today = isoDay(Date.now());

    // 1. Conversation list
    var items = scrapeConversationList();
    if (items && items.length > 0) {
      // Unread delta check runs on EVERY poll tick, not just when the list
      // structure changes. listKey only fingerprints name+preview of the first
      // five rows, so an unread-count bump on row 6+ (or a count-only change)
      // would never enter the listKey gate and the notification would be missed.
      items.forEach(function (item) {
        var prev = prevUnread[item.chatId] || 0;
        if (item.unread > 0 && item.unread > prev) {
          api.emit('notify', {
            title: 'LinkedIn: ' + (item.name || 'New message'),
            body: item.preview || '',
            tag: 'linkedin:' + item.chatId,
            silent: false,
          });
        }
        prevUnread[item.chatId] = item.unread;
      });

      var listKey = JSON.stringify({
        n: items.length,
        first: items.slice(0, 5).map(function (i) { return i.name + '|' + i.preview; }),
      });

      if (listKey !== lastListKey) {
        lastListKey = listKey;

        // Redux store snapshot (legacy flat ingest for the accounts pane)
        api.ingest({
          messages: items.map(function (i) {
            return { id: i.chatId, from: i.name, body: i.preview, unread: i.unread };
          }),
          snapshotKey: listKey,
        });

        // Per-conversation-per-day memory ingest (list-level snippet only;
        // written to :preview key so a richer thread ingest is never overwritten).
        items.forEach(function (item) {
          if (!item.preview) return;
          api.emit('linkedin_conversation', {
            chatId: item.chatId,
            chatName: item.name,
            day: today,
            messages: [{
              from: item.name,
              body: item.preview,
              timestamp: item.ts ? Math.floor(item.ts / 1000) : null,
              fromMe: false,
            }],
            isSeed: false,
          });
        });
      }
    }

    // 2. Active thread — richer per-message ingest when a conversation is open
    var thread = scrapeActiveThread();
    if (thread && thread.msgs.length > 0) {
      var threadKey = JSON.stringify({
        chatId: thread.chatId,
        count: thread.msgs.length,
        last: thread.msgs[thread.msgs.length - 1].body.slice(0, 40),
      });
      if (threadKey !== lastThreadKey) {
        lastThreadKey = threadKey;
        api.emit('linkedin_conversation', {
          chatId: thread.chatId,
          chatName: null, // resolved from list on the service side if available
          day: today,
          messages: thread.msgs,
          isSeed: true,
        });
      }
    }

    // 3. Connection requests (only fires when on /mynetwork pages)
    var requests = scrapeConnectionRequests();
    if (requests) {
      var requestsKey = JSON.stringify(requests.map(function (r) { return r.name; }));
      if (requestsKey !== lastRequestsKey) {
        lastRequestsKey = requestsKey;
        api.emit('linkedin_requests', { requests: requests });
        api.log('info', '[linkedin-recipe] connection requests: ' + requests.length);
      }
    }
  });

  // ── send-message helper (callable via CDP Runtime.evaluate) ───────────────
  // Usage: window.__linkedinSend("Hello!") → { ok: true } | { ok: false, error: "..." }
  window.__linkedinSend = function (text) {
    var input = document.querySelector([
      '.msg-form__contenteditable',
      '[data-placeholder*="message"][contenteditable]',
      '[contenteditable="true"][role="textbox"]',
    ].join(', '));
    var sendBtn = document.querySelector([
      '.msg-form__send-btn',
      'button[type="submit"][class*="send"]',
      'button[class*="msg-form__send"]',
    ].join(', '));
    if (!input) return { ok: false, error: 'compose input not found' };
    if (!sendBtn) return { ok: false, error: 'send button not found' };
    input.focus();
    document.execCommand('insertText', false, text);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    setTimeout(function () { sendBtn.click(); }, 100);
    return { ok: true };
  };

  api.log('info', '[linkedin-recipe] v0.3 ready');
})(window.__openhumanRecipe);
