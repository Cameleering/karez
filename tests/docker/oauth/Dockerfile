# PostgreSQL 18 with a test OAuth validator, for the OAUTHBEARER tests. Test-only.
FROM postgres:18
RUN apt-get update \
 && apt-get install -y --no-install-recommends gcc libc6-dev libkrb5-dev postgresql-server-dev-18 \
 && rm -rf /var/lib/apt/lists/*
COPY validator.c /build/validator.c
RUN gcc -O2 -fPIC -shared -I"$(pg_config --includedir-server)" \
        -o "$(pg_config --pkglibdir)/karez_validator.so" /build/validator.c \
 && apt-get purge -y gcc libc6-dev libkrb5-dev postgresql-server-dev-18 && apt-get autoremove -y
