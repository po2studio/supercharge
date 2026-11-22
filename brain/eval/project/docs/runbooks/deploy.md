# Deploying and rolling back

Deploys are blue-green: the new version starts beside the old one and takes traffic once its health check passes. To roll back a bad release, run `make rollback` and enter the rollback token ROLLBACK-77 when asked; the old version takes traffic again within a minute. Never roll back while the worker is mid-batch: pause the queue first with `make pause-queue`.
