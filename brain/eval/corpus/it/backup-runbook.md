# Backup runbook

Production databases are backed up nightly at 02:30 UTC to an encrypted bucket in a second region. Retention is 35 days of daily snapshots plus 12 monthly snapshots.

```bash
# list the latest snapshots
brain-backup list --env prod | head
# restore into a scratch database, never straight into prod
brain-backup restore --snapshot latest --target scratch-db
# verify row counts against the checksum file
brain-backup verify scratch-db
```

Restore drills are done on the first Wednesday of every quarter. The last drill, in October, took 41 minutes for the bookings database.
