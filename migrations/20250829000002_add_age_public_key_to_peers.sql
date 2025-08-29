-- Add Age public key to peers table
ALTER TABLE peers ADD COLUMN age_public_key TEXT;
