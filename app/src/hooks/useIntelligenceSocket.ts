import { useCallback, useEffect } from 'react';

import { useCoreState } from '../providers/CoreStateProvider';
import { socketService } from '../services/socketService';
import { useAppSelector } from '../store/hooks';
import { selectSocketStatus } from '../store/socketSelectors';

export const useIntelligenceSocket = () => {
  const socketStatus = useAppSelector(selectSocketStatus);

  return {
    isConnected: socketStatus === 'connected',
    isReady: socketStatus === 'connected',
    sendMessage: async () => {},
    sendChatInit: async () => {},
    sendTyping: () => {},
  };
};

export const useIntelligenceSocketManager = () => {
  const { snapshot } = useCoreState();
  const socketStatus = useAppSelector(selectSocketStatus);
  const isConnected = socketStatus === 'connected';
  const token = snapshot.sessionToken;

  const connect = useCallback(() => {
    if (token && !isConnected) {
      socketService.connect(token);
    }
  }, [isConnected, token]);

  const disconnect = useCallback(() => {
    socketService.disconnect();
  }, []);

  useEffect(() => {
    if (token && !isConnected) {
      connect();
    }
  }, [connect, isConnected, token]);

  return { connect, disconnect, isConnected, isReady: Boolean(token) && isConnected };
};

export const useIntelligenceEvents = () => ({
  onAgentResponse: () => () => {},
  onExecutionProgress: () => () => {},
  onExecutionComplete: () => () => {},
});
