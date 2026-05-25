import { expect, type Page } from '@playwright/test';

const CORE_RPC_URL = process.env.PW_CORE_RPC_URL || 'http://127.0.0.1:17788/rpc';
const CORE_RPC_TOKEN = process.env.PW_CORE_RPC_TOKEN || 'openhuman-playwright-token';

let nextRpcId = 1;

interface JsonRpcSuccess<T> {
  result: T;
}

interface JsonRpcFailure {
  error: { message?: string; code?: number; data?: unknown };
}

function buildBypassJwt(userId: string): string {
  const payload = Buffer.from(
    JSON.stringify({
      sub: userId,
      userId,
      exp: Math.floor(Date.now() / 1000) + 3600,
    })
  ).toString('base64url');
  return `eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.${payload}.sig`;
}

export async function callCoreRpc<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  const response = await fetch(CORE_RPC_URL, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${CORE_RPC_TOKEN}`,
    },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: nextRpcId++,
      method,
      params,
    }),
  });

  if (!response.ok) {
    throw new Error(`RPC ${method} failed with HTTP ${response.status}`);
  }

  const payload = (await response.json()) as JsonRpcSuccess<T> & JsonRpcFailure;
  if (payload.error) {
    throw new Error(`RPC ${method} failed: ${payload.error.message || 'unknown error'}`);
  }
  return payload.result;
}

export async function resetCoreForWebUser(_userId: string): Promise<void> {
  await callCoreRpc('openhuman.auth_clear_session', {});
  await callCoreRpc('openhuman.config_set_onboarding_completed', { value: true });
}

export async function seedBrowserCoreMode(page: Page): Promise<void> {
  await page.addInitScript(
    ({ rpcUrl, token }) => {
      window.localStorage.setItem('openhuman_core_mode', 'cloud');
      window.localStorage.setItem('openhuman_core_rpc_url', rpcUrl);
      window.localStorage.setItem('openhuman_core_rpc_token', token);
    },
    {
      rpcUrl: CORE_RPC_URL,
      token: CORE_RPC_TOKEN,
    }
  );
}

async function applyBrowserCoreModeInPage(page: Page): Promise<void> {
  await page.evaluate(
    ({ rpcUrl, token }) => {
      window.localStorage.setItem('openhuman_core_mode', 'cloud');
      window.localStorage.setItem('openhuman_core_rpc_url', rpcUrl);
      window.localStorage.setItem('openhuman_core_rpc_token', token);
    },
    {
      rpcUrl: CORE_RPC_URL,
      token: CORE_RPC_TOKEN,
    }
  );
}

async function completeAuthCallback(page: Page, token: string): Promise<void> {
  await page.goto(`/#/callback/auth?token=${encodeURIComponent(token)}&key=auth`);
  try {
    await expect
      .poll(async () => page.evaluate(() => window.location.hash), { timeout: 10_000 })
      .toMatch(/^#\/home/);
    return;
  } catch {
    const runtimePickerVisible = await page
      .getByText(/Select a Runtime|Connect to Your Runtime/)
      .count()
      .then(count => count > 0)
      .catch(() => false);
    if (!runtimePickerVisible) {
      throw new Error('auth callback did not reach /home and no runtime picker fallback was available');
    }
  }

  await applyBrowserCoreModeInPage(page);
  await page.goto(`/#/callback/auth?token=${encodeURIComponent(token)}&key=auth`);
  await expect
    .poll(async () => page.evaluate(() => window.location.hash), { timeout: 15_000 })
    .toMatch(/^#\/home/);
}

export async function resetCoreForWebGuest(): Promise<void> {
  await resetCoreForWebUser('guest');
}

export async function bootRuntimeReadyGuestPage(page: Page): Promise<void> {
  await resetCoreForWebGuest();
  await seedBrowserCoreMode(page);
  await page.goto('/#/');
  await page.waitForSelector('#root');
}

export async function signInViaCallbackToken(page: Page, token: string): Promise<void> {
  await completeAuthCallback(page, token);
  await waitForAppReady(page);
}

export async function signInViaBypassUser(page: Page, userId: string): Promise<void> {
  await completeAuthCallback(page, buildBypassJwt(userId));
  await waitForAppReady(page);
}

export async function bootAuthenticatedPage(page: Page, userId: string, hash: string = '/home'): Promise<void> {
  await resetCoreForWebUser(userId);
  await seedBrowserCoreMode(page);
  const token = buildBypassJwt(userId);
  await completeAuthCallback(page, token);
  if (hash !== '/home') {
    await page.goto(`/#${hash}`);
  }
  await waitForAppReady(page);
}

export async function waitForAppReady(page: Page): Promise<void> {
  await page.waitForSelector('#root');
  await expect
    .poll(async () => {
      const text = await page.locator('#root').innerText().catch(() => '');
      return text.trim().length;
    })
    .toBeGreaterThan(20);
  await expect(page.getByText(/Select a Runtime|Connect to Your Runtime/)).toHaveCount(0);
}
