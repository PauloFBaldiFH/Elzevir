-- 69: a suspended hold keeps its place in the queue, but no Item fills it, until it is
-- resumed by hand or at the start of `suspended_until` (docs/specs/circulacao/reservas.md,
-- item 19). A hold set aside is never suspended.
ALTER TABLE holds ADD COLUMN suspended INTEGER NOT NULL DEFAULT 0
    CHECK (suspended IN (0, 1) AND (suspended = 0 OR found IS NULL));
ALTER TABLE holds ADD COLUMN suspended_until TEXT
    CHECK (suspended_until IS NULL OR suspended = 1);
