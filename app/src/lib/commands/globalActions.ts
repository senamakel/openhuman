import { hotkeyManager } from './hotkeyManager';
import { registry } from './registry';

// Group headings shown (in this order) by the command palette and the
// keyboard-shortcuts help directory. Kept in sync with the `group` values
// assigned to each action below.
export const GROUP_ORDER = ['Navigation', 'Chat', 'View', 'General'] as const;

/**
 * Side-effecting handlers the global command layer drives. The owning
 * `CommandProvider` supplies these (navigation, new chat, sidebar toggle, and
 * the two provider-level overlays) so the shortcut map itself stays declarative
 * and testable here, decoupled from React state.
 */
export interface GlobalActionHandlers {
  navigate: (path: string) => void;
  newChat: () => void;
  toggleSidebar: () => void;
  openPalette: () => void;
  openShortcuts: () => void;
}

interface BindCombo {
  combo: string;
  /** Allow this combo to fire while a text input / editable is focused. */
  allowInInput?: boolean;
}

interface GlobalActionDef {
  id: string;
  label: string;
  group: (typeof GROUP_ORDER)[number] | 'Help';
  /** Primary shortcut — shown in the palette / help list and bound. */
  shortcut: string;
  /** `allowInInput` for the primary shortcut's binding. */
  allowInInput?: boolean;
  /** Extra key combos bound to the same handler (not shown separately). */
  aliases?: BindCombo[];
  handler: () => void;
  keywords?: string[];
}

/**
 * Builds the canonical list of global actions from the supplied handlers. This
 * is the single source of truth for the app-wide shortcut map: the command
 * palette, the keyboard-shortcuts help directory, and the actual key bindings
 * all derive from it.
 */
function buildGlobalActions(h: GlobalActionHandlers): GlobalActionDef[] {
  const nav = (path: string) => () => {
    h.navigate(path);
  };

  return [
    // ── Navigation ──────────────────────────────────────────────────────
    {
      id: 'nav.home',
      label: 'Go Home',
      group: 'Navigation',
      shortcut: 'mod+1',
      handler: nav('/home'),
      keywords: ['dashboard'],
    },
    {
      id: 'nav.chat',
      label: 'Go to Chat',
      group: 'Navigation',
      shortcut: 'mod+2',
      handler: nav('/chat'),
      keywords: ['conversations', 'messages', 'inbox'],
    },
    {
      id: 'nav.intelligence',
      label: 'Go to Knowledge & Memory',
      group: 'Navigation',
      shortcut: 'mod+3',
      handler: nav('/settings/intelligence'),
      keywords: ['memory', 'knowledge', 'intelligence'],
    },
    {
      id: 'nav.skills',
      label: 'Go to Connections',
      group: 'Navigation',
      shortcut: 'mod+4',
      handler: nav('/connections'),
      keywords: ['plugins', 'tools', 'connections', 'apps', 'skills'],
    },
    {
      id: 'nav.activity',
      label: 'Go to Activity',
      group: 'Navigation',
      shortcut: 'mod+5',
      handler: nav('/activity'),
      keywords: ['tasks', 'automations', 'alerts', 'background'],
    },
    {
      id: 'nav.settings',
      label: 'Open Settings',
      group: 'Navigation',
      shortcut: 'mod+,',
      handler: nav('/settings'),
      keywords: ['preferences', 'config'],
    },

    // ── Chat ────────────────────────────────────────────────────────────
    {
      id: 'chat.new',
      label: 'New Chat',
      group: 'Chat',
      shortcut: 'mod+n',
      handler: h.newChat,
      keywords: ['new', 'thread', 'compose', 'conversation', 'session'],
    },

    // ── View ────────────────────────────────────────────────────────────
    {
      id: 'view.toggle-sidebar',
      label: 'Toggle Sidebar',
      group: 'View',
      shortcut: 'mod+b',
      handler: h.toggleSidebar,
      keywords: ['sidebar', 'panel', 'rail', 'hide', 'show', 'collapse', 'expand'],
    },

    // ── General ─────────────────────────────────────────────────────────
    {
      id: 'meta.command-palette',
      label: 'Command Palette',
      group: 'General',
      shortcut: 'mod+k',
      allowInInput: true,
      aliases: [{ combo: 'mod+p', allowInInput: true }],
      handler: h.openPalette,
      keywords: ['command', 'palette', 'search', 'actions', 'run'],
    },
    {
      id: 'meta.keyboard-shortcuts',
      label: 'Keyboard Shortcuts',
      group: 'General',
      // `mod+/` is allowed in inputs so it can replace the command palette
      // while its search box is focused; the bare `?` alias is not, so it
      // never hijacks a literal "?" typed into a message.
      shortcut: 'mod+/',
      allowInInput: true,
      aliases: [{ combo: '?', allowInInput: false }],
      handler: h.openShortcuts,
      keywords: ['keyboard', 'shortcuts', 'keys', 'hotkeys', 'help', 'cheatsheet'],
    },
  ];
}

/**
 * Registers every global action (palette entries + key bindings) against the
 * provided global scope frame. Returns a disposer that unwinds all of them.
 */
export function registerGlobalActions(
  handlers: GlobalActionHandlers,
  globalScopeSymbol: symbol
): () => void {
  const actions = buildGlobalActions(handlers);

  const disposers: Array<() => void> = [];
  for (const a of actions) {
    const disposeRegistry = registry.registerAction(
      {
        id: a.id,
        label: a.label,
        group: a.group,
        shortcut: a.shortcut,
        handler: a.handler,
        keywords: a.keywords,
        allowInInput: a.allowInInput,
      },
      globalScopeSymbol
    );

    const combos: BindCombo[] = [
      { combo: a.shortcut, allowInInput: a.allowInInput },
      ...(a.aliases ?? []),
    ];
    const bindingSyms: symbol[] = [];
    for (const { combo, allowInInput } of combos) {
      bindingSyms.push(
        hotkeyManager.bind(globalScopeSymbol, {
          shortcut: combo,
          handler: a.handler,
          allowInInput,
          id: `${a.id}:${combo}`,
        })
      );
    }

    disposers.push(() => {
      disposeRegistry();
      for (const sym of bindingSyms) hotkeyManager.unbind(globalScopeSymbol, sym);
    });
  }

  return () => {
    for (const d of disposers) d();
  };
}
