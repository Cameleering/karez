-- Test-only credentials (also written in tests/docker/README.md and the integration tests).
CREATE ROLE karez_scram LOGIN PASSWORD 'karez_test_only_scram';
CREATE ROLE karez_clear LOGIN PASSWORD 'karez_test_only_clear';
CREATE ROLE karez_trust LOGIN;
ALTER SCHEMA public OWNER TO karez_scram;
