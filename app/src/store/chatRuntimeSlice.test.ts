import { describe, it, expect } from 'vitest';
import { configureStore } from '@reduxjs/toolkit';
import chatRuntimeReducer, {
  setQueueStatusForThread,
  clearQueueStatusForThread,
  clearRuntimeForThread,
  clearAllChatRuntime,
  type QueueStatus,
} from './chatRuntimeSlice';

function makeStore() {
  return configureStore({
    reducer: { chatRuntime: chatRuntimeReducer },
  });
}

describe('chatRuntimeSlice queue status', () => {
  it('sets queue status for a thread', () => {
    const store = makeStore();
    const status: QueueStatus = { steersPending: 1, followupsPending: 2, collectsPending: 0 };
    store.dispatch(setQueueStatusForThread({ threadId: 't1', status }));
    expect(store.getState().chatRuntime.queueStatusByThread['t1']).toEqual(status);
  });

  it('clears queue status for a thread', () => {
    const store = makeStore();
    const status: QueueStatus = { steersPending: 1, followupsPending: 0, collectsPending: 0 };
    store.dispatch(setQueueStatusForThread({ threadId: 't1', status }));
    store.dispatch(clearQueueStatusForThread({ threadId: 't1' }));
    expect(store.getState().chatRuntime.queueStatusByThread['t1']).toBeUndefined();
  });

  it('clearRuntimeForThread removes queue status', () => {
    const store = makeStore();
    const status: QueueStatus = { steersPending: 1, followupsPending: 0, collectsPending: 0 };
    store.dispatch(setQueueStatusForThread({ threadId: 't1', status }));
    store.dispatch(clearRuntimeForThread({ threadId: 't1' }));
    expect(store.getState().chatRuntime.queueStatusByThread['t1']).toBeUndefined();
  });

  it('clearAllChatRuntime removes all queue statuses', () => {
    const store = makeStore();
    store.dispatch(
      setQueueStatusForThread({
        threadId: 't1',
        status: { steersPending: 1, followupsPending: 0, collectsPending: 0 },
      })
    );
    store.dispatch(
      setQueueStatusForThread({
        threadId: 't2',
        status: { steersPending: 0, followupsPending: 1, collectsPending: 0 },
      })
    );
    store.dispatch(clearAllChatRuntime());
    expect(store.getState().chatRuntime.queueStatusByThread).toEqual({});
  });

  it('updates queue status when set again', () => {
    const store = makeStore();
    store.dispatch(
      setQueueStatusForThread({
        threadId: 't1',
        status: { steersPending: 1, followupsPending: 0, collectsPending: 0 },
      })
    );
    store.dispatch(
      setQueueStatusForThread({
        threadId: 't1',
        status: { steersPending: 0, followupsPending: 0, collectsPending: 0 },
      })
    );
    expect(store.getState().chatRuntime.queueStatusByThread['t1']).toEqual({
      steersPending: 0,
      followupsPending: 0,
      collectsPending: 0,
    });
  });

  it('isolates queue status across threads', () => {
    const store = makeStore();
    store.dispatch(
      setQueueStatusForThread({
        threadId: 't1',
        status: { steersPending: 1, followupsPending: 0, collectsPending: 0 },
      })
    );
    store.dispatch(
      setQueueStatusForThread({
        threadId: 't2',
        status: { steersPending: 0, followupsPending: 2, collectsPending: 0 },
      })
    );
    expect(store.getState().chatRuntime.queueStatusByThread['t1']?.steersPending).toBe(1);
    expect(store.getState().chatRuntime.queueStatusByThread['t2']?.followupsPending).toBe(2);
  });
});
