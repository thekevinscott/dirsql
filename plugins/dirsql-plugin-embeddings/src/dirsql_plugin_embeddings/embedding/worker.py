import json
from hashlib import sha256

from . import model
from .cache import make_cache
from .progress import stderr_is_tty
from .values import ProtocolError, decode_value

MALFORMED_SHAPE = (
    'malformed request: expected {"call": [value, model_id?]} on one line'
)
MALFORMED_ARITY = 'malformed request: "call" must carry 1 or 2 arguments'
MALFORMED_MODEL_ID = "the model id must be TEXT"


class Worker:
    def __init__(self):
        self._models = {}
        self._cache = make_cache()
        self._compute_cached = self._cache.wrap(self._compute)
        self._pending = None

    def _model(self, model_id):
        if model_id not in self._models:
            self._models[model_id] = model.load_model(model_id)
        return self._models[model_id]

    def _compute(self, digest, identifier):
        text, loaded = self._pending
        (vector,) = loaded.encode([text], show_progress_bar=stderr_is_tty())
        return [float(component) for component in vector]

    def embed(self, text, model_id):
        loaded = self._model(model_id)
        identifier = model.model_identifier(model_id, loaded)
        digest = sha256(text.encode("utf-8")).hexdigest()
        self._pending = (text, loaded)
        return self._compute_cached(digest, identifier)

    def handle(self, line):
        try:
            request = json.loads(line)
        except json.JSONDecodeError as error:
            return {"err": f"malformed request: invalid JSON: {error}"}
        if not isinstance(request, dict) or "call" not in request:
            return {"err": MALFORMED_SHAPE}
        call = request["call"]
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
        try:
            return {"ok": self.embed(text, model_id)}
        except Exception as error:
            return {"err": f"embed({model_id!r}) failed: {error}"}

    def serve(self, stdin, stdout):
        for line in stdin:
            if not line.strip():
                continue
            response = self.handle(line)
            stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
            stdout.flush()
