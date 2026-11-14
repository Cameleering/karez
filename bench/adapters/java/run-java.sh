#!/bin/sh
# Runs the JVM adapter. Settings are the same for every JVM client: fixed heap, G1 (the default).
# Bulk scenarios (S4 to S6) buffer whole results in some clients' defaults, so they get a larger heap.
HEAP=512m
case " $* " in *" s4 "*|*" s5 "*|*" s6 "*) HEAP=3g ;; *" s8 "*) HEAP=6g ;; esac
exec java -Xms$HEAP -Xmx$HEAP -XX:+UseG1GC -jar "$(dirname "$0")/target/karez-bench-java.jar" "$@"
