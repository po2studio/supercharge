# 0002: Money is stored as integers

Status: accepted

## Context

Floating point cannot represent most decimal amounts exactly, and a reminder that says an invoice is 0.01 off looks careless.

## Decision

Every amount is a 64-bit integer in the smallest unit of its currency (aurar for ISK, cents for EUR). Rounding happens once, at the edge, when an amount is shown.
