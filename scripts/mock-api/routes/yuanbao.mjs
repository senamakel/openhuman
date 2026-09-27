/**
 * Minimal Yuanbao sign-token response for credential-channel E2E coverage.
 * Specs opt in by setting the channel's `api_domain` to this mock server.
 */

import { json } from "../http.mjs";

const SIGN_TOKEN_PATH = "/api/v5/robotLogic/sign-token";

export async function handleYuanbao(ctx) {
  if (ctx.method !== "POST" || ctx.url !== SIGN_TOKEN_PATH) return false;

  json(ctx.res, 200, {
    code: 0,
    data: {
      token: "e2e-yuanbao-token",
      bot_id: "e2e-yuanbao-bot",
      product: "yuanbao",
      source: "openhuman-e2e",
      duration: 3600,
    },
  });
  return true;
}
