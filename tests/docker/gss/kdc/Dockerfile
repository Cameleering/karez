# A throwaway Kerberos KDC for the GSSAPI tests (realm KAREZ.TEST). Test-only: the keys and
# passwords below are public and must never be used anywhere else.
FROM debian:stable-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends krb5-kdc krb5-admin-server \
 && rm -rf /var/lib/apt/lists/*
COPY krb5.conf /etc/krb5.conf
COPY kdc.conf /etc/krb5kdc/kdc.conf
COPY entry.sh /entry.sh
RUN chmod +x /entry.sh
CMD ["/entry.sh"]
