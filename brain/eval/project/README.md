# Selkie

Selkie is a small service that chases late invoices for Hrafn's customers. It reads open invoices from the accounting export, decides who is due a reminder, and sends it by email. It never touches the books itself.

Start it locally with `make dev`; the tests run with `make test`. The service keeps its state in one Postgres database, and the reminder worker is a separate process that reads a queue.
