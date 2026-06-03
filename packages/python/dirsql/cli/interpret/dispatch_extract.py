"""Single-request handler for the `dirsql interpret` helper.

Given a parsed request line and a name->table lookup, call the user's
``extract`` callback and return the NDJSON response payload the loop
will write back to stdout.
"""

from __future__ import annotations

from typing import Any


def dispatch_extract(
    req: dict[str, Any],
    tables: dict[str, Any],
) -> dict[str, Any]:
    """Build the `{"type": "result", ...}` response for one extract request.

    - Unknown table name -> ``ok: false`` with a name-bearing error.
    - User's ``extract`` raising -> ``ok: false`` with ``str(exc)``.
    - Success -> ``ok: true`` with the row list the callback returned.

    ``req["id"]`` is echoed back unchanged so the caller can correlate
    request and response.
    """
    rid = req.get("id")
    name = req.get("table")
    path = req.get("path")
    table = tables.get(name)
    if table is None:
        return {
            "type": "result",
            "id": rid,
            "ok": False,
            "error": f"unknown table: {name!r}",
        }
    try:
        rows = table.extract(path)
    except Exception as exc:
        return {"type": "result", "id": rid, "ok": False, "error": str(exc)}
    return {"type": "result", "id": rid, "ok": True, "rows": rows}
