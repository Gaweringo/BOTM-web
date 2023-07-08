ALTER TABLE users
   ADD COLUMN access_token TEXT,
   ADD COLUMN expiry_timestamp timestamptz;

UPDATE users SET access_token = '', expiry_timestamp = '-infinity' WHERE access_token IS NULL;

ALTER TABLE users
  ALTER COLUMN access_token SET NOT NULL,
  ALTER COLUMN expiry_timestamp SET NOT NULL;
