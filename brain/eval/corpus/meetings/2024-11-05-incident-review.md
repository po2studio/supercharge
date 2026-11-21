# Incident review: ERR-5137

Date of incident: 29 October 2024. Duration: 2 hours 10 minutes (from 14:05 to 16:15 UTC).

**Impact:** hotel availability showed stale rooms; 112 bookings were taken for rooms that were no longer available. All affected guests were rebooked or refunded; total goodwill cost EUR 6,300.

**Root cause:** ERR-5137, the inventory cache was not invalidated after a supplier feed schema change. The refresh job silently skipped records with the new field.

**What went well:** alerting fired within 4 minutes; support had a template ready.

**Actions:** add schema validation to the feed importer (Kári, by 15 Nov); alert when cache age exceeds 5 minutes (Sunna, by 22 Nov).
