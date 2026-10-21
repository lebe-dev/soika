-- NULL marks accounts whose language has not been detected yet.
ALTER TABLE users ADD COLUMN locale TEXT CHECK (locale IN ('en', 'ru'));
