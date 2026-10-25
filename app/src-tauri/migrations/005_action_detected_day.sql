-- Existing timestamps retain their source/display meaning. The original
-- business day cannot be recovered reliably from older mixed timestamp formats.
alter table action_items add column detected_day text;
