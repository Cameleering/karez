#!/bin/sh
# Creates the realm, one user and the PostgreSQL service principal on first start, then runs the KDC.
set -e
if [ ! -f /var/lib/krb5kdc/principal ]; then
  kdb5_util create -s -r KAREZ.TEST -P karez_test_only_master
  kadmin.local -q "addprinc -pw karez_test_only_gss karez_gss@KAREZ.TEST"
  kadmin.local -q "addprinc -randkey postgres/localhost@KAREZ.TEST"
  # the keytab goes to a folder shared with the PostgreSQL container (test only: readable by all)
  kadmin.local -q "ktadd -k /keytabs/postgres.keytab postgres/localhost@KAREZ.TEST"
  chmod 644 /keytabs/postgres.keytab
fi
exec krb5kdc -n
