# 0001: The reminder queue is a Postgres table

Status: accepted

## Context

Selkie sends a few thousand reminders a day. A separate message broker would be one more thing to run and to back up.

## Decision

Due reminders sit in a `reminder_queue` table. The worker claims rows with `SELECT ... FOR UPDATE SKIP LOCKED`, sends the mail, and deletes the row in the same transaction.

## Consequences

A crash between sending and deleting can send one reminder twice, so every mail carries an idempotency key.
