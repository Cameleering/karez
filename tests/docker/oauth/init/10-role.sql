-- The role the OAuth tests log in as. It has no password: only a bearer token the validator accepts.
CREATE ROLE karez_oauth LOGIN;
GRANT CONNECT ON DATABASE postgres TO karez_oauth;
