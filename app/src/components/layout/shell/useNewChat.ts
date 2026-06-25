import { useCallback } from 'react';
import { useNavigate } from 'react-router-dom';

import { setActiveAccount } from '../../../store/accountsSlice';
import { useAppDispatch, useAppSelector } from '../../../store/hooks';
import { createNewThread, loadThreadMessages, setSelectedThread } from '../../../store/threadSlice';
import { AGENT_ACCOUNT_ID } from '../../../utils/accountsFullscreen';
import { chatThreadPath } from '../../../utils/chatRoutes';

/**
 * The "New Chat" action behind the ⌘N / Ctrl+N shortcut.
 *
 * Unlike {@link useHomeNav}, this always lands on a *blank* thread regardless of
 * the current route. `useHomeNav`'s off-chat branch only navigates to `/chat`
 * and lets the mounting Conversations page own blank-thread landing — but that
 * restores the persisted `selectedThreadId` first, so from a non-chat route it
 * would reopen the previous conversation instead of starting a new one. Here we
 * explicitly select/create the blank thread before navigating, so a new chat is
 * always a new chat:
 *
 *  - switch back to the agent account (a selected connected app would otherwise
 *    keep rendering its webview instead of the agent thread);
 *  - reuse an existing empty thread if one exists (avoids piling up blanks),
 *    else create one;
 *  - select + load it and navigate straight to it. Selecting the thread before
 *    navigation also prevents the Conversations page from racing to create a
 *    second blank thread on mount.
 */
export function useNewChat(): () => void {
  const navigate = useNavigate();
  const dispatch = useAppDispatch();
  const threads = useAppSelector(state => state.thread.threads);

  return useCallback(() => {
    dispatch(setActiveAccount(AGENT_ACCOUNT_ID));

    const empty = threads.find(thr => (thr.messageCount ?? 0) === 0);
    if (empty) {
      dispatch(setSelectedThread(empty.id));
      void dispatch(loadThreadMessages(empty.id));
      navigate(chatThreadPath(empty.id));
      return;
    }

    void dispatch(createNewThread())
      .unwrap()
      .then(thr => {
        dispatch(setSelectedThread(thr.id));
        void dispatch(loadThreadMessages(thr.id));
        navigate(chatThreadPath(thr.id));
      })
      .catch(() => {});
  }, [navigate, dispatch, threads]);
}
