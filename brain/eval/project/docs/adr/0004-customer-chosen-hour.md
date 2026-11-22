# 0004: Each customer chooses the hour reminders go out

Status: accepted

## Context

Customers outside Iceland received their reminders at an odd hour, and some bookkeepers wanted them after lunch.

## Decision

Each customer picks the hour its reminders go out, in its own time zone. The default is 09:00.

## Consequences

The worker schedules per customer, not once a day.
