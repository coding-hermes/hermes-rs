#!/usr/bin/env python3
"""Generate specs/TEST_CLASSIFICATION_MANIFEST.json (HRS-007).

Single source of truth for the pinned per-case classification of the
scoped upstream subset (v0.22.0 @ e06df9c2eae6901f4c88f464e0ae6e753c0428b8,
tests/providers/ + tests/conformance/, 21 files / 91 cases).

Every classification is embedded here with a one-line evidence citation
(test-source facts read at the pin), so the manifest can be regenerated
deterministically without re-reading upstream. Regeneration fails closed
if the pinned checkout's collection no longer yields exactly these IDs.

Class vocabulary (mutually exclusive, assigned by strongest constraint):
  observable        - case's pass/fail decision is computable from
                      adapter-visible inputs/outputs (request kwargs,
                      registry state, config resolution, persisted files).
                      Adapter-targetable now.
  python-internal   - case verifies Python process/import semantics
                      (import order, entry-point registry mechanics,
                      module-singleton cache invalidation) that have no
                      cross-language observation point.
  platform-host     - case's outcome depends on OS process semantics
                      (SIGKILL, multi-process racing, journal-mode
                      resolution, filesystem layout) beyond any adapter
                      boundary; its contract transfers, its executor
                      cannot.
  provider-live     - case contacts an external provider API without a
                      local fake. (None in this subset - listed for
                      completeness of the vocabulary.)
  explicitly-excluded - case is excluded from paired runs with a reason
                      (test-source skip conditions etc.).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
UPSTREAM = Path("/tmp/hermes-upstream-pin-hrs001")
ISOLATED_HOME = "/tmp/hrs007-isolated-home"  # OUTSIDE the pinned checkout
PIN = "e06df9c2eae6901f4c88f464e0ae6e753c0428b8"
TAG = "v0.22.0"
PYTEST = str(UPSTREAM / ".venv" / "bin" / "python")

OBS = "observable"
INT = "python-internal"
PLT = "platform-host"
LIV = "provider-live"
EXC = "explicitly-excluded"

# (test_id_suffix, class, evidence, adapter_note or None)
CASES: list[tuple[str, str, str, str | None]] = [
    # ---- tests/providers/test_auth_registry_import_order.py (1) ----
    (
        "tests/providers/test_auth_registry_import_order.py::test_tui_import_exposes_auth_registry_to_provider_plugins",
        INT,
        "spawns a fresh CPython child importing hermes_cli.auth + a Python plugin file; verifies import-order semantics",
        "Subprocess contract (HERMES_HOME, PYTHONPATH, cwd) is documented for the adapter; the import-order verdict itself has no cross-language observation point.",
    ),
    # ---- tests/providers/test_auth_registry_mid_discovery.py (3) ----
    (
        "tests/providers/test_auth_registry_mid_discovery.py::test_plugins_discovered_after_auth_import_resolve",
        INT,
        "subprocess runs Python probe asserting PROVIDER_REGISTRY rows after import-order-dependent discovery",
        None,
    ),
    (
        "tests/providers/test_auth_registry_mid_discovery.py::test_post_discovery_registration_is_mirrored",
        INT,
        "monkeypatches providers._discover_entry_point_providers and module-level registry state; asserts Python dict mirroring",
        None,
    ),
    (
        "tests/providers/test_auth_registry_mid_discovery.py::test_user_plugin_alias_repoints_and_display_name_follows",
        INT,
        "monkeypatches _current_source/_BUNDLED_PLUGINS_DIR; asserts in-process alias/display-name precedence",
        None,
    ),
    # ---- tests/providers/test_e2e_wiring.py (2) ----
    (
        "tests/providers/test_e2e_wiring.py::TestNvidiaProfileWiring::test_nvidia_tool_messages_drop_name_fields",
        OBS,
        "ChatCompletionsTransport.build_kwargs(...) on in-process transport; asserts exact tool-message dict; no network",
        "Direct adapter target: input messages + profile -> expected request kwargs.",
    ),
    (
        "tests/providers/test_e2e_wiring.py::TestDeepSeekProfileWiring::test_deepseek_no_forced_max_tokens",
        OBS,
        "build_kwargs asserts max_tokens absent for deepseek profile; pure function of inputs",
        None,
    ),
    # ---- tests/providers/test_entry_point_discovery.py (6) ----
    (
        "tests/providers/test_entry_point_discovery.py::test_entry_point_callable_and_module_targets",
        INT,
        "monkeypatches importlib.metadata.entry_points; verifies Python entry-point registration semantics",
        None,
    ),
    (
        "tests/providers/test_entry_point_discovery.py::test_entry_point_not_enabled_is_skipped",
        INT,
        "same in-process monkeypatched entry_points; enabled/disable set semantics are Python plugin-loader state",
        None,
    ),
    (
        "tests/providers/test_entry_point_discovery.py::test_entry_point_disabled_wins_over_enabled",
        INT,
        "in-process monkeypatched entry_points; disable-over-enable precedence in the Python loader",
        None,
    ),
    (
        "tests/providers/test_entry_point_discovery.py::test_general_plugin_register_ctx_not_invoked",
        INT,
        "in-process entry-point fake; asserts register(ctx) call convention of the Python plugin loader",
        None,
    ),
    (
        "tests/providers/test_entry_point_discovery.py::test_entry_point_failure_is_isolated",
        INT,
        "in-process entry-point fake raising inside one target; asserts loader isolation behavior",
        None,
    ),
    (
        "tests/providers/test_entry_point_discovery.py::test_filesystem_plugins_win_over_entry_points",
        INT,
        "in-process fakes for both discovery layers; precedence asserted on Python registry state",
        None,
    ),
    # ---- tests/providers/test_fetch_models_base_url.py (8) ----
    (
        "tests/providers/test_fetch_models_base_url.py::TestFetchModelsBaseUrlOverride::test_base_url_override_used",
        OBS,
        "profile.fetch_models() against a local 127.0.0.1 HTTPServer fake returning a models JSON; asserts selected URL + result",
        "Local fake server is adapter-reproducible: request URL/headers in, model list out.",
    ),
    (
        "tests/providers/test_fetch_models_base_url.py::TestFetchModelsBaseUrlOverride::test_custom_base_url_beats_models_url",
        OBS,
        "same local fake server; custom base_url precedence in fetch_models URL selection",
        None,
    ),
    (
        "tests/providers/test_fetch_models_base_url.py::TestFetchModelsBaseUrlOverride::test_default_base_url_does_not_shadow_models_url",
        OBS,
        "same local fake server; default base_url must not shadow the /models URL",
        None,
    ),
    (
        "tests/providers/test_fetch_models_base_url.py::TestCustomProviderBaseUrlPassthrough::test_custom_passes_base_url",
        OBS,
        "CustomProfile.fetch_models against local fake server (base_url 127.0.0.1:1 override); pure request/response contract",
        None,
    ),
    (
        "tests/providers/test_fetch_models_base_url.py::TestFetchModelsRedirectCredentialStripping::test_cross_host_redirect_strips_credentials",
        OBS,
        "two local HTTPServers; 302 cross-host redirect; asserts credential headers stripped on follow",
        "HTTP redirect credential policy is observable wire behavior.",
    ),
    (
        "tests/providers/test_fetch_models_base_url.py::TestFetchModelsRedirectCredentialStripping::test_same_origin_redirect_keeps_credentials",
        OBS,
        "same local-redirect rig; same-origin keeps authorization + x-api-key headers",
        None,
    ),
    (
        "tests/providers/test_fetch_models_base_url.py::TestModelPickerBaseUrlIntegration::test_picker_passes_base_url",
        INT,
        "MagicMock profile + patched hermes_cli.auth resolver; asserts a Python call kwarg inside hermes_cli.models picker",
        "The observation point is a mocked Python-internal call, not adapter-visible I/O.",
    ),
    (
        "tests/providers/test_fetch_models_base_url.py::test_profiles_without_model_listing_never_hit_the_network",
        OBS,
        "patched open_credentialed_url opener must never be called; fetch_models returns None for no-listing profiles",
        "Observable as 'no request issued' + None return; assert_not_called is adapter-reproducible as zero requests seen.",
    ),
    # ---- tests/providers/test_installed_plugin_discovery.py (2) ----
    (
        "tests/providers/test_installed_plugin_discovery.py::test_flat_installed_model_provider_plugins_are_discovered_alongside_nested_ones",
        OBS,
        "writes real plugin dirs under isolated HERMES_HOME; asserts discovered profiles incl. base_url/auth_type/env_vars",
        "Filesystem-layout -> registry outcome is adapter-observable.",
    ),
    (
        "tests/providers/test_installed_plugin_discovery.py::test_other_plugins_in_the_flat_directory_are_left_to_the_plugin_manager",
        OBS,
        "non-model-provider dirs under plugins/ must not become provider profiles; registry-state assertion",
        None,
    ),
    # ---- tests/providers/test_meta_ai_profile.py (4) ----
    (
        "tests/providers/test_meta_ai_profile.py::TestMetaAIProfile::test_images_ride_user_turns_not_tool_results",
        OBS,
        "get_provider_profile('meta-ai').prepare_messages/_build kwargs path; message-shape assertion, no network",
        None,
    ),
    (
        "tests/providers/test_meta_ai_profile.py::TestMetaAIProfile::test_live_catalog_filters_non_chat_models",
        OBS,
        "monkeypatched ProviderProfile.fetch_models fake returns mixed catalog; asserts chat-only filtering",
        "Filter is a pure function of a fetched list; the fetch itself is faked in-process.",
    ),
    (
        "tests/providers/test_meta_ai_profile.py::TestReasoningEffort::test_mapping",
        OBS,
        "pure mapping function reasoning-effort -> provider field values",
        None,
    ),
    (
        "tests/providers/test_meta_ai_profile.py::TestReasoningEffort::test_never_uses_extra_body",
        OBS,
        "asserts mapping never lands reasoning in extra_body; pure kwargs assertion",
        None,
    ),
    # ---- tests/providers/test_named_custom_reasoning.py (6) ----
    (
        "tests/providers/test_named_custom_reasoning.py::test_bare_named_custom_provider_gets_custom_profile",
        INT,
        "monkeypatches providers._REGISTRY/_ALIASES/_PROVIDER_LIST_CACHE module singletons; in-process lookup semantics",
        None,
    ),
    (
        "tests/providers/test_named_custom_reasoning.py::test_named_custom_fallback_does_not_override_registered_routes",
        INT,
        "same module-singleton monkeypatching; fallback precedence on Python registry state",
        None,
    ),
    (
        "tests/providers/test_named_custom_reasoning.py::test_bare_named_custom_memo_caches_the_helper_result",
        INT,
        "asserts a memoization counter via monkeypatched helper; Python caching mechanics",
        None,
    ),
    (
        "tests/providers/test_named_custom_reasoning.py::test_bare_named_custom_memo_invalidates_on_config_change",
        INT,
        "memo invalidation keyed on Python config module state",
        None,
    ),
    (
        "tests/providers/test_named_custom_reasoning.py::test_bare_named_custom_memo_is_keyed_by_home",
        INT,
        "memo keyed by HERMES_HOME value; in-process cache-key semantics",
        None,
    ),
    (
        "tests/providers/test_named_custom_reasoning.py::test_named_custom_route_keeps_final_reasoning_effort",
        OBS,
        "transport.build_kwargs for custom/custom:<name> profiles with reasoning_config effort low/medium/high; pure kwargs assertion (base_url points at a dead 127.0.0.1:1 port, no network)",
        None,
    ),
    # ---- tests/providers/test_oauth_pkce_plugin.py (4) ----
    (
        "tests/providers/test_oauth_pkce_plugin.py::test_pkce_handler_add_status_refresh_logout_against_fake_idp",
        OBS,
        "tests/providers/fake_pkce_idp.py: local 127.0.0.1 HTTPServer IdP (PKCE S256 verified); real HTTP flow, webbrowser/open + browser check monkeypatched",
        "Loopback IdP is adapter-reproducible; the OAuth client behavior (authorize URL, code exchange, refresh, logout) is observable wire behavior.",
    ),
    (
        "tests/providers/test_oauth_pkce_plugin.py::test_token_url_off_authorize_allowlist_refused_before_any_request",
        OBS,
        "httpx.post monkeypatched to raise; asserts no token endpoint contact off-allowlist; observable as zero requests + refusal",
        None,
    ),
    (
        "tests/providers/test_oauth_pkce_plugin.py::test_spent_refresh_token_is_grant_dead_and_marks_the_pool_row_dead",
        OBS,
        "fake IdP rotates refresh tokens (old -> 400); asserts grant-dead handling + pool row state on disk",
        None,
    ),
    (
        "tests/providers/test_oauth_pkce_plugin.py::test_alias_login_stores_the_row_under_the_canonical_profile_name",
        OBS,
        "fake IdP login via alias name; asserts stored pool row keyed by canonical profile name",
        None,
    ),
    # ---- tests/providers/test_plugin_discovery.py (3) ----
    (
        "tests/providers/test_plugin_discovery.py::test_bundled_plugins_discovered",
        OBS,
        "reads the checkout's bundled plugins/model-providers tree; asserts discovered profile set",
        "Filesystem -> registry outcome; adapter runs against the same plugin corpus.",
    ),
    (
        "tests/providers/test_plugin_discovery.py::test_all_profiles_register",
        OBS,
        "every discovered profile registers and resolves via get_provider_profile",
        None,
    ),
    (
        "tests/providers/test_plugin_discovery.py::test_user_plugin_overrides_bundled",
        OBS,
        "isolated HERMES_HOME plugin tree overrides bundled plugin of same name; registry assertion",
        None,
    ),
    # ---- tests/providers/test_profile_home_layers.py (2) ----
    (
        "tests/providers/test_profile_home_layers.py::test_secondary_profile_plugin_resolves_for_its_home_only",
        OBS,
        "two isolated HERMES_HOMEs with plugin trees; profile-scoped resolution asserted on registry state",
        None,
    ),
    (
        "tests/providers/test_profile_home_layers.py::test_plugin_installed_after_discovery_is_found_without_a_restart",
        OBS,
        "plugin dir added after first discovery; re-resolution observed on registry state",
        None,
    ),
    # ---- tests/providers/test_profile_wiring.py (6) ----
    (
        "tests/providers/test_profile_wiring.py::TestNvidiaProfileParity::test_max_tokens_match",
        OBS,
        "transport.build_kwargs parity pin for nvidia profile; pure kwargs assertion",
        "Core adapter target family: profile + inputs -> exact request kwargs.",
    ),
    (
        "tests/providers/test_profile_wiring.py::TestKimiProfileParity::test_temperature_omitted",
        OBS,
        "kimi profile must omit temperature; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_profile_wiring.py::TestDeveloperRoleParity::test_legacy_path_swaps_for_gpt5",
        OBS,
        "developer-role rewrite for gpt-5 models, legacy flag path; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_profile_wiring.py::TestDeveloperRoleParity::test_profile_path_swaps_for_gpt5",
        OBS,
        "same rewrite via provider profile path; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_profile_wiring.py::TestRequestOverridesParity::test_extra_body_override_legacy",
        OBS,
        "request_overrides landing in extra_body, legacy path; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_profile_wiring.py::TestRequestOverridesParity::test_top_level_override",
        OBS,
        "top-level override keys land top-level; kwargs assertion",
        None,
    ),
    # ---- tests/providers/test_provider_profiles.py (10) ----
    (
        "tests/providers/test_provider_profiles.py::TestOpenRouterProfile::test_sticky_session_id_normalizes_cron_timestamp",
        OBS,
        "sticky-session header construction normalizing cron timestamps; header/kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestOpenRouterProfile::test_pareto_min_coding_score_emitted_for_pareto_model",
        OBS,
        "conditional scoring header for pareto models; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestOpenRouterProfile::test_grok_session_id_sets_cache_affinity_header",
        OBS,
        "cache-affinity header wiring; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestOpenRouterProfile::test_grok_conv_id_normalizes_cron_timestamp",
        OBS,
        "conversation-id normalization in headers; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestOpenRouterProfile::test_mandatory_anthropic_verbosity_coexists_with_grok_header",
        OBS,
        "verbosity param + grok header coexistence; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestOpenRouterProfile::test_speed_tier_slugs_pin_endpoints_and_rewrite_wire_model",
        OBS,
        "speed-tier slug -> endpoint pinning + wire-model rewrite; kwargs/URL assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestNousProfile::test_sticky_session_id_normalizes_cron_timestamp",
        OBS,
        "nous sticky-session header normalization; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestNousProfile::test_extra_body_ignores_provider_preferences",
        OBS,
        "nous ignores provider preferences in extra_body; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestQwenProfile::test_prepare_messages_protects_nested_image_url_retry_mutation",
        OBS,
        "prepare_messages must not mutate nested image_url structures across retries; message-shape assertion",
        None,
    ),
    (
        "tests/providers/test_provider_profiles.py::TestAlibabaRegionalAndTokenPlanProfiles::test_cn_variants_resolve_in_auth_registry",
        INT,
        "monkeypatched credential env vars; asserts auth-registry rows for CN variants - Python auth-registry resolution",
        None,
    ),
    # ---- tests/providers/test_provider_registry.py (4) ----
    (
        "tests/providers/test_provider_registry.py::test_list_providers_reuses_cached_snapshot_until_registration_changes",
        INT,
        "module-level _PROVIDER_LIST_CACHE snapshot/copy semantics; Python cache mechanics",
        None,
    ),
    (
        "tests/providers/test_provider_registry.py::test_list_providers_dedupes_aliases_in_cached_snapshot",
        INT,
        "alias dedupe on the module-level cached snapshot",
        None,
    ),
    (
        "tests/providers/test_provider_registry.py::test_provider_lookups_reuse_the_home_plugin_stamp_within_its_ttl",
        INT,
        "monkeypatched _plugin_dir_stamps/_scan_home_layer; TTL stamp mechanics of the Python resolver",
        None,
    ),
    (
        "tests/providers/test_provider_registry.py::test_api_key_profiles_declare_their_registry_key_env",
        OBS,
        "declared env_vars must contain the registry key env for api_key profiles; invariant over registry rows",
        "Invariant over registry content, which the adapter materializes.",
    ),
    # ---- tests/providers/test_transport_parity.py (9) ----
    (
        "tests/providers/test_transport_parity.py::TestNvidiaParity::test_user_max_tokens_overrides",
        OBS,
        "nvidia max_tokens override semantics pinned via build_kwargs; pure kwargs assertion",
        "Adapter target family.",
    ),
    (
        "tests/providers/test_transport_parity.py::TestKimiParity::test_thinking_enabled",
        OBS,
        "xor contract: reasoning_effort only, never paired with extra_body.thinking; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_transport_parity.py::TestOpenRouterParity::test_provider_preferences",
        OBS,
        "provider preferences land in extra_body.provider; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_transport_parity.py::TestNousParity::test_tags",
        OBS,
        "nous portal tags in extra_body.tags; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_transport_parity.py::TestQwenParity::test_vl_high_resolution",
        OBS,
        "qwen vl_high_resolution_images flag in extra_body; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_transport_parity.py::TestQwenParity::test_metadata_top_level",
        OBS,
        "qwen metadata at top level, not extra_body; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_transport_parity.py::TestCustomOllamaParity::test_ollama_num_ctx",
        OBS,
        "ollama num_ctx in extra_body.options; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_transport_parity.py::TestCustomOllamaParity::test_think_false_when_disabled",
        OBS,
        "think=false for local custom endpoint when reasoning disabled; kwargs assertion",
        None,
    ),
    (
        "tests/providers/test_transport_parity.py::TestCustomOllamaParity::test_think_omitted_for_mistral_custom",
        OBS,
        "think omitted for https custom (mistral) endpoint; kwargs assertion",
        None,
    ),
    # ---- tests/conformance/test_profile_write_tripwire.py (4) ----
    (
        "tests/conformance/test_profile_write_tripwire.py::test_no_default_profile_writes[session_db]",
        OBS,
        "ProfileTripwire snapshots default-profile tree; session-store writes under active profile must not touch it; filesystem-tree diff verdict",
        "Verdict is a filesystem diff - adapter-observable; exercises SessionDB/config/memory/cron persistence surfaces.",
    ),
    (
        "tests/conformance/test_profile_write_tripwire.py::test_no_default_profile_writes[config_save]",
        OBS,
        "same tripwire; settings/config save surface",
        None,
    ),
    (
        "tests/conformance/test_profile_write_tripwire.py::test_no_default_profile_writes[memory_store]",
        OBS,
        "same tripwire; memory store surface",
        None,
    ),
    (
        "tests/conformance/test_profile_write_tripwire.py::test_no_default_profile_writes[cron_store]",
        OBS,
        "same tripwire; cron jobs.json save/load surface",
        None,
    ),
    # ---- tests/conformance/test_vector_generator.py (4) ----
    (
        "tests/conformance/test_vector_generator.py::test_corpus_ids_unique_and_categorized",
        OBS,
        "vector corpus ids unique + categories grid/scar/adversarial with minimum counts; pure corpus invariants",
        "Corpus invariants transfer to the Rust conformance vector set.",
    ),
    (
        "tests/conformance/test_vector_generator.py::test_oracles_importable_and_self_free",
        INT,
        "imports Python format_message renderers UNBOUND and renders through them; verifies Python oracle health",
        "The oracle itself is Python code; a Rust adapter re-implements oracles separately (see contract section 5).",
    ),
    (
        "tests/conformance/test_vector_generator.py::test_vector_files_shape",
        OBS,
        "generate(tmp_path) produces per-platform JSON docs with oracle provenance + expect taxonomy; file-shape assertion",
        None,
    ),
    (
        "tests/conformance/test_vector_generator.py::test_committed_vectors_match_regeneration",
        OBS,
        "committed tests/conformance/vectors/ regenerate byte-equal (modulo commit stamp); lockstep drift check",
        "The Rust-side analogue regenerates the same vectors from its own oracles.",
    ),
    # ---- tests/conformance/persistence/test_cell1_prefix_durability.py (3) ----
    (
        "tests/conformance/persistence/test_cell1_prefix_durability.py::test_acknowledged_appends_survive_sigkill[None]",
        PLT,
        "real SIGKILL to a child OS process mid-append; on-disk prefix durability of SQLite journal",
        "Contract transfers (acknowledged prefix survives crash); adapter runs its own equivalent against the Rust store.",
    ),
    (
        "tests/conformance/persistence/test_cell1_prefix_durability.py::test_acknowledged_appends_survive_sigkill[DELETE]",
        PLT,
        "same cell pinned to explicit DELETE journal mode",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell1_prefix_durability.py::test_acknowledged_appends_survive_sigkill[WAL]",
        PLT,
        "same cell requesting WAL; upstream skips via pytest.skip when the resolver downgrade gates trip",
        None,
    ),
    # ---- tests/conformance/persistence/test_cell2_consume_once.py (1) ----
    (
        "tests/conformance/persistence/test_cell2_consume_once.py::test_exactly_one_claimant_wins",
        PLT,
        "8 concurrent OS processes released from a file barrier race claim_handoff; exactly-one-winner under real process concurrency",
        None,
    ),
    # ---- tests/conformance/persistence/test_cell3_rotation_atomicity.py (3) ----
    (
        "tests/conformance/persistence/test_cell3_rotation_atomicity.py::test_rotation_is_atomic_under_sigkill[None]",
        PLT,
        "SIGKILL rotation atomicity cell; journal-mode resolution via child config",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell3_rotation_atomicity.py::test_rotation_is_atomic_under_sigkill[DELETE]",
        PLT,
        "same cell pinned to explicit DELETE",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell3_rotation_atomicity.py::test_rotation_is_atomic_under_sigkill[WAL]",
        PLT,
        "same cell requesting WAL; conditional skip as in cell1",
        None,
    ),
    # ---- tests/conformance/persistence/test_cell5_delivery_outbox_exactly_once.py (6) ----
    (
        "tests/conformance/persistence/test_cell5_delivery_outbox_exactly_once.py::test_crash_between_send_and_record_never_double_delivers[recorded-unsent]",
        PLT,
        "outbox exactly-once cell: parent+child processes, SIGKILL at kill_point=recorded-unsent, real gateway platform adapter; crash-window contract",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell5_delivery_outbox_exactly_once.py::test_crash_between_send_and_record_never_double_delivers[attempting-unsent]",
        PLT,
        "same cell, kill_point=attempting-unsent",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell5_delivery_outbox_exactly_once.py::test_crash_between_send_and_record_never_double_delivers[sent-unacked]",
        PLT,
        "same cell, kill_point=sent-unacked",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell5_delivery_outbox_exactly_once.py::test_crash_between_send_and_record_never_double_delivers[resend-crash-attempting]",
        PLT,
        "same cell, kill_point=resend-crash-attempting (child crashes during resend)",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell5_delivery_outbox_exactly_once.py::test_boot_killed_inside_plain_redelivery_never_double_delivers",
        PLT,
        "boot-time redelivery killed mid-flight; exactly-once across restart",
        None,
    ),
    (
        "tests/conformance/persistence/test_cell5_delivery_outbox_exactly_once.py::test_concurrent_reboots_claim_each_obligation_once",
        PLT,
        "concurrent reboot processes claim each outbox obligation exactly once",
        None,
    ),
]

CLASS_ORDER = [OBS, INT, PLT, LIV, EXC]

COLLECTION_ENV = {
    "python": "3.14.5 (pinned checkout .venv)",
    "pytest": "9.1.1",
    "pytest_asyncio": "1.3.0",
    "collection_command": [
        "HERMES_HOME=<isolated> .venv/bin/python -m pytest --collect-only -q",
        "tests/providers/ tests/conformance/ -p no:cacheprovider",
    ],
}


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def collect_ids() -> list[str]:
    """Collect from the pinned checkout; fail closed on any drift."""
    import os

    env = dict(os.environ)
    env["HERMES_HOME"] = ISOLATED_HOME
    proc = subprocess.run(
        [
            PYTEST,
            "-m",
            "pytest",
            "--collect-only",
            "-q",
            "-p",
            "no:cacheprovider",
            "tests/providers/",
            "tests/conformance/",
        ],
        cwd=str(UPSTREAM),
        env=env,
        capture_output=True,
        text=True,
        timeout=300,
    )
    if proc.returncode != 0:
        raise SystemExit(
            f"FAIL: collection failed rc={proc.returncode}\n{proc.stderr[-2000:]}"
        )
    ids = [
        ln.strip()
        for ln in proc.stdout.splitlines()
        if ln.startswith("tests/") and "::" in ln
    ]
    if not ids:
        raise SystemExit("FAIL: collected zero test IDs from pinned checkout")
    return ids


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--out", default=str(REPO / "specs" / "TEST_CLASSIFICATION_MANIFEST.json")
    )
    ap.add_argument(
        "--skip-collection",
        action="store_true",
        help="skip re-collection (recorded ids used as-is)",
    )
    args = ap.parse_args()

    live_ids = None if args.skip_collection else collect_ids()

    embedded_ids = [
        f"{UPSTREAM.name}/{tid}" if False else tid for tid, _, _, _ in CASES
    ]
    # long parametrize ids recorded exactly as pytest renders them
    if live_ids is not None:
        missing = [i for i in live_ids if i not in set(embedded_ids)]
        extra = [i for i in embedded_ids if i not in set(live_ids)]
        if missing or extra:
            print(
                "FAIL: embedded classification does not match live collection",
                file=sys.stderr,
            )
            for m in missing:
                print(f"  missing (collected, unclassified): {m}", file=sys.stderr)
            for e in extra:
                print(f"  extra (classified, not collected): {e}", file=sys.stderr)
            return 2
        ids = live_ids
    else:
        ids = embedded_ids

    by_id = {tid: (cls, ev, note) for tid, cls, ev, note in CASES}
    cases = []
    counts = {c: 0 for c in CLASS_ORDER}
    for tid in ids:
        cls, ev, note = by_id[tid]
        cases.append(
            {
                "id": tid,
                "class": cls,
                "evidence": ev,
                "adapter_note": note,
                "upstream_run": "scoped"
                if not tid.startswith("tests/e2e/")
                else "excluded-from-scope",
            }
        )
        counts[cls] += 1

    total = len(cases)
    assert sum(counts.values()) == total, "counts must reconcile"

    upstream_head = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=str(UPSTREAM), capture_output=True, text=True
    ).stdout.strip()
    if upstream_head != PIN:
        raise SystemExit(f"FAIL: pinned checkout HEAD {upstream_head} != pin {PIN}")

    manifest = {
        "schema": "hermes-rs/test-classification-manifest",
        "version": 1,
        "generated_by": "hrs007",
        "upstream": {
            "repo": "NousResearch/hermes-agent",
            "tag": TAG,
            "commit": PIN,
            "checkout": str(UPSTREAM),
            "checkout_head_verified": upstream_head,
        },
        "scope": {
            "roots": ["tests/providers/", "tests/conformance/"],
            "test_files": 21,
            "note": (
                "Scoped subset per specs/UPSTREAM_BASELINE.md section 5 (HRS-001 "
                "scoped run: providers + conformance, 21 test files). The full "
                "5,724-file suite is out of scope for a single classification pass."
            ),
        },
        "collection_env": COLLECTION_ENV,
        "harness_sha": PIN,
        "totals": {"collected": total, "classified": total, "by_class": counts},
        "reconciliation": {
            "arithmetic": "counts.by_class sums to totals.classified == totals.collected == len(cases)",
            "checks": {
                "sum_by_class_equals_total": sum(counts.values()) == total,
                "cases_listed_equals_total": len(cases) == total,
                "ids_unique": len({c["id"] for c in cases}) == total,
            },
        },
        "class_vocabulary": {
            OBS: "pass/fail computable from adapter-visible I/O (request kwargs, registry/filesystem state, local fake servers)",
            INT: "verifies Python process/import/registry-cache semantics with no cross-language observation point",
            PLT: "outcome depends on OS process semantics (SIGKILL, process races, journal-mode resolution) beyond an adapter boundary",
            LIV: "contacts an external provider API without a local fake (none in this subset)",
            EXC: "excluded from paired runs with a recorded reason (e.g. upstream skip conditions)",
        },
        "cases": cases,
    }

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {out} ({out.stat().st_size} bytes)")
    print(f"total={total} by_class={counts}")
    for k, v in manifest["reconciliation"]["checks"].items():
        print(f"  check {k}: {v}")
    if not all(manifest["reconciliation"]["checks"].values()):
        return 2
    print(f"manifest_sha256={sha256_file(out)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
