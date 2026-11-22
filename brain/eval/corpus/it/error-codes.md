# Skerry production error codes

| Code | Meaning | First action |
|---|---|---|
| ERR-5101 | Supplier timeout | retry once, then fall back to cached rates |
| ERR-5173 | Paylink payment token expired; checkout returns HTTP 402 | rotate the Paylink API key in the vault and restart the payments worker |
| ERR-5188 | Duplicate booking reference | escalate to the booking team |
| ERR-5204 | Currency feed unavailable | switch to the ECB fallback |

All codes are logged with the request id. Anything not on this list should be treated as a bug and filed in the tracker.
