# Selkie architecture

The API is a Kotlin service in front of one Postgres database. The reminder worker reads due reminders from a queue table and sends them through the mail provider. The worker calls the Hrafn invoice API to confirm an invoice is still open right before sending, and it stays under that API's per-key request limit by sending at most 8 lookups a second from a single shared token bucket.

Amounts are integers in the smallest currency unit (see ADR 0002). Timestamps are stored in UTC and shown in the customer's time zone.
