"""Ensure diagnostic probes stay read-only and never print credentials/raw payloads."""
import contextlib
import io
import json
import unittest
from unittest.mock import patch
import diagnose_cloudflare as diagnostic


class Response(io.BytesIO):
    code = 401
    headers = {"cf-ray": "test-ray"}


class DiagnosticTest(unittest.TestCase):
    def test_logs_only_allowlisted_fields(self):
        response = Response(json.dumps({"success": False, "errors": [
            {"code": 10000, "message": "private-response"}],
            "private": "private-response"}).encode())
        output = io.StringIO()
        with patch.dict(diagnostic.os.environ, {"CLOUDFLARE_API_TOKEN": "private-token"}), \
             patch.object(diagnostic.urllib.request, "urlopen", return_value=response), \
             contextlib.redirect_stdout(output):
            diagnostic.request("test", "/private-account", {"sql": "SELECT 1"})
        text = output.getvalue()
        for private in ["private-token", "private-response", "private-account", "SELECT 1"]:
            self.assertNotIn(private, text)
        self.assertEqual(json.loads(text)["codes"], [10000])

    def test_all_database_probes_are_read_only(self):
        calls = []

        def request(label, path, payload=None):
            calls.append((label, path, payload))
            if label == "user-token-verify":
                return {"success": False}
            if label == "account-token-verify":
                return {"success": True, "result": {"status": "active"}}
            return {"success": True, "result": [{"results": []}]}

        env = {"CLOUDFLARE_API_TOKEN": "token", "CLOUDFLARE_ACCOUNT_ID": "account", "D1_DATABASE_ID": "db"}
        with patch.dict(diagnostic.os.environ, env), patch.object(diagnostic, "request", side_effect=request), \
             patch.object(diagnostic.time, "sleep"), contextlib.redirect_stdout(io.StringIO()):
            diagnostic.main()
        queries = [payload for _, _, payload in calls if payload is not None]
        self.assertEqual(len(queries), 9)
        for query in queries:
            self.assertTrue(query["sql"].startswith("SELECT "))
            self.assertNotIn(";", query["sql"])
            self.assertEqual(query["params"], [])

    def test_ai_preflight_fails_before_document_processing(self):
        calls = []
        def request(label, path, payload=None):
            calls.append((label, payload))
            if label == "workers-ai-inference":
                return {"success": False, "errors": [{"code": 10000}]}
            return {"success": True, "result": {"status": "active"}}
        env = {"CLOUDFLARE_API_TOKEN": "token", "CLOUDFLARE_ACCOUNT_ID": "account", "D1_DATABASE_ID": "db"}
        with patch.dict(diagnostic.os.environ, env), patch.object(diagnostic, "request", side_effect=request), \
             contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit) as error:
            diagnostic.main(ai=True)
        self.assertEqual(error.exception.code, 1)
        self.assertEqual([label for label, _ in calls], ["user-token-verify", "d1-select", "workers-ai-inference"])
        self.assertEqual(calls[-1][1]["max_tokens"], 1)
        self.assertEqual(calls[1][1]["sql"], "SELECT 1 AS ok")


if __name__ == "__main__":
    unittest.main()
