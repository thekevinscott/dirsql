import json
from hashlib import sha256

from . import model
from .cache import cache_dir
from .model_identifier import model_identifier
from .store import VectorStore
from .values import ProtocolError, decode_value

MALFORMED_SHAPE = (
    'malformed request: expected {"call": [value, model_id?]}'
    ' or {"calls": [[value, model_id?], ...]} on one line'
)
MALFORMED_CALLS = 'malformed request: "calls" must be a list of argument lists'
MALFORMED_ARITY = "malformed request: a call must carry 1 or 2 arguments"
MALFORMED_MODEL_ID = "the model id must be TEXT"


class Worker:
    def __init__(self):
        self._models = {}
        self._store = VectorStore(cache_dir())

    def _model(self, model_id):
        if model_id not in self._models:
            self._models[model_id] = model.load_model(model_id)
        return self._models[model_id]

    def embed(self, texts, model_id):
        """The vector for each of ``texts``, with whether the cache served it."""
        loaded = self._model(model_id)
        identifier = model_identifier(model_id, loaded)
        digests = [sha256(text.encode("utf-8")).hexdigest() for text in texts]
        found = self._store.lookup(identifier, digests)
        missing = {}
        for digest, text in zip(digests, texts):
            if digest not in found:
                missing[digest] = text
        computed = {}
        if missing:
            # No progress bar: dirsql's own progress line counts the calls. The
            # model *download* bar is TTY-gated in `progress.configure`.
            vectors = loaded.encode(list(missing.values()), show_progress_bar=False)
            computed = dict(zip(missing, vectors.tolist()))
            self._store.insert(identifier, computed)
        return [
            (found[digest], True) if digest in found else (computed[digest], False)
            for digest in digests
        ]

    def _decode(self, call):
        """A call's ``(text, model_id)``, or the response that answers it."""
        if not isinstance(call, list) or len(call) not in (1, 2):
            return {"err": MALFORMED_ARITY}
        value, *rest = call
        try:
            text = decode_value(value)
        except ProtocolError as error:
            return {"err": str(error)}
        if text is None:
            return {"ok": None}
        (model_id,) = rest or [model.DEFAULT_MODEL_ID]
        if not isinstance(model_id, str):
            return {"err": MALFORMED_MODEL_ID}
        return text, model_id

    def _embed_responses(self, calls):
        responses = [None] * len(calls)
        by_model = {}
        for index, (_, model_id) in enumerate(calls):
            by_model.setdefault(model_id, []).append(index)
        for model_id, indices in by_model.items():
            texts = [calls[index][0] for index in indices]
            try:
                group = [
                    {"ok": vector, "meta": {"cached": cached}}
                    for vector, cached in self.embed(texts, model_id)
                ]
            except Exception as error:
                group = [{"err": f"embed({model_id!r}) failed: {error}"}] * len(texts)
            for index, response in zip(indices, group):
                responses[index] = response
        return responses

    def _responses(self, calls):
        responses = [None] * len(calls)
        pending = {}
        for index, call in enumerate(calls):
            decoded = self._decode(call)
            if isinstance(decoded, dict):
                responses[index] = decoded
            else:
                pending[index] = decoded
        embedded = self._embed_responses(list(pending.values()))
        for index, response in zip(pending, embedded):
            responses[index] = response
        return responses

    def handle(self, line):
        try:
            request = json.loads(line)
        except json.JSONDecodeError as error:
            return {"err": f"malformed request: invalid JSON: {error}"}
        if not isinstance(request, dict):
            return {"err": MALFORMED_SHAPE}
        if "calls" in request:
            calls = request["calls"]
            if not isinstance(calls, list):
                return {"err": MALFORMED_CALLS}
            return {"results": self._responses(calls)}
        if "call" not in request:
            return {"err": MALFORMED_SHAPE}
        (response,) = self._responses([request["call"]])
        return response

    def serve(self, stdin, stdout):
        for line in stdin:
            if not line.strip():
                continue
            response = self.handle(line)
            stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
            stdout.flush()
