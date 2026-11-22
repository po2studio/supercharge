# 0005: Selkie handles invoices in ISK and EUR

Status: accepted; amended by [0006](0006-add-nok.md)

## Context

Every customer so far invoices in Icelandic krónur or euros, and each currency needs its own rounding and formatting rules.

## Decision

Selkie handles invoices in ISK and EUR only. An invoice in any other currency is rejected when it is imported.

## Consequences

Support turns away customers who invoice in other currencies.
