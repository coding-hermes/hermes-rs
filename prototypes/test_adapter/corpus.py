"""Shared vector corpus for the HRS-007 fail-closed adapter prototype.

Derived from upstream v0.22.0 (e06df9c2eae6901f4c88f464e0ae6e753c0428b8)
transport-parity test cases classified `observable` in
specs/TEST_CLASSIFICATION_MANIFEST.json. Each vector states the upstream
case it mirrors and the expected request-construction behavior. Vectors
are CONTRACT: both the Python mirrored expectations below and the stub
Rust target encode the same fixture constants, so breaking the target's
behavior is detectable per-vector while the corpus stays byte-stable.
"""

VECTORS = [
    {
        "id": "nv-mt-override",
        "upstream_case": "tests/providers/test_transport_parity.py::TestNvidiaParity::test_user_max_tokens_overrides",
        "input": {
            "endpoint": "/chat/completions",
            "model": "nvidia/llama-3.1-nemotron-70b-instruct",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "nvidia",
            "max_tokens": 4096,
        },
        "expect": {
            "url": "https://integrate.api.nvidia.com/v1/chat/completions",
            "wire_model": "nvidia/llama-3.1-nemotron-70b-instruct",
            "max_completion_tokens": 4096,
        },
    },
    {
        "id": "kimi-reasoning-xor",
        "upstream_case": "tests/providers/test_transport_parity.py::TestKimiParity::test_thinking_enabled",
        "input": {
            "endpoint": "/chat/completions",
            "model": "kimi-k2",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "kimi-coding",
            "reasoning": {"enabled": True, "effort": "high"},
        },
        "expect": {
            "url": "https://api.moonshot.ai/v1/chat/completions",
            "wire_model": "kimi-k2",
            "reasoning_effort": "high",
            "extra_body_thinking": None,
        },
    },
    {
        "id": "or-provider-prefs",
        "upstream_case": "tests/providers/test_transport_parity.py::TestOpenRouterParity::test_provider_preferences",
        "input": {
            "endpoint": "/chat/completions",
            "model": "anthropic/claude-sonnet-4.6",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "openrouter",
            "provider_preferences": {"allow": ["anthropic"], "sort": "price"},
        },
        "expect": {
            "url": "https://openrouter.ai/api/v1/chat/completions",
            "wire_model": "anthropic/claude-sonnet-4.6",
            "extra_body_provider": {"allow": ["anthropic"], "sort": "price"},
        },
    },
    {
        "id": "meta-vision-flags",
        "upstream_case": "tests/providers/test_meta_ai_profile.py::TestMetaAIProfile::test_images_ride_user_turns_not_tool_results",
        "input": {
            "endpoint": "/chat/completions",
            "model": "muse-spark-latest",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "meta-ai",
        },
        "expect": {
            "url": "https://api.meta.ai/v1/chat/completions",
            "wire_model": "muse-spark-latest",
            "supports_vision": True,
            "supports_vision_tool_messages": False,
        },
    },
    {
        "id": "qwen-vl-hi-res",
        "upstream_case": "tests/providers/test_transport_parity.py::TestQwenParity::test_vl_high_resolution",
        "input": {
            "endpoint": "/chat/completions",
            "model": "qwen3.5-plus",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "qwen-oauth",
        },
        "expect": {
            "url": "https://portal.qwen.ai/v1/chat/completions",
            "wire_model": "qwen3.5-plus",
            "extra_body_vl_high_resolution_images": True,
        },
    },
    {
        "id": "qwen-metadata-toplevel",
        "upstream_case": "tests/providers/test_transport_parity.py::TestQwenParity::test_metadata_top_level",
        "input": {
            "endpoint": "/chat/completions",
            "model": "qwen3.5-plus",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "qwen-oauth",
            "qwen_session_metadata": {"sessionId": "s123", "promptId": "p456"},
        },
        "expect": {
            "url": "https://portal.qwen.ai/v1/chat/completions",
            "wire_model": "qwen3.5-plus",
            "metadata": {"sessionId": "s123", "promptId": "p456"},
            "extra_body_metadata": None,
        },
    },
    {
        "id": "ollama-numctx",
        "upstream_case": "tests/providers/test_transport_parity.py::TestCustomOllamaParity::test_ollama_num_ctx",
        "input": {
            "endpoint": "/chat/completions",
            "model": "llama3.1",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "custom",
            "base_url": "http://127.0.0.1:11434/v1",
            "ollama_num_ctx": 131072,
        },
        "expect": {
            "url": "http://127.0.0.1:11434/v1/chat/completions",
            "wire_model": "llama3.1",
            "extra_body_options_num_ctx": 131072,
        },
    },
    {
        "id": "custom-think-false",
        "upstream_case": "tests/providers/test_transport_parity.py::TestCustomOllamaParity::test_think_false_when_disabled",
        "input": {
            "endpoint": "/chat/completions",
            "model": "qwen3:72b",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "custom",
            "base_url": "http://127.0.0.1:11434/v1",
            "reasoning": {"enabled": False, "effort": "none"},
        },
        "expect": {
            "url": "http://127.0.0.1:11434/v1/chat/completions",
            "wire_model": "qwen3:72b",
            "extra_body_think": False,
        },
    },
    {
        "id": "custom-mistral-think-omit",
        "upstream_case": "tests/providers/test_transport_parity.py::TestCustomOllamaParity::test_think_omitted_for_mistral_custom",
        "input": {
            "endpoint": "/chat/completions",
            "model": "mistral-small-latest",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "custom",
            "base_url": "https://api.mistral.ai/v1",
            "reasoning": {"enabled": False, "effort": "none"},
        },
        "expect": {
            "url": "https://api.mistral.ai/v1/chat/completions",
            "wire_model": "mistral-small-latest",
            "extra_body_think": None,
            "reasoning_effort": "none",
        },
    },
    {
        "id": "ds-no-forced-max-tokens",
        "upstream_case": "tests/providers/test_e2e_wiring.py::TestDeepSeekProfileWiring::test_deepseek_no_forced_max_tokens",
        "input": {
            "endpoint": "/chat/completions",
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "hi"}],
            "profile": "deepseek",
        },
        "expect": {
            "url": "https://api.deepseek.com/v1/chat/completions",
            "wire_model": "deepseek-chat",
            "max_tokens": None,
            "max_completion_tokens": None,
        },
    },
    {
        "id": "nvidia-toolmsg-drop-name",
        "upstream_case": "tests/providers/test_e2e_wiring.py::TestNvidiaProfileWiring::test_nvidia_tool_messages_drop_name_fields",
        "input": {
            "endpoint": "/chat/completions",
            "model": "mistralai/mistral-large-3-675b-instruct-2512",
            "profile": "nvidia",
            "messages": [
                {"role": "user", "content": "run a command"},
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "terminal", "arguments": "{}"},
                        }
                    ],
                },
                {
                    "role": "tool",
                    "name": "terminal",
                    "tool_name": "terminal",
                    "tool_call_id": "call_1",
                    "content": "ok",
                },
            ],
        },
        "expect": {
            "url": "https://integrate.api.nvidia.com/v1/chat/completions",
            "wire_model": "mistralai/mistral-large-3-675b-instruct-2512",
            "tool_message_index": 2,
            "tool_message": {"role": "tool", "tool_call_id": "call_1", "content": "ok"},
        },
    },
    {
        "id": "mc-reasoning-map",
        "upstream_case": "tests/providers/test_meta_ai_profile.py::TestReasoningEffort::test_mapping",
        "input": {
            "endpoint": "/chat/completions",
            "model": "muse-spark-latest",
            "messages": [{"role": "user", "content": "hello"}],
            "profile": "meta-ai",
            "reasoning": {"enabled": False},
        },
        "expect": {
            "url": "https://api.meta.ai/v1/chat/completions",
            "wire_model": "muse-spark-latest",
            "reasoning_effort": "minimal",
        },
    },
]

VECTOR_IDS = [v["id"] for v in VECTORS]
