# Skerry platform overview

Skerry is Hrafn's booking and itinerary engine for small tour operators, guesthouses and activity providers in the Nordics. It was launched in 2019 and today processes roughly 38,000 bookings a month in peak season (June to August) and about 9,000 in January.

## What Skerry does

A customer embeds the Skerry widget on their website. Visitors choose dates, party size and add-ons, and Skerry checks live availability against the operator's inventory, applies the price computed by the Geysir pricing engine, takes payment through Paylink and sends a confirmation email in English, Icelandic, German or French. Operators manage everything in a web console; a mobile app, Project Skúmur, is in development.

## Architecture

The system is a modular monolith written in Kotlin, with a React front end. The core pieces are:

- the **booking service**, which owns reservations and holds a 15-minute soft lock on inventory during checkout;
- the **inventory cache**, which keeps supplier availability in memory and refreshes it every 90 seconds;
- the **pricing engine** (Geysir), which is called synchronously and must answer in under 120 ms;
- the **notification worker**, which sends email and SMS through two providers with automatic failover;
- the **search edge**, an nginx-based proxy that terminates TLS and serves cached public search pages.

Data is stored in MySQL 8 today. The migration to PostgreSQL is tracked separately as Project Puffin.

## Environments

There are three environments: dev (rebuilt nightly), staging (mirror of production with anonymised data) and production, hosted in the Frankfurt region with a warm standby in Dublin. Every pull request deploys a preview environment that lives for 72 hours.

## Non-functional targets

Availability target for production is 99.9% measured monthly, with a p95 checkout latency under 900 ms. Recovery point objective is 15 minutes and recovery time objective is 2 hours. Last year we met the availability target in 10 of 12 months; the two misses were a certificate expiry and a cache failure.

## Integrations

Skerry integrates with 14 channel managers, three accounting packages and two calendar standards (iCal and CalDAV). The most used integration is the ferry-and-bus timetable feed, which about 60% of operators enable. New integrations are built when at least three paying customers ask for them.

## Support model

First line is the support team, 08:00 to 18:00 on working days, with an engineer on call outside those hours for P1 incidents only. See the support playbook for response times.

## Known limitations

Multi-currency carts are not supported; a booking is made in a single currency. Group bookings above 40 people must be created by the operator. Offline mode in the mobile app is planned but not committed.

## Who to ask

Product questions go to the Head of Product. Architecture questions go to the CTO. Pricing-engine questions go to the pricing squad channel.

## Release history highlights

Version 3 (2022) introduced the soft lock and the Geysir pricing engine. Version 4 (2024) rewrote the operator console and added the public API with per-key rate limits. The current release series is 4.x with a release every Tuesday that has changes to ship; in 2024 we shipped 61 releases and rolled back two.

## Security and compliance

All traffic is encrypted in transit, data at rest is encrypted with keys managed by the cloud provider, and operators can enable two-factor authentication for console users; it is mandatory for owners from 1 April 2025. Penetration tests are done yearly by an external firm, and the platform is covered by a data processing agreement summarised in the legal folder.

## Glossary pointers

Terms such as soft lock, operator and guest are explained in the glossary in the misc folder.
