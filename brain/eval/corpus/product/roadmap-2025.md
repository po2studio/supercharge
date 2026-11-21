---
title: Product roadmap 2025
date: 2025-01-08
---

# Roadmap 2025

## Q1
- Project Puffin: move bookings storage to PostgreSQL.
- Project Skúmur: mobile operator app, closed beta.
- Rate-limit headers on all public API responses.

## Q2
- Multi-currency carts for the Geysir pricing engine, first with ISK and EUR.
- Self-service onboarding wizard, so new operators can go live without a call.
- Project Skarfur: a lightweight embeddable widget under 40 KB, aimed at small guesthouses.

## Q3
- Group booking flow for parties above 40.
- Public status page.

## Q4
- Review of the French and German notification templates.
- Decide on an offline mode for the mobile app.

## Not planned

We deliberately do not plan a consumer marketplace this year. The board agreed in January that the focus is on operators and that a marketplace would put us in competition with our own customers.

## Detail and rationale

### Reliability theme

Two of last year's three availability misses were avoidable: an expired certificate and a stale cache. In Q1 the platform squad adds certificate-expiry monitoring, cache-age alerts and schema validation for supplier feeds. The target is to meet 99.9% availability in all twelve months of 2025. A public status page follows in Q3 once the internal metrics are trustworthy enough to publish.

### Pricing squad

Multi-currency carts are the most requested feature in churn interviews. The first release supports ISK and EUR only, with the exchange rate frozen at the moment the cart is created. SEK, NOK and DKK follow if demand justifies it. The pricing squad also wants to experiment with demand-based price suggestions for operators, shown in the console as a recommendation rather than applied automatically.

### Onboarding

The self-service wizard guides a new operator through creating a resource, connecting a calendar, setting prices and embedding the widget. The aim is to halve the average time to go-live from ten working days to five, and to remove the need for a kickoff call for operators with a single resource.

### Platform and developer experience

Besides the database work, the platform squad will reduce the average build time from 14 minutes to under 8 and move the preview environments to a cheaper cluster. The release process stays as it is: Tuesday mornings, with a Friday freeze.

### Dependencies and risks

The mobile app depends on the rate-limit headers and the new auth endpoints. The database migration touches almost every service, so feature work in the booking squad is deliberately light in March. If hiring is delayed, the group booking flow slips to Q4 first.
