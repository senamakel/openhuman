import { useEffect, useRef } from 'react';
import { socketService } from '../services/socketService';
import type { Socket } from 'socket.io-client';

interface UseSocketOptions {
  autoConnect?: boolean;
}

/**
 * React hook for using the Socket.IO connection
 * 
 * @example
 * ```tsx
 * const { socket, isConnected, emit, on, off } = useSocket();
 * 
 * useEffect(() => {
 *   on('ready', () => {
 *     console.log('Socket ready!');
 *   });
 *   
 *   return () => {
 *     off('ready');
 *   };
 * }, [on, off]);
 * ```
 */
export const useSocket = (options: UseSocketOptions = {}) => {
  const { autoConnect = true } = options;
  const listenersRef = useRef<Array<{ event: string; callback: (...args: unknown[]) => void }>>([]);

  useEffect(() => {
    if (autoConnect) {
      socketService.connect();
    }

    return () => {
      // Cleanup: remove all listeners registered through this hook
      listenersRef.current.forEach(({ event, callback }) => {
        socketService.off(event, callback);
      });
      listenersRef.current = [];
    };
  }, [autoConnect]);

  const emit = (event: string, data?: unknown) => {
    socketService.emit(event, data);
  };

  const on = (event: string, callback: (...args: unknown[]) => void) => {
    socketService.on(event, callback);
    listenersRef.current.push({ event, callback });
  };

  const off = (event: string, callback?: (...args: unknown[]) => void) => {
    socketService.off(event, callback);
    if (callback) {
      listenersRef.current = listenersRef.current.filter(
        (listener) => listener.event !== event || listener.callback !== callback
      );
    } else {
      listenersRef.current = listenersRef.current.filter((listener) => listener.event !== event);
    }
  };

  const once = (event: string, callback: (...args: unknown[]) => void) => {
    socketService.once(event, callback);
  };

  return {
    socket: socketService.getSocket() as Socket | null,
    isConnected: socketService.isConnected(),
    emit,
    on,
    off,
    once,
    connect: () => socketService.connect(),
    disconnect: () => socketService.disconnect(),
  };
};
