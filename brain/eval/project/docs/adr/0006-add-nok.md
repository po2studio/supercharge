# 0006: Norwegian krone invoices are handled too

Status: accepted

## Context

A Norwegian bookkeeping firm wants to use Selkie for its Icelandic clients' invoices in NOK.

## Decision

Selkie also handles invoices in NOK, besides the ISK and EUR of ADR 0005, with the same integer rules. Other currencies are still rejected.

## Consequences

Formatting and rounding for NOK are added to the reminder templates.
