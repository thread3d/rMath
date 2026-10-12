"""Machine-readable description of the notebook server's HTTP API.

One source of truth for two things:

* ``GET /api`` — a compact index an agent or a human can read to discover the
  surface, and
* ``GET /api/openapi.json`` — an OpenAPI 3.1 document generated from the same
  table, so the docs cannot drift from the routes without a test noticing.

Nothing here imports the kernel or the notebooks: it is pure data, so it can be
served and unit-tested without a running server.
"""

API_TITLE = "rMath notebook API"
API_VERSION = "1.0.0"

# The MCP server shipped alongside this API; advertised in the index so an
# agent that speaks MCP knows how to reach the kernel without HTTP.
MCP_COMMAND = ["python3", "-m", "rmathpy.mcp"]


def _obj(props, required=(), description=None):
    schema = {
        "type": "object",
        "properties": props,
        "additionalProperties": False,
    }
    if required:
        schema["required"] = list(required)
    if description:
        schema["description"] = description
    return schema


_STR = {"type": "string"}
_BOOL = {"type": "boolean"}
_STATUS = {"type": "string", "enum": ["ok", "error"]}

_EVAL_REQUEST = _obj(
    {
        "input": dict(_STR, description="Mathematica-style rMath source; may "
                      "hold several statements separated by newlines or ``;``."),
        "steps": dict(_BOOL, description="Include the step-by-step trace."),
    },
    required=["input"],
)

_AGENT_REQUEST = _obj(
    {
        "mode": {"type": "string", "enum": ["raw", "cell", "fix"],
                 "default": "raw"},
        "task": dict(_STR, description="Free-form task (mode ``raw``)."),
        "input": dict(_STR, description="Cell input (mode ``cell``)."),
        "result": dict(_STR, description="Cell output (mode ``cell``)."),
        "messages": dict(_STR, description="Cell messages (mode ``cell``)."),
        "instruction": dict(_STR, description="Extra instruction (mode ``cell``)."),
        "report": dict(_STR, description="Bug report (mode ``fix``)."),
        "extra": dict(_STR, description="Extra context (mode ``fix``)."),
    },
)

_LEAN_RELATION = {"type": "string", "enum": ["=", "<", "<=", ">", ">=", "!="],
                  "default": "="}

# method, path, summary, request schema, response schema, tag.
# `streaming` marks the one NDJSON endpoint.
ENDPOINTS = [
    ("GET", "/api", "This discovery document: server name, version and every route.",
     None, None, "meta", False),
    ("GET", "/api/openapi.json", "OpenAPI 3.1 description of this API.",
     None, None, "meta", False),
    ("GET", "/api/health", "Liveness probe.",
     None, _obj({"ok": _BOOL}), "meta", False),
    ("GET", "/api/names", "Names of the symbols the kernel knows about.",
     None, _obj({"names": {"type": "array", "items": _STR}}), "notebook", False),
    ("POST", "/api/eval", "Evaluate rMath source in the persistent session.",
     _EVAL_REQUEST,
     _obj({"status": _STATUS, "result": _STR, "input_form": _STR,
           "tex": _STR, "in": {"type": "integer"}, "out": {"type": "integer"},
           "prints": {"type": "array", "items": _STR},
           "messages": {"type": "array", "items": _obj({"tag": _STR, "text": _STR})},
           "graphics": {"type": "object"},
           "svg": dict(_STR, description="Rendered SVG when the result is graphics.")}),
     "notebook", False),
    ("POST", "/api/reset", "Clear all user definitions (pristine kernel).",
     _obj({}), _obj({"status": _STATUS}), "notebook", False),
    ("POST", "/api/split", "Split source into top-level statements.",
     _obj({"input": _STR}, required=["input"]),
     _obj({"statements": {"type": "array", "items": _STR}}), "notebook", False),
    ("POST", "/api/save", "Write cell inputs to a ``.mt`` script.",
     _obj({"inputs": {"type": "array", "items": _STR}, "name": _STR},
          required=["inputs"]),
     _obj({"status": _STATUS, "path": _STR}), "notebook", False),
    ("POST", "/api/open", "Read a ``.mt`` script and split it into statements.",
     _obj({"path": _STR}, required=["path"]),
     _obj({"status": _STATUS, "path": _STR, "text": _STR,
           "statements": {"type": "array", "items": _STR}}), "notebook", False),
    ("POST", "/api/save-svg", "Write an SVG plot to the work directory.",
     _obj({"svg": _STR, "name": _STR}, required=["svg"]),
     _obj({"status": _STATUS, "path": _STR}), "notebook", False),
    ("POST", "/api/session", "The autosaved notebook inputs.",
     _obj({}),
     _obj({"status": _STATUS, "inputs": {"type": "array", "items": _STR},
           "path": _STR}), "notebook", False),
    ("POST", "/api/autosave", "Persist (or clear) the notebook's cell inputs.",
     _obj({"inputs": {"type": "array", "items": _STR}, "path": _STR}),
     _obj({"status": _STATUS, "count": {"type": "integer"}}), "notebook", False),
    ("GET", "/api/lean/status", "Lean toolchain availability and versions.",
     None, {"type": "object"}, "lean", False),
    ("POST", "/api/lean/check", "Type-check a Lean snippet against Mathlib.",
     _obj({"code": _STR}, required=["code"]), {"type": "object"}, "lean", False),
    ("POST", "/api/lean/verify",
     "Translate an rMath claim to Lean and verify it (authoritative one-shot run).",
     _obj({"input": _STR, "result": _STR, "relation": _LEAN_RELATION},
          required=["input"]),
     _obj({"outcome": {"type": "string",
                       "enum": ["verified", "unproved", "cannot verify", "error"]}}),
     "lean", False),
    ("POST", "/api/lean/search", "Search Mathlib premises (Loogle / LeanSearch).",
     _obj({"query": _STR,
           "backend": {"type": "string", "enum": ["loogle", "leansearch"]}},
          required=["query"]), {"type": "object"}, "lean", False),
    ("POST", "/api/lean/explore",
     "Fast, untrusted diagnostics from the persistent LSP server.",
     _obj({"input": _STR, "result": _STR, "result_form": _STR},
          required=["input"]), {"type": "object"}, "lean", False),
    ("GET", "/api/agent/status", "Harness endpoint, model and key availability.",
     None, {"type": "object"}, "agent", False),
    ("POST", "/api/agent", "Run one DeepSeek Harness task and return its answer.",
     _AGENT_REQUEST, {"type": "object"}, "agent", False),
    ("POST", "/api/agent/stream",
     "Run one harness task, streaming events as newline-delimited JSON.",
     _AGENT_REQUEST, {"type": "object", "description": "NDJSON stream, "
                      "Content-Type application/x-ndjson."}, "agent", True),
    ("POST", "/api/agent/cancel", "Cancel the harness or ACP task in flight.",
     _obj({}), {"type": "object"}, "agent", False),
    ("POST", "/api/agent/key", "Store the provider API key (mode 0600, outside the repo).",
     _obj({"key": _STR, "base_url": _STR}, required=["key"]),
     _obj({"status": _STATUS, "path": _STR}), "agent", False),
    ("POST", "/api/agent/model", "Choose the harness model for subsequent runs.",
     _obj({"model": _STR}, required=["model"]),
     _obj({"status": _STATUS, "model": _STR, "path": _STR}), "agent", False),
]

_PAGE_ENDPOINTS = [
    ("GET", "/", "The notebook page itself.", "page"),
    ("GET", "/katex/{path}", "KaTeX CSS, JS and font assets.", "page"),
]


def index(port: int = 8787) -> dict:
    """The compact discovery document served at ``GET /api``."""
    return {
        "name": API_TITLE,
        "version": API_VERSION,
        "description": ("JSON API over the rMath symbolic kernel and the web "
                        "notebook. All routes are served from the notebook "
                        "process; requests are refused unless they come from "
                        "the local machine."),
        "base_url": "http://127.0.0.1:%d" % port,
        "openapi": "/api/openapi.json",
        "mcp": {
            "transport": "stdio",
            "command": list(MCP_COMMAND),
            "description": "Model Context Protocol server exposing eval/plot and "
                           "Lean verification as MCP tools.",
        },
        "endpoints": [
            {"method": method, "path": path, "summary": summary, "tag": tag}
            for (method, path, summary, _req, _resp, tag, _stream) in ENDPOINTS
        ] + [
            {"method": method, "path": path, "summary": summary, "tag": tag}
            for (method, path, summary, tag) in _PAGE_ENDPOINTS
        ],
    }


def openapi(port: int = 8787) -> dict:
    """An OpenAPI 3.1 document generated from the same table as ``index``."""
    paths = {}
    for (method, path, summary, request, response, tag, streaming) in ENDPOINTS:
        operation = {
            "summary": summary,
            "tags": [tag],
            "responses": {
                "200": {
                    "description": "Success.",
                    "content": {
                        ("application/x-ndjson" if streaming else "application/json"):
                            {"schema": response or {"type": "object"}}
                    },
                }
            },
        }
        if request is not None:
            operation["requestBody"] = {
                "required": bool(request.get("required")),
                "content": {"application/json": {"schema": request}},
            }
        else:
            operation.setdefault("responses", {})
        paths.setdefault(path, {})[method.lower()] = operation

    return {
        "openapi": "3.1.0",
        "info": {
            "title": API_TITLE,
            "version": API_VERSION,
            "description": "JSON API over the rMath symbolic kernel and web notebook.",
        },
        "servers": [{"url": "http://127.0.0.1:%d" % port,
                     "description": "Local notebook server"}],
        "tags": [
            {"name": "meta"}, {"name": "notebook"},
            {"name": "lean"}, {"name": "agent"},
        ],
        "paths": paths,
    }
