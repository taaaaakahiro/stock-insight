"""Check production D1 capacity without issuing a SQL query or logging credentials."""

import json
import os
import re
import sys
import urllib.error
import urllib.request

DEFAULT_ALERT_BYTES = 450_000_000


def main():
    try:
        alert_bytes = int(os.getenv("D1_STORAGE_ALERT_BYTES") or DEFAULT_ALERT_BYTES)
    except ValueError:
        sys.exit("::error::D1_STORAGE_ALERT_BYTES must be a positive integer")
    if alert_bytes <= 0:
        sys.exit("::error::D1_STORAGE_ALERT_BYTES must be a positive integer")

    account_id = os.getenv("CLOUDFLARE_ACCOUNT_ID", "")
    database_id = os.getenv("D1_DATABASE_ID", "")
    token = os.getenv("CLOUDFLARE_API_TOKEN")
    if not (token and re.fullmatch(r"[\da-fA-F]{32}", account_id)
            and re.fullmatch(r"[\da-fA-F-]{36}", database_id)):
        sys.exit("::error::Cloudflare D1 credentials or database identifiers are missing or invalid")

    request = urllib.request.Request(
        f"https://api.cloudflare.com/client/v4/accounts/{account_id}/d1/database/{database_id}",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            body = json.load(response)
        size = body["result"]["file_size"] if body["success"] is True else None
    except urllib.error.HTTPError as exc:
        sys.exit(f"::error::Cloudflare D1 metadata request failed (HTTP {exc.code})")
    except (urllib.error.URLError, TimeoutError, ValueError, KeyError, TypeError):
        sys.exit("::error::Could not read Cloudflare D1 metadata")

    if type(size) is not int or size < 0:
        sys.exit("::error::Cloudflare D1 metadata did not contain a valid database size")
    if size >= alert_bytes:
        sys.exit(f"::error::D1 database is {size:,} bytes (alert at {alert_bytes:,} bytes)")
    print(f"OK: D1 database is {size:,} bytes (alert at {alert_bytes:,} bytes)")


if __name__ == "__main__":
    main()
