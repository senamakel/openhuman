import type { ApiResponse } from '../../types/api';
import { apiClient } from '../apiClient';

export interface CreditBalance {
  balanceUsd: number;
  topUpBalanceUsd: number;
}

export interface TeamUsage {
  remainingUsd: number;
  cycleBudgetUsd: number;
  dailyUsage: number;
  totalInputTokensThisCycle: number;
  totalOutputTokensThisCycle: number;
}

export interface TopUpResult {
  url: string;
  gatewayTransactionId: string;
  amountUsd: number;
  gateway: string;
}

export interface CreditTransaction {
  id: string;
  type: 'EARN' | 'SPEND';
  action: string;
  amountUsd: number;
  balanceAfterUsd: number;
  createdAt: string;
}

export interface PaginatedTransactions {
  transactions: CreditTransaction[];
  total: number;
}

/**
 * Credits API endpoints
 */
export const creditsApi = {
  /**
   * Get the current user's credit balance (general + top-up)
   * GET /credits/balance
   */
  getBalance: async (): Promise<CreditBalance> => {
    const response = await apiClient.get<ApiResponse<CreditBalance>>('/payments/credits/balance');
    return response.data;
  },

  /**
   * Get team inference budget usage for the current billing cycle
   * GET /teams/me/usage
   */
  getTeamUsage: async (): Promise<TeamUsage> => {
    const response = await apiClient.get<ApiResponse<TeamUsage>>('/teams/me/usage');
    return response.data;
  },

  /**
   * Start a top-up (get Stripe or Coinbase payment URL)
   * POST /credits/top-up
   */
  topUp: async (
    amountUsd: number,
    gateway: 'stripe' | 'coinbase' = 'stripe'
  ): Promise<TopUpResult> => {
    const response = await apiClient.post<ApiResponse<TopUpResult>>('/payments/credits/top-up', {
      amountUsd,
      gateway,
    });
    return response.data;
  },

  /**
   * Get paginated credit transaction history
   * GET /credits/transactions
   */
  getTransactions: async (limit = 20, offset = 0): Promise<PaginatedTransactions> => {
    const response = await apiClient.get<ApiResponse<PaginatedTransactions>>(
      `/credits/transactions?limit=${limit}&offset=${offset}`
    );
    return response.data;
  },
};
