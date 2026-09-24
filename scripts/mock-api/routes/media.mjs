import { json } from "../http.mjs";

// OpenRouter media proxy (`/agent-integrations/openrouter/{images,videos}`),
// in the backend's `{success, data}` envelope around OpenRouter's own bodies.
//
// Video jobs deliberately report `completed` with NO `unsigned_urls` on their
// second poll before the output appears on the third — the shape that used to
// make the core give up on a billed, about-to-deliver generation. Clients must
// poll through it.

const PREFIX = "/agent-integrations/openrouter";

// A 1×1 transparent PNG.
const PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg==";
// A minimal MP4 `ftyp` box — enough for type sniffing.
const MP4_BYTES = Buffer.from("AAAAGGZ0eXBtcDQyAAAAAG1wNDJpc29t", "base64");

/** Poll counts per job id, so each job walks the scripted lifecycle once. */
const pollsByJob = new Map();
let nextJob = 1;

/** Resets job state between tests. */
export function resetMediaMock() {
  pollsByJob.clear();
  nextJob = 1;
}

function jobStatus(jobId) {
  const polls = (pollsByJob.get(jobId) ?? 0) + 1;
  pollsByJob.set(jobId, polls);
  if (polls === 1) return { status: "in_progress", unsigned_urls: [] };
  if (polls === 2) return { status: "completed", unsigned_urls: [] };
  return {
    status: "completed",
    unsigned_urls: [`https://cdn.mock/${jobId}/0.mp4`],
    usage: { cost: 0.12 },
  };
}

export function handleMedia(ctx) {
  const { method, url, parsedBody, res } = ctx;
  if (!url.startsWith(PREFIX)) return false;
  const path = url.slice(PREFIX.length).split("?")[0];

  if (method === "GET" && path === "/images/models") {
    json(res, 200, {
      success: true,
      data: {
        object: "list",
        data: [
          {
            id: "bytedance-seed/seedream-5-0-lite",
            display_name: "Seedream 5.0 Lite",
            supported_parameters: {
              aspect_ratio: {
                type: "enum",
                values: ["1:1", "16:9", "9:16", "4:3", "3:4", "auto"],
              },
              n: { type: "range", min: 1, max: 4 },
              input_references: { type: "range", min: 0, max: 14 },
              seed: { type: "boolean" },
            },
          },
        ],
      },
    });
    return true;
  }

  if (method === "POST" && path === "/images") {
    if (typeof parsedBody?.prompt !== "string" || !parsedBody.prompt.trim()) {
      json(res, 400, { success: false, error: "prompt is required" });
      return true;
    }
    const n = Math.max(1, Math.min(4, Number(parsedBody.n ?? 1)));
    json(res, 200, {
      success: true,
      data: {
        created: Math.floor(Date.now() / 1000),
        data: Array.from({ length: n }, () => ({
          b64_json: PNG_BASE64,
          media_type: "image/png",
        })),
        usage: { cost: 0.035 * n },
      },
    });
    return true;
  }

  if (method === "GET" && path === "/videos/models") {
    json(res, 200, {
      success: true,
      data: {
        object: "list",
        data: [
          {
            id: "bytedance/seedance-2.0-mini",
            display_name: "Seedance 2.0 Mini",
            supported_resolutions: ["480p", "720p"],
            supported_durations: [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            supported_frame_images: ["first_frame", "last_frame"],
            generate_audio: true,
            seed: true,
          },
        ],
      },
    });
    return true;
  }

  if (method === "POST" && path === "/videos") {
    const id = `gen-vid-1790000000-mock${String(nextJob++).padStart(16, "0")}`;
    json(res, 200, {
      success: true,
      data: { id, polling_url: `/api/v1/videos/${id}`, status: "pending" },
    });
    return true;
  }

  const content = path.match(/^\/videos\/([^/]+)\/content$/);
  if (method === "GET" && content) {
    res.writeHead(200, {
      "Content-Type": "video/mp4",
      "Content-Length": MP4_BYTES.length,
    });
    res.end(MP4_BYTES);
    return true;
  }

  const job = path.match(/^\/videos\/([^/]+)$/);
  if (method === "GET" && job) {
    json(res, 200, {
      success: true,
      data: { id: job[1], ...jobStatus(job[1]) },
    });
    return true;
  }

  return false;
}
