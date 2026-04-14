import { useEffect, useMemo, useState } from 'react';

import AddAccountModal from '../components/accounts/AddAccountModal';
import { AgentIcon, ProviderIcon } from '../components/accounts/providerIcons';
import WebviewHost from '../components/accounts/WebviewHost';
import { AgentChatPanel } from './Conversations';
import { startWebviewAccountService } from '../services/webviewAccountService';
import { addAccount, setActiveAccount } from '../store/accountsSlice';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import type { Account, ProviderDescriptor } from '../types/accounts';
import { AGENT_ACCOUNT_ID as AGENT_ID } from '../utils/accountsFullscreen';

function makeAccountId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  return `acct-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

interface RailButtonProps {
  active: boolean;
  onClick: () => void;
  tooltip: string;
  badge?: number;
  children: React.ReactNode;
}

const RailButton = ({ active, onClick, tooltip, badge, children }: RailButtonProps) => (
  <button
    onClick={onClick}
    className={`group relative flex h-11 w-11 items-center justify-center rounded-xl transition-all ${
      active
        ? 'bg-primary-50 ring-2 ring-primary-500'
        : 'hover:bg-stone-100 hover:scale-105'
    }`}
    aria-label={tooltip}>
    {children}
    {badge && badge > 0 ? (
      <span className="absolute -right-0.5 -top-0.5 flex min-w-[16px] items-center justify-center rounded-full bg-coral-500 px-1 text-[9px] font-semibold text-white">
        {badge > 99 ? '99+' : badge}
      </span>
    ) : null}
    <span className="pointer-events-none absolute left-full ml-3 whitespace-nowrap rounded-md bg-stone-900 px-2 py-1 text-xs text-white opacity-0 shadow-md transition-opacity group-hover:opacity-100 z-50">
      {tooltip}
    </span>
  </button>
);

const Accounts = () => {
  const dispatch = useAppDispatch();
  const accountsById = useAppSelector(state => state.accounts.accounts);
  const order = useAppSelector(state => state.accounts.order);
  const activeAccountId = useAppSelector(state => state.accounts.activeAccountId);
  const unreadByAccount = useAppSelector(state => state.accounts.unread);

  const [addOpen, setAddOpen] = useState(false);

  useEffect(() => {
    startWebviewAccountService();
  }, []);

  const accounts: Account[] = useMemo(
    () => order.map(id => accountsById[id]).filter((a): a is Account => Boolean(a)),
    [order, accountsById]
  );

  const selectedId = activeAccountId ?? AGENT_ID;
  const active = selectedId === AGENT_ID ? null : (accountsById[selectedId] ?? null);
  const isAgentSelected = selectedId === AGENT_ID;

  const handlePickProvider = (p: ProviderDescriptor) => {
    setAddOpen(false);
    const id = makeAccountId();
    const acct: Account = {
      id,
      provider: p.id,
      label: p.label,
      createdAt: new Date().toISOString(),
      status: 'pending',
    };
    dispatch(addAccount(acct));
    dispatch(setActiveAccount(id));
  };

  const selectAgent = () => dispatch(setActiveAccount(AGENT_ID));
  const selectAccount = (id: string) => dispatch(setActiveAccount(id));

  return (
    <div className="relative flex h-full overflow-hidden">
      {/* Narrow icon rail — floats when Agent is selected, flush to the
          edge when an app webview is taking the full pane. */}
      <aside
        className={`z-30 flex w-16 flex-none flex-col items-center gap-2 bg-white/60 py-3 backdrop-blur-md transition-all duration-300 ${
          isAgentSelected
            ? 'my-3 ml-3 rounded-2xl border border-stone-200/70 shadow-soft'
            : 'border-r border-stone-200/60'
        }`}>
        <RailButton active={isAgentSelected} onClick={selectAgent} tooltip="Agent">
          <AgentIcon className="h-9 w-9 rounded-lg" />
        </RailButton>

        {accounts.map(acct => (
          <RailButton
            key={acct.id}
            active={acct.id === selectedId}
            onClick={() => selectAccount(acct.id)}
            tooltip={acct.label}
            badge={unreadByAccount[acct.id]}>
            <ProviderIcon provider={acct.provider} className="h-8 w-8 rounded-md" />
          </RailButton>
        ))}

        <button
          onClick={() => setAddOpen(true)}
          className="group relative mt-2 flex h-11 w-11 items-center justify-center rounded-xl border border-dashed border-stone-300 text-stone-400 hover:bg-stone-50 hover:text-stone-600"
          aria-label="Add app">
          <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
          </svg>
          <span className="pointer-events-none absolute left-full ml-3 whitespace-nowrap rounded-md bg-stone-900 px-2 py-1 text-xs text-white opacity-0 shadow-md transition-opacity group-hover:opacity-100 z-50">
            Add app
          </span>
        </button>
      </aside>

      {/* Main pane */}
      <main className="flex min-w-0 flex-1 flex-col">
        {isAgentSelected ? (
          <AgentChatPanel />
        ) : active ? (
          <div className="flex-1">
            <WebviewHost accountId={active.id} provider={active.provider} />
          </div>
        ) : (
          <div className="flex flex-1 items-center justify-center text-sm text-stone-400">
            Select or add an app to get started.
          </div>
        )}
      </main>

      <AddAccountModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onPick={handlePickProvider}
      />
    </div>
  );
};

export default Accounts;
