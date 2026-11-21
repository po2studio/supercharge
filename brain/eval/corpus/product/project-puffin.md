# Project Puffin

Puffin migrates the Skerry bookings database from MySQL 8 to PostgreSQL 16. Lead: Guðmundur Ólafsson. The reasons are better JSON support, row-level security for multi-tenant isolation and cheaper managed hosting.

Plan: dual-write from 3 February 2025, shadow reads from 24 February, and a final cutover window on 14 March 2025 between 01:00 and 04:00 UTC. A rollback is possible until 21 March, after which the old MySQL cluster is frozen read-only for 60 days.
