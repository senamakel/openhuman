#!/usr/bin/env python3
"""TokenJuice ML ("Kompress") stdio worker.

Long-lived, line-oriented JSON protocol over stdin/stdout. The ModernBERT
token-classification model is loaded once at startup, then the process answers
compression requests until stdin closes (the Rust parent drops the child on
shutdown via kill_on_drop).

Protocol
--------
Startup (one or more lines until ready):
    {"status": "downloading", "model": "<id>"}   # optional progress keepalive
    {"status": "loading", "model": "<id>"}
    {"ready": true, "model": "<id>", "device": "cpu"}
or, on fatal load failure:
    {"ready": false, "error": "<message>"}        (then exit non-zero)

Request line: {"op": "compress", "id": <str>, "text": <str>,
               "target_ratio": <float>, "max_input_chars": <int>}
              {"op": "ping", "id": <str>}
              {"op": "shutdown", "id": <str>}
Response line: {"id": <str>, "compressed_text": <str>,
                "stats": {"input_chars": int, "output_chars": int,
                          "ratio": float, "model_ms": int}}
               {"id": <str>, "pong": true}
               {"id": <str>, "error": <str>}

The compression model is a ModernBERT token classifier that scores each token's
salience; low-salience runs are dropped to approach the target ratio. The model
id and device are passed via environment variables (TOKENJUICE_ML_MODEL_ID,
TOKENJUICE_ML_DEVICE) so the Rust side stays the single source of config.
"""

import json
import os
import re
import sys
import time

MODEL_ID = os.environ.get("TOKENJUICE_ML_MODEL_ID", "answerdotai/ModernBERT-base")
DEVICE_PREF = os.environ.get("TOKENJUICE_ML_DEVICE", "cpu")


def _emit(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False))
    sys.stdout.write("\n")
    sys.stdout.flush()


def _pick_device():
    if DEVICE_PREF != "auto":
        return DEVICE_PREF
    try:
        import torch

        if torch.cuda.is_available():
            return "cuda"
        if getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
            return "mps"
    except Exception:
        pass
    return "cpu"


class Kompressor:
    """Salience-based redundancy remover.

    Uses a ModernBERT encoder to embed sentences and drops sentences whose
    salience (norm of the [CLS]-pooled embedding relative to the document mean)
    is lowest, until the target ratio is reached. This keeps the most
    information-dense sentences. A pure-Python sentence splitter avoids extra
    deps. (A fine-tuned token-classification head can be slotted in later; the
    protocol and Rust side do not change.)
    """

    def __init__(self, device):
        import torch  # noqa: F401  (imported for side effect / availability)
        from transformers import AutoModel, AutoTokenizer

        self.device = device
        self.tok = AutoTokenizer.from_pretrained(MODEL_ID)
        self.model = AutoModel.from_pretrained(MODEL_ID).to(device)
        self.model.eval()

    def compress(self, text, target_ratio, max_input_chars):
        import torch

        if len(text) > max_input_chars:
            text = text[:max_input_chars]
        sentences = _split_sentences(text)
        if len(sentences) <= 3:
            return text  # nothing meaningful to drop

        with torch.no_grad():
            enc = self.tok(
                sentences,
                return_tensors="pt",
                padding=True,
                truncation=True,
                max_length=128,
            ).to(self.device)
            out = self.model(**enc)
            # Mean-pool token embeddings per sentence.
            mask = enc["attention_mask"].unsqueeze(-1).float()
            summed = (out.last_hidden_state * mask).sum(dim=1)
            counts = mask.sum(dim=1).clamp(min=1.0)
            emb = summed / counts  # (n_sent, hidden)
            doc_mean = emb.mean(dim=0, keepdim=True)
            # Salience = distance from the document centroid (distinctive
            # sentences carry more unique information).
            salience = (emb - doc_mean).norm(dim=1)

        n_keep = max(3, int(round(len(sentences) * target_ratio)))
        if n_keep >= len(sentences):
            return text
        order = salience.argsort(descending=True).tolist()
        keep_idx = sorted(order[:n_keep])
        kept = [sentences[i] for i in keep_idx]
        return " ".join(kept)


_SENT_RE = re.compile(r"(?<=[.!?])\s+|\n+")


def _split_sentences(text):
    parts = [p.strip() for p in _SENT_RE.split(text)]
    return [p for p in parts if p]


def main():
    device = _pick_device()
    try:
        _emit({"status": "loading", "model": MODEL_ID})
        kompressor = Kompressor(device)
    except Exception as exc:  # pragma: no cover - exercised only without torch
        _emit({"ready": False, "error": f"{type(exc).__name__}: {exc}"})
        return 1

    _emit({"ready": True, "model": MODEL_ID, "device": device})

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception as exc:
            _emit({"id": None, "error": f"bad request json: {exc}"})
            continue

        op = req.get("op", "compress")
        req_id = req.get("id")
        if op == "ping":
            _emit({"id": req_id, "pong": True})
            continue
        if op == "shutdown":
            _emit({"id": req_id, "pong": True})
            return 0

        text = req.get("text") or ""
        target_ratio = float(req.get("target_ratio", 0.5))
        max_input_chars = int(req.get("max_input_chars", 200000))
        try:
            t0 = time.time()
            out = kompressor.compress(text, target_ratio, max_input_chars)
            model_ms = int((time.time() - t0) * 1000)
            _emit(
                {
                    "id": req_id,
                    "compressed_text": out,
                    "stats": {
                        "input_chars": len(text),
                        "output_chars": len(out),
                        "ratio": (len(out) / len(text)) if text else 1.0,
                        "model_ms": model_ms,
                    },
                }
            )
        except Exception as exc:  # pragma: no cover - defensive
            _emit({"id": req_id, "error": f"{type(exc).__name__}: {exc}"})

    return 0


if __name__ == "__main__":
    sys.exit(main())
