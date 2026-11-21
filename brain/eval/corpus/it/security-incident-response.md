# Security incident response plan

This plan describes how Hrafn reacts to suspected security incidents affecting the Skerry platform, staff devices or customer data. It is reviewed every January by the CTO and the security lead.

## Severity levels

- **SEV-1**: confirmed breach of customer personal data, ransomware, or attacker with production access. Response starts immediately, day or night, and the CEO is told within 30 minutes.
- **SEV-2**: lost or stolen unencrypted device, compromised staff account, or an exposed secret in a public repository. Response within 1 hour.
- **SEV-3**: suspicious but contained events such as a blocked phishing campaign, a malware alert on a single laptop. Response next working day.

## Roles

The incident commander is the on-call engineer for SEV-1 and SEV-2; Guðmundur Ólafsson acts as escalation. A scribe records the timeline in the shared incident document. The communications lead is the CEO's delegate, usually the Head of Support, who drafts customer notices.

## First hour checklist

1. Contain: disable the affected account, revoke tokens, isolate the host from the network.
2. Preserve evidence: do not reboot or reimage until a disk snapshot is taken.
3. Open an incident channel named after the date, e.g. inc-2025-01-09.
4. Assess whether personal data is affected. If it is, the legal contact must be told straight away because the Icelandic data protection authority must be notified within 72 hours of us becoming aware.
5. Record every action with a timestamp.

## Communication

Customers affected by a SEV-1 are told within 24 hours, with a plain-language description of what happened, what data was involved and what we are doing. Never speculate about the cause in public channels. Press enquiries go to the CEO only.

## Recovery

Restore from the last known good backup (see the backup runbook), rotate every secret that the compromised system could read, and re-enable services in stages. Production traffic is monitored at double alert sensitivity for 72 hours after recovery.

## Post-incident review

Within five working days of closing a SEV-1 or SEV-2 we hold a blameless review. The output is a written report with a timeline, root cause, what went well, what did not, and at least one preventative action with an owner and a due date. Reports are stored in the meetings folder and summarised at the next all-hands.

## Exercises

Twice a year we run a tabletop exercise. The most recent one simulated a leaked cloud key and revealed that two engineers did not know where the break-glass credentials were stored; they are now kept in the physical safe at reception and in the 1Password emergency kit.

## Contact list

Security lead: security@hrafn.example. IT on-call: 555-0142. Data protection contact: legal@hrafn.example. Insurance: cyber policy number is held by the CFO; the insurer must be called within 48 hours of any SEV-1.

## Appendix: examples

**Example SEV-2.** A developer pushes a cloud access key to a public repository. The scanner flags it within 4 minutes, the on-call engineer rotates the key, reviews cloud audit logs for the window of exposure and confirms no unusual activity. The incident is closed after 3 hours with a review item: add the pre-commit secret scan to the developer onboarding checklist.

**Example SEV-3.** The mail filter blocks a campaign pretending to be a courier company. Nobody clicked; security adds the sender domain to the block list and posts a short reminder in the staff channel.

## Metrics

We track time to detect, time to contain and time to recover for every incident and report the median per quarter to the board. The 2024 median time to contain for SEV-2 incidents was 38 minutes, against a target of 60.
