#!/usr/bin/env python3
"""OpenHuman runtime Python server.

Private JSONL stdio protocol. Rust owns the process and sends one compact JSON
request per line. This server keeps expensive Python backends warm for the
life of the Rust core process.
"""

import json
import sys

PROTOCOL = 1
SPACY_MODEL = "en_core_web_sm"

_spacy_nlp = None


def _emit(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False, separators=(",", ":")))
    sys.stdout.write("\n")
    sys.stdout.flush()


def _error(req_id, code, message):
    return {"id": req_id, "ok": False, "error": {"code": code, "message": str(message)}}


def _configure_stdio():
    if hasattr(sys.stdin, "reconfigure"):
        sys.stdin.reconfigure(encoding="utf-8")
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")


def _load_spacy():
    global _spacy_nlp
    if _spacy_nlp is not None:
        return _spacy_nlp

    import spacy

    try:
        _spacy_nlp = spacy.load(SPACY_MODEL, disable=["parser"])
    except Exception:
        _spacy_nlp = spacy.load(SPACY_MODEL)
    return _spacy_nlp


def _spacy_extract(params):
    text = (params or {}).get("text") or ""
    nlp = _load_spacy()
    doc = nlp(text)
    entities = [
        {
            "text": ent.text,
            "label": ent.label_,
            "start": int(ent.start_char),
            "end": int(ent.end_char),
        }
        for ent in doc.ents
    ]
    seen = set()
    nouns = []
    for tok in doc:
        if tok.pos_ in ("NOUN", "PROPN") and not tok.is_stop and tok.is_alpha:
            key = (tok.lemma_ or tok.text).lower().strip()
            if len(key) >= 2 and key not in seen:
                seen.add(key)
                nouns.append(key)
    return {"entities": entities, "nouns": nouns}


def _handle(req):
    req_id = req.get("id")
    method = req.get("method")
    params = req.get("params") or {}
    if method == "spacy.extract":
        return {"id": req_id, "ok": True, "result": _spacy_extract(params)}
    return _error(req_id, "unknown_method", f"unknown runtime_python_server method: {method}")


def main():
    _configure_stdio()
    try:
        _load_spacy()
    except Exception as exc:
        _emit({"ready": False, "error": f"{type(exc).__name__}: {exc}"})
        return 1

    _emit({"ready": True, "protocol": PROTOCOL, "backends": ["spacy"]})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception as exc:
            _emit(_error(None, "bad_json", exc))
            continue
        if not isinstance(req, dict):
            _emit(_error(None, "bad_request", "request must be a JSON object"))
            continue
        try:
            _emit(_handle(req))
        except Exception as exc:
            _emit(_error(req.get("id"), type(exc).__name__, exc))
    return 0


if __name__ == "__main__":
    sys.exit(main())
