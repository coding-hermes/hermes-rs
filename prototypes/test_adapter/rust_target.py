"""Stub "Rust target" for the HRS-007 fail-closed adapter prototype.

Stands in for the future hermes-rs request-construction engine. It is a
pure, contract-driven function: (vector input) -> (request the target
would send). Real network I/O is deliberately absent — the adapter
observes the request OBJECT; a real Rust implementation replaces this
module behind the same boundary.

BUG INJECTION: DEFAULT BEHAVIOR IS DELIBERATELY BROKEN (HRS007-STUB-TARGET-BROKEN).
Every extra_body* expectation is emitted with a flipped "shape key"
(``__extra_body__`` wrapper instead of merged extra_body fields) — the
real-world analogue of an engine that shims provider quirks in the wrong
JSON position. The paired runner must therefore FAIL every affected
vector against the stub as-is. Flip to correct behavior by injecting
``"broken": false`` (see rust_target_run) — the prototype's tests prove
both arms. THIS IS THE POINT: a green paired run requires a correct
target, and no Python code can substitute for it.
"""

BROKEN = True  # default: deliberately broken (see module docstring)


def _endpoint_for_profile(profile: str, base_url: str | None, endpoint: str) -> str:
    # Profile default base URLs verified at upstream pin e06df9c2
    # (plugins/model-providers/<name>/__init__.py).
    defaults = {
        "nvidia": "https://integrate.api.nvidia.com/v1",
        "kimi-coding": "https://api.moonshot.ai/v1",
        "openrouter": "https://openrouter.ai/api/v1",
        "nous": "https://inference-api.nousresearch.com/v1",
        "qwen-oauth": "https://portal.qwen.ai/v1",
        "deepseek": "https://api.deepseek.com/v1",
        "meta-ai": "https://api.meta.ai/v1",
        "custom": "http://127.0.0.1:11434/v1",
    }
    base = (base_url or defaults.get(profile, "")).rstrip("/")
    return f"{base}{endpoint}"


def rust_target_run(vector: dict, broken: bool | None = None) -> dict:
    """Build the request the (stub) Rust target would send for `vector`.

    Returns {"url", "wire_model", "body"} — body is the JSON-serializable
    request body dict exactly as it would hit the wire.
    """
    if broken is None:
        broken = BROKEN
    inp = vector["input"]
    exp = vector["expect"]
    url = _endpoint_for_profile(inp["profile"], inp.get("base_url"), inp["endpoint"])

    body: dict = {
        "model": inp["model"],
        "messages": [dict(m) for m in inp.get("messages", [])],
    }

    wire_model = inp["model"]

    # --- max-tokens semantics ---
    if inp.get("max_tokens") is not None:
        body["max_completion_tokens"] = inp["max_tokens"]
    profile = inp["profile"]

    result_extras: dict = {}
    if profile == "meta-ai":
        # capability flags verified at the pin (tests/providers/test_meta_ai_profile.py)
        result_extras["supports_vision"] = True
        result_extras["supports_vision_tool_messages"] = False
    if profile == "deepseek":
        body.pop("max_completion_tokens", None)
        body.pop("max_tokens", None)

    # --- reasoning mapping (profile-specific, verified at the pin) ---
    reasoning = inp.get("reasoning")
    extra: dict = {}
    if reasoning is not None:
        if profile in ("kimi-coding", "custom"):
            # XOR contract: recognized effort goes top-level ONLY,
            # never paired with extra_body.thinking.
            if reasoning.get("enabled") and reasoning.get("effort") in (
                "low",
                "medium",
                "high",
            ):
                body["reasoning_effort"] = reasoning["effort"]
            elif reasoning.get("enabled") is False and profile == "custom":
                # think=false only for local ollama-style endpoints
                if (inp.get("base_url") or "").startswith("http://127.0.0.1"):
                    extra["think"] = False
                else:
                    body["reasoning_effort"] = "none"
        elif profile == "meta-ai":
            # meta maps effort names and never uses extra_body
            if reasoning.get("enabled") is False:
                body["reasoning_effort"] = "minimal"
            elif not reasoning.get("enabled") and not reasoning.get("effort"):
                body["reasoning_effort"] = "minimal"
            else:
                effort_map = {
                    "low": "low",
                    "medium": "medium",
                    "high": "high",
                    "max": "xhigh",
                }
                body["reasoning_effort"] = effort_map.get(
                    reasoning.get("effort"), "minimal"
                )

    # --- provider-quirk fields ---
    if profile == "nvidia":
        tool_idx = exp.get("tool_message_index")
        if tool_idx is not None:
            msg = body["messages"][tool_idx]
            body["messages"][tool_idx] = {
                k: v for k, v in msg.items() if k in ("role", "tool_call_id", "content")
            }
    if profile == "openrouter" and inp.get("provider_preferences"):
        extra["provider"] = inp["provider_preferences"]
    if profile == "qwen-oauth":
        extra["vl_high_resolution_images"] = True
        if inp.get("qwen_session_metadata"):
            body["metadata"] = inp["qwen_session_metadata"]
    if profile == "custom" and inp.get("ollama_num_ctx"):
        extra["options"] = {"num_ctx": inp["ollama_num_ctx"]}

    # --- THE INJECTED DEFECT ---
    # Broken mode wraps every extra_body field in a sentinel object
    # instead of merging them into the body's extra_body position.
    if extra:
        if broken:
            body["__extra_body__"] = extra
        else:
            body["extra_body"] = extra

    # Broken mode also materializes "absent" fields the contract says
    # must NOT be on the wire (e.g. deepseek max_tokens), another
    # realistic engine defect shape.
    if broken and profile == "deepseek":
        body["max_tokens"] = 4096

    result = {"url": url, "wire_model": wire_model, "body": body}
    result.update(result_extras)
    return result
