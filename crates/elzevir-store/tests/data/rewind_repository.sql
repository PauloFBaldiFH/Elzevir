-- Undo migrations 66, 65 and 64, then 63 and 62, for fixtures that reconstruct an older catalog schema.
ALTER TABLE patrons DROP COLUMN anonymized_at;
DROP TABLE koha_keys;
DROP TRIGGER instance_datestamp_added;
DROP TRIGGER instance_datestamp_changed;
DROP TABLE instance_datestamps;
DROP TRIGGER repo_instance_retained;
DROP TABLE repo_versions;
DROP TABLE repo_files;
DROP TABLE repo_objects;
DROP TABLE repo_collection_members;
DROP TABLE repo_collections;
DROP TABLE repo_deposits;
