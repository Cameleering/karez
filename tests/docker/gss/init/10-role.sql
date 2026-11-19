-- The role the Kerberos tests log in as. It has no password: only a Kerberos ticket for
-- karez_gss@KAREZ.TEST.
CREATE ROLE karez_gss LOGIN;
GRANT CONNECT ON DATABASE postgres TO karez_gss;
