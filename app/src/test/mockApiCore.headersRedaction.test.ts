import { expect, it } from 'vitest';

// @ts-ignore - test-only JS module outside app/src
import { clearRequestLog, getMockServerPort } from '../../../scripts/mock-api-core.mjs';

it('redacts sensitive request headers in the mock API log', async () => {
  clearRequestLog();

  const mockApiUrl = `http://127.0.0.1:${getMockServerPort()}`;
  await fetch(`${mockApiUrl}/__admin/health`, {
    headers: {
      Authorization: 'Bearer secret-token',
      'Proxy-Authorization': 'Basic secret-token',
      'X-Test-Version': '1.2.3',
    },
  });

  const requestsResponse = await fetch(`${mockApiUrl}/__admin/requests`);
  const requestsPayload = (await requestsResponse.json()) as {
    data?: Array<{ headers?: Record<string, string> }>;
  };
  const request = requestsPayload.data?.find(
    entry => entry.headers?.['x-test-version'] === '1.2.3'
  );
  expect(request?.headers).toMatchObject({
    authorization: '[REDACTED]',
    'proxy-authorization': '[REDACTED]',
    'x-test-version': '1.2.3',
  });
});
