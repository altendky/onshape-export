#!/bin/sh
set -eu

chown -R onshape-export:onshape-export /data
exec setpriv --reuid=onshape-export --regid=onshape-export --init-groups "$@"
