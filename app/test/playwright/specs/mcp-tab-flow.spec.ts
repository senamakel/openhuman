import { expect, type Page, test } from '@playwright/test';

const APP_VERSION = '0.57.18';

const MOCK_REGISTRY_SERVERS = [
  {
    qualified_name: 'io.github.test/memory-server',
    display_name: 'Memory Server',
    description: 'A test MCP server for memory operations',
    icon_url: null,
    use_count: 1200,
    is_deployed: false,
    source: 'mcp_official',
  },
  {
    qualified_name: 'io.github.test/github-tools',
    display_name: 'GitHub Tools',
    description: 'MCP server for GitHub API integration',
    icon_url: null,
    use_count: 5600,
    is_deployed: true,
    source: 'mcp_official',
  },
  {
    qualified_name: 'io.github.test/notion-connector',
    display_name: 'Notion Connector',
    description: 'Connect to Notion workspaces via MCP',
    icon_url: null,
    use_count: 980,
    is_deployed: false,
    source: 'mcp_official',
  },
];

const MOCK_INSTALLED_SERVERS = [
  {
    server_id: 'srv_installed_1',
    qualified_name: 'io.github.test/memory-server',
    display_name: 'Memory Server',
    description: 'A test MCP server for memory operations',
    command_kind: 'node',
    command: 'npx',
    args: ['-y', '@modelcontextprotocol/server-memory'],
    env_keys: [],
    installed_at: 1700000000,
    enabled: true,
  },
];

const MOCK_STATUSES = [
  {
    server_id: 'srv_installed_1',
    qualified_name: 'io.github.test/memory-server',
    display_name: 'Memory Server',
    status: 'connected',
    tool_count: 5,
  },
];

function rpcOk(id: number, result: unknown) {
  return {
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ jsonrpc: '2.0', id, result }),
  };
}

async function mockAllRpcCalls(page: Page) {
  await page.route('**/rpc', async (route, request) => {
    const body = JSON.parse(request.postData() || '{}');
    const method: string = body.method;
    const id: number = body.id;

    switch (method) {
      case 'openhuman.update_version':
        return route.fulfill(
          rpcOk(id, {
            result: { version: APP_VERSION, target_triple: 'x86_64-apple-darwin', asset_prefix: '' },
          })
        );

      case 'openhuman.app_state_snapshot':
        return route.fulfill(
          rpcOk(id, {
            result: {
              auth: { isAuthenticated: true, userId: 'pw-mcp-user', user: null, profileId: null },
              sessionToken: 'fake-session-token',
              currentUser: { _id: 'pw-mcp-user', displayName: 'Test User' },
              onboardingCompleted: true,
              chatOnboardingCompleted: true,
              analyticsEnabled: false,
              meetAutoOrchestratorHandoff: false,
              localState: {},
              keyringStatus: { isUnlocked: true, hasPassphrase: false },
              runtime: {
                screenIntelligence: { enabled: false },
                localAi: { enabled: false },
                autocomplete: { enabled: false },
                service: { running: false },
              },
            },
          })
        );

      case 'openhuman.mcp_clients_registry_search':
        return route.fulfill(
          rpcOk(id, { servers: MOCK_REGISTRY_SERVERS, page: 1, total_pages: 1 })
        );

      case 'openhuman.mcp_clients_installed_list':
        return route.fulfill(rpcOk(id, { installed: MOCK_INSTALLED_SERVERS }));

      case 'openhuman.mcp_clients_status':
        return route.fulfill(rpcOk(id, { servers: MOCK_STATUSES }));

      case 'openhuman.mcp_clients_registry_get':
        return route.fulfill(
          rpcOk(id, {
            server: {
              ...MOCK_REGISTRY_SERVERS[1],
              connections: [{ type: 'stdio', published: true }],
              required_env_keys: ['GITHUB_TOKEN'],
            },
          })
        );

      default:
        return route.fulfill(rpcOk(id, {}));
    }
  });
}

async function seedLocalStorage(page: Page) {
  await page.addInitScript(() => {
    window.localStorage.setItem('openhuman_core_mode', 'cloud');
    window.localStorage.setItem('openhuman_core_rpc_url', 'http://127.0.0.1:17788/rpc');
    window.localStorage.setItem('openhuman_core_rpc_token', 'test-token');
    window.localStorage.setItem('openhuman:walkthrough_completed', 'true');
    window.localStorage.removeItem('openhuman:walkthrough_pending');
  });
}

test.describe('MCP Tab — Table View', () => {
  test.beforeEach(async ({ page }) => {
    await seedLocalStorage(page);
    await mockAllRpcCalls(page);
    await page.goto('/#/skills?tab=mcp');
    await page.waitForSelector('#root', { state: 'visible', timeout: 20_000 });
  });

  test('renders search bar and filter chips', async ({ page }) => {
    const searchInput = page.locator('input[type="search"]');
    await expect(searchInput).toBeVisible({ timeout: 10_000 });

    const allChip = page.getByRole('button', { name: /^All$/ });
    const installedChip = page.getByRole('button', { name: /Installed/ });
    const registryChip = page.getByRole('button', { name: /Registry/ });

    await expect(allChip).toBeVisible();
    await expect(installedChip).toBeVisible();
    await expect(registryChip).toBeVisible();
  });

  test('displays installed servers with status chip', async ({ page }) => {
    const installedChip = page.locator('table tbody span:has-text("Installed")');
    await expect(installedChip.first()).toBeVisible({ timeout: 10_000 });

    const serverName = page.locator('table tbody tr').first().locator('td:first-child');
    await expect(serverName).toContainText('Memory Server');
  });

  test('displays registry servers with Install button', async ({ page }) => {
    const registryChip = page.locator('table tbody span:has-text("Registry")');
    await expect(registryChip.first()).toBeVisible({ timeout: 10_000 });

    const installButtons = page.locator('table tbody button:has-text("Install")');
    await expect(installButtons.first()).toBeVisible();
  });

  test('filter chip "Installed" shows only installed servers', async ({ page }) => {
    await page.waitForTimeout(1000);
    const installedChip = page.getByRole('button', { name: /Installed/ });
    await installedChip.click();

    const sourceChips = page.locator('table tbody td:nth-child(3) span');
    const count = await sourceChips.count();
    expect(count).toBeGreaterThan(0);
    for (let i = 0; i < count; i++) {
      await expect(sourceChips.nth(i)).toContainText('Installed');
    }
  });

  test('filter chip "Registry" shows only registry servers', async ({ page }) => {
    await page.waitForTimeout(1000);
    const registryFilterChip = page.getByRole('button', { name: /Registry/ });
    await registryFilterChip.click();

    const sourceChips = page.locator('table tbody td:nth-child(3) span');
    const count = await sourceChips.count();
    expect(count).toBeGreaterThan(0);
    for (let i = 0; i < count; i++) {
      await expect(sourceChips.nth(i)).toContainText('Registry');
    }
  });

  test('installed servers are excluded from registry rows', async ({ page }) => {
    await page.waitForTimeout(1000);
    const registryFilterChip = page.getByRole('button', { name: /Registry/ });
    await registryFilterChip.click();

    const rows = page.locator('table tbody tr');
    const count = await rows.count();
    for (let i = 0; i < count; i++) {
      const nameCell = rows.nth(i).locator('td:first-child');
      const text = await nameCell.innerText();
      expect(text).not.toContain('Memory Server');
    }
  });

  test('clicking Install navigates to install dialog', async ({ page }) => {
    const installButton = page.locator('table tbody button:has-text("Install")').first();
    await expect(installButton).toBeVisible({ timeout: 10_000 });
    await installButton.click();

    const backButton = page.locator('button:has-text("Back")');
    await expect(backButton).toBeVisible({ timeout: 5_000 });
  });

  test('back button returns to table view', async ({ page }) => {
    const installButton = page.locator('table tbody button:has-text("Install")').first();
    await expect(installButton).toBeVisible({ timeout: 10_000 });
    await installButton.click();

    const backButton = page.locator('button:has-text("Back")');
    await expect(backButton).toBeVisible({ timeout: 5_000 });
    await backButton.click();

    const table = page.locator('table');
    await expect(table).toBeVisible({ timeout: 5_000 });
  });

  test('no Smithery branding visible', async ({ page }) => {
    await page.waitForTimeout(2000);
    const bodyText = await page.locator('body').innerText();
    expect(bodyText.toLowerCase()).not.toContain('smithery');
  });
});
