# Lease and withdraw public Pod Announcements

Status: ready-for-agent
Blocked by: None
Source: ../PRD.md

## What to build

Make public Pod discovery state self-expiring and explicitly withdrawable. Origin Nodes must publish signed Pod Announcements with renewable 30-day leases, refresh them when relevant public state changes, and issue signed Pod Withdrawals when a Pod stops being public. Discovery lifecycle changes must never delete an existing Subscription or synchronized content.

## Acceptance criteria

- [ ] A newly produced public Pod Announcement contains a signed expiry exactly 30 days after issuance and verifies under the Origin identity.
- [ ] An Origin can renew an announcement before expiry, and consumers deterministically prefer the current valid lease over older announcements for the same Pod.
- [ ] Changes to public Pod metadata, Package version, or latest event pointer result in a refreshed announcement.
- [ ] Making a public Pod private or withdrawing it produces an Origin-signed Pod Withdrawal bound to that Pod identity.
- [ ] Invalid, stale, mismatched, or forged renewals and withdrawals are rejected without changing local discovery state.
- [ ] Expiry and withdrawal remove the Pod from new discovery and relaying while preserving existing Subscriptions and synchronized content.
- [ ] Announcement lease and withdrawal state survive SQLite restart.
- [ ] Public HTTP contracts expose the current announcement and withdrawal behavior with typed, inspectable failures.

## Comments

