#!/usr/bin/env python3
"""HRS-003 Python client for the hermes-rs session-store adapter.

Real Python package used: the CPython standard library ``sqlite3`` module
(verified: ``sqlite3.sqlite_version``) plus ``json``/``subprocess`` from the
same stdlib. Nothing beyond CPython 3.11's stdlib is required: no C extension
is built, no ``hermes`` package is imported, no third-party dependency is
installed.

The client launches the Rust ``hermes-rs-session-adapter`` binary as a
subprocess and drives it with newline-delimited JSON-RPC over stdio. All
structured data crossing the boundary is plain JSON values.

Error propagation is structured, never a crash: a failing call raises
:class:`AdapterError` carrying the server's machine code, upstream cause
member, and human detail.
"""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass


class AdapterError(Exception):
    """A structured error returned by the adapter (not a crash)."""

    def __init__(self, error: str, cause: str, detail: str):
        super().__init__(f"{error} [{cause}]: {detail}")
        self.error = error
        self.cause = cause
        self.detail = detail


@dataclass
class OpenReport:
    schema_version_before: int | None
    schema_version_after: int | None
    created_new_store: bool
    degraded: list


class SessionAdapter:
    """One adapter subprocess; one open store per process."""

    def __init__(self, adapter_bin: str):
        self._proc = subprocess.Popen(
            [adapter_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self._next_id = 0

    def _call(self, method: str, params: dict | None = None):
        if self._proc.poll() is not None:
            stderr = self._proc.stderr.read() if self._proc.stderr else ""
            raise AdapterError(
                "ProcessDead",
                "unknown",
                f"adapter exited rc={self._proc.returncode}: {stderr.strip()}",
            )
        self._next_id += 1
        frame = {"id": self._next_id, "method": method}
        if params is not None:
            frame["params"] = params
        line = json.dumps(frame)
        assert self._proc.stdin is not None and self._proc.stdout is not None
        self._proc.stdin.write(line + "\n")
        self._proc.stdin.flush()
        resp_line = self._proc.stdout.readline()
        if not resp_line:
            stderr = self._proc.stderr.read() if self._proc.stderr else ""
            raise AdapterError(
                "ProcessDead",
                "unknown",
                f"adapter closed stdout (rc={self._proc.poll()}): {stderr.strip()}",
            )
        resp = json.loads(resp_line)
        if "error" in resp:
            err = resp["error"]
            raise AdapterError(
                err.get("error", "?"), err.get("cause", "?"), err.get("detail", "")
            )
        if resp.get("id") != self._next_id:
            raise AdapterError(
                "ProtocolError", "unknown", f"response id mismatch: {resp!r}"
            )
        return resp["result"]

    # ── lifecycle ────────────────────────────────────────────────────────

    def open(
        self, path: str, *, create: bool = False, read_only: bool = False
    ) -> OpenReport:
        result = self._call(
            "open", {"path": path, "create": create, "read_only": read_only}
        )
        rep = result["report"]
        self.capabilities = result["capabilities"]
        self.open_report = OpenReport(
            schema_version_before=rep["schema_version_before"],
            schema_version_after=rep["schema_version_after"],
            created_new_store=rep["created_new_store"],
            degraded=rep["degraded"],
        )
        return self.open_report

    def shutdown(self) -> None:
        """Send the shutdown frame, close stdin, wait for a clean exit."""
        try:
            self._call("shutdown")
        except AdapterError:
            pass
        try:
            if self._proc.stdin:
                self._proc.stdin.close()
        except BrokenPipeError:
            pass
        self._proc.wait(timeout=10)

    def close(self) -> None:
        """Best-effort cleanup for context-manager/finally use."""
        try:
            if self._proc.poll() is None:
                self.shutdown()
        except Exception:
            self._proc.kill()
            self._proc.wait(timeout=10)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.close()

    # ── §6.1 operations ──────────────────────────────────────────────────

    def ensure_session(
        self, session_id: str, source: str = "unknown", model: str | None = None
    ) -> dict:
        return self._call(
            "ensure_session",
            {"session_id": session_id, "source": source, "model": model},
        )

    def append_message(self, session_id: str, role: str, content: str, **extra) -> dict:
        params = {"session_id": session_id, "role": role, "content": content}
        params.update(extra)
        return self._call("append_message", params)

    def get_messages(self, session_id: str, limit: int | None = None) -> dict:
        return self._call("get_messages", {"session_id": session_id, "limit": limit})

    def end_session(self, session_id: str, end_reason: str) -> dict:
        return self._call(
            "end_session", {"session_id": session_id, "end_reason": end_reason}
        )

    def search(self, query: str, limit: int = 10) -> dict:
        return self._call("search", {"query": query, "limit": limit})

    def checkpoint(self) -> dict:
        return self._call("checkpoint")

    def backup(self, dest: str) -> dict:
        return self._call("backup", {"dest": dest})

    def close_store(self) -> dict:
        return self._call("close")


def adapter_path(repo_root: str | None = None) -> str:
    """Locate the built adapter binary (cargo target layout, debug profile)."""
    root = repo_root or os.environ.get("HERMES_RS_ROOT", os.getcwd())
    return os.path.join(root, "target", "debug", "hermes-rs-session-adapter")


if __name__ == "__main__":
    # Tiny smoke entry used by docs: prints the protocol version.
    import sys

    bin_path = sys.argv[1] if len(sys.argv) > 1 else adapter_path()
    out = subprocess.run(
        [bin_path, "--protocol"], capture_output=True, text=True, check=True
    )
    print(out.stdout.strip())
