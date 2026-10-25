#!/bin/sh

./fetch.sh
status=$?

echo
echo "fetch.sh exited with status: $status"
echo "Press Enter to close..."
read _
exit "$status"