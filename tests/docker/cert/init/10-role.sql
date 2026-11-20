-- The role the client-certificate tests log in as: no password, only a certificate with CN karez_cert.
CREATE ROLE karez_cert LOGIN;
GRANT CONNECT ON DATABASE karez TO karez_cert;
